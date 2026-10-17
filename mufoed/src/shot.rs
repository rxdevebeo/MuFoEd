//! Looking at the window without a person at it: `MUFOED_DIALOG` names a
//! dialog to open at start, and `MUFOED_SHOT` a PNG the window is saved to
//! after a few frames, before the editor quits.

use std::path::PathBuf;

use eframe::egui;

/// Frames to wait for the window to settle (fonts, layout, measured rows).
const SETTLE_FRAMES: u32 = 30;

pub(crate) struct Shot {
    path: PathBuf,
    frames: u32,
    /// `MUFOED_SHOT_FRAMES`, else [`SETTLE_FRAMES`].
    settle: u32,
}

impl Shot {
    pub(crate) fn from_env() -> Option<Self> {
        let path = std::env::var_os("MUFOED_SHOT")?;
        Some(Self {
            path: PathBuf::from(path),
            frames: 0,
            settle: std::env::var("MUFOED_SHOT_FRAMES")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(SETTLE_FRAMES),
        })
    }

    /// Each frame: asks for the screenshot once settled, saves it when it
    /// comes and closes the window; `true` once closing, unsaved changes
    /// or not.
    pub(crate) fn frame(&mut self, ctx: &egui::Context) -> bool {
        self.frames += 1;
        if self.frames == self.settle {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|input| {
            input.raw.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(error) = save(&image, &self.path) {
                eprintln!("MUFOED_SHOT: {error}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return true;
        }
        ctx.request_repaint();
        false
    }
}

fn save(image: &egui::ColorImage, path: &std::path::Path) -> Result<(), String> {
    let [width, height] = image.size;
    let size = resvg::tiny_skia::IntSize::from_wh(
        u32::try_from(width).map_err(|e| e.to_string())?,
        u32::try_from(height).map_err(|e| e.to_string())?,
    )
    .ok_or("empty screenshot")?;
    let data: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(egui::Color32::to_array)
        .collect();
    let pixmap = resvg::tiny_skia::Pixmap::from_vec(data, size).ok_or("bad screenshot")?;
    pixmap.save_png(path).map_err(|e| e.to_string())
}

/// The dialog `MUFOED_DIALOG` names.
pub(crate) fn dialog() -> Option<String> {
    std::env::var("MUFOED_DIALOG")
        .ok()
        .filter(|name| !name.is_empty())
}
