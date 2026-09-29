//! Parsing of `settings.xml`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::settings::{DocumentZoom, Settings, Zoom};
use crate::model::values::Twips;

use super::{is_wml, parse_i32, val_attr, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `settings.xml` part.
    pub(crate) fn parse_settings_root(&mut self) -> Result<Settings> {
        self.enter()?;
        let mut settings = Settings::default();
        self.expect_root("settings")?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_wml(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    match name.local() {
                        "defaultTabStop" => {
                            settings.default_tab_stop =
                                val_attr(&attrs).and_then(parse_i32).map(Twips);
                        }
                        "zoom" => settings.zoom = Some(Self::parse_zoom(&attrs)),
                        "evenAndOddHeaders" => settings.even_and_odd_headers = true,
                        "displayBackgroundShape" => settings.display_background_shape = true,
                        "hideSpellingErrors" => settings.hide_spelling_errors = true,
                        "hideGrammaticalErrors" => settings.hide_grammatical_errors = true,
                        "proofState" => settings.proofing = true,
                        "trackRevisions" => settings.track_revisions = true,
                        "doNotHyphenateCaps" => settings.do_not_hyphenate_caps = true,
                        "autoHyphenation" => settings.auto_hyphenation = true,
                        "hyphenationZone" => {
                            settings.hyphenation_zone =
                                val_attr(&attrs).and_then(parse_i32).map(Twips);
                        }
                        "documentProtection" => {
                            settings.document_protection =
                                wml_attr(&attrs, "edit").map(|value| self.intern(value));
                        }
                        "decimalSymbol" => {
                            settings.decimal_symbol =
                                val_attr(&attrs).map(|value| self.intern(value));
                        }
                        "listSeparator" => {
                            settings.list_separator =
                                val_attr(&attrs).map(|value| self.intern(value));
                        }
                        "themeFontLang" => {
                            settings.theme_font_lang =
                                wml_attr(&attrs, "val").map(|value| self.intern(value));
                        }
                        "mirrorMargins" => settings.mirror_margins = true,
                        "footnotePr" => {
                            settings.footnote_properties = self.parse_note_properties()?;
                            continue;
                        }
                        "endnotePr" => {
                            settings.endnote_properties = self.parse_note_properties()?;
                            continue;
                        }
                        "compat" => {
                            let pairs = self.parse_compat()?;
                            settings.compatibility.extend(pairs);
                            continue;
                        }
                        _ => {
                            self.record_foreign(&name);
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of settings part")),
            }
        }
        self.leave();
        Ok(settings)
    }

    /// Parses a `w:zoom` element.
    fn parse_zoom(attrs: &[Attr]) -> Zoom {
        Zoom {
            percent: wml_attr(attrs, "percent").and_then(|value| value.trim().parse().ok()),
            kind: val_attr(attrs).and_then(DocumentZoom::from_strict),
        }
    }

    /// Parses a `w:compat` element, collecting `w:compatSetting` key/values.
    fn parse_compat(&mut self) -> Result<Vec<(std::sync::Arc<str>, std::sync::Arc<str>)>> {
        self.enter()?;
        let mut pairs = Vec::new();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) && name.local() == "compatSetting" {
                        let key = wml_attr(&attrs, "name").map(|value| self.intern(value));
                        let value = wml_attr(&attrs, "val").map(|value| self.intern(value));
                        if let (Some(key), Some(value)) = (key, value) {
                            pairs.push((key, value));
                        }
                    }
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                XmlEvent::Eof => return Err(self.invalid("unexpected end of compat settings")),
            }
        }
        self.leave();
        Ok(pairs)
    }
}
