//! Parsing of `settings.xml`.

use strict_ooxml_core::error::Result;
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::settings::{CompatFlags, DocumentZoom, MathProperties, Settings, Zoom};
use crate::model::values::Twips;

use super::{attr_in_ns, is_math, is_wml, parse_i32, val_attr, wml_attr, PartParser};

impl PartParser<'_> {
    /// Parses a `settings.xml` part.
    pub(crate) fn parse_settings_root(&mut self) -> Result<Settings> {
        self.enter()?;
        let mut settings = Settings::default();
        self.expect_root("settings")?;
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    // `m:mathPr` is the one child of `w:settings` in another
                    // namespace, and it is the ONLY reason this branch exists:
                    // the guard below drops everything that is not `wml`, so the
                    // whole block was recorded as foreign markup and thrown away
                    // in forty corpus documents. Checked before the guard, because
                    // after it there is nothing left to check.
                    if name.local() == "mathPr" {
                        settings.math_properties = Some(self.parse_math_properties()?);
                        continue;
                    }
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
                        // The Strict spelling of the flag Transitional puts in
                        // `w:sectPr`; both land on the same field.
                        "gutterAtTop" => settings.gutter_at_top = true,
                        "footnotePr" => {
                            settings.footnote_properties = self.parse_note_properties()?;
                            continue;
                        }
                        "endnotePr" => {
                            settings.endnote_properties = self.parse_note_properties()?;
                            continue;
                        }
                        "compat" => {
                            let (pairs, flags) = self.parse_compat()?;
                            settings.compatibility.extend(pairs);
                            settings.compat_flags = flags;
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
    ///
    /// `w:zoom/@w:percent` is `ST_DecimalNumberOrPercent`, whose only branch is
    /// `s:ST_Percentage` - so the schema's valid lexical form carries the sign
    /// (`93%`), while every producer that writes the bare number writes one their
    /// own schema rejects. Both spellings mean the same thing: a percentage of
    /// the normal size. Parsing only the bare form, as this used to, turned a
    /// producer's `93%` into "no zoom at all" and the next write put `100%` in
    /// its place - a document that drifted towards full size on every round trip.
    fn parse_zoom(attrs: &[Attr]) -> Zoom {
        Zoom {
            percent: wml_attr(attrs, "percent").and_then(|value| {
                value
                    .trim()
                    .strip_suffix('%')
                    .unwrap_or(value.trim())
                    .trim()
                    .parse()
                    .ok()
            }),
            kind: val_attr(attrs).and_then(DocumentZoom::from_strict),
        }
    }

    /// Parses an `m:mathPr` block into [`MathProperties`].
    ///
    /// Every child is `m:val` and every one of them is optional in
    /// `CT_MathPr`, so this is a straight name-to-field map. The value stays a
    /// string on purpose: the fourteen are seven distinct simple types
    /// (`CT_String`, `CT_OnOff`, `CT_TwipsMeasure`, `CT_OMathJc`, `CT_LimLoc`),
    /// a corpus carries exactly one legal spelling of each, and a model that
    /// cannot hold an out-of-set value would have to drop it - which is the loss
    /// this block exists to stop. A malformed value is carried verbatim and the
    /// XSD gate names it, which is the project's rule: a named bad value is a
    /// measurement, a silently defaulted one is not.
    fn parse_math_properties(&mut self) -> Result<MathProperties> {
        self.enter()?;
        let mut properties = MathProperties::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if !is_math(&name) {
                        self.record_foreign(&name);
                        self.skip_element()?;
                        continue;
                    }
                    // `m:dispDef` is an on/off flag the producer may write bare,
                    // so the element's presence is the signal and @m:val refines it.
                    let value = attr_in_ns(&attrs, crate::MATH_STRICT_NS, "val");
                    let slot = match name.local() {
                        "mathFont" => &mut properties.math_font,
                        "brkBin" => &mut properties.break_binary_operator,
                        "brkBinSub" => &mut properties.break_binary_sub,
                        "smallFrac" => &mut properties.small_fraction,
                        "dispDef" => &mut properties.display_default,
                        "lMargin" => &mut properties.left_margin,
                        "rMargin" => &mut properties.right_margin,
                        "defJc" => &mut properties.default_justification,
                        "preSp" => &mut properties.pre_space,
                        "postSp" => &mut properties.post_space,
                        "interSp" => &mut properties.inter_space,
                        "intraSp" => &mut properties.intra_space,
                        "wrapIndent" => &mut properties.wrap_indent,
                        "wrapRight" => &mut properties.wrap_right,
                        "intLim" => &mut properties.integral_limit,
                        "naryLim" => &mut properties.nary_limit,
                        _ => {
                            self.record_foreign(&name);
                            self.skip_element()?;
                            continue;
                        }
                    };
                    *slot = Some(match value {
                        Some(text) => self.intern(text),
                        // Bare `<m:dispDef/>` means on, and `CT_OnOff` says so by
                        // its default. Writing it back as absent would turn a set
                        // flag into an unset one on the round trip.
                        None => self.intern("1"),
                    });
                    self.skip_element()?;
                }
                XmlEvent::EndElement { .. } => break,
                _ => {}
            }
        }
        Ok(properties)
    }

    /// Parses a `w:compat` element: the `w:compatSetting` key/value pairs AND the
    /// seven on/off children `CT_Compat` declares.
    ///
    /// The second half used not to be collected at all. `w:compat` is regenerated
    /// from the model, so a flag the model did not carry was not "left out of the
    /// output" - it was deleted from the document with nothing in the report
    /// saying so, in thirty-three places across the corpus (reaudit П-2).
    ///
    /// An unrecognised `w:` child is still skipped rather than kept: the table of
    /// legal names is the schema's, and a name outside it is not something this
    /// model can hold. It is recorded, so the removal is named.
    fn parse_compat(
        &mut self,
    ) -> Result<(Vec<(std::sync::Arc<str>, std::sync::Arc<str>)>, CompatFlags)> {
        self.enter()?;
        let mut pairs = Vec::new();
        let mut flags = CompatFlags::default();
        loop {
            match self.next_event()? {
                XmlEvent::StartElement { name, attrs } => {
                    if is_wml(&name) && name.local() == "compatSetting" {
                        let key = wml_attr(&attrs, "name").map(|value| self.intern(value));
                        let value = wml_attr(&attrs, "val").map(|value| self.intern(value));
                        if let (Some(key), Some(value)) = (key, value) {
                            pairs.push((key, value));
                        }
                    } else if is_wml(&name) {
                        // `CT_OnOff` is a union: a bare element means on, and
                        // `w:val="0"`/`"false"`/`"off"` mean off. The corpus writes
                        // every flag bare, so treating presence as on is what the
                        // input says; reading only `@w:val` would have seen none
                        // of them at all.
                        let on = match wml_attr(&attrs, "val") {
                            None => true,
                            Some(value) => !matches!(value.trim(), "0" | "false" | "off" | "no"),
                        };
                        match name.local() {
                            "spaceForUL" => flags.space_for_underline = on,
                            "balanceSingleByteDoubleByteWidth" => {
                                flags.balance_single_byte_double_byte_width = on;
                            }
                            "doNotLeaveBackslashAlone" => flags.do_not_leave_backslash_alone = on,
                            "ulTrailSpace" => flags.underline_trailing_space = on,
                            "doNotExpandShiftReturn" => flags.do_not_expand_shift_return = on,
                            "adjustLineHeightInTable" => flags.adjust_line_height_in_table = on,
                            "applyBreakingRules" => flags.apply_breaking_rules = on,
                            _ => {
                                self.record_foreign(&name);
                            }
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
        Ok((pairs, flags))
    }
}
