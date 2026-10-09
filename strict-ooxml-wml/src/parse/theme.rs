//! Parsing of the theme part (`theme/theme1.xml`).

use std::sync::Arc;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::escape::{escape_attr, escape_text};
use strict_ooxml_core::xml::qname::QName;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::support::SupportStatus;
use crate::model::theme::{FontSet, Theme, ThemeColors, ThemeFonts, ThemeRunFonts, ThemeTypeface};

use super::{is_drawingml, plain_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `theme1.xml` part.
    pub(crate) fn parse_theme_root(&mut self) -> Result<Theme> {
        self.nested(|parser| {
            parser.expect_root_ns("theme", crate::DRAWINGML_STRICT_NS)?;
            let location = parser.location();
            parser.record(
                "a:theme",
                SupportStatus::Supported,
                None,
                Some(location.clone()),
            );
            let mut theme = Theme {
                fonts: ThemeFonts::default(),
                colors: ThemeColors::default(),
                shape_defaults: None,
                text_defaults: None,
                object_defaults_xml: None,
                format_scheme_xml: None,
                location: location.clone(),
            };
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_drawingml(&name) && name.local() == "themeElements" {
                            parser.parse_theme_elements(&mut theme)?;
                        } else if is_drawingml(&name) && name.local() == "objectDefaults" {
                            parser.parse_object_defaults(&mut theme, &name, &attrs)?;
                        } else {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of theme part")),
                }
            }
            parser.expect_end_of_part()?;
            Ok(theme)
        })
    }

    /// Parses `a:themeElements` into `theme`.
    fn parse_theme_elements(&mut self, theme: &mut Theme) -> Result<()> {
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_drawingml(&name) {
                            match name.local() {
                                "clrScheme" => {
                                    parser.parse_color_scheme(theme)?;
                                    continue;
                                }
                                "fontScheme" => {
                                    parser.parse_font_scheme(theme)?;
                                    continue;
                                }
                                "fmtScheme" => {
                                    // The capture reads through the end tag.
                                    let kept = parser.capture_fragment(name.clone(), attrs)?;
                                    theme.format_scheme_xml =
                                        kept.map(|markup| markup.to_string());
                                    if theme.format_scheme_xml.is_none() {
                                        parser.record(
                                            "a:fmtScheme",
                                            SupportStatus::Partial,
                                            Some(
                                                "theme fill, line and effect styles carry content \
                                                 this writer may not write; a placeholder is written"
                                                    .to_owned(),
                                            ),
                                            Some(parser.location()),
                                        );
                                    }
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of theme elements")),
                }
            }
            Ok(())
        })
    }

    /// Parses `a:clrScheme` into `theme.colors`.
    fn parse_color_scheme(&mut self, theme: &mut Theme) -> Result<()> {
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_drawingml(&name) {
                            let slot = name.local().to_owned();
                            let (color, system) = parser.parse_scheme_color()?;
                            if let Some(system) = system {
                                theme.colors.set_system(slot.clone(), system);
                            }
                            if let Some(color) = color {
                                theme.colors.insert(slot, color);
                            }
                        } else {
                            parser.skip_element()?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of colour scheme")),
                }
            }
            Ok(())
        })
    }

    /// Parses a colour-scheme slot, returning its `#rrggbb` value and, for an
    /// `a:sysClr`, the system colour it names.
    fn parse_scheme_color(&mut self) -> Result<(Option<Arc<str>>, Option<Arc<str>>)> {
        self.nested(|parser| {
            let mut color = None;
            let mut system = None;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_drawingml(&name) && color.is_none() {
                            let value = match name.local() {
                                "srgbClr" => plain_attr(&attrs, "val"),
                                "sysClr" => {
                                    system = plain_attr(&attrs, "val").map(|v| parser.intern(v));
                                    plain_attr(&attrs, "lastClr").or(plain_attr(&attrs, "val"))
                                }
                                _ => None,
                            };
                            color = normalize_hex(value);
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of colour slot")),
                }
            }
            Ok((color, system))
        })
    }

    /// Parses `a:fontScheme` into `theme.fonts`.
    fn parse_font_scheme(&mut self, theme: &mut Theme) -> Result<()> {
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, .. } => {
                        if is_drawingml(&name) {
                            match name.local() {
                                "majorFont" => {
                                    theme.fonts.major = parser.parse_font_set()?;
                                    continue;
                                }
                                "minorFont" => {
                                    theme.fonts.minor = parser.parse_font_set()?;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of font scheme")),
                }
            }
            Ok(())
        })
    }

    /// Parses an `a:majorFont`/`a:minorFont` element.
    fn parse_font_set(&mut self) -> Result<FontSet> {
        self.nested(|parser| {
            let mut set = FontSet::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_drawingml(&name) {
                            let face = theme_typeface(&attrs);
                            match name.local() {
                                "latin" => merge_typeface(&mut set.latin, face),
                                "ea" => merge_typeface(&mut set.east_asia, face),
                                "cs" => merge_typeface(&mut set.cs, face),
                                "font" => {
                                    let script = plain_attr(&attrs, "script");
                                    let typeface = plain_attr(&attrs, "typeface");
                                    if let (Some(script), Some(typeface)) = (script, typeface) {
                                        let script = parser.intern(script);
                                        let typeface = parser.intern(typeface);
                                        set.scripts.push((script, typeface));
                                    }
                                }
                                _ => {}
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of font set")),
                }
            }
            Ok(set)
        })
    }

    /// Copies `a:objectDefaults` and remembers the first `defRPr` faces.
    ///
    /// The start tag has already been read. The copy is what the writer emits:
    /// the element is larger than the three typefaces (`a:lnDef`, list levels,
    /// `a:sym`), and a shell around only those faces would drop the rest.
    fn parse_object_defaults(
        &mut self,
        theme: &mut Theme,
        name: &QName,
        attrs: &[Attr],
    ) -> Result<()> {
        let mut markup = String::new();
        let mut capture = ObjectFontCapture::default();
        self.copy_theme_element(&mut markup, name, attrs, &mut capture)?;
        theme.shape_defaults = capture.shape;
        theme.text_defaults = capture.text;
        theme.object_defaults_xml = Some(markup);
        Ok(())
    }

    /// Writes one element whose start tag was already consumed, then its subtree.
    fn copy_theme_element(
        &mut self,
        out: &mut String,
        name: &QName,
        attrs: &[Attr],
        capture: &mut ObjectFontCapture,
    ) -> Result<()> {
        write_markup_start(out, name, attrs);
        let previous = capture.role;
        if is_drawingml(name) {
            match name.local() {
                "spDef" => capture.role = ObjectFontCapture::SHAPE,
                "txDef" => capture.role = ObjectFontCapture::TEXT,
                _ => {}
            }
        }
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_drawingml(&name)
                            && name.local() == "defRPr"
                            && capture.wants_first_def()
                        {
                            let fonts = parser.copy_def_rpr(out, &name, &attrs)?;
                            capture.store(fonts);
                        } else {
                            parser.copy_theme_element(out, &name, &attrs, capture)?;
                        }
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(text) | XmlEvent::CData(text) => {
                        if !text.trim().is_empty() {
                            out.push_str(&escape_text(&text));
                        }
                    }
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of theme markup")),
                }
            }
            Ok(())
        })?;
        capture.role = previous;
        write_markup_end(out, name);
        Ok(())
    }

    /// Copies `a:defRPr` and returns its latin/ea/cs faces.
    fn copy_def_rpr(
        &mut self,
        out: &mut String,
        name: &QName,
        attrs: &[Attr],
    ) -> Result<ThemeRunFonts> {
        write_markup_start(out, name, attrs);
        let mut fonts = ThemeRunFonts::default();
        let mut capture = ObjectFontCapture::default();
        self.nested(|parser| {
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_drawingml(&name) {
                            let face = theme_typeface(&attrs);
                            match name.local() {
                                "latin" => fonts.latin = face,
                                "ea" => fonts.east_asia = face,
                                "cs" => fonts.cs = face,
                                _ => {}
                            }
                        }
                        parser.copy_theme_element(out, &name, &attrs, &mut capture)?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(text) | XmlEvent::CData(text) => {
                        if !text.trim().is_empty() {
                            out.push_str(&escape_text(&text));
                        }
                    }
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of defRPr")),
                }
            }
            Ok(())
        })?;
        write_markup_end(out, name);
        Ok(fonts)
    }
}

