//! Parsing of `settings.xml`.

use strict_ooxml_core::error::Result;

/// The relationship namespace, spelled out rather than borrowed: `strict-ooxml-core`
/// does not export it and the writer has its own copy, and a third place to look
/// is how two of them drift.
const NS_R: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
use strict_ooxml_core::xml::{Attr, XmlEvent};

use crate::model::settings::{
    ColorSchemeMapping, CompatFlags, DocumentZoom, MathProperties, RevisionSaveIds, Settings,
    ThemeFontLang, Zoom,
};
use crate::model::values::Twips;

use super::{attr_in_ns, is_math, is_wml, parse_i32, parse_on_off, val_attr, wml_attr, PartParser};

/// `CT_OnOff` children of `w:settings` carried as a (name, value) pair.
///
/// Thirty-one elements were being dropped from `settings.xml` across the corpus
/// and most of them are this one shape: a `CT_OnOff` with no payload beyond on
/// or off. They are a table rather than thirty struct fields so that adding the
/// thirty-first is a line here and nothing else.
const FLAT_ON_OFF: &[&str] = &[
    "bookFoldPrinting",
    "bordersDoNotSurroundFooter",
    "bordersDoNotSurroundHeader",
    "doNotAutoCompressPictures",
    "doNotIncludeSubdocsInStats",
    "doNotUseMarginsForDrawingGridOrigin",
    "embedSystemFonts",
    "embedTrueTypeFonts",
    "noPunctuationKerning",
    "savePreviewPicture",
];

/// Numeric children of `w:settings`, carried as the producer's own text.
const FLAT_NUMERIC: &[&str] = &[
    "displayHorizontalDrawingGridEvery",
    "displayVerticalDrawingGridEvery",
    "drawingGridHorizontalSpacing",
    "drawingGridVerticalSpacing",
];

