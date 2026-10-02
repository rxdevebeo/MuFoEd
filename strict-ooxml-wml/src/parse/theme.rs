//! Parsing of the theme part (`theme/theme1.xml`).

use std::sync::Arc;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::XmlEvent;

use crate::model::support::SupportStatus;
use crate::model::theme::{FontSet, Theme, ThemeColors, ThemeFonts};

use super::{is_drawingml, plain_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `theme1.xml` part.
    pub(crate) fn parse_theme_root(&mut self) -> Result<Theme> {
        self.enter()?;
        self.expect_root_ns("theme", crate::DRAWINGML_STRICT_NS)?;
        let location = self.location();
        self.record(
            "a:theme",
            SupportStatus::Supported,
            None,
            Some(location.clone()),
        );
        let mut theme = Theme {
            fonts: ThemeFonts::default(),
            colors: ThemeColors::default(),
            location: location.clone(),
        };
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_drawingml(&name) && name.local() == "themeElements" {
                        self.parse_theme_elements(&mut theme)?;
                    } else {
                        self.record_foreign(&name);
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of theme part")),
            }
        }
        self.expect_end_of_part()?;
        self.leave();
        Ok(theme)
    }

    /// Parses `a:themeElements` into `theme`.
    fn parse_theme_elements(&mut self, theme: &mut Theme) -> Result<()> {
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_drawingml(&name) {
                        match name.local() {
                            "clrScheme" => {
                                self.parse_color_scheme(theme)?;
                                continue;
                            }
                            "fontScheme" => {
                                self.parse_font_scheme(theme)?;
                                continue;
                            }
                            "fmtScheme" => {
                                self.record(
                                    "a:fmtScheme",
                                    SupportStatus::Partial,
                                    Some(
                                        "theme effects/fills/line styles are not resolved (5A scope)"
                                            .to_owned(),
                                    ),
                                    Some(self.location()),
                                );
                            }
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of theme elements")),
            }
        }
        self.leave();
        Ok(())
    }

    /// Parses `a:clrScheme` into `theme.colors`.
    fn parse_color_scheme(&mut self, theme: &mut Theme) -> Result<()> {
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_drawingml(&name) {
                        let slot = name.local().to_owned();
                        if let Some(color) = self.parse_scheme_color()? {
                            theme.colors.insert(slot, color);
                        }
                    } else {
                        self.skip_element()?;
                    }
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of colour scheme")),
            }
        }
        self.leave();
        Ok(())
    }

    /// Parses a colour-scheme slot, returning its `#rrggbb` value.
    fn parse_scheme_color(&mut self) -> Result<Option<Arc<str>>> {
        self.enter()?;
        let mut color = None;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_drawingml(&name) && color.is_none() {
                        let value = match name.local() {
                            "srgbClr" => plain_attr(&attrs, "val"),
                            "sysClr" => plain_attr(&attrs, "lastClr").or(plain_attr(&attrs, "val")),
                            _ => None,
                        };
                        color = normalize_hex(value);
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of colour slot")),
            }
        }
        self.leave();
        Ok(color)
    }

    /// Parses `a:fontScheme` into `theme.fonts`.
    fn parse_font_scheme(&mut self, theme: &mut Theme) -> Result<()> {
        self.enter()?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, .. } => {
                    if is_drawingml(&name) {
                        match name.local() {
                            "majorFont" => {
                                theme.fonts.major = self.parse_font_set()?;
                                continue;
                            }
                            "minorFont" => {
                                theme.fonts.minor = self.parse_font_set()?;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of font scheme")),
            }
        }
        self.leave();
        Ok(())
    }

    /// Parses an `a:majorFont`/`a:minorFont` element.
    fn parse_font_set(&mut self) -> Result<FontSet> {
        self.enter()?;
        let mut set = FontSet::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_drawingml(&name) {
                        let typeface = plain_attr(&attrs, "typeface")
                            .filter(|value| !value.trim().is_empty())
                            .map(Arc::from);
                        match name.local() {
                            "latin" => set.latin = typeface.or(set.latin),
                            "ea" => set.east_asia = typeface.or(set.east_asia),
                            "cs" => set.cs = typeface.or(set.cs),
                            _ => {}
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of font set")),
            }
        }
        self.leave();
        Ok(set)
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
    use super::normalize_hex;

    #[test]
    fn normalizes_hex_values() {
        assert_eq!(normalize_hex(Some("FFFFFF")).as_deref(), Some("#ffffff"));
        assert_eq!(normalize_hex(Some("#4472C4")).as_deref(), Some("#4472c4"));
        assert_eq!(normalize_hex(Some("0x000000")).as_deref(), Some("#000000"));
        assert_eq!(normalize_hex(Some("ZZZ")).as_deref(), None);
        assert_eq!(normalize_hex(Some("12345")).as_deref(), None);
        assert_eq!(normalize_hex(None), None);
    }
}
