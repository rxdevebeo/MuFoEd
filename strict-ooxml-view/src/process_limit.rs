//! Native Windows containment and process memory counters.

#[derive(Clone, Copy, Default, serde::Serialize)]
#[allow(clippy::struct_field_names)] // Log field names carry explicit units.
pub(crate) struct Memory {
    pub(crate) rss_bytes: usize,
    pub(crate) private_bytes: usize,
    pub(crate) peak_rss_bytes: usize,
}

#[cfg(windows)]
#[allow(unsafe_code)] // Narrow FFI boundary; every handle and buffer is validated below.
mod native {
    use super::Memory;
    use std::{io, mem, os::windows::io::AsRawHandle, process::Child, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOB_OBJECT_LIMIT_PROCESS_MEMORY,
            },
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        },
    };

    pub(crate) struct Limit(HANDLE);
    impl Limit {
        pub(crate) fn attach(child: &Child, bytes: usize) -> io::Result<Self> {
            // SAFETY: null security/name pointers request an unnamed, non-inheritable job.
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let limit = Self(handle);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            info.ProcessMemoryLimit = bytes;
            // SAFETY: info has the exact layout/size required by this information class;
            // child owns a live process handle. It is waiting on stdin and cannot render yet.
            let applied = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    ptr::from_ref(&info).cast(),
                    u32::try_from(mem::size_of_val(&info))
                        .map_err(|error| io::Error::other(error.to_string()))?,
                )
            };
            if applied == 0 {
                return Err(io::Error::last_os_error());
            }
            if unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(limit)
        }
    }
    impl Drop for Limit {
        fn drop(&mut self) {
            // SAFETY: this is our unique owned job handle; closing kills leftover workers.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub(crate) fn memory(child: &Child) -> io::Result<Memory> {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        counters.cb = u32::try_from(mem::size_of_val(&counters))
            .map_err(|error| io::Error::other(error.to_string()))?;
        // SAFETY: counters is writable and large enough for the requested extended counters.
        if unsafe {
            GetProcessMemoryInfo(
                child.as_raw_handle(),
                ptr::from_mut(&mut counters).cast(),
                counters.cb,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Memory {
            rss_bytes: counters.WorkingSetSize,
            private_bytes: counters.PrivateUsage,
            peak_rss_bytes: counters.PeakWorkingSetSize,
        })
    }
}
#[cfg(windows)]
pub(crate) use native::{memory, Limit};

#[cfg(all(test, windows))]
#[allow(unsafe_code, clippy::expect_used)]
mod tests {
    use super::Limit;
    use std::{
        io::Read,
        process::{Command, Stdio},
        ptr,
    };
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };

    #[test]
    #[ignore = "subprocess probe invoked by job_denies_commit_above_budget"]
    fn allocation_probe() {
        std::io::stdin()
            .read_to_end(&mut Vec::new())
            .expect("start gate");
        // SAFETY: request anonymous memory; a successful allocation is released immediately.
        let allocation = unsafe {
            VirtualAlloc(
                ptr::null(),
                128 * 1024 * 1024,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if !allocation.is_null() {
            unsafe {
                VirtualFree(allocation, 0, MEM_RELEASE);
            }
        }
        assert!(
            allocation.is_null(),
            "128 MiB commit must be denied by a 64 MiB job"
        );
    }

    #[test]
    fn job_denies_commit_above_budget() {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--ignored",
                "--exact",
                "process_limit::tests::allocation_probe",
                "--nocapture",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("probe");
        let limit = Limit::attach(&child, 64 * 1024 * 1024);
        if let Err(error) = limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("job assignment failed: {error}");
        }
        let _limit = limit.expect("job");
        drop(child.stdin.take());
        let result = child.wait_with_output().expect("probe completion");
        assert!(
            result.status.success(),
            "{} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    }
}

#[cfg(not(windows))]
pub(crate) struct Limit;
#[cfg(not(windows))]
impl Limit {
    // The Windows limit can fail to attach; callers handle that one signature.
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn attach(_child: &std::process::Child, _bytes: usize) -> std::io::Result<Self> {
        Ok(Self)
    }
}
#[cfg(not(windows))]
pub(crate) fn memory(child: &std::process::Child) -> std::io::Result<Memory> {
    let status = std::fs::read_to_string(format!("/proc/{}/status", child.id()))?;
    let number = |key: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|line| line.split_whitespace().next())
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            .saturating_mul(1024)
    };
    Ok(Memory {
        rss_bytes: number("VmRSS:"),
        private_bytes: number("VmSize:"),
        peak_rss_bytes: number("VmHWM:"),
    })
}
