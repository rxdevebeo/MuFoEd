//! Style cascade → computed paragraph/run properties (`STAGE-4-TASK.md` §5.3).
//!
//! Cascade order: built-in defaults → paragraph style `basedOn` chain
//! (furthest ancestor first) → the style itself → the paragraph's direct
//! `pPr`/`rPr`. A run additionally applies its character style chain and its
//! own `rPr`. `TriState::Absent` leaves the inherited value untouched.

use strict_ooxml_wml::model::props::{ParagraphProperties, RunProperties};
use strict_ooxml_wml::model::theme::Theme;
use strict_ooxml_wml::model::values::{
    Color, Fonts, Highlight, Indentation, Justification, LineSpacingRule, Spacing, TabStop,
    ThemeColorRef, TriState, Underline, VertAlign,
};
use strict_ooxml_wml::model::Document;

/// Effective run formatting.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedRun {
    /// Font family (ascii/hAnsi).
    pub family: String,
    /// Font size in points.
    pub size_pt: f64,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strikethrough (single or double).
    pub strike: bool,
    /// Text colour (`#rrggbb`), or `None` for the default (black).
    pub color: Option<String>,
    /// Highlight fill colour (`#rrggbb`).
    pub highlight: Option<String>,
    /// Vertical alignment (baseline/superscript/subscript).
    pub vert_align: VertAlign,
    /// All-capitals (`w:caps`).
    pub caps: bool,
}

impl Default for ComputedRun {
    fn default() -> Self {
        Self {
            family: "Calibri".to_owned(),
            size_pt: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: None,
            highlight: None,
            vert_align: VertAlign::Baseline,
            caps: false,
        }
    }
}

/// Numbering reference of a paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberingRef {
    /// Numbering instance id (`w:numId`).
    pub num_id: u32,
    /// List level (`w:ilvl`).
    pub ilvl: u8,
}

/// Effective paragraph formatting plus the paragraph's default run format.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedParagraph {
    /// Alignment.
    pub alignment: Justification,
    /// Space before, in points.
    pub space_before_pt: f64,
    /// Space after, in points.
    pub space_after_pt: f64,
    /// Line spacing value in points (interpretation depends on `line_rule`).
    pub line_pt: Option<f64>,
    /// Line-spacing rule.
    pub line_rule: LineSpacingRule,
    /// Leading indentation, in points.
    pub indent_start_pt: f64,
    /// Trailing indentation, in points.
    pub indent_end_pt: f64,
    /// First-line indentation in points (negative = hanging).
    pub first_line_pt: f64,
    /// Custom tab stops.
    pub tabs: Vec<TabStop>,
    /// Keep lines together.
    pub keep_lines: bool,
    /// Keep with next paragraph.
    pub keep_next: bool,
    /// Start on a new page.
    pub page_break_before: bool,
    /// Numbering reference.
    pub numbering: Option<NumberingRef>,
    /// Default run format for runs without explicit properties.
    pub default_run: ComputedRun,
}

impl Default for ComputedParagraph {
    fn default() -> Self {
        Self {
            alignment: Justification::Start,
            space_before_pt: 0.0,
            space_after_pt: 0.0,
            line_pt: None,
            line_rule: LineSpacingRule::Auto,
            indent_start_pt: 0.0,
            indent_end_pt: 0.0,
            first_line_pt: 0.0,
            tabs: Vec::new(),
            keep_lines: false,
            keep_next: false,
            page_break_before: false,
            numbering: None,
            default_run: ComputedRun::default(),
        }
    }
}

impl ComputedParagraph {
    /// Returns the effective left indentation for the first line, in points.
    #[must_use]
    pub fn first_line_offset_pt(&self) -> f64 {
        self.indent_start_pt + self.first_line_pt
    }
}

/// Computes the effective paragraph properties of `para`.
#[must_use]
pub fn compute_paragraph(
    document: &Document,
    para: &strict_ooxml_wml::model::Paragraph,
) -> ComputedParagraph {
    let mut computed = ComputedParagraph::default();
    let theme = document.theme.as_ref();

    // `w:docDefaults` is the root of the cascade (ISO/IEC 29500-1 §17.7.1): it
    // applies before any style, so a document that only sets Calibri/11 pt and
    // `w:spacing` there still gets the producer's line height and paragraph
    // spacing.
    if let Some(defaults) = document.styles.defaults() {
        apply_paragraph_props(&mut computed, &defaults.paragraph, theme);
        apply_run_props(&mut computed.default_run, &defaults.run, theme);
    }

    let style_id = para.props.style.as_ref().or_else(|| {
        document
            .styles
            .default_for(strict_ooxml_wml::model::values::StyleType::Paragraph)
    });
    if let Some(style_id) = style_id {
        apply_paragraph_style(document, &mut computed, style_id, theme);
    }
    apply_paragraph_props(&mut computed, &para.props, theme);
    computed
}

