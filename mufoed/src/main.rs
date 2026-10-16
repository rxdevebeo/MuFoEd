//! MultiFormatEditor (MuFoEd): a desktop demonstrator of the strict-ooxml
//! toolkit.
//!
//! One window, one tab per document (as in WPS Office), a Word-style ribbon.
//! Documents are opened from DOCX (Strict and Transitional), RTF, PDF and
//! DjVu - the last two converted, scanned pages recognized by a local Ollama
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

mod app;
mod canvas;
mod docfonts;
mod formats;
mod i18n;
mod icons;
mod nav;
mod raster;
mod ribbon;
mod settings;
mod stack;
mod tab;
mod theme;
mod trace;
mod worker;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let mut options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("MultiFormatEditor")
            .with_app_id("mufoed")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(window_icon()),
        ..eframe::NativeOptions::default()
    };
    restrict_backends(&mut options);
    eframe::run_native(
        "MultiFormatEditor",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, files)))),
    )
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

/// A blue rounded square with a white page, drawn at start-up.
fn window_icon() -> eframe::egui::IconData {
    const SIZE: usize = 64;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let corner = |cx: usize, cy: usize| {
                let dx = x as f32 - cx as f32;
                let dy = y as f32 - cy as f32;
                dx * dx + dy * dy > 100.0
            };
            let outside = (x < 10 && y < 10 && corner(10, 10))
                || (x > 53 && y < 10 && corner(53, 10))
                || (x < 10 && y > 53 && corner(10, 53))
                || (x > 53 && y > 53 && corner(53, 53));
            let page = (18..46).contains(&x) && (12..52).contains(&y);
            let line = page && (20..44).contains(&x) && [22, 28, 34, 40].contains(&y);
            let pixel = if outside {
                [0, 0, 0, 0]
            } else if line {
                [43, 87, 154, 255]
            } else if page {
                [255, 255, 255, 255]
            } else {
                [43, 87, 154, 255]
            };
            let offset = (y * SIZE + x) * 4;
            if let Some(slot) = rgba.get_mut(offset..offset + 4) {
                slot.copy_from_slice(&pixel);
            }
        }
    }
    eframe::egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}
