//! The 3D model panel: a viewport with an orbiting camera, display modes,
//! the animation timeline and the model's statistics.

// Viewport sizes are small positive point values.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use egui::{pos2, vec2, Color32, Rect, Sense, TextureHandle};
use glam::{Vec2, Vec3};

use super::Gallery;
use crate::model3d::{self, Camera, Options, Scene, Shading, UpAxis, View};
use crate::Locale;

/// Frames per second of the frame-stepping buttons and the frame counter.
const FPS: f32 = 30.0;
/// Playback speeds offered.
const SPEEDS: [f32; 8] = [0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];

enum Load {
    Loading(Receiver<Result<Scene, String>>),
    Ready(Arc<Scene>),
    Failed(String),
}

/// What the last picture was drawn from; a change asks for a new one.
#[derive(Clone, Copy, PartialEq)]
struct Key {
    camera: Camera,
    options: Options,
    clip: Option<usize>,
    time: f32,
    size: [u32; 2],
    up: UpAxis,
}

/// The state of the model panel for one file.
pub(super) struct ModelView {
    pub(super) path: PathBuf,
    load: Load,
    camera: Camera,
    framed: bool,
    options: Options,
    clip: Option<usize>,
    time: f32,
    playing: bool,
    speed: f32,
    looping: bool,
    turntable: bool,
    texture: Option<TextureHandle>,
    drawn: Option<Key>,
    /// Bounds of the rest pose, for the statistics.
    bounds: Option<(Vec3, Vec3)>,
}

impl ModelView {
    /// Starts loading `path` in the background.
    pub(super) fn open(path: &Path, ctx: &egui::Context) -> Self {
        let (sender, receiver) = channel();
        let file = path.to_path_buf();
        let wake = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("image-gallery-model".into())
            .spawn(move || {
                let result = model3d::load(&file).map_err(|error| error.to_string());
                let _ = sender.send(result);
                wake.request_repaint();
            });
        Self {
            path: path.to_path_buf(),
            load: Load::Loading(receiver),
            camera: Camera::default(),
            framed: false,
            options: Options::default(),
            clip: None,
            time: 0.0,
            playing: false,
            speed: 1.0,
            looping: true,
            turntable: false,
            texture: None,
            drawn: None,
            bounds: None,
        }
    }

    fn poll(&mut self) {
        if let Load::Loading(receiver) = &self.load {
            if let Ok(result) = receiver.try_recv() {
                self.load = match result {
                    Ok(scene) => {
                        self.bounds = scene.geometry(&scene.pose(None, 0.0)).bounds();
                        self.clip = (!scene.animations.is_empty()).then_some(0);
                        self.playing = self.clip.is_some();
                        Load::Ready(Arc::new(scene))
                    }
                    Err(error) => Load::Failed(error),
                };
            }
        }
    }

    fn duration(&self, scene: &Scene) -> f32 {
        self.clip
            .and_then(|clip| scene.animations.get(clip))
            .map_or(0.0, |animation| animation.duration)
    }

    /// Frames the model's current pose.
    fn frame_model(&mut self, scene: &Scene) {
        let geometry = scene.geometry(&scene.pose(self.clip, self.time));
        self.camera.fit(geometry.bounds());
        self.framed = true;
    }
}

impl Gallery {
    /// Shows the model panel for `path`, opening it if needed.
    pub(super) fn model_ui(&mut self, ui: &mut egui::Ui, path: &Path, locale: &Locale) {
        if self.model.as_ref().is_none_or(|model| model.path != path) {
            self.model = Some(ModelView::open(path, ui.ctx()));
        }
        let Some(view) = self.model.as_mut() else {
            return;
        };
        view.poll();
        let scene = match &view.load {
            Load::Loading(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(locale.text("Loading the model…"));
                });
                return;
            }
            Load::Failed(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                return;
            }
            Load::Ready(scene) => Arc::clone(scene),
        };
        if !view.framed {
            view.frame_model(&scene);
        }
        toolbar(ui, view, &scene, locale);
        viewport(ui, view, &scene, locale);
        display_options(ui, view, locale);
        if !scene.animations.is_empty() {
            timeline(ui, view, &scene, locale);
        }
        statistics(ui, &scene, view.bounds, locale);
    }
}

fn toolbar(ui: &mut egui::Ui, view: &mut ModelView, scene: &Scene, locale: &Locale) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .small_button(locale.text("Fit"))
            .on_hover_text(locale.text("Frame the whole model (double-click)"))
            .clicked()
        {
            view.frame_model(scene);
        }
        for (name, standard) in [
            ("Front", View::Front),
            ("Back", View::Back),
            ("Left", View::Left),
            ("Right", View::Right),
            ("Top", View::Top),
            ("Bottom", View::Bottom),
            ("3/4", View::Isometric),
        ] {
            if ui.small_button(locale.text(name)).clicked() {
                view.camera.look(standard);
                view.frame_model(scene);
            }
        }
        ui.separator();
        let mut orthographic = view.camera.orthographic;
        if ui
            .selectable_label(orthographic, locale.text("Orthographic"))
            .on_hover_text(locale.text("Parallel projection, without perspective"))
            .clicked()
        {
            orthographic = !orthographic;
            view.camera.orthographic = orthographic;
        }
    });
}

