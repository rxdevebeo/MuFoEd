//! Render in a disposable process, containing OOM and enforcing a real timeout.

use crate::process_limit::{self, Memory};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use strict_ooxml_view::{failed, render_controlled, DocumentView, Entry, RenderSettings};

pub(crate) const MEMORY_LIMIT: usize = 2 * 1024 * 1024 * 1024;
const MAX_RESULT_BYTES: u64 = 96 * 1024 * 1024;
static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize, Serialize)]
struct Request {
    entry: Entry,
    transitional: bool,
    settings: RenderSettings,
    timeout_ms: u64,
}

pub(crate) fn log(value: &serde_json::Value) {
    let path = std::env::var_os("STRICT_VIEW_LOG").map_or_else(
        || std::path::PathBuf::from("render_out/viewer-metrics.jsonl"),
        std::path::PathBuf::from,
    );
    let result = (|| -> io::Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_data()
    })();
    if let Err(error) = result {
        eprintln!("metrics log failed: {error}");
    }
}

pub(crate) fn run(
    entry: &Entry,
    transitional: bool,
    settings: RenderSettings,
    timeout: Duration,
) -> DocumentView {
    let started = Instant::now();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = format!(
        "{}-{timestamp}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    log(
        &serde_json::json!({"event":"render.begin","request":id,"document":entry.name,"path":entry.path,"input_bytes":entry.size,"time_unix_ms":timestamp,"memory_limit_bytes":MEMORY_LIMIT,"timeout_ms":timeout.as_millis(),"hard_commit_limit":cfg!(windows)}),
    );
    let directory = tempfile::Builder::new().prefix("strict-view-").tempdir();
    let mut peak = Memory::default();
    let result = directory
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|directory| {
            execute(
                entry,
                transitional,
                settings,
                timeout,
                &directory.path().join("metadata.json"),
                &directory.path().join("worker.stderr"),
                &id,
                &mut peak,
            )
        });
    let view = match result {
        Ok(mut view) => {
            view.page_files = directory.ok().map(std::sync::Arc::new);
            view
        }
        Err(reason) => failed(entry, &reason),
    };
    let svg_bytes = view.svg_bytes();
    let elapsed_ms = started.elapsed().as_millis();
    log(
        &serde_json::json!({"event":"render.end","request":id,"document":entry.name,"elapsed_ms":elapsed_ms,"pages":view.pages.len(),"svg_bytes":svg_bytes,"memory":peak,"outcome":view.pipeline.outcome,"note":view.note,"extreme":elapsed_ms>5000 || svg_bytes>8*1024*1024 || peak.rss_bytes>512*1024*1024 || peak.private_bytes>1024*1024*1024}),
    );
    view
}

#[allow(clippy::too_many_arguments)]
fn execute(
    entry: &Entry,
    transitional: bool,
    settings: RenderSettings,
    timeout: Duration,
    path: &std::path::Path,
    stderr_path: &std::path::Path,
    id: &str,
    peak: &mut Memory,
) -> Result<DocumentView, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let stderr = File::create(stderr_path).map_err(|error| error.to_string())?;
    let mut command = Command::new(executable);
    command
        .arg("--render-worker")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(stderr);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let child = command.spawn().map_err(|error| error.to_string())?;
    let mut child = OwnedChild(child);
    let _limit = process_limit::Limit::attach(&child.0, MEMORY_LIMIT)
        .map_err(|error| format!("could not establish render memory limit: {error}"))?;
    // Sending the request is the start gate: no file parsing happens before assignment.
    let mut input = child.0.stdin.take().ok_or("worker stdin unavailable")?;
    let request = Request {
        entry: entry.clone(),
        transitional,
        settings,
        timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
    };
    serde_json::to_writer(&mut input, &request).map_err(|error| error.to_string())?;
    drop(input);
    let deadline = Instant::now();
    let mut warned = false;
    let status = loop {
        if let Ok(memory) = process_limit::memory(&child.0) {
            peak.rss_bytes = peak.rss_bytes.max(memory.rss_bytes);
            peak.private_bytes = peak.private_bytes.max(memory.private_bytes);
            peak.peak_rss_bytes = peak.peak_rss_bytes.max(memory.peak_rss_bytes);
            if !warned
                && (memory.rss_bytes > 512 * 1024 * 1024
                    || memory.private_bytes > 1024 * 1024 * 1024)
            {
                warned = true;
                log(
                    &serde_json::json!({"event":"render.extreme-memory","request":id,"document":entry.name,"memory":memory}),
                );
            }
            if memory.rss_bytes > MEMORY_LIMIT {
                return Err("render worker exceeded 2 GiB RSS; process terminated".to_owned());
            }
        }
        if let Some(status) = child.0.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if deadline.elapsed() > timeout {
            return Err(format!(
                "rendering did not finish within {} s; worker process terminated",
                timeout.as_secs()
            ));
        }
        if fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_RESULT_BYTES) {
            return Err("render result exceeded 96 MiB; worker process terminated".to_owned());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let mut error = String::new();
        if let Ok(file) = File::open(stderr_path) {
            let _ = file.take(4096).read_to_string(&mut error);
        }
        return Err(format!(
            "render worker exited with {status}; memory limit is 2 GiB. {}",
            error.chars().take(1000).collect::<String>()
        ));
    }
    let file = File::open(path).map_err(|error| format!("worker result unavailable: {error}"))?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_RESULT_BYTES {
        return Err("render result exceeded 96 MiB".to_owned());
    }
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("invalid worker result: {error}"))
}
struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) fn entry(path: &std::path::Path) -> Result<(), String> {
    let request: Request =
        serde_json::from_reader(io::stdin().lock()).map_err(|error| error.to_string())?;
    let control =
        strict_ooxml::OpenControl::with_timeout(Duration::from_millis(request.timeout_ms));
    let thread = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            render_controlled(
                &request.entry,
                request.transitional,
                request.settings,
                &control,
            )
            .map_err(|error| error.to_string())
        })
        .map_err(|error| error.to_string())?;
    let mut view = thread
        .join()
        .map_err(|_| "render thread panicked".to_owned())??;
    let directory = path.parent().ok_or("worker output directory unavailable")?;
    for page in &mut view.pages {
        fs::write(directory.join(format!("{}.svg", page.number)), &page.svg)
            .map_err(|error| format!("could not store page {}: {error}", page.number))?;
        page.svg = String::new();
    }
    let file = File::create(path).map_err(|error| error.to_string())?;
    let mut output = io::BufWriter::new(file);
    serde_json::to_writer(&mut output, &view).map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())
}