/// Computes the effective run properties of `run`.
#[must_use]
pub fn compute_run(
    document: &Document,
    para: &ComputedParagraph,
    run: &strict_ooxml_wml::model::Run,
) -> ComputedRun {
    let theme = document.theme.as_ref();
    let mut computed = para.default_run.clone();
    if let Some(style_id) = &run.props.style {
        if let Some(style) = document.styles.get(style_id) {
            for ancestor in style.based_on_chain.iter().rev() {
                if let Some(ancestor) = document.styles.get(ancestor) {
                    apply_run_props(&mut computed, &ancestor.run, theme);
                }
            }
            apply_run_props(&mut computed, &style.run, theme);
        }
    }
    apply_run_props(&mut computed, &run.props, theme);
    computed
}

/// Applies a paragraph style's `basedOn` chain and its own properties.
fn apply_paragraph_style(
    document: &Document,
    computed: &mut ComputedParagraph,
    style_id: &strict_ooxml_wml::model::StyleId,
    theme: Option<&Theme>,
) {
    let Some(style) = document.styles.get(style_id) else {
        return;
    };
    for ancestor in style.based_on_chain.iter().rev() {
        if let Some(ancestor) = document.styles.get(ancestor) {
            apply_paragraph_props(computed, &ancestor.paragraph, theme);
            apply_run_props(&mut computed.default_run, &ancestor.run, theme);
        }
    }
    apply_paragraph_props(computed, &style.paragraph, theme);
    apply_run_props(&mut computed.default_run, &style.run, theme);
}

/// Merges direct paragraph properties onto `computed`.
pub fn apply_paragraph_props(
    computed: &mut ComputedParagraph,
    props: &ParagraphProperties,
    theme: Option<&Theme>,
) {
    if let Some(alignment) = props.alignment {
        computed.alignment = alignment;
    }
    if let Some(spacing) = &props.spacing {
        apply_spacing(computed, spacing);
    }
    if let Some(indentation) = &props.indentation {
        apply_indentation(computed, indentation);
    }
    if !props.tabs.is_empty() {
        computed.tabs.clone_from(&props.tabs);
    }
    if props.keep_lines {
        computed.keep_lines = true;
    }
    if props.keep_next {
        computed.keep_next = true;
    }
    if props.page_break_before {
        computed.page_break_before = true;
    }
    if let Some(numbering) = &props.numbering {
        if let Some(num_id) = numbering.num_id {
            computed.numbering = Some(NumberingRef {
                num_id: num_id.0,
                ilvl: numbering.ilvl.map_or(0, |ilvl| ilvl.0),
            });
        }
    }
    if let Some(run) = &props.run_props {
        apply_run_props(&mut computed.default_run, run, theme);
    }
}

fn apply_spacing(computed: &mut ComputedParagraph, spacing: &Spacing) {
    if let Some(before) = spacing.before {
        computed.space_before_pt = f64::from(before.value()) / 20.0;
    }
    if let Some(after) = spacing.after {
        computed.space_after_pt = f64::from(after.value()) / 20.0;
    }
    if let Some(rule) = spacing.line_rule {
        computed.line_rule = rule;
    }
    if let Some(line) = spacing.line {
        computed.line_pt = Some(f64::from(line.value()) / 20.0);
    }
}

fn apply_indentation(computed: &mut ComputedParagraph, indentation: &Indentation) {
    if let Some(start) = indentation.start {
        computed.indent_start_pt = f64::from(start.value()) / 20.0;
    }
    if let Some(end) = indentation.end {
        computed.indent_end_pt = f64::from(end.value()) / 20.0;
    }
    if let Some(hanging) = indentation.hanging {
        computed.first_line_pt = -f64::from(hanging.value()) / 20.0;
    } else if let Some(first) = indentation.first_line {
        computed.first_line_pt = f64::from(first.value()) / 20.0;
    }
}

/// Merges direct run properties onto `computed`.
pub fn apply_run_props(computed: &mut ComputedRun, props: &RunProperties, theme: Option<&Theme>) {
    if let Some(fonts) = &props.fonts {
        apply_fonts(computed, fonts, theme);
    }
    if let Some(bold) = toggle(props.bold) {
        computed.bold = bold;
    }
    if let Some(italic) = toggle(props.italic) {
        computed.italic = italic;
    }
    if let Some(underline) = &props.underline {
        computed.underline = !matches!(underline, Underline::None);
    }
    if let Some(strike) = toggle(props.strike) {
        computed.strike = strike;
    }
    if let Some(strike) = toggle(props.double_strike) {
        computed.strike = strike;
    }
    if let Some(color) = &props.color {
        computed.color = parse_color(color);
    }
    if let Some(theme_color) = &props.color_theme {
        if let Some(resolved) = resolve_theme_color(theme_color, theme) {
            computed.color = Some(resolved);
        }
    }
    if let Some(highlight) = props.highlight {
        computed.highlight = highlight_color(highlight);
    }
    if let Some(size) = props.size {
        computed.size_pt = f64::from(size.value()) / 2.0;
    }
    if let Some(vert) = props.vert_align {
        computed.vert_align = vert;
    }
    if props.caps {
        computed.caps = true;
    }
}