fn viewport(ui: &mut egui::Ui, view: &mut ModelView, scene: &Arc<Scene>, locale: &Locale) {
    let width = ui.available_width().max(64.0);
    let height = (width * 0.8).clamp(120.0, 900.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let modifiers = ui.input(|input| input.modifiers);
    let dt = ui.input(|input| input.stable_dt).min(0.1);
    let mut interacting = false;
    if response.dragged_by(egui::PointerButton::Primary) && !modifiers.shift {
        let delta = response.drag_delta();
        view.camera.orbit(Vec2::new(delta.x, delta.y));
        interacting = true;
    }
    if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
        || (response.dragged_by(egui::PointerButton::Primary) && modifiers.shift)
    {
        let delta = response.drag_delta();
        view.camera.pan(Vec2::new(delta.x, delta.y), height);
        interacting = true;
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        let pinch = ui.input(egui::InputState::zoom_delta);
        if scroll.abs() > 0.0 || (pinch - 1.0).abs() > 1e-4 {
            view.camera.zoom((scroll * 0.002).exp() * pinch);
            interacting = true;
        }
    }
    if response.double_clicked() {
        view.frame_model(scene);
    }
    // Animation and turntable advance with the clock.
    let duration = view.duration(scene);
    if view.playing && duration > 0.0 {
        view.time += dt * view.speed;
        if view.time > duration {
            if view.looping {
                view.time %= duration;
            } else {
                view.time = duration;
                view.playing = false;
            }
        }
        ui.ctx().request_repaint();
        interacting = true;
    }
    if view.turntable {
        view.camera.orbit(Vec2::new(-dt * 40.0, 0.0));
        ui.ctx().request_repaint();
        interacting = true;
    }
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        ((width * ppp) as u32).clamp(16, 1600),
        ((height * ppp) as u32).clamp(16, 1600),
    ];
    // Smooth edges when still; one sample a pixel while moving.
    let mut options = view.options;
    options.supersample = if interacting { 1 } else { 2 };
    let key = Key {
        camera: view.camera,
        options,
        clip: view.clip,
        time: view.time,
        size,
        up: scene.up,
    };
    if view.drawn != Some(key) || view.texture.is_none() {
        let geometry = scene.geometry(&scene.pose(view.clip, view.time));
        let picture = model3d::render::render(
            &geometry,
            &scene.materials,
            &scene.textures,
            &view.camera,
            &options,
            size[0],
            size[1],
        );
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [picture.width() as usize, picture.height() as usize],
            picture.as_raw(),
        );
        match &mut view.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                view.texture = Some(ui.ctx().load_texture(
                    "image-gallery-model",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        view.drawn = Some(key);
        if !interacting {
            // The supersampled picture follows on the next quiet frame.
            ui.ctx().request_repaint();
        }
    }
    if let Some(texture) = &view.texture {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    }
    response.on_hover_text_at_pointer(
        locale.text("Drag: turn · Right or Shift-drag: move · Wheel: zoom · Double-click: fit"),
    );
}

fn display_options(ui: &mut egui::Ui, view: &mut ModelView, locale: &Locale) {
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("image-gallery-model-shading")
            .width(110.0)
            .selected_text(locale.text(shading_name(view.options.shading)))
            .show_ui(ui, |ui| {
                for shading in [
                    Shading::Smooth,
                    Shading::Flat,
                    Shading::Unlit,
                    Shading::Normals,
                    Shading::Hidden,
                ] {
                    ui.selectable_value(
                        &mut view.options.shading,
                        shading,
                        locale.text(shading_name(shading)),
                    );
                }
            });
        ui.checkbox(&mut view.options.ground, locale.text("Ground"))
            .on_hover_text(locale.text("The plane at level 0 with a grid"));
        ui.checkbox(&mut view.options.wireframe, locale.text("Wireframe"));
        ui.checkbox(&mut view.options.textures, locale.text("Textures"));
        ui.checkbox(
            &mut view.options.vertex_colors,
            locale.text("Vertex colours"),
        );
        ui.checkbox(&mut view.options.axes, locale.text("Axes"));
        ui.checkbox(&mut view.options.bounds, locale.text("Bounds"));
        ui.checkbox(
            &mut view.options.backface_culling,
            locale.text("Hide back faces"),
        );
        ui.checkbox(&mut view.turntable, locale.text("Turntable"));
        let mut z_up = matches!(&view.load, Load::Ready(scene) if scene.up == UpAxis::Z);
        if ui
            .checkbox(&mut z_up, locale.text("Z up"))
            .on_hover_text(locale.text("The file's vertical axis is Z (CAD, 3D printing)"))
            .changed()
        {
            if let Load::Ready(scene) = &mut view.load {
                Arc::make_mut(scene).up = if z_up { UpAxis::Z } else { UpAxis::Y };
                let scene = Arc::clone(scene);
                view.frame_model(&scene);
            }
        }
        let mut background = view.options.background;
        if ui
            .color_edit_button_srgba_unmultiplied(&mut background)
            .changed()
        {
            view.options.background = background;
        }
    });
}