/// First `a:defRPr` faces under `a:spDef` and `a:txDef` while the subtree is copied.
#[derive(Default)]
struct ObjectFontCapture {
    shape: Option<ThemeRunFonts>,
    text: Option<ThemeRunFonts>,
    role: u8,
}

impl ObjectFontCapture {
    const SHAPE: u8 = 1;
    const TEXT: u8 = 2;

    fn wants_first_def(&self) -> bool {
        match self.role {
            Self::SHAPE => self.shape.is_none(),
            Self::TEXT => self.text.is_none(),
            _ => false,
        }
    }

    fn store(&mut self, fonts: ThemeRunFonts) {
        match self.role {
            Self::SHAPE => self.shape = Some(fonts),
            Self::TEXT => self.text = Some(fonts),
            _ => {}
        }
    }
}

fn write_markup_start(out: &mut String, name: &QName, attrs: &[Attr]) {
    out.push('<');
    out.push_str(&markup_name(name));
    for attr in attrs {
        out.push(' ');
        out.push_str(&markup_attr(&attr.name));
        out.push_str("=\"");
        let value = strict_theme_percentage(name.local(), attr.name.local(), &attr.value)
            .unwrap_or_else(|| attr.value.clone());
        out.push_str(&escape_attr(&value));
        out.push('"');
    }
    out.push('>');
}

