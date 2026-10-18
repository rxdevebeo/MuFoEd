//! Explicit opt-in conversion of opaque graphics by an embedding application.
//! The importer itself never starts processes. The optional Tectonic adapter
//! requires caller-selected executable paths and never enables shell escape.
use crate::{LatexError, SourceProvider};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A graphic needing a real renderer rather than text substitution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicKind {
    /// A complete TikZ environment or inline command.
    Tikz,
    /// A chemfig command.
    Chemfig,
    /// An image container unsupported by the native importer (for example PDF).
    Image,
}

/// Borrowed conversion request. Implementations must enforce their own CPU,
/// filesystem and network policy. The importer bounds input and returned bytes.
pub struct RenderRequest<'a> {
    /// Graphic kind.
    pub kind: GraphicKind,
    /// Logical source name.
    pub name: &'a str,
    /// TeX graphic source, empty for an image.
    pub tex: &'a str,
    /// Original document preamble with the document class removed.
    pub preamble: &'a str,
    /// Original image bytes, empty for a TeX graphic.
    pub bytes: &'a [u8],
    /// Maximum accepted output bytes.
    pub max_output_bytes: usize,
}

/// Rendered image to embed using the existing picture model.
pub struct RenderedGraphic {
    /// Name with a supported extension; container signatures are still checked.
    pub name: String,
    /// Encoded image bytes.
    pub bytes: Vec<u8>,
}

/// Optional conversion service; returning None leaves an explicit import loss.
pub trait GraphicRenderer {
    /// Converts a graphic without changing its mathematical or diagram content.
    fn render(&self, request: &RenderRequest<'_>) -> Result<Option<RenderedGraphic>, LatexError>;
}

/// Adds graphic conversion to any existing source provider.
pub struct RenderingSource<S, R> {
    /// Source file provider.
    pub source: S,
    /// Explicitly configured conversion service.
    pub renderer: R,
}

impl<S: SourceProvider, R: GraphicRenderer> SourceProvider for RenderingSource<S, R> {
    fn read(&self, name: &str) -> Result<Vec<u8>, LatexError> {
        self.source.read(name)
    }
    fn render_graphic(
        &self,
        request: &RenderRequest<'_>,
    ) -> Result<Option<RenderedGraphic>, LatexError> {
        self.renderer.render(request)
    }
}

/// Opt-in Tectonic + Poppler adapter. Executables and TeX packages are not
/// bundled or installed by this crate. By default Tectonic uses only its cache.
/// Callers enable network package resolution explicitly via `only_cached`.
pub struct TectonicRenderer {
    /// Absolute path to Tectonic.
    pub tectonic: PathBuf,
    /// Absolute path to Poppler's pdftoppm.
    pub pdftoppm: PathBuf,
    /// Directory for isolated, uniquely named conversion jobs.
    pub temporary_root: PathBuf,
    /// Maximum duration of each subprocess.
    pub timeout: Duration,
    /// Use only locally cached TeX resources (recommended for normal imports).
    pub only_cached: bool,
}

impl GraphicRenderer for TectonicRenderer {
    fn render(&self, request: &RenderRequest<'_>) -> Result<Option<RenderedGraphic>, LatexError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = self.temporary_root.join(format!(
            "graphic-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&self.temporary_root).map_err(|e| io("graphic-directory", e))?;
        std::fs::create_dir(&root).map_err(|e| io("graphic-directory", e))?;
        let root = std::fs::canonicalize(root).map_err(|e| io("graphic-directory", e))?;
        let result = (|| {
            if request.kind == GraphicKind::Image {
                if !request.bytes.starts_with(b"%PDF-") {
                    return Ok(None);
                }
                std::fs::write(root.join("graphic.pdf"), request.bytes)
                    .map_err(|e| io("graphic.pdf", e))?;
            } else {
                let package = match request.kind {
                    GraphicKind::Tikz => "tikz",
                    GraphicKind::Chemfig => "chemfig",
                    GraphicKind::Image => unreachable!(),
                };
                let source = format!("\\documentclass[border=2pt]{{standalone}}\n{}\n\\usepackage{{{package}}}\n\\begin{{document}}\n{}\n\\end{{document}}\n", request.preamble, request.tex);
                std::fs::write(root.join("graphic.tex"), source)
                    .map_err(|e| io("graphic.tex", e))?;
                let mut command = Command::new(&self.tectonic);
                command
                    .current_dir(&root)
                    .args(["--untrusted", "--keep-logs"]);
                if self.only_cached {
                    command.arg("--only-cached");
                }
                command.arg("graphic.tex");
                if !run(&mut command, self.timeout)? {
                    return Ok(None);
                }
            }
            let mut command = Command::new(&self.pdftoppm);
            command.current_dir(&root).args([
                "-f",
                "1",
                "-singlefile",
                "-scale-to",
                "4096",
                "-png",
                "graphic.pdf",
                "graphic",
            ]);
            if !run(&mut command, self.timeout)? {
                return Ok(None);
            }
            let path = root.join("graphic.png");
            let size = std::fs::metadata(&path)
                .map_err(|e| io("graphic.png", e))?
                .len();
            if size > request.max_output_bytes as u64 {
                return Err(LatexError::LimitExceeded {
                    limit: "media_part_bytes",
                    observed: usize::try_from(size).unwrap_or(usize::MAX),
                });
            }
            let bytes = std::fs::read(path).map_err(|e| io("graphic.png", e))?;
            Ok(Some(RenderedGraphic {
                name: "graphic.png".into(),
                bytes,
            }))
        })();
        // Only the unique directory created by this invocation is removed.
        let _ = std::fs::remove_dir_all(root);
        result
    }
}

fn run(command: &mut Command, timeout: Duration) -> Result<bool, LatexError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| io("graphic-process", e))?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| io("graphic-process", e))? {
            return Ok(status.success());
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn io(name: &str, source: std::io::Error) -> LatexError {
    LatexError::Io {
        name: name.into(),
        source,
    }
}
