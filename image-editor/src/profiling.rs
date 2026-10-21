//! Opt-in wall-clock diagnostics. No prompts, model paths or credentials are logged.
use std::{
    fs::OpenOptions,
    io::Write,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
static OUTPUT: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
fn output() -> Option<&'static Mutex<std::fs::File>> {
    OUTPUT
        .get_or_init(|| {
            let path = std::env::var_os("IMAGE_STUDIO_TIMINGS")?;
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(file) => Some(Mutex::new(file)),
                Err(error) => {
                    eprintln!("[timing] cannot open diagnostics: {error}");
                    None
                }
            }
        })
        .as_ref()
}
pub(crate) fn enabled() -> bool {
    output().is_some()
}
fn emit(value: &serde_json::Value) {
    if let Some(file) = output() {
        let mut file = file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(error) = writeln!(file, "{value}") {
            eprintln!("[timing] write failed: {error}");
        }
    }
}
pub(crate) struct Scope {
    name: String,
    start: Option<Instant>,
    at_ms: u128,
    #[cfg(feature = "plakat")]
    device: Option<candle_core::Device>,
}
pub(crate) fn scope(name: impl Into<String>) -> Scope {
    let active = enabled();
    Scope {
        name: name.into(),
        start: active.then(Instant::now),
        at_ms: if active {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        } else {
            0
        },
        #[cfg(feature = "plakat")]
        device: None,
    }
}
#[cfg(feature = "plakat")]
pub(crate) fn gpu(
    name: impl Into<String>,
    device: &candle_core::Device,
) -> candle_core::Result<Scope> {
    if enabled() {
        device.synchronize()?;
    }
    let mut value = scope(name);
    if value.start.is_some() {
        value.device = Some(device.clone());
    }
    Ok(value)
}
impl Drop for Scope {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        #[cfg(feature = "plakat")]
        let sync_error = self
            .device
            .as_ref()
            .and_then(|d| d.synchronize().err())
            .map(|e| e.to_string());
        #[cfg(not(feature = "plakat"))]
        let sync_error: Option<String> = None;
        emit(&serde_json::json!({"stage":self.name,"start_ms":self.at_ms,
            "elapsed_ms":start.elapsed().as_secs_f64()*1000.0,"pid":std::process::id(),
            "source":"rust","sync_error":sync_error}));
    }
}
pub(crate) fn python(value: &serde_json::Value) {
    if let Some(records) = value["timings"].as_array() {
        for item in records {
            emit(item);
        }
    }
}

#[cfg(feature = "plakat-cuda")]
pub(crate) struct MemoryWatch {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<serde_json::Value>>,
}
#[cfg(feature = "plakat-cuda")]
pub(crate) fn watch_memory(
    name: &'static str,
    device: &candle_core::Device,
) -> Option<MemoryWatch> {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let candle_core::Device::Cuda(device) = device else {
        return None;
    };
    if !enabled() {
        return None;
    }
    let context = device.cuda_stream().context().clone();
    let initial = context
        .mem_get_info()
        .ok()
        .filter(|(free, total)| *total > 0 && free <= total);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let worker = std::thread::spawn(move || {
        let began = Instant::now();
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let (mut samples, mut invalid, mut min_free, mut total) =
            (0_u64, 0_u64, usize::MAX, 0_usize);
        while !flag.load(Ordering::Acquire) {
            match context.mem_get_info() {
                Ok((free, capacity)) if capacity > 0 && free <= capacity => {
                    samples += 1;
                    min_free = min_free.min(free);
                    total = capacity;
                }
                _ => invalid += 1,
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        serde_json::json!({"stage":name,"source":"cuda_memory","start_ms":at,
            "elapsed_ms":began.elapsed().as_secs_f64()*1000.0,"pid":std::process::id(),
            "samples":samples,"invalid_samples":invalid,"total_bytes":total,"start_free_bytes":initial.map(|v|v.0),
            "min_free_bytes":if samples>0 {Some(min_free)} else {None}})
    });
    Some(MemoryWatch {
        stop,
        worker: Some(worker),
    })
}
#[cfg(feature = "plakat-cuda")]
impl Drop for MemoryWatch {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(worker) = self.worker.take() {
            if let Ok(value) = worker.join() {
                emit(&value);
            }
        }
    }
}