/// Strict `ST_Percentage` is a percent string. Transitional DrawingML stores the
/// same value in thousandths (`100000` is 100%). Only the attributes whose
/// Strict type is a percentage are rewritten; a line width or a coordinate is
/// left as written.
fn strict_theme_percentage(element: &str, attr: &str, value: &str) -> Option<String> {
    let applies = matches!(
        (element, attr),
        ("spcPct" | "buSzPct", "val") | ("miter", "lim")
    );
    if !applies || value.ends_with('%') {
        return None;
    }
    let number: i64 = value.parse().ok()?;
    let negative = number < 0;
    let magnitude = number.unsigned_abs();
    let whole = magnitude / 1000;
    let fraction = magnitude % 1000;
    let body = if fraction == 0 {
        whole.to_string()
    } else {
        let digits = format!("{fraction:03}");
        format!("{whole}.{}", digits.trim_end_matches('0'))
    };
    Some(if negative {
        format!("-{body}%")
    } else {
        format!("{body}%")
    })
}

fn write_markup_end(out: &mut String, name: &QName) {
    out.push_str("</");
    out.push_str(&markup_name(name));
    out.push('>');
}

/// DrawingML is written with the `a` prefix the theme root declares.
fn markup_name(name: &QName) -> String {
    if is_drawingml(name) {
        return format!("a:{}", name.local());
    }
    match &name.prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{}", name.local()),
        _ => name.local().to_owned(),
    }
}

fn markup_attr(name: &QName) -> String {
    match &name.prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{}", name.local()),
        _ => name.local().to_owned(),
    }
}

/// Fills empty slots of `slot` from `next`. The first face in document order wins.
fn merge_typeface(slot: &mut ThemeTypeface, next: ThemeTypeface) {
    if slot.name.is_none() {
        slot.name = next.name;
    }
    if slot.panose.is_none() {
        slot.panose = next.panose;
    }
    if slot.pitch_family.is_none() {
        slot.pitch_family = next.pitch_family;
    }
    if slot.charset.is_none() {
        slot.charset = next.charset;
    }
}

/// `CT_TextFont` attributes. An empty `typeface` is "no face", not a missing element.
fn theme_typeface(attrs: &[strict_ooxml_core::xml::Attr]) -> ThemeTypeface {
    ThemeTypeface {
        name: plain_attr(attrs, "typeface")
            .filter(|value| !value.trim().is_empty())
            .map(Arc::from),
        panose: plain_attr(attrs, "panose").map(Arc::from),
        pitch_family: plain_attr(attrs, "pitchFamily").map(Arc::from),
        charset: plain_attr(attrs, "charset").map(Arc::from),
    }
}

/// Normalizes a six-digit hex value into `#rrggbb`.
fn normalize_hex(value: Option<&str>) -> Option<Arc<str>> {
    let value = value?.trim();
    let digits = value
        .strip_prefix('#')
        .unwrap_or(value)
        .trim_start_matches("0x");
    if digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(Arc::from(format!("#{}", digits.to_ascii_lowercase())))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_hex, strict_theme_percentage};

    #[test]
    fn normalizes_hex_values() {
        assert_eq!(normalize_hex(Some("FFFFFF")).as_deref(), Some("#ffffff"));
        assert_eq!(normalize_hex(Some("#4472C4")).as_deref(), Some("#4472c4"));
        assert_eq!(normalize_hex(Some("0x000000")).as_deref(), Some("#000000"));
        assert_eq!(normalize_hex(Some("ZZZ")).as_deref(), None);
        assert_eq!(normalize_hex(Some("12345")).as_deref(), None);
        assert_eq!(normalize_hex(None), None);
    }

    #[test]
    fn theme_percentages_become_strict_strings() {
        assert_eq!(
            strict_theme_percentage("spcPct", "val", "100000").as_deref(),
            Some("100%")
        );
        assert_eq!(
            strict_theme_percentage("miter", "lim", "400000").as_deref(),
            Some("400%")
        );
        assert_eq!(
            strict_theme_percentage("buSzPct", "val", "95000").as_deref(),
            Some("95%")
        );
        assert_eq!(strict_theme_percentage("ln", "w", "12700"), None);
        assert_eq!(strict_theme_percentage("spcPct", "val", "100%"), None);
    }
}
