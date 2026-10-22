//! MultiFormatEditor (MuFoEd): a desktop demonstrator of the strict-ooxml
//! toolkit.
//!
//! One window, one tab per document (as in WPS Office), a Word-style ribbon.
//! Documents are opened from DOCX (Strict and Transitional), RTF, HTML, LaTeX,
//! PDF and DjVu - the last two converted, scanned pages recognized by a local Ollama
//! vision model - shown as the toolkit renders them, edited through its
//! transactional editor, and saved as Strict DOCX or exported to PDF, SVG and
//! plain text. Interface strings come from JSON files (`locales/`).
//!
//! ```text
//! mufoed [FILE...]
//! ```
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Pixel arithmetic converts between f32 and f64 and to sizes everywhere; the
// values are bounded screen coordinates.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    // Interface state is a set of independent switches.
    clippy::struct_excessive_bools,
    // Geometry reads as x, y, w, h.
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::doc_markdown
)]

mod ai;
mod app;
mod bootstrap;
mod branding;
mod browsers;
mod canvas;
mod chat;
mod cleanup;
mod clipboard;
mod context_window;
mod dialog;
mod docfonts;
mod document_preview;
mod file_tabs;
mod formats;
mod formula;
mod i18n;
mod image_studio;
// Shared host state: tabs, atomic preferences and recovery are identical in both applications.
// Standalone-only tab-strip helpers are replaced by the shared MuFoEd strip.
#[allow(dead_code)]
#[path = "../../image-editor/src/document_tabs.rs"]
mod document_tabs;
mod icons;
mod model_runtime;
mod nav;
mod paragraph;
#[path = "../../image-editor/src/preferences.rs"]
mod preferences;
mod print;
mod raster;
// Screenshot-only helpers are used by the standalone host.
#[allow(dead_code)]
#[path = "../../image-editor/src/recovery_host.rs"]
mod recovery_host;
mod ribbon;
mod ruler;
mod settings;
mod shot;
mod spell;
mod stack;
mod tab;
mod theme;
mod trace;
mod worker;

use std::path::PathBuf;

fn main() -> eframe::Result {
    bootstrap::initialize();
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if files.first().is_some_and(|arg| arg == "--check-runtime") {
        println!("{}", bootstrap::diagnostics());
        return Ok(());
    }
    let mut options = eframe::NativeOptions {
        viewport: window_viewport(),
        // The size lives in the settings file, so the options window can turn
        // remembering it off. eframe's own copy would ignore that switch.
        persist_window: false,
        ..eframe::NativeOptions::default()
    };
    restrict_backends(&mut options);
    eframe::run_native(
        "MultiFormatEditor",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, files)))),
    )
}

/// The window: a remembered place when the options ask for one, otherwise
/// the ordinary size.
fn window_viewport() -> eframe::egui::ViewportBuilder {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("MultiFormatEditor")
        .with_app_id("mufoed")
        .with_inner_size([settings::DEFAULT_WIDTH, settings::DEFAULT_HEIGHT])
        .with_min_inner_size([720.0, 480.0])
        .with_drag_and_drop(true)
        .with_icon(branding::window_icon());
    let remembered = settings::Settings::load();
    if !remembered.remember_window {
        return viewport;
    }
    let Some(window) = remembered.window else {
        return viewport;
    };
    if window.width.is_finite()
        && window.height.is_finite()
        && window.width >= 720.0
        && window.height >= 480.0
        && window.x.is_finite()
        && window.y.is_finite()
    {
        viewport = viewport
            .with_inner_size([window.width, window.height])
            .with_position(eframe::egui::pos2(window.x, window.y));
        if window.maximized {
            viewport = viewport.with_maximized(true);
        }
    }
    viewport
}

/// On Windows, draw with Direct3D 12 only.
///
/// wgpu's default set includes OpenGL, and initializing it (with the NVIDIA
/// driver, at least) leaves hidden windows owned by threads that never pump
/// messages. A keyboard-layout change - Punto Switcher's automatic one, or any
/// other program's `WM_INPUTLANGCHANGEREQUEST` - makes the system notify every
/// GUI thread of the process synchronously, so the window's own thread waited
/// on those forever and the window froze. Without OpenGL there are no such
/// threads. `WGPU_BACKEND` still chooses another backend when set.
fn restrict_backends(options: &mut eframe::NativeOptions) {
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        use eframe::egui_wgpu::wgpu::Backends;
        setup.instance_descriptor.backends = Backends::from_env().unwrap_or(Backends::DX12);
    }
    #[cfg(not(windows))]
    let _ = options;
}
