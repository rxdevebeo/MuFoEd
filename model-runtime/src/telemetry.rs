//! Timestamped host/GPU observations. Unsupported telemetry stays unknown.
use std::{
    process::Command,
    time::{Duration, Instant},
};

/// One physical GPU, identified by a stable driver UUID.
#[derive(Clone, Debug)]
pub struct Gpu {
    /// Persistent device identity.
    pub uuid: String,
    /// Backend ordinal for this observation only.
    pub ordinal: usize,
    /// Physical bytes.
    pub total: u64,
    /// Currently free bytes, including allocations from other processes.
    pub free: u64,
}
/// Physical memory observation; values are bytes, never estimates.
#[derive(Clone, Debug)]
pub struct Observation {
    /// Collection time in this process.
    pub at: Instant,
    /// Total physical host memory.
    pub ram_total: Option<u64>,
    /// Available physical host memory.
    pub ram_available: Option<u64>,
    /// GPUs successfully observed; an empty vector means unknown/unavailable.
    pub gpus: Vec<Gpu>,
    /// Diagnostic when the GPU probe was unavailable.
    pub diagnostic: Option<String>,
}
impl Observation {
    /// Resolve the actual physical identity. Ambiguous multi-GPU automatic selection
    /// is refused; a CUDA UUID visibility binding or explicit UUID is required.
    pub fn gpu(&self, device: &str) -> Option<&Gpu> {
        if device != "auto" {
            return self.gpus.iter().find(|g| g.uuid == device);
        }
        if let Ok(visible) = std::env::var("CUDA_VISIBLE_DEVICES") {
            let first = visible.split(',').next()?.trim();
            if first.starts_with("GPU-") {
                return self.gpus.iter().find(|g| g.uuid == first);
            }
            if first.is_empty() || first == "-1" {
                return None;
            }
        }
        if self.gpus.len() == 1 {
            self.gpus.first()
        } else {
            None
        }
    }
}
/// Injectable read-only telemetry, called on backend workers, never under scheduler locks.
pub trait Telemetry: Send + Sync {
    /// Return an observation, preserving unavailable values as `None`.
    fn observe(&self) -> Observation;
}
/// Local physical RAM plus NVIDIA driver telemetry.
pub struct SystemTelemetry;
impl Telemetry for SystemTelemetry {
    fn observe(&self) -> Observation {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let mut command = Command::new("nvidia-smi");
        command.args([
            "--query-gpu=uuid,index,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        // Bound even a hung driver helper; keep its output off inherited consoles.
        let output = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                let began = Instant::now();
                loop {
                    if child.try_wait()?.is_some() {
                        return child.wait_with_output();
                    }
                    if began.elapsed() > Duration::from_secs(3) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "nvidia-smi timed out",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
        let (gpus, diagnostic) = match output {
            Ok(out) if out.status.success() => {
                (parse_gpus(&String::from_utf8_lossy(&out.stdout)), None)
            }
            Ok(_) => (Vec::new(), Some("NVIDIA telemetry unavailable".into())),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        Observation {
            at: Instant::now(),
            ram_total: (system.total_memory() > 0).then(|| system.total_memory()),
            ram_available: (system.total_memory() > 0).then(|| system.available_memory()),
            gpus,
            diagnostic,
        }
    }
}
fn parse_gpus(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            if fields.len() != 4 || !fields[0].starts_with("GPU-") {
                return None;
            }
            let total = fields[2].parse::<u64>().ok()?.checked_mul(1024 * 1024)?;
            let free = fields[3].parse::<u64>().ok()?.checked_mul(1024 * 1024)?;
            (free <= total).then_some(Gpu {
                uuid: fields[0].into(),
                ordinal: fields[1].parse().ok()?,
                total,
                free,
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uuid_and_units_and_unknown() {
        let g = parse_gpus("GPU-abc, 2, 16384, 8192\nnot supported");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].ordinal, 2);
        assert_eq!(g[0].free, 8 * 1024 * 1024 * 1024);
        assert!(parse_gpus("GPU-a,0,N/A,N/A").is_empty());
    }
}
