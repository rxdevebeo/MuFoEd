//! The application artwork supplied by the owner, embedded in every build.
use eframe::egui;

pub(crate) fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/mascot_icon_256x256.png"))
        .expect("bundled application icon is a valid PNG")
}

pub(crate) fn paint(ui: &egui::Ui, rect: egui::Rect) {
    let id = egui::Id::new("mufoed-brand-icon");
    let texture = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(id))
        .unwrap_or_else(|| {
            let icon = eframe::icon_data::from_png_bytes(include_bytes!(
                "../assets/mascot_icon_32x32.png"
            ))
            .expect("bundled small application icon is a valid PNG");
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            let texture =
                ui.ctx()
                    .load_texture("mufoed-brand-icon", pixels, egui::TextureOptions::LINEAR);
            ui.ctx()
                .data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        });
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}
