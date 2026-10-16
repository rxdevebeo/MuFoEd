//! Document faces for interface text: the navigation pane shows each heading
//! in the face the page draws it with.
//!
//! A face is looked up the way the page's is: the renderer's bundled faces
//! first (the substitutes layout measured with), then the installed fonts the
//! rasterizer knows. It is added to egui once, as a family of its own with
//! the interface fonts behind it for the glyphs it lacks, and is used from the
//! pass after it was installed (egui panics on a family it does not have).

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use strict_ooxml_render_svg::font::builtin::face_source;

use crate::worker::HeadingFont;

/// A face: family, bold, italic.
type Key = (String, bool, bool);

enum Face {
    /// Not found anywhere; interface text it is.
    Missing,
    /// Installed during this pass; usable from the next.
    Installed(u64),
}

pub(crate) struct DocumentFonts {
    definitions: FontDefinitions,
    faces: HashMap<Key, Face>,
}

impl DocumentFonts {
    pub(crate) fn new(definitions: FontDefinitions) -> Self {
        Self {
            definitions,
            faces: HashMap::new(),
        }
    }

    /// The egui family that draws the regular face of `family`, once it can.
    pub(crate) fn family_named(&mut self, ctx: &egui::Context, family: &str) -> Option<FontFamily> {
        self.family(
            ctx,
            &HeadingFont {
                family: family.to_owned(),
                bold: false,
                italic: false,
                size_pt: 11.0,
                color: None,
            },
        )
    }

    /// The egui family that draws `font`, once it can.
    pub(crate) fn family(&mut self, ctx: &egui::Context, font: &HeadingFont) -> Option<FontFamily> {
        let key: Key = (font.family.clone(), font.bold, font.italic);
        let pass = ctx.cumulative_pass_nr();
        let name = format!("doc:{}:{}{}", key.0, u8::from(key.1), u8::from(key.2));
        match self.faces.get(&key) {
            Some(Face::Missing) => return None,
            Some(Face::Installed(at)) => {
                return (*at < pass).then(|| FontFamily::Name(name.into()));
            }
            None => {}
        }
        let Some(data) = face_data(&font.family, font.bold, font.italic) else {
            self.faces.insert(key, Face::Missing);
            return None;
        };
        self.definitions
            .font_data
            .insert(name.clone(), Arc::new(data));
        let mut chain = vec![name.clone()];
        chain.extend(
            self.definitions
                .families
                .get(&FontFamily::Proportional)
                .cloned()
                .unwrap_or_default(),
        );
        self.definitions
            .families
            .insert(FontFamily::Name(name.into()), chain);
        ctx.set_fonts(self.definitions.clone());
        self.faces.insert(key, Face::Installed(pass));
        None
    }
}

/// The program of a face, bundled or installed.
fn face_data(family: &str, bold: bool, italic: bool) -> Option<FontData> {
    if let Some(source) = face_source(family, bold, italic) {
        // A variable face is shown at its default instance.
        return Some(FontData::from_static(source.data));
    }
    let (bytes, index) = crate::raster::installed_face(family, bold, italic)?;
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    Some(data)
}
