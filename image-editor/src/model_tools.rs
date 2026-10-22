//! Cancellable subprocess tools for optional segmentation runtimes.
use crate::{EditError, Raster};
#[path = "persistent.rs"]
mod persistent;
use std::sync::Arc;
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// One background process. Its scratch files are removed after termination.
pub struct ModelJob {
    managed: Option<persistent::Job>,
    child: Option<Child>,
    command: Option<Command>,
    ticket: Option<mufoed_model_runtime::Ticket>,
    lease: Option<mufoed_model_runtime::Lease>,
    finished: bool,
    directory: Arc<tempfile::TempDir>,
}
/// Completed model operation. Downloads return no raster.
pub struct Output {
    /// Optional processed image or mask.
    pub raster: Option<Raster>,
    /// Bounded runtime diagnostics.
    pub log: String,
}
/// Installed Python override, or the user's existing Python 3.11 distribution.
pub fn python() -> PathBuf {
    std::env::var_os("IMAGE_STUDIO_PYTHON").map_or_else(
        || {
            let installed = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
                .join("Programs/Python/Python311/python.exe");
            if installed.is_file() {
                installed
            } else {
                "python".into()
            }
        },
        PathBuf::from,
    )
}
impl ModelJob {
    /// Queue an editor-owned script with an explicit argument vector and optional source.
    /// `poll` spawns it after inference admission (or immediately for a download).
    /// A staging root keeps model download publication on the destination volume.
    pub fn start(
        script: &str,
        args: &[OsString],
        source: Option<&Raster>,
        staging_root: Option<&Path>,
    ) -> Result<Self, EditError> {
        Self::start_with_runtime(
            script,
            args,
            source,
            staging_root,
            &mufoed_model_runtime::Runtime::global(),
        )
    }
    /// Use a host-owned runtime shared with text and other model backends.
    /// Downloads (`source == None`) do not request inference admission.
    #[allow(clippy::too_many_lines)] // Queue and isolated download/inference transports share setup.
    pub fn start_with_runtime(
        script: &str,
        args: &[OsString],
        source: Option<&Raster>,
        staging_root: Option<&Path>,
        runtime: &mufoed_model_runtime::Runtime,
    ) -> Result<Self, EditError> {
        let mut estimate = mufoed_model_runtime::Memory::default();
        let mut ticket = if source.is_some() {
            let mut task = mufoed_model_runtime::Task::new("image", script);
            if args
                .windows(2)
                .any(|pair| pair[0] == "--device" && pair[1] == "cpu")
            {
                task.device = "cpu".into();
                task.memory.vram = Some(0);
            }
            let pixels = source.map_or(0, |s| u64::from(s.width()) * u64::from(s.height()));
            let weight = args
                .windows(2)
                .filter(|a| a[0] == "--model" || a[0] == "--aura-model")
                .map(|a| {
                    std::fs::metadata(&a[1])
                        .map_or(0, |m| if m.is_file() { m.len() } else { 3 << 30 })
                })
                .sum::<u64>();
            task.memory.ram = Some((4 << 30) + weight * 3 + pixels * 160);
            if task.memory.vram != Some(0) {
                task.memory.vram = Some(if script == "sam2_tool.py" {
                    5 << 30
                } else if script == "upscale_tool.py" {
                    8 << 30
                } else {
                    6 << 30
                });
            }
            estimate = task.memory;
            Some(
                runtime
                    .enqueue(task)
                    .map_err(|e| EditError::Invalid(e.to_string()))?,
            )
        } else {
            None
        };
        let directory = if let Some(root) = staging_root {
            std::fs::create_dir_all(root)?;
            tempfile::tempdir_in(root)?
        } else {
            tempfile::tempdir()?
        };
        let directory = Arc::new(directory);
        if let Some(source) = source.filter(|_| {
            runtime.is_managed()
                && matches!(script, "birefnet.py" | "sam2_tool.py" | "upscale_tool.py")
        }) {
            source.save_png(directory.path().join("input.png"))?;
            let managed = persistent::Job::start(
                ticket.take().expect("inference ticket"),
                script,
                args,
                directory.clone(),
                estimate,
            )?;
            return Ok(Self {
                managed: Some(managed),
                child: None,
                command: None,
                ticket: None,
                lease: None,
                finished: false,
                directory,
            });
        }
        let mut command = Command::new(python());
        let packages = Path::new(env!("CARGO_MANIFEST_DIR")).join(".runtime/python-packages");
        if packages.is_dir() {
            let mut paths = vec![packages];
            if let Some(existing) = std::env::var_os("PYTHONPATH") {
                paths.extend(std::env::split_paths(&existing));
            }
            command.env(
                "PYTHONPATH",
                std::env::join_paths(paths).map_err(|e| EditError::Invalid(e.to_string()))?,
            );
        }
        command
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("scripts")
                    .join(script),
            )
            .args(args)
            .arg("--staging")
            .arg(directory.path())
            .arg("--output")
            .arg(directory.path().join("output.png"));
        if let Some(source) = source {
            source.save_png(directory.path().join("input.png"))?;
            command
                .arg("--input")
                .arg(directory.path().join("input.png"));
        }
        let log = File::create(directory.path().join("process.log"))?;
        command
            .env("USE_TF", "0")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        Ok(Self {
            managed: None,
            child: None,
            command: Some(command),
            ticket,
            lease: None,
            finished: false,
            directory,
        })
    }
    /// Poll without waiting for inference or a download.
    pub fn poll(&mut self) -> Result<Option<Output>, EditError> {
        if self.finished {
            return Ok(None);
        }
        if let Some(job) = &mut self.managed {
            let result = job.poll();
            if !matches!(result, Ok(None)) {
                self.finished = true;
            }
            return result;
        }
        if let Some(ticket) = &mut self.ticket {
            let Some(lease) = ticket
                .try_start()
                .map_err(|e| EditError::Invalid(e.to_string()))?
            else {
                return Ok(None);
            };
            self.lease = Some(lease);
            self.ticket = None;
        }
        if let Some(mut command) = self.command.take() {
            match command.spawn() {
                Ok(child) => self.child = Some(child),
                Err(error) => {
                    self.finished = true;
                    self.lease = None;
                    return Err(error.into());
                }
            }
        }
        let Some(status) = self.child.as_mut().expect("started process").try_wait()? else {
            return Ok(None);
        };
        self.finished = true;
        self.child = None;
        self.lease = None; // The process has exited and was reaped; GPU allocations are gone.
        let mut log = String::new();
        File::open(self.directory.path().join("process.log"))?
            .take(16384)
            .read_to_string(&mut log)?;
        if !status.success() {
            return Err(EditError::Invalid(format!("Model tool failed: {log}")));
        }
        let path = self.directory.path().join("output.png");
        Ok(Some(Output {
            raster: if path.exists() {
                Some(Raster::open(path)?)
            } else {
                None
            },
            log,
        }))
    }
    /// Best-effort byte progress for a download.
    pub fn progress(&self) -> Option<(u64, u64)> {
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(self.directory.path().join("progress.json")).ok()?,
        )
        .ok()?;
        Some((value["received"].as_u64()?, value["total"].as_u64()?))
    }
}
impl Drop for ModelJob {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        // lease drops only after termination/reaping; queued ticket cancels on drop.
    }
}
