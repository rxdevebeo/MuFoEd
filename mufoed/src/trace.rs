//! A diagnostic log and a watchdog for the interface thread.
//!
//! `MUFOED_TRACE=1` writes every input event the window receives and every
//! request a document handles, with timings, to `trace.log` in the
//! configuration directory. Independently of it, a watchdog pokes the
//! interface twice a second and writes a line when a frame has not come for
//! three seconds, and another when it comes back: a freeze leaves the events
//! that preceded it and how long it lasted in the log.

use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long without a frame counts as a stall.
const STALL: Duration = Duration::from_secs(3);

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn millis() -> u64 {
    u64::try_from(start().elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn file() -> Option<&'static Mutex<std::fs::File>> {
    static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let dir = crate::settings::config_dir()?;
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("trace.log"))
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Whether input and requests are traced.
pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("MUFOED_TRACE").is_ok_and(|value| !value.is_empty() && value != "0")
    })
}

/// Writes a line, whether tracing is on or not.
pub(crate) fn write(line: &str) {
    let Some(file) = file() else {
        return;
    };
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("?");
    if let Ok(mut file) = file.lock() {
        let _ = writeln!(file, "{:>9} [{name}] {line}", millis());
    }
}

/// Writes a line when tracing is on.
pub(crate) fn log(line: impl FnOnce() -> String) {
    if enabled() {
        write(&line());
    }
}

static LAST_FRAME: AtomicU64 = AtomicU64::new(0);
static STALLED: AtomicBool = AtomicBool::new(false);
/// What the interface was doing when the frame began.
static LAST_INPUT: Mutex<String> = Mutex::new(String::new());

/// Work a document thread is doing now: thread, start, what.
static BUSY: Mutex<Vec<(String, u64, String, bool)>> = Mutex::new(Vec::new());

/// Marks the start of a piece of document work; the guard ends it.
pub(crate) fn busy(what: impl FnOnce() -> String) -> BusyGuard {
    let thread = format!("{:?}", std::thread::current().id());
    if let Ok(mut busy) = BUSY.lock() {
        busy.retain(|(id, ..)| *id != thread);
        busy.push((thread.clone(), millis(), what(), false));
    }
    BusyGuard { thread }
}

/// Ends the work [`busy`] began, noting how long a stalled one took.
pub(crate) struct BusyGuard {
    thread: String,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Ok(mut busy) = BUSY.lock() {
            if let Some((_, started, what, reported)) =
                busy.iter().find(|(id, ..)| *id == self.thread).cloned()
            {
                if reported {
                    write(&format!(
                        "document work finished after {} ms: {what}",
                        millis().saturating_sub(started)
                    ));
                }
            }
            busy.retain(|(id, ..)| *id != self.thread);
        }
    }
}

/// Called at the start of every frame with a summary of its input.
pub(crate) fn frame(input: &str) {
    let now = millis();
    let last = LAST_FRAME.swap(now, Ordering::Relaxed);
    if STALLED.swap(false, Ordering::Relaxed) {
        write(&format!(
            "interface resumed after {} ms",
            now.saturating_sub(last)
        ));
    }
    if !input.is_empty() {
        if let Ok(mut last_input) = LAST_INPUT.lock() {
            input.clone_into(&mut last_input);
        }
    }
}

/// Starts the watchdog; it keeps the interface repainting twice a second so
/// an idle window is not mistaken for a frozen one.
pub(crate) fn watchdog(ctx: eframe::egui::Context) {
    let _ = start();
    LAST_FRAME.store(millis(), Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name("mufoed-watchdog".to_owned())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(500));
            ctx.request_repaint();
            let silent = millis().saturating_sub(LAST_FRAME.load(Ordering::Relaxed));
            let stall = u64::try_from(STALL.as_millis()).unwrap_or(u64::MAX);
            if let Ok(mut busy) = BUSY.lock() {
                for (_, started, what, reported) in busy.iter_mut() {
                    let took = millis().saturating_sub(*started);
                    if took > stall && !*reported {
                        *reported = true;
                        write(&format!("document work running for {took} ms: {what}"));
                    }
                }
            }
            if silent > stall && !STALLED.swap(true, Ordering::Relaxed) {
                let input = LAST_INPUT
                    .lock()
                    .map(|input| input.clone())
                    .unwrap_or_default();
                write(&format!(
                    "interface stalled for {silent} ms; last input: {input}"
                ));
                // Twice, a second apart: the same frames mean a wait, others a loop.
                for attempt in 1..=2 {
                    if let Some(stack) = crate::stack::interface_stack() {
                        write(&format!(
                            "interface thread stack ({attempt}):
{stack}"
                        ));
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        });
    drop(spawned);
}