impl PartParser<'_> {
    /// Parses a `settings.xml` part.
    pub(crate) fn parse_settings_root(&mut self) -> Result<Settings> {
        self.nested(|parser| {
            let mut settings = Settings::default();
            parser.expect_root("settings")?;
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        // `m:mathPr` is the one child of `w:settings` in another
                        // namespace, and it is the ONLY reason this branch exists:
                        // the guard below drops everything that is not `wml`, so the
                        // whole block was recorded as foreign markup and thrown away
                        // in forty corpus documents. Checked before the guard, because
                        // after it there is nothing left to check.
                        if name.local() == "mathPr" {
                            settings.math_properties = Some(parser.parse_math_properties()?);
                            continue;
                        }
                        if !is_wml(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
                            continue;
                        }
                        // `CT_Settings` declares ninety-six children and the model
                        // carries fifty-one of them across seven shapes: a scalar, a
                        // boolean, a keyed list, a nested block, and three name-keyed
                        // maps. The dispatch is a separate function because the inline
                        // form grew past the function-length lint, and a `w:settings`
                        // reader that lives in one screen is a reader nobody re-checks
                        // against the schema.
                        //
                        // A parser-closing element arrives as `StartElement` with a pending end, so
                        // an element left open produces an `End` next - and this loop's
                        // `End` is the end of `w:settings`. `true` means the child read
                        // its own subtree and must not be skipped; `false` means it is
                        // still open and skipping it is what consumes that `End`.
                        if parser.settings_child(&name, &attrs, &mut settings)? {
                            continue;
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of settings")),
                }
            }
            parser.expect_end_of_part()?;
            Ok(settings)
        })
    }

    /// Applies one `w:settings` child. Returns `true` when the element was fully
    /// consumed, so the caller does not have to skip it again.
    #[allow(clippy::too_many_lines)]
    fn settings_child(
        &mut self,
        name: &super::QName,
        attrs: &[Attr],
        settings: &mut Settings,
    ) -> Result<bool> {
        if let Some(consumed) = self.settings_named(name, attrs, settings)? {
            return Ok(consumed);
        }
        match name.local() {
            "defaultTabStop" => {
                settings.default_tab_stop = val_attr(attrs).and_then(parse_i32).map(Twips);
            }
            "zoom" => settings.zoom = Some(Self::parse_zoom(attrs)),
            "evenAndOddHeaders" => match parse_on_off(attrs) {
                Some(false) => settings.even_and_odd_headers_off = true,
                Some(true) => settings.even_and_odd_headers = true,
                None => {}
            },
            "displayBackgroundShape" => {
                settings.display_background_shape = parse_on_off(attrs).unwrap_or(false);
            }
            "hideSpellingErrors" => {
                settings.hide_spelling_errors = parse_on_off(attrs).unwrap_or(false);
            }
            "hideGrammaticalErrors" => {
                settings.hide_grammatical_errors = parse_on_off(attrs).unwrap_or(false);
            }
            "proofState" => {
                settings.proof_state = Some(crate::model::settings::ProofState {
                    spelling: wml_attr(attrs, "spelling")
                        .and_then(crate::model::settings::ProofCleanliness::from_strict),
                    grammar: wml_attr(attrs, "grammar")
                        .and_then(crate::model::settings::ProofCleanliness::from_strict),
                });
            }
            "trackRevisions" => {
                settings.track_revisions = parse_on_off(attrs).unwrap_or(false);
            }
            "doNotHyphenateCaps" => {
                settings.do_not_hyphenate_caps = parse_on_off(attrs).unwrap_or(false);
            }
            "autoHyphenation" => {
                settings.auto_hyphenation = parse_on_off(attrs).unwrap_or(false);
            }
            "hyphenationZone" => {
                settings.hyphenation_zone = val_attr(attrs).and_then(parse_i32).map(Twips);
            }
            "characterSpacingControl" => {
                settings.character_spacing_control =
                    val_attr(attrs).map(|value| self.intern(value));
            }
            "view" => {
                settings.view = val_attr(attrs).map(|value| self.intern(value));
            }
            "docVars" => {
                let pairs = self.parse_keyed_children("docVar")?;
                settings.document_variables.extend(pairs);
                return Ok(true);
            }
            "noLineBreaksAfter" => {
                let pair = self.keyed_value(attrs, "lang");
                if let Some(pair) = pair {
                    settings.no_line_breaks_after.push(pair);
                }
                self.skip_element()?;
                return Ok(true);
            }
            "noLineBreaksBefore" => {
                let pair = self.keyed_value(attrs, "lang");
                if let Some(pair) = pair {
                    settings.no_line_breaks_before.push(pair);
                }
                self.skip_element()?;
                return Ok(true);
            }
            "attachedTemplate" => {
                settings.attached_template =
                    attr_in_ns(attrs, NS_R, "id").map(|value| self.intern(value));
                self.skip_element()?;
                return Ok(true);
            }
            "stylePaneFormatFilter" => {
                settings.style_pane_filter = self.attribute_pairs(attrs);
                self.skip_element()?;
                return Ok(true);
            }
            "revisionView" => {
                settings.revision_view = self.attribute_pairs(attrs);
                self.skip_element()?;
                return Ok(true);
            }
            "compat" => {
                let (pairs, flags) = self.parse_compat()?;
                settings.compatibility.extend(pairs);
                settings.compat_flags = flags;
                return Ok(true);
            }
            _ if FLAT_ON_OFF.contains(&name.local()) => {
                // The value is kept as the producer wrote it. `CT_OnOff`
                // is union(xsd:boolean) with an ST_OnOff enumeration on
                // top, so a bare element, `w:val="true"` and `w:val="1"`
                // all mean on and `w:val="0"`/`"false"`/`"off"` mean off.
                // Writing a bare element for an input that said
                // `w:val="0"` would switch a flag ON, which for
                // `w:embedTrueTypeFonts` decides whether the embedded
                // font parts exist at all.
                settings.on_off_flags.push((
                    self.intern(name.local()),
                    val_attr(attrs).map(|value| self.intern(value)),
                ));
                self.skip_element()?;
                return Ok(true);
            }
            _ if FLAT_NUMERIC.contains(&name.local()) => {
                // Four different types - ST_TwipsMeasure, ST_DecimalNumber
                // and two more - and all four round trip as the producer's
                // own text, so an unmodelled value is still carried rather
                // than rounded away by a reader that understood it.
                if let Some(value) = val_attr(attrs) {
                    settings
                        .numeric_settings
                        .push((self.intern(name.local()), self.intern(value)));
                }
                self.skip_element()?;
                return Ok(true);
            }
            _ => {
                self.record_foreign(name);
                return Ok(false);
            }
        }
        Ok(false)
    }
    /// The `w:settings` children that are neither scalar nor nested.
    ///
    /// Split out of [`settings_child`](Self::settings_child) purely so both fit
    /// the function-length lint; the split is by nothing in particular, and it is
    /// the second of two functions that between them hold every child `CT_Settings`
    /// declares that this model carries.
    fn settings_named(
        &mut self,
        name: &super::QName,
        attrs: &[Attr],
        settings: &mut Settings,
    ) -> Result<Option<bool>> {
        match name.local() {
            "documentProtection" => {
                // `@w:edit` is the mode and has its own field; the rest
                // of the attributes are kept verbatim, because
                // `@w:enforcement` is what the corpus writes and an
                // element with only `w:edit` read off it writes back
                // as nothing at all.
                let extra: Vec<_> = self
                    .attribute_pairs(attrs)
                    .into_iter()
                    .filter(|(name, _)| name.as_ref() != "edit")
                    .collect();
                if !extra.is_empty() {
                    settings.document_protection_attributes = extra;
                }
                settings.document_protection =
                    wml_attr(attrs, "edit").map(|value| self.intern(value));
            }
            "decimalSymbol" => {
                settings.decimal_symbol = val_attr(attrs).map(|value| self.intern(value));
            }
            "listSeparator" => {
                settings.list_separator = val_attr(attrs).map(|value| self.intern(value));
            }
            "themeFontLang" => {
                let language = ThemeFontLang {
                    val: wml_attr(attrs, "val").map(|value| self.intern(value)),
                    east_asia: wml_attr(attrs, "eastAsia").map(|value| self.intern(value)),
                    bidi: wml_attr(attrs, "bidi").map(|value| self.intern(value)),
                };
                if !language.is_empty() {
                    settings.theme_font_lang = Some(language);
                }
            }
            "mirrorMargins" => {
                settings.mirror_margins = parse_on_off(attrs).unwrap_or(false);
            }
            // The Strict spelling of the flag Transitional puts in
            // `w:sectPr`; both land on the same field.
            "gutterAtTop" => {
                settings.gutter_at_top = parse_on_off(attrs).unwrap_or(false);
            }
            "footnotePr" => {
                settings.footnote_properties = self.parse_note_properties()?;
                return Ok(Some(true));
            }
            "endnotePr" => {
                settings.endnote_properties = self.parse_note_properties()?;
                return Ok(Some(true));
            }
            "clrSchemeMapping" => {
                settings.color_scheme_mapping = Some(self.parse_color_scheme_mapping(attrs));
                return Ok(Some(false));
            }
            "rsids" => {
                settings.revision_save_ids = Some(self.parse_revision_save_ids()?);
                return Ok(Some(true));
            }
            _ => return Ok(None),
        }
        // `false`: the arms above read attributes off an element they did not
        // consume, so the element is still open and the caller has to skip it.
        // `true` is reserved for an arm that read the subtree itself.
        Ok(Some(false))
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
        self.nested(|parser| {
            let mut properties = MathProperties::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if !is_math(&name) {
                            parser.record_foreign(&name);
                            parser.skip_element()?;
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
                                parser.record_foreign(&name);
                                parser.skip_element()?;
                                continue;
                            }
                        };
                        *slot = Some(match value {
                            Some(text) => parser.intern(text),
                            // Bare `<m:dispDef/>` means on, and `CT_OnOff` says so by
                            // its default. Writing it back as absent would turn a set
                            // flag into an unset one on the round trip.
                            None => parser.intern("1"),
                        });
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    // Defence in depth (AUD-04). This loop was written as `_ => {}`,
                    // so `Eof` was a spin: the reader said the part ended, the loop
                    // said nothing happened, and a `w:settings` truncated inside
                    // `m:mathPr` never returned.
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of m:mathPr")),
                }
            }
            Ok(properties)
        })
    }

    /// Every `w:`-namespaced attribute of an element as (local name, value).
    ///
    /// Used for the handful of `w:settings` children whose payload is a fixed set
    /// of attributes and nothing else - `w:stylePaneFormatFilter` has fourteen,
    /// `w:revisionView` four, `w:documentProtection` five. Modelling each as a
    /// struct would be thirty fields to carry five values, and the schema already
    /// says which attributes are legal, so the XSD gate is the right place to
    /// check them rather than the model.
    fn attribute_pairs(
        &mut self,
        attrs: &[Attr],
    ) -> Vec<(std::sync::Arc<str>, std::sync::Arc<str>)> {
        let mut pairs = Vec::new();
        for attribute in attrs {
            if attribute
                .name
                .ns
                .as_ref()
                .is_some_and(|ns| ns == crate::WML_STRICT_NS)
            {
                pairs.push((
                    self.intern(attribute.name.local()),
                    self.intern(&attribute.value),
                ));
            }
        }
        pairs
    }

    /// A (language, value) pair for `w:noLineBreaksAfter` / `Before`.
    fn keyed_value(
        &mut self,
        attrs: &[Attr],
        key: &str,
    ) -> Option<(std::sync::Arc<str>, std::sync::Arc<str>)> {
        let name = wml_attr(attrs, key)?;
        let value = wml_attr(attrs, "val")?;
        Some((self.intern(name), self.intern(value)))
    }

    /// Parses the children of a keyed container, `w:docVars` style.
    fn parse_keyed_children(
        &mut self,
        child: &str,
    ) -> Result<Vec<(std::sync::Arc<str>, std::sync::Arc<str>)>> {
        self.nested(|parser| {
            let mut pairs = Vec::new();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) && name.local() == child {
                            if let (Some(key), Some(value)) =
                                (wml_attr(&attrs, "name"), wml_attr(&attrs, "val"))
                            {
                                pairs.push((parser.intern(key), parser.intern(value)));
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of keyed container"))
                    }
                }
            }
            Ok(pairs)
        })
    }

    /// Parses a `w:rsids` block: one root and an unbounded list of entries.
    fn parse_revision_save_ids(&mut self) -> Result<RevisionSaveIds> {
        self.nested(|parser| {
            let mut ids = RevisionSaveIds::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) {
                            let value = val_attr(&attrs).map(|text| parser.intern(text));
                            match name.local() {
                                "rsidRoot" => ids.root = value,
                                "rsid" => {
                                    if let Some(value) = value {
                                        ids.entries.push(value);
                                    }
                                }
                                _ => {
                                    parser.record_foreign(&name);
                                }
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => return Err(parser.invalid("unexpected end of rsids")),
                }
            }
            Ok(ids)
        })
    }

    /// Parses `w:clrSchemeMapping`'s twelve optional attributes.
    ///
    /// Attribute order is not recorded: `CT_ColorSchemeMapping` has no
    /// `xsd:sequence`, so any order validates, and the writer emits them in the
    /// declaration order anyway so a diff of two settings parts reads sensibly.
    fn parse_color_scheme_mapping(&mut self, attrs: &[Attr]) -> ColorSchemeMapping {
        let mut take = |local: &str| wml_attr(attrs, local).map(|value| self.intern(value));
        ColorSchemeMapping {
            background_1: take("bg1"),
            text_1: take("t1"),
            background_2: take("bg2"),
            text_2: take("t2"),
            accent_1: take("accent1"),
            accent_2: take("accent2"),
            accent_3: take("accent3"),
            accent_4: take("accent4"),
            accent_5: take("accent5"),
            accent_6: take("accent6"),
            hyperlink: take("hyperlink"),
            followed_hyperlink: take("followedHyperlink"),
        }
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
        self.nested(|parser| {
            let mut pairs = Vec::new();
            let mut flags = CompatFlags::default();
            loop {
                match parser.next_event()? {
                    XmlEvent::StartElement { name, attrs } => {
                        if is_wml(&name) && name.local() == "compatSetting" {
                            let key = wml_attr(&attrs, "name").map(|value| parser.intern(value));
                            let value = wml_attr(&attrs, "val").map(|value| parser.intern(value));
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
                                Some(value) => {
                                    !matches!(value.trim(), "0" | "false" | "off" | "no")
                                }
                            };
                            match name.local() {
                                "spaceForUL" => flags.space_for_underline = on,
                                "balanceSingleByteDoubleByteWidth" => {
                                    flags.balance_single_byte_double_byte_width = on;
                                }
                                "doNotLeaveBackslashAlone" => {
                                    flags.do_not_leave_backslash_alone = on;
                                }
                                "ulTrailSpace" => flags.underline_trailing_space = on,
                                "doNotExpandShiftReturn" => flags.do_not_expand_shift_return = on,
                                "adjustLineHeightInTable" => flags.adjust_line_height_in_table = on,
                                "applyBreakingRules" => flags.apply_breaking_rules = on,
                                _ => {
                                    parser.record_foreign(&name);
                                }
                            }
                        }
                        parser.skip_element()?;
                    }
                    XmlEvent::EndElement { .. } => break,
                    XmlEvent::Text(_) | XmlEvent::CData(_) => {}
                    XmlEvent::Eof => {
                        return Err(parser.invalid("unexpected end of compat settings"))
                    }
                }
            }
            Ok((pairs, flags))
        })
    }
}
