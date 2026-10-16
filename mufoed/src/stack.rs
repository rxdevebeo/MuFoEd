//! The interface thread's call stack, read from the watchdog when the
//! interface stops answering (Windows x64; elsewhere there is none).
//!
//! The thread is suspended only while its return addresses are copied into a
//! fixed buffer: it may hold the heap lock, so nothing is allocated until it
//! runs again. Names come from dbghelp afterwards - exports for system DLLs,
//! full names and lines for this program when its PDB lies beside it.

/// Remembers the calling thread as the interface thread.
pub(crate) fn remember_interface_thread() {
    #[cfg(all(windows, target_arch = "x86_64"))]
    imp::remember();
}

/// The interface thread's stack, one frame per line, when it can be read.
pub(crate) fn interface_stack() -> Option<String> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        imp::stack()
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        None
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[allow(unsafe_code)] // Narrow Win32 boundary: suspend, unwind, symbolize.
mod imp {
    use std::fmt::Write as _;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HMODULE};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        GetThreadContext, RtlLookupFunctionEntry, RtlVirtualUnwind, SymFromAddrW,
        SymGetLineFromAddrW64, SymInitializeW, SymSetOptions, CONTEXT, CONTEXT_FULL_AMD64,
        IMAGEHLP_LINEW64, SYMBOL_INFOW, SYMOPT_DEFERRED_LOADS, SYMOPT_LOAD_LINES, SYMOPT_UNDNAME,
        UNW_FLAG_NHANDLER,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
        THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME,
    };

    const MAX_FRAMES: usize = 64;
    /// Longest symbol name read, in UTF-16 units.
    const NAME: usize = 512;

    static INTERFACE: AtomicU32 = AtomicU32::new(0);

    pub(super) fn remember() {
        // SAFETY: no preconditions.
        INTERFACE.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    }

    /// `CONTEXT` must be 16-byte aligned for `GetThreadContext`.
    #[repr(C, align(16))]
    struct Aligned(CONTEXT);

    fn capture(thread: HANDLE, frames: &mut [u64; MAX_FRAMES]) -> usize {
        // SAFETY: an all-zero CONTEXT is a valid value; the flags are set below.
        let mut context: Aligned = unsafe { std::mem::zeroed() };
        context.0.ContextFlags = CONTEXT_FULL_AMD64;
        // SAFETY: the thread is suspended and the context is aligned.
        if unsafe { GetThreadContext(thread, &raw mut context.0) } == 0 {
            return 0;
        }
        let mut count = 0;
        while count < MAX_FRAMES {
            let pc = context.0.Rip;
            if pc == 0 {
                break;
            }
            if let Some(slot) = frames.get_mut(count) {
                *slot = pc;
            }
            count += 1;
            let mut image_base = 0u64;
            // SAFETY: a lookup in the loaded images' unwind tables.
            let entry =
                unsafe { RtlLookupFunctionEntry(pc, &raw mut image_base, std::ptr::null_mut()) };
            if entry.is_null() {
                // A leaf function: the return address is on top of the stack.
                let rsp = context.0.Rsp;
                if rsp == 0 || !rsp.is_multiple_of(8) {
                    break;
                }
                // SAFETY: Rsp points into the suspended thread's committed stack.
                context.0.Rip = unsafe { std::ptr::read(rsp as *const u64) };
                context.0.Rsp = rsp + 8;
            } else {
                let mut handler_data = std::ptr::null_mut();
                let mut establisher = 0u64;
                // SAFETY: the entry belongs to `image_base`; the context is the
                // suspended thread's, unwound one frame.
                unsafe {
                    RtlVirtualUnwind(
                        UNW_FLAG_NHANDLER,
                        image_base,
                        pc,
                        entry,
                        &raw mut context.0,
                        &raw mut handler_data,
                        &raw mut establisher,
                        std::ptr::null_mut(),
                    );
                }
            }
        }
        count
    }

    pub(super) fn stack() -> Option<String> {
        let id = INTERFACE.load(Ordering::Relaxed);
        if id == 0 {
            return None;
        }
        // SAFETY: plain handle request; checked for null.
        let thread = unsafe {
            OpenThread(
                THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME | THREAD_QUERY_INFORMATION,
                0,
                id,
            )
        };
        if thread.is_null() {
            return None;
        }
        let mut frames = [0u64; MAX_FRAMES];
        // SAFETY: a valid thread handle; resumed right after the capture,
        // which allocates nothing.
        let count = unsafe {
            if SuspendThread(thread) == u32::MAX {
                CloseHandle(thread);
                return None;
            }
            let count = capture(thread, &mut frames);
            ResumeThread(thread);
            CloseHandle(thread);
            count
        };
        let mut out = String::new();
        for (index, pc) in frames.iter().take(count).enumerate() {
            // A return address points after the call; look up the call itself.
            let lookup = if index == 0 {
                *pc
            } else {
                pc.saturating_sub(1)
            };
            let _ = writeln!(out, "    #{index:02} {}", describe(lookup));
        }
        Some(out)
    }

    fn wide(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(buffer.get(..end).unwrap_or(&[]))
    }

    fn module_of(address: u64) -> (String, u64) {
        let mut module: HMODULE = std::ptr::null_mut();
        // SAFETY: FROM_ADDRESS takes the address itself as the name pointer.
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address as *const u16,
                &raw mut module,
            )
        };
        if found == 0 {
            return ("?".to_owned(), address);
        }
        let mut name = [0u16; 260];
        // SAFETY: the buffer's length is passed.
        let len = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), 260) };
        let path = wide(name.get(..len as usize).unwrap_or(&[]));
        let file = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned();
        (file, address.saturating_sub(module as u64))
    }

    fn describe(address: u64) -> String {
        static SYMBOLS: OnceLock<Mutex<bool>> = OnceLock::new();
        let lock = SYMBOLS.get_or_init(|| {
            // SAFETY: dbghelp initialization for this process, once.
            // The executable's directory, where its PDB is.
            let search: Vec<u16> = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.as_os_str().to_owned()))
                .map_or_else(
                    || vec![0],
                    |dir| {
                        use std::os::windows::ffi::OsStrExt as _;
                        dir.encode_wide().chain(std::iter::once(0)).collect()
                    },
                );
            let ready = unsafe {
                SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES);
                SymInitializeW(GetCurrentProcess(), search.as_ptr(), 1) != 0
            };
            Mutex::new(ready)
        });
        let (module, offset) = module_of(address);
        let mut text = format!("{module}+0x{offset:x}");
        // dbghelp is not thread-safe: one caller at a time.
        let Ok(ready) = lock.lock() else {
            return text;
        };
        if !*ready {
            return text;
        }
        let mut buffer = vec![0u64; (std::mem::size_of::<SYMBOL_INFOW>() + NAME * 2) / 8 + 1];
        let symbol = buffer.as_mut_ptr().cast::<SYMBOL_INFOW>();
        let mut displacement = 0u64;
        // SAFETY: the buffer holds a SYMBOL_INFOW followed by NAME wide chars.
        unsafe {
            (*symbol).SizeOfStruct = std::mem::size_of::<SYMBOL_INFOW>() as u32;
            (*symbol).MaxNameLen = NAME as u32;
            if SymFromAddrW(GetCurrentProcess(), address, &raw mut displacement, symbol) != 0 {
                let len = ((*symbol).NameLen as usize).min(NAME);
                let name =
                    std::slice::from_raw_parts((&raw const (*symbol).Name).cast::<u16>(), len);
                let _ = write!(
                    text,
                    " {}+0x{displacement:x}",
                    String::from_utf16_lossy(name)
                );
            }
            let mut line = IMAGEHLP_LINEW64 {
                SizeOfStruct: std::mem::size_of::<IMAGEHLP_LINEW64>() as u32,
                ..IMAGEHLP_LINEW64::default()
            };
            let mut column = 0u32;
            if SymGetLineFromAddrW64(GetCurrentProcess(), address, &raw mut column, &raw mut line)
                != 0
                && !line.FileName.is_null()
            {
                let mut len = 0;
                while *line.FileName.add(len) != 0 && len < 1024 {
                    len += 1;
                }
                let file = String::from_utf16_lossy(std::slice::from_raw_parts(line.FileName, len));
                let short = file.rsplit(['\\', '/']).take(3).collect::<Vec<_>>();
                let short: Vec<&str> = short.into_iter().rev().collect();
                let _ = write!(text, " ({}:{})", short.join("/"), line.LineNumber);
            }
        }
        text
    }
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    #[test]
    fn a_sleeping_thread_stack_names_its_frames() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let sleeper = std::thread::spawn(move || {
            super::remember_interface_thread();
            ready_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1500));
        });
        ready_rx.recv().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let stack = super::interface_stack().expect("stack");
        sleeper.join().unwrap();
        eprintln!("{stack}");
        assert!(stack.lines().count() >= 3, "{stack}");
        assert!(stack.contains("ntdll.dll"), "{stack}");
        assert!(
            stack.contains("a_sleeping_thread_stack_names_its_frames"),
            "{stack}"
        );
    }
}