fn apply_fonts(computed: &mut ComputedRun, fonts: &Fonts, theme: Option<&Theme>) {
    if let Some(family) = fonts.ascii.as_ref() {
        computed.family = family.to_string();
    } else if let Some(family) = theme_font(fonts.ascii_theme.as_deref(), theme) {
        computed.family = family;
    } else if let Some(family) = fonts.h_ansi.as_ref() {
        computed.family = family.to_string();
    } else if let Some(family) = theme_font(fonts.h_ansi_theme.as_deref(), theme) {
        computed.family = family;
    }
}

/// Resolves a `w:*Theme` font reference through `theme`.
fn theme_font(reference: Option<&str>, theme: Option<&Theme>) -> Option<String> {
    let reference = reference?;
    theme?.font(reference).map(ToString::to_string)
}

/// Resolves a themed run colour (`w:themeColor`/`themeTint`/`themeShade`).
fn resolve_theme_color(reference: &ThemeColorRef, theme: Option<&Theme>) -> Option<String> {
    let theme = theme?;
    let (red, green, blue) = parse_hex(theme.color(reference.color.as_str())?)?;
    let tint = reference
        .tint
        .as_deref()
        .and_then(|value| u8::from_str_radix(value, 16).ok());
    let shade = reference
        .shade
        .as_deref()
        .and_then(|value| u8::from_str_radix(value, 16).ok());
    let apply = |channel: u8| -> u8 {
        if let Some(shade) = shade {
            (u16::from(channel) * u16::from(shade) / 255) as u8
        } else if let Some(tint) = tint {
            (255 - (255 - u16::from(channel)) * (255 - u16::from(tint)) / 255) as u8
        } else {
            channel
        }
    };
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        apply(red),
        apply(green),
        apply(blue)
    ))
}

/// Parses `#rrggbb` into its channels.
fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if digits.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&digits[0..2], 16).ok()?;
    let green = u8::from_str_radix(&digits[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&digits[4..6], 16).ok()?;
    Some((red, green, blue))
}

fn toggle(state: TriState) -> Option<bool> {
    match state {
        TriState::On => Some(true),
        TriState::Off => Some(false),
        TriState::Absent => None,
    }
}

/// Parses a `w:color` value into `#rrggbb` (or `None` for `auto`/invalid).
#[must_use]
pub fn parse_color(color: &Color) -> Option<String> {
    let value = color.as_str();
    if value.eq_ignore_ascii_case("auto") {
        return None;
    }
    if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(format!("#{}", value.to_ascii_lowercase()))
    } else {
        None
    }
}

/// Resolves a DrawingML shape colour (explicit RGB or theme reference).
#[must_use]
pub fn resolve_shape_color(
    theme: Option<&Theme>,
    color: &strict_ooxml_wml::model::drawing::ShapeColor,
) -> Option<String> {
    if let Some(value) = &color.value {
        return parse_color(value);
    }
    color
        .theme
        .as_ref()
        .and_then(|reference| resolve_theme_color(reference, theme))
}

/// Maps a highlight name to its conventional RGB colour.
#[must_use]
pub fn highlight_color(highlight: Highlight) -> Option<String> {
    let hex = match highlight {
        Highlight::Black => "000000",
        Highlight::Blue => "0000ff",
        Highlight::Cyan => "00ffff",
        Highlight::Green => "00ff00",
        Highlight::Magenta => "ff00ff",
        Highlight::Red => "ff0000",
        Highlight::Yellow => "ffff00",
        Highlight::White => "ffffff",
        Highlight::DarkBlue => "000080",
        Highlight::DarkCyan => "008080",
        Highlight::DarkGreen => "008000",
        Highlight::DarkMagenta => "800080",
        Highlight::DarkRed => "800000",
        Highlight::DarkYellow => "808000",
        Highlight::DarkGray => "808080",
        Highlight::LightGray => "c0c0c0",
        Highlight::None => return None,
    };
    Some(format!("#{hex}"))
}

/// Uppercases text for `w:caps` runs (deterministic, locale-independent).
#[must_use]
pub fn apply_caps(text: &str, run: &ComputedRun) -> String {
    if run.caps {
        text.to_uppercase()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::default_trait_access)]