fn shading_name(shading: Shading) -> &'static str {
    match shading {
        Shading::Smooth => "Smooth",
        Shading::Flat => "Flat",
        Shading::Unlit => "Unlit",
        Shading::Normals => "Normals",
        Shading::Hidden => "No surfaces",
    }
}

fn timeline(ui: &mut egui::Ui, view: &mut ModelView, scene: &Scene, locale: &Locale) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(locale.text("Animation"));
        let name = |index: usize| {
            scene
                .animations
                .get(index)
                .map_or_else(String::new, |clip| clip.name.clone())
        };
        egui::ComboBox::from_id_salt("image-gallery-model-clip")
            .width(160.0)
            .selected_text(
                view.clip
                    .map_or_else(|| locale.text("Rest pose").to_owned(), name),
            )
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(view.clip.is_none(), locale.text("Rest pose"))
                    .clicked()
                {
                    view.clip = None;
                    view.time = 0.0;
                    view.playing = false;
                }
                for index in 0..scene.animations.len() {
                    if ui
                        .selectable_label(view.clip == Some(index), name(index))
                        .clicked()
                    {
                        view.clip = Some(index);
                        view.time = 0.0;
                    }
                }
            });
    });
    let duration = view.duration(scene);
    if view.clip.is_none() || duration <= 0.0 {
        return;
    }
    let step = 1.0 / FPS;
    ui.horizontal(|ui| {
        if ui
            .small_button("⏮")
            .on_hover_text(locale.text("To the start"))
            .clicked()
        {
            view.time = 0.0;
        }
        if ui
            .small_button("◀|")
            .on_hover_text(locale.text("Previous frame"))
            .clicked()
        {
            view.playing = false;
            view.time = (view.time - step).max(0.0);
        }
        let label = if view.playing { "⏸" } else { "▶" };
        let hint = if view.playing { "Pause" } else { "Play" };
        if ui.button(label).on_hover_text(locale.text(hint)).clicked() {
            if !view.playing && view.time >= duration {
                view.time = 0.0;
            }
            view.playing = !view.playing;
        }
        if ui
            .small_button("|▶")
            .on_hover_text(locale.text("Next frame"))
            .clicked()
        {
            view.playing = false;
            view.time = (view.time + step).min(duration);
        }
        if ui
            .small_button("⏭")
            .on_hover_text(locale.text("To the end"))
            .clicked()
        {
            view.playing = false;
            view.time = duration;
        }
        ui.checkbox(&mut view.looping, locale.text("Loop"));
    });
    let slider = egui::Slider::new(&mut view.time, 0.0..=duration)
        .show_value(false)
        .clamping(egui::SliderClamping::Always);
    let width = ui.available_width();
    ui.spacing_mut().slider_width = width - 8.0;
    if ui.add(slider).dragged() {
        view.playing = false;
    }
    ui.horizontal(|ui| {
        let frame = (view.time * FPS).round() as u32;
        let frames = (duration * FPS).round() as u32;
        ui.monospace(format!(
            "{:.2} / {:.2} s · {frame} / {frames}",
            view.time, duration
        ));
        ui.label(locale.text("Speed"));
        egui::ComboBox::from_id_salt("image-gallery-model-speed")
            .width(60.0)
            .selected_text(format!("{}×", view.speed))
            .show_ui(ui, |ui| {
                for speed in SPEEDS {
                    ui.selectable_value(&mut view.speed, speed, format!("{speed}×"));
                }
            });
    });
}

fn statistics(ui: &mut egui::Ui, scene: &Scene, bounds: Option<(Vec3, Vec3)>, locale: &Locale) {
    let stats = scene.stats();
    ui.separator();
    egui::Grid::new("image-gallery-model-stats")
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String| {
                ui.weak(locale.text(label));
                ui.label(value);
                ui.end_row();
            };
            row("Triangles", stats.triangles.to_string());
            row("Vertices", stats.vertices.to_string());
            row("Nodes", stats.nodes.to_string());
            row("Materials", stats.materials.to_string());
            row("Textures", stats.textures.to_string());
            if stats.skins > 0 {
                row("Skins", stats.skins.to_string());
            }
            row("Animations", stats.animations.to_string());
            if let Some((low, high)) = bounds {
                let size: Vec3 = high - low;
                row(
                    "Size",
                    format!("{:.3} × {:.3} × {:.3}", size.x, size.y, size.z),
                );
            }
        });
}
