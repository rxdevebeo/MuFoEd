//! Persistent Python resources controlled by the common model manager.
use super::{python, Output};
use crate::{EditError, Raster};
use mufoed_model_runtime::{residency::Resource, Memory, Ticket};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
fn backend_versions() -> Result<String, String> {
    let mut command = Command::new(python());
    command.args(["-c", "import importlib.metadata as m,json,sys; names=['torch','transformers','onnxruntime','spandrel','aura-sr','safetensors']; versions={};\nfor name in names:\n try: versions[name]=m.version(name)\n except m.PackageNotFoundError: versions[name]=None\nprint(json.dumps([sys.version,versions],sort_keys=True))"]);
    let packages = Path::new(env!("CARGO_MANIFEST_DIR")).join(".runtime/python-packages");
    let mut paths = vec![packages];
    if let Some(existing) = std::env::var_os("PYTHONPATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    command
        .env(
            "PYTHONPATH",
            std::env::join_paths(paths).map_err(|e| e.to_string())?,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err("Python backend version probe failed".into());
            }
            return String::from_utf8(output.stdout).map_err(|e| e.to_string());
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Python version probe timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct Process {
    child: Child,
    ram: Option<u64>,
    vram: Option<u64>,
}
struct Worker {
    footprint: Mutex<Memory>,
    process: Mutex<Option<Process>>,
    directory: tempfile::TempDir,
    packages: PathBuf,
}
impl Worker {
    fn new() -> Result<Self, String> {
        Ok(Self {
            footprint: Mutex::new(Memory::default()),
            process: Mutex::new(None),
            directory: tempfile::tempdir().map_err(|e| e.to_string())?,
            packages: Path::new(env!("CARGO_MANIFEST_DIR")).join(".runtime/python-packages"),
        })
    }
    fn stop(process: &mut Option<Process>) -> Result<(), String> {
        if let Some(worker) = process.as_mut() {
            if worker
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_none()
            {
                worker.child.kill().map_err(|e| e.to_string())?;
                worker.child.wait().map_err(|e| e.to_string())?;
            }
        }
        *process = None;
        Ok(())
    }
    fn spawn(&self) -> Result<Process, String> {
        let mut command = Command::new(python());
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/model_worker.py"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null());
        let log = std::fs::File::create(self.directory.path().join("worker.log"))
            .map_err(|e| e.to_string())?;
        command.stderr(log).env("USE_TF", "0");
        let mut paths = vec![self.packages.clone()];
        if let Some(existing) = std::env::var_os("PYTHONPATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        command.env(
            "PYTHONPATH",
            std::env::join_paths(paths).map_err(|e| e.to_string())?,
        );
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        Ok(Process {
            child: command.spawn().map_err(|e| e.to_string())?,
            ram: None,
            vram: None,
        })
    }
    fn request(
        &self,
        request: Value,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.execute(request, cancel, timeout, None)
    }
    fn read_phase(phase_file: &Path, lease: &mufoed_model_runtime::Lease, last_phase: &mut String) {
        if let Ok(bytes) = std::fs::read(phase_file) {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                let detail = value["detail"].as_str().unwrap_or_default();
                let name = value["phase"].as_str().unwrap_or_default();
                let identity = format!("{name}:{detail}");
                if identity != *last_phase {
                    let phase = match name {
                        "loading" => Some(mufoed_model_runtime::Phase::Loading),
                        "running" => Some(mufoed_model_runtime::Phase::Running),
                        "offloading" => Some(mufoed_model_runtime::Phase::Offloading),
                        _ => None,
                    };
                    if let Some(phase) = phase {
                        lease.stage(phase, detail);
                    }
                    *last_phase = identity;
                }
            }
        }
    }
    fn execute(
        &self,
        mut request: Value,
        cancel: &AtomicBool,
        timeout: Duration,
        lease: Option<&mufoed_model_runtime::Lease>,
    ) -> Result<Value, String> {
        let mut process = self
            .process
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if process
                .as_mut()
                .is_some_and(|p| p.child.try_wait().ok().flatten().is_some())
            {
                *process = None;
            }
            if process.is_none() {
                *process = Some(self.spawn()?);
            }
            let response = self.directory.path().join("response.json");
            if response.exists() {
                std::fs::remove_file(&response).map_err(|e| e.to_string())?;
            }
            request["response"] = json!(response);
            let phase_file = self.directory.path().join("phase.json");
            if phase_file.exists() {
                std::fs::remove_file(&phase_file).map_err(|e| e.to_string())?;
            }
            request["phase_file"] = json!(phase_file);
            let mut last_phase = String::new();
            let worker = process.as_mut().expect("spawned");
            let line = request.to_string() + "\n";
            worker
                .child
                .stdin
                .as_mut()
                .ok_or("worker stdin closed")?
                .write_all(line.as_bytes())
                .map_err(|e| e.to_string())?;
            let began = Instant::now();
            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err("cancelled".into());
                }
                if began.elapsed() > timeout {
                    return Err("Python model worker timed out".into());
                }
                if let Some(status) = worker.child.try_wait().map_err(|e| e.to_string())? {
                    return Err(format!("Python model worker exited: {status}"));
                }
                if let Some(lease) = lease {
                    Self::read_phase(&phase_file, lease, &mut last_phase);
                }
                if response.exists() {
                    let value: Value = serde_json::from_slice(
                        &std::fs::read(&response).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    if value["ok"] != true {
                        return Err(value["error"]
                            .as_str()
                            .unwrap_or("worker failed")
                            .to_owned());
                    }
                    worker.ram = value["ram"].as_u64();
                    worker.vram = value["vram"].as_u64();
                    *self
                        .footprint
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Memory {
                        ram: worker.ram,
                        vram: worker.vram,
                    };
                    return Ok(value);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }))
        .unwrap_or_else(|_| Err("Python host worker panicked".into()));
        if result.is_err() {
            Self::stop(&mut process)?;
            *self
                .footprint
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Memory {
                ram: Some(0),
                vram: Some(0),
            };
        }
        result
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = Self::stop(
            self.process
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}
impl Resource for Worker {
    fn memory(&self) -> Memory {
        // Never wait for the compute mutex on the UI thread.
        if let Ok(mut process) = self.process.try_lock() {
            if process
                .as_mut()
                .is_some_and(|p| p.child.try_wait().ok().flatten().is_some())
            {
                *process = None;
                *self
                    .footprint
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Memory {
                    ram: Some(0),
                    vram: Some(0),
                };
            }
        }
        *self
            .footprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn offload(&self) -> Result<bool, String> {
        self.request(
            json!({"action":"offload"}),
            &AtomicBool::new(false),
            Duration::from_secs(60),
        )
        .map(|v| v["offloaded"] == true)
    }
    fn unload(&self) -> Result<(), String> {
        Self::stop(
            &mut self
                .process
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )?;
        *self
            .footprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Memory {
            ram: Some(0),
            vram: Some(0),
        };
        Ok(())
    }
}
fn fingerprint(path: &Path, hash: &mut Sha256) -> Result<(), String> {
    hash.update(mufoed_model_runtime::fingerprint::file(path)?.as_bytes());
    Ok(())
}

/// Asynchronous host job; dropping only requests cancellation, worker retains its lease.
pub(super) struct Job {
    receiver: mpsc::Receiver<Result<Output, EditError>>,
    cancel: Arc<AtomicBool>,
}
impl Job {
    pub(super) fn start(
        ticket: Ticket,
        script: &str,
        args: &[OsString],
        directory: Arc<tempfile::TempDir>,
        estimate: Memory,
    ) -> Result<Self, EditError> {
        let script = script.to_owned();
        let mut args: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        args.extend([
            "--input".into(),
            directory
                .path()
                .join("input.png")
                .to_string_lossy()
                .into_owned(),
            "--output".into(),
            directory
                .path()
                .join("output.png")
                .to_string_lossy()
                .into_owned(),
            "--staging".into(),
            directory.path().to_string_lossy().into_owned(),
        ]);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("image-resident-model".into())
            .spawn(move || {
                let result = (|| {
                    let lease = ticket
                        .wait(&worker_cancel, Duration::from_secs(900))
                        .map_err(|e| e.to_string())?;
                    let setup_timing = crate::profiling::scope("python.identity_and_admission");
                    let mut hash = Sha256::new();
                    hash.update(format!("python-worker-v1:{script}:{}", backend_versions()?));
                    for name in [script.as_str(), "model_worker.py", "worker_cache.py"] {
                        hash.update(mufoed_model_runtime::fingerprint::file(
                            &Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("scripts")
                                .join(name),
                        )?);
                    }
                    for pair in args[1..].chunks_exact(2) {
                        if matches!(pair[0].as_str(), "--model" | "--aura-model") {
                            fingerprint(Path::new(&pair[1]), &mut hash)?;
                        } else if matches!(pair[0].as_str(), "--variant" | "--device") {
                            hash.update(pair[1].as_bytes());
                        }
                    }
                    let device = if estimate.vram == Some(0) {
                        "cpu"
                    } else {
                        "auto"
                    };
                    let uuid = lease.prepare(estimate, device).map_err(|e| e.to_string())?;
                    hash.update(uuid.as_bytes());
                    let key = format!("python:{script}:{:x}", hash.finalize());
                    drop(setup_timing);
                    let execute_timing = crate::profiling::scope("python.execute");
                    let value = lease.retry(estimate, device, &worker_cancel, |_| {
                        let output = directory.path().join("output.png");
                        if output.exists() {
                            std::fs::remove_file(&output).map_err(|e| e.to_string())?;
                        }
                        let worker = lease.cached(&key, Worker::new)?;
                        worker.execute(
                            json!({"action":"run","script":script,"args":args}),
                            &worker_cancel,
                            Duration::from_secs(900),
                            Some(&lease),
                        )
                    })?;
                    drop(execute_timing);
                    crate::profiling::python(&value);
                    let _decode = crate::profiling::scope("python.output_png");
                    let path = directory.path().join("output.png");
                    let raster = Raster::open(path).map_err(|e| e.to_string())?;
                    Ok(Output {
                        raster: Some(raster),
                        log: format!("Resident worker: {value}"),
                    })
                })()
                .map_err(EditError::Invalid);
                let _ = sender.send(result);
            })?;
        Ok(Self { receiver, cancel })
    }
    pub(super) fn poll(&mut self) -> Result<Option<Output>, EditError> {
        match self.receiver.try_recv() {
            Ok(result) => result.map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(EditError::Invalid("resident worker stopped".into()))
            }
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    #[test]
    fn protocol_child() {
        let Ok(mode) = std::env::var("IMAGE_WORKER_TEST_CHILD") else {
            return;
        };
        for line in std::io::stdin().lock().lines() {
            let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
            if mode == "hang" {
                loop {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            let response = if mode == "error" {
                json!({"ok":false,"error":"CUDA out of memory"})
            } else {
                json!({"ok":true,"ram":1024,"vram":2048})
            };
            let path = PathBuf::from(request["response"].as_str().unwrap());
            let pending = path.with_extension("pending");
            std::fs::write(&pending, response.to_string()).unwrap();
            std::fs::rename(pending, path).unwrap();
        }
    }
    fn worker(mode: &str) -> Worker {
        let worker = Worker::new().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "model_tools::persistent::tests::protocol_child",
                "--nocapture",
            ])
            .env("IMAGE_WORKER_TEST_CHILD", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        *worker.process.lock().unwrap() = Some(Process {
            child: command.spawn().unwrap(),
            ram: None,
            vram: None,
        });
        worker
    }
    #[test]
    fn acknowledged_footprint_is_read_without_compute_lock() {
        let worker = Arc::new(worker("hang"));
        let cancel = Arc::new(AtomicBool::new(false));
        let child = worker.clone();
        let flag = cancel.clone();
        let thread = std::thread::spawn(move || {
            child.request(json!({"action":"run"}), &flag, Duration::from_secs(3))
        });
        std::thread::sleep(Duration::from_millis(50));
        let began = Instant::now();
        let _ = worker.memory();
        assert!(began.elapsed() < Duration::from_millis(100));
        cancel.store(true, Ordering::Release);
        assert!(thread.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(worker.process.lock().unwrap().is_none());
    }
    #[test]
    fn timeout_and_error_kill_and_reap_the_owned_worker() {
        for mode in ["hang", "error"] {
            let worker = worker(mode);
            let result = worker.request(
                json!({"action":"run"}),
                &AtomicBool::new(false),
                Duration::from_millis(100),
            );
            assert!(result.is_err());
            assert!(worker.process.lock().unwrap().is_none());
            assert_eq!(worker.memory().vram, Some(0));
        }
    }
    #[test]
    fn successful_worker_remains_resident_until_confirmed_unload() {
        let worker = worker("ok");
        worker
            .request(
                json!({"action":"run"}),
                &AtomicBool::new(false),
                Duration::from_secs(3),
            )
            .unwrap();
        assert_eq!(worker.memory().ram, Some(1024));
        assert_eq!(worker.memory().vram, Some(2048));
        assert!(worker.process.lock().unwrap().is_some());
        worker.unload().unwrap();
        assert!(worker.process.lock().unwrap().is_none());
        assert_eq!(worker.memory().vram, Some(0));
    }
}