mod tests {
    use super::{compute_paragraph, compute_run, parse_color, ComputedParagraph};
    use std::sync::Arc;
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::part::PartId;
    use strict_ooxml_wml::model::values::{Color, TriState};
    use strict_ooxml_wml::model::{
        Document, Paragraph, ParagraphProperties, Run, RunContent, RunProperties, Style,
        StyleTable, TextNode,
    };

    fn location() -> SourceLocation {
        SourceLocation::new(PartId::new("/word/document.xml"), 1, 1, 0)
    }

    fn empty_document() -> Document {
        Document {
            body: strict_ooxml_wml::model::Body::default(),
            styles: StyleTable::new(),
            numbering: strict_ooxml_wml::model::NumberingTable::new(),
            footnotes: strict_ooxml_wml::model::NoteTable::new(),
            endnotes: strict_ooxml_wml::model::NoteTable::new(),
            settings: strict_ooxml_wml::model::Settings::default(),
            theme: None,
            sections: Vec::new(),
            headers_footers: Vec::new(),
            media: strict_ooxml_wml::model::MediaIndex::new(),
            support: strict_ooxml_wml::model::SupportModel::new(),
            source: strict_ooxml_wml::model::DocumentSource {
                main_document: PartId::new("/word/document.xml"),
                styles: None,
                numbering: None,
                settings: None,
                footnotes: None,
                endnotes: None,
                theme: None,
            },
        }
    }

    #[test]
    fn defaults_are_sane() {
        let document = empty_document();
        let para = Paragraph {
            props: ParagraphProperties::default(),
            inlines: Vec::new(),
            rsids: Default::default(),
            para_id: None,
            text_id: None,
            location: location(),
        };
        let computed = compute_paragraph(&document, &para);
        assert_eq!(computed.default_run.size_pt, 11.0);
        assert_eq!(computed.default_run.family, "Calibri");
        assert!(!computed.default_run.bold);
    }

    #[test]
    fn style_chain_then_direct_props() {
        let mut document = empty_document();
        let mut base = Style {
            id: strict_ooxml_wml::model::StyleId::new("Base"),
            style_type: strict_ooxml_wml::model::values::StyleType::Paragraph,
            name: None,
            based_on: None,
            next: None,
            link: None,
            is_default: false,
            hidden: false,
            ui_priority: None,
            table: Default::default(),
            paragraph: ParagraphProperties::default(),
            run: RunProperties {
                bold: TriState::On,
                ..RunProperties::default()
            },
            based_on_chain: Vec::new(),
            location: location(),
        };
        base.run.size = Some(strict_ooxml_wml::model::values::HalfPoints(28));
        let mut child = base.clone();
        child.id = strict_ooxml_wml::model::StyleId::new("Child");
        child.based_on = Some(strict_ooxml_wml::model::StyleId::new("Base"));
        child.based_on_chain = vec![strict_ooxml_wml::model::StyleId::new("Base")];
        child.run.italic = TriState::On;
        child.run.size = None;
        document.styles.insert(base);
        document.styles.insert(child);

        let para = Paragraph {
            props: ParagraphProperties {
                style: Some(strict_ooxml_wml::model::StyleId::new("Child")),
                run_props: Some(RunProperties {
                    underline: Some(strict_ooxml_wml::model::values::Underline::Single),
                    ..RunProperties::default()
                }),
                ..ParagraphProperties::default()
            },
            inlines: Vec::new(),
            rsids: Default::default(),
            para_id: None,
            text_id: None,
            location: location(),
        };
        let computed = compute_paragraph(&document, &para);
        assert!(computed.default_run.bold, "inherited from Base");
        assert!(computed.default_run.italic, "from Child");
        assert_eq!(computed.default_run.size_pt, 14.0, "from Base");
        assert!(computed.default_run.underline, "from pPr/rPr");
    }

    #[test]
    fn run_props_override_and_colors() {
        let document = empty_document();
        let para = ComputedParagraph::default();
        let run = Run {
            props: RunProperties {
                color: Some(Color::new("FF0000")),
                ..RunProperties::default()
            },
            content: vec![RunContent::Text(TextNode {
                text: "x".to_owned(),
                space: Default::default(),
            })],
            location: location(),
        };
        let computed = compute_run(&document, &para, &run);
        assert_eq!(computed.color.as_deref(), Some("#ff0000"));
        assert!(super::apply_caps("ab", &computed) == "ab");
        assert_eq!(
            super::apply_caps("ab", &ComputedParagraph::default().default_run),
            "ab"
        );
        let _ = parse_color(&Color::new("auto"));
        assert!(parse_color(&Color::new("auto")).is_none());
        assert!(parse_color(&Color::new("zzz")).is_none());
        let _ = Arc::<str>::from("x");
    }
}
