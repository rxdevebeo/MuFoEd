//! A small window that hosts the editor the way `MuFoEd` will: egui, a grey
//! desk, a white sheet, and the same blue as the editor's ribbon.

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke, Vec2};
use mathml_editor::{MathEditor, Text};

const ACCENT: Color32 = Color32::from_rgb(43, 87, 154);

fn main() -> eframe::Result {
    let text = Text::for_code(&system_code());
    let title = text.title.clone();
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_title(title),
        ..Default::default()
    };
    restrict_backends(&mut options);
    eframe::run_native(
        "mathml-editor",
        options,
        Box::new(move |cc| Ok(Box::new(Demo::new(&cc.egui_ctx, text)))),
    )
}

struct Demo {
    editor: MathEditor,
}

impl Demo {
    fn new(ctx: &egui::Context, text: Text) -> Self {
        office_look(ctx);
        let mut editor = MathEditor::from_mathml(QUADRATIC).unwrap_or_else(|_| MathEditor::new());
        editor.set_text(text);
        editor.set_show_source(true);
        Self { editor }
    }
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let desk = egui::Frame::new()
            .fill(Color32::from_rgb(230, 230, 230))
            .inner_margin(Margin::same(16));
        egui::CentralPanel::default().frame(desk).show(ui, |ui| {
            ui.label(
                egui::RichText::new(&self.editor.text().heading)
                    .size(18.0)
                    .strong()
                    .color(ACCENT),
            );
            ui.add_space(8.0);
            self.editor.show(ui);
        });
    }
}

/// On Windows, draw with Direct3D 12 only.
///
/// The same guard as `MuFoEd`: wgpu's default set includes OpenGL, and
/// initializing it leaves hidden windows on threads that never pump messages.
/// A layout change from Punto Switcher (or any `WM_INPUTLANGCHANGEREQUEST`)
/// makes Windows notify every GUI thread of the process synchronously, so the
/// window's thread waits on those forever and the process hangs. `WGPU_BACKEND`
/// still chooses another backend when set.
fn restrict_backends(options: &mut eframe::NativeOptions) {
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        use eframe::egui_wgpu::wgpu::Backends;
        setup.instance_descriptor.backends = Backends::from_env().unwrap_or(Backends::DX12);
    }
    #[cfg(not(windows))]
    let _ = options;
}

fn system_code() -> String {
    sys_locale::get_locale()
        .and_then(|locale| locale.split(['-', '_']).next().map(str::to_owned))
        .unwrap_or_else(|| "en".to_owned())
}

fn office_look(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = ACCENT;
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.panel_fill = Color32::from_rgb(250, 250, 250);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(226, 232, 242);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(226, 232, 242);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(205, 222, 245);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        style.spacing.button_padding = Vec2::new(6.0, 3.0);
    });
}

const QUADRATIC: &str = concat!(
    "<math xmlns=\"http://www.w3.org/1998/Math/MathML\">",
    "<mrow><mi>x</mi><mo>=</mo><mfrac><mrow><mo>-</mo><mi>b</mi><mo>±</mo>",
    "<msqrt><mrow><msup><mi>b</mi><mn>2</mn></msup><mo>-</mo><mn>4</mn>",
    "<mi>a</mi><mi>c</mi></mrow></msqrt></mrow><mrow><mn>2</mn><mi>a</mi></mrow>",
    "</mfrac></mrow></math>"
);
