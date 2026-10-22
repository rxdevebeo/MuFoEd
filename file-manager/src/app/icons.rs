//! Small vector file icons, independent of OS icon handlers and installed fonts.
use crate::entry::Entry;
use egui::{pos2, vec2, Color32, FontId, Painter, Rect, Stroke};

pub(super) fn paint(p: &Painter, rect: Rect, entry: &Entry, up: bool, foreground: Color32) {
    let x = rect.left();
    let y = rect.top();
    let line = Stroke::new(1.2, foreground);
    if up {
        p.line_segment([pos2(x + 8.0, y + 14.0), pos2(x + 8.0, y + 3.0)], line);
        p.line_segment([pos2(x + 3.0, y + 8.0), pos2(x + 8.0, y + 3.0)], line);
        p.line_segment([pos2(x + 13.0, y + 8.0), pos2(x + 8.0, y + 3.0)], line);
        return;
    }
    if entry.is_dir {
        let yellow = Color32::from_rgb(232, 177, 45);
        p.rect_filled(
            Rect::from_min_size(pos2(x + 1.0, y + 2.0), vec2(7.0, 5.0)),
            2,
            yellow,
        );
        p.rect_filled(
            Rect::from_min_size(pos2(x, y + 5.0), vec2(16.0, 10.0)),
            2,
            yellow,
        );
        return;
    }
    let extension = entry.extension();
    let (colour, badge) = match extension.as_str() {
        "doc" | "docx" | "docm" | "rtf" | "odt" => (Color32::from_rgb(54, 113, 194), "W"),
        "pdf" => (Color32::from_rgb(203, 65, 65), "P"),
        "zip" | "7z" | "rar" | "gz" | "tar" | "bz2" | "xz" => {
            (Color32::from_rgb(149, 97, 171), "Z")
        }
        "md" | "markdown" => (Color32::from_rgb(65, 139, 139), "M"),
        "rs" | "py" | "js" | "json" | "toml" | "xml" | "html" | "css" | "c" | "cpp" | "cs" => {
            (Color32::from_rgb(65, 139, 139), "<>")
        }
        _ if crate::media::kind(&entry.path).is_some() => (
            Color32::from_rgb(174, 99, 155),
            if crate::media::kind(&entry.path) == Some(crate::media::Kind::Video) {
                "▶"
            } else {
                "♪"
            },
        ),
        _ if image_gallery::Format::from_path(&entry.path).is_some() => {
            (Color32::from_rgb(78, 151, 91), "I")
        }
        _ => (Color32::from_rgb(125, 132, 144), ""),
    };
    p.rect_filled(rect.shrink2(vec2(2.0, 0.0)), 1, colour);
    p.line_segment(
        [pos2(x + 4.0, y + 3.0), pos2(x + 10.0, y + 3.0)],
        Stroke::new(1.0, Color32::WHITE),
    );
    p.text(
        pos2(x + 8.0, y + 10.0),
        egui::Align2::CENTER_CENTER,
        badge,
        FontId::proportional(9.0),
        Color32::WHITE,
    );
    if entry.link {
        p.line_segment([pos2(x, y + 16.0), pos2(x + 5.0, y + 11.0)], line);
        p.line_segment([pos2(x + 1.0, y + 11.0), pos2(x + 5.0, y + 11.0)], line);
    }
}
