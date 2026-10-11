//! Style cascade → computed paragraph/run properties (`STAGE-4-TASK.md` §5.3).
//!
//! Cascade order: built-in defaults → paragraph style `basedOn` chain
//! (furthest ancestor first) → the style itself → the paragraph's direct
//! `pPr`/`rPr`. A run additionally applies its character style chain and its
//! own `rPr`. Toggle properties XOR inside a style chain. Direct `On`/`Off`
//! assign a state, so a paragraph mark and a run that both say italic stay
//! italic. `TriState::Absent` leaves the inherited value untouched.

use strict_ooxml_wml::model::props::{FrameProperties, ParagraphProperties, RunProperties};
use strict_ooxml_wml::model::theme::Theme;
use strict_ooxml_wml::model::values::{
    Color, Fonts, Highlight, Indentation, Justification, LineSpacingRule, Shading, Spacing,
    TabStop, ThemeColorRef, TriState, Underline, VertAlign,
};
use strict_ooxml_wml::model::Document;

use crate::layout::border::ResolvedBorder;

/// Effective run formatting.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedRun {
    /// Rotation of delivered text around its baseline origin, in degrees.
    pub rotation_deg: f64,
    /// Font family (ascii/hAnsi).
    pub family: String,
    /// Deterministic substitute selected using the document font-table hints.
    pub family_fallback: Option<String>,
    /// Substitute for an unavailable complex-script face.
    pub complex_family_fallback: Option<String>,
    /// Font size in points.
    pub size_pt: f64,
    /// Additional pitch after each character, in points (`w:spacing` on `w:rPr`).
    pub spacing_pt: f64,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline style (`w:u/@w:val`), `None` when the run is not underlined.
    pub underline: Option<Underline>,
    /// Underline colour (`w:u/@w:color`, `@w:themeColor`), `#rrggbb`. `None`
    /// draws the underline in the text colour.
    pub underline_color: Option<String>,
    /// Single strikethrough (`w:strike`).
    pub strike: bool,
    /// Double strikethrough (`w:dstrike`).
    pub double_strike: bool,
    /// Text colour (`#rrggbb`), or `None` for the default (black).
    pub color: Option<String>,
    /// Highlight fill colour (`#rrggbb`).
    pub highlight: Option<String>,
    /// Vertical alignment (baseline/superscript/subscript).
    pub vert_align: VertAlign,
    /// All-capitals (`w:caps`).
    pub caps: bool,
    /// Small capitals (`w:smallCaps`): lowercase letters are drawn as smaller
    /// capitals.
    pub small_caps: bool,
    /// Baseline raise (positive) or lowering (negative) from `w:position`, in
    /// points.
    pub position_pt: f64,
    /// Run shading fill (`w:rPr/w:shd`), `#rrggbb`.
    pub shading: Option<String>,
    /// Hidden text (`w:vanish`).
    pub vanish: bool,
    /// Complex-script face (`w:rFonts/@w:cs`). Latin and Cyrillic keep [`Self::family`].
    pub complex_family: Option<String>,
    /// East Asian face (`w:rFonts/@w:eastAsia` or `@w:eastAsiaTheme`), used for
    /// CJK text.
    pub east_asia_family: Option<String>,
    /// Substitute for an unavailable East Asian face.
    pub east_asia_family_fallback: Option<String>,
    /// Character width scale (`w:w`). `1.0` is 100% of the normal advance.
    pub char_scale: f64,
}

impl Default for ComputedRun {
    fn default() -> Self {
        Self {
            rotation_deg: 0.0,
            family: "Calibri".to_owned(),
            family_fallback: None,
            complex_family_fallback: None,
            size_pt: 11.0,
            spacing_pt: 0.0,
            bold: false,
            italic: false,
            underline: None,
            underline_color: None,
            strike: false,
            double_strike: false,
            color: None,
            highlight: None,
            vert_align: VertAlign::Baseline,
            caps: false,
            small_caps: false,
            position_pt: 0.0,
            shading: None,
            vanish: false,
            complex_family: None,
            east_asia_family: None,
            east_asia_family_fallback: None,
            char_scale: 1.0,
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
    /// Frame attributes after the style cascade and direct overrides.
    pub frame: Option<FrameProperties>,
    /// Alignment.
    pub alignment: Justification,
    /// Space before, in points.
    pub space_before_pt: f64,
    /// Space after, in points.
    pub space_after_pt: f64,
    /// Extra gap above the first line from `w:pBdr/w:top` (`space` + width), pt.
    pub border_before_pt: f64,
    /// Extra gap below the last line from `w:pBdr/w:bottom` (`space` + width), pt.
    pub border_after_pt: f64,
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
    /// Paragraph shading fill (`w:shd/@w:fill`), `#rrggbb`.
    pub shading: Option<String>,
    /// Paragraph border edges (`w:pBdr`) after the cascade.
    pub borders: ParagraphBorders,
    /// Ignore this paragraph's spacing next to a paragraph of the same style
    /// (`w:contextualSpacing`).
    pub contextual_spacing: bool,
    /// Keep at least two lines of the paragraph on each page (`w:widowControl`).
    pub widow_control: bool,
    /// Leave the paragraph's lines out of line numbering (`w:suppressLineNumbers`).
    pub suppress_line_numbers: bool,
    /// `w:spacing/@w:beforeAutospacing`: the space before is Word's HTML
    /// spacing, not `w:before`.
    pub before_autospacing: bool,
    /// `w:spacing/@w:afterAutospacing`.
    pub after_autospacing: bool,
    /// Effective paragraph style (`w:pStyle`, else the default paragraph style).
    pub style: Option<strict_ooxml_wml::model::StyleId>,
    /// Numbering reference.
    pub numbering: Option<NumberingRef>,
    /// Default run format for runs without explicit properties.
    pub default_run: ComputedRun,
}

impl Default for ComputedParagraph {
    fn default() -> Self {
        Self {
            frame: None,
            alignment: Justification::Start,
            space_before_pt: 0.0,
            space_after_pt: 0.0,
            border_before_pt: 0.0,
            border_after_pt: 0.0,
            line_pt: None,
            line_rule: LineSpacingRule::Auto,
            indent_start_pt: 0.0,
            indent_end_pt: 0.0,
            first_line_pt: 0.0,
            tabs: Vec::new(),
            keep_lines: false,
            keep_next: false,
            page_break_before: false,
            shading: None,
            borders: ParagraphBorders::default(),
            contextual_spacing: false,
            widow_control: false,
            suppress_line_numbers: false,
            before_autospacing: false,
            after_autospacing: false,
            style: None,
            numbering: None,
            default_run: ComputedRun::default(),
        }
    }
}

/// The edges of a paragraph border (`w:pBdr`), each `None` when not drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphBorders {
    /// Top edge.
    pub top: Option<ResolvedBorder>,
    /// Bottom edge.
    pub bottom: Option<ResolvedBorder>,
    /// Leading edge.
    pub left: Option<ResolvedBorder>,
    /// Trailing edge.
    pub right: Option<ResolvedBorder>,
    /// Edge between consecutive paragraphs with the same borders.
    pub between: Option<ResolvedBorder>,
}

impl ParagraphBorders {
    /// Whether any edge is drawn.
    #[must_use]
    pub fn any(&self) -> bool {
        self.top.is_some()
            || self.bottom.is_some()
            || self.left.is_some()
            || self.right.is_some()
            || self.between.is_some()
    }
}

/// Space Word gives a paragraph whose `w:spacing` asks for autospacing, in pt.
pub const AUTOSPACING_PT: f64 = 14.0;

/// Points in one line of `w:beforeLines`/`w:afterLines` (hundredths of a
/// line of the 12 pt grid Word measures them on).
const SPACING_LINE_PT: f64 = 12.0;

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

    // A cell paragraph takes its table style (and the `w:tblStylePr`
    // conditions of its cell) before the paragraph style (§17.7.2).
    apply_table_style(document, &mut computed, theme);

    let style_id = para.props.style.as_ref().or_else(|| {
        document
            .styles
            .default_for(strict_ooxml_wml::model::values::StyleType::Paragraph)
    });
    if let Some(style_id) = style_id {
        apply_paragraph_style(document, &mut computed, style_id, theme);
    }
    apply_paragraph_props_mode(&mut computed, &para.props, theme, ToggleMode::Direct);
    resolve_font_hints(document, &mut computed.default_run);
    computed.style = style_id.cloned();
    if computed.before_autospacing {
        computed.space_before_pt = AUTOSPACING_PT;
    }
    if computed.after_autospacing {
        computed.space_after_pt = AUTOSPACING_PT;
    }
    computed.border_before_pt = border_pad_pt(computed.borders.top.as_ref());
    computed.border_after_pt = border_pad_pt(computed.borders.bottom.as_ref());
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
    apply_direct_run_props(&mut computed, &run.props, theme);
    resolve_font_hints(document, &mut computed);
    // `auto` text colour follows the run's own shading: Word draws it white
    // on a dark fill.
    if computed.color.is_none() && computed.shading.as_deref().is_some_and(is_dark) {
        computed.color = Some("#ffffff".to_owned());
    }
    computed
}

fn resolve_font_hints(document: &Document, run: &mut ComputedRun) {
    run.family_fallback = font_hint_substitute(document, &run.family);
    run.complex_family_fallback = run
        .complex_family
        .as_deref()
        .and_then(|family| font_hint_substitute(document, family));
    run.east_asia_family_fallback = run
        .east_asia_family
        .as_deref()
        .and_then(|family| font_hint_substitute(document, family));
}

fn font_hint_substitute(document: &Document, family: &str) -> Option<String> {
    if crate::font::family::known_family(family).is_some() {
        return None;
    }
    let hints = &document
        .font_table
        .as_ref()?
        .fonts
        .iter()
        .find(|font| font.name.eq_ignore_ascii_case(family.trim()))?
        .hints;
    if let Some(mapped) = hints
        .alt_name
        .as_deref()
        .and_then(crate::font::family::known_family)
    {
        return Some(mapped.to_owned());
    }
    let mapped = if hints.pitch.as_deref() == Some("fixed") {
        "Cousine"
    } else {
        match hints.family.as_deref() {
            Some("roman") => "Tinos",
            Some("swiss") => "Arimo",
            Some("modern") => "Cousine",
            _ => "Carlito",
        }
    };
    Some(mapped.to_owned())
}

/// Applies the table style of the cell being laid out, if any.
///
/// Base `pPr`/`rPr` of the style chain toggle like any style; a condition's
/// `rPr` assigns, so a style-wide bold and a `firstRow` bold stay bold.
#[inline(never)]
fn apply_table_style(document: &Document, computed: &mut ComputedParagraph, theme: Option<&Theme>) {
    let Some((style_id, mask)) = crate::table_style::current_cell() else {
        return;
    };
    let Some(style) = document.styles.get(&style_id) else {
        return;
    };
    for layer in crate::table_style::chain(document, style) {
        apply_paragraph_props(computed, &layer.paragraph, theme);
        apply_run_props(&mut computed.default_run, &layer.run, theme);
    }
    for condition in crate::table_style::conditions(document, style, mask) {
        apply_paragraph_props(computed, &condition.paragraph, theme);
        apply_direct_run_props(&mut computed.default_run, &condition.run, theme);
    }
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

/// Merges paragraph properties from a style or from document defaults.
///
/// The paragraph's own `pPr` is applied with direct assignment, so a mark and
/// a run that both enable a toggle do not cancel.
pub fn apply_paragraph_props(
    computed: &mut ComputedParagraph,
    props: &ParagraphProperties,
    theme: Option<&Theme>,
) {
    apply_paragraph_props_mode(computed, props, theme, ToggleMode::Cascade);
}

fn apply_paragraph_props_mode(
    computed: &mut ComputedParagraph,
    props: &ParagraphProperties,
    theme: Option<&Theme>,
    mark: ToggleMode,
) {
    if let Some(frame) = &props.frame {
        let inherited = computed.frame.get_or_insert_with(FrameProperties::default);
        macro_rules! overlay {
            ($($field:ident),+ $(,)?) => { $(
                if frame.$field.is_some() {
                    inherited.$field = frame.$field.clone();
                }
            )+ };
        }
        overlay!(
            drop_cap,
            lines,
            width,
            height,
            v_space,
            h_space,
            wrap,
            h_anchor,
            v_anchor,
            x,
            x_align,
            y,
            y_align,
            height_rule,
            anchor_lock
        );
    }
    if let Some(alignment) = props.alignment {
        computed.alignment = alignment;
    }
    if let Some(spacing) = &props.spacing {
        apply_spacing(computed, spacing);
    }
    if let Some(indentation) = &props.indentation {
        apply_indentation(computed, indentation);
    }
    // `w:pBdr`: each edge a layer names replaces the inherited one, and a
    // `nil`/`none` edge removes it. The pads above and below are derived from
    // the final edges in `compute_paragraph`.
    let edges = [
        (&props.borders.top, &mut computed.borders.top),
        (&props.borders.bottom, &mut computed.borders.bottom),
        (&props.borders.start, &mut computed.borders.left),
        (&props.borders.end, &mut computed.borders.right),
        (&props.borders.between, &mut computed.borders.between),
    ];
    for (declared, target) in edges {
        if let Some(border) = declared {
            *target = crate::layout::border::resolve_border(border, theme);
        }
    }
    apply_flag(&mut computed.contextual_spacing, props.contextual_spacing);
    apply_flag(&mut computed.widow_control, props.widow_control);
    apply_flag(
        &mut computed.suppress_line_numbers,
        props.suppress_line_numbers,
    );
    if let Some(fill) = props
        .shading
        .as_ref()
        .and_then(|shading| shading_fill_color(shading, theme))
    {
        computed.shading = Some(fill);
    }
    if !props.tabs.is_empty() {
        computed.tabs.clone_from(&props.tabs);
    }
    apply_flag(&mut computed.keep_lines, props.keep_lines);
    apply_flag(&mut computed.keep_next, props.keep_next);
    apply_flag(&mut computed.page_break_before, props.page_break_before);
    if let Some(numbering) = &props.numbering {
        if let Some(num_id) = numbering.num_id {
            computed.numbering = Some(NumberingRef {
                num_id: num_id.0,
                ilvl: numbering.ilvl.map_or(0, |ilvl| ilvl.0),
            });
        } else if let (Some(ilvl), Some(inherited)) = (numbering.ilvl, computed.numbering.as_mut())
        {
            // `w:ilvl` alone moves the inherited list to another level.
            inherited.ilvl = ilvl.0;
        }
    }
    if let Some(run) = &props.run_props {
        // A style mark toggles. The paragraph's own mark assigns, once.
        apply_run_props_mode(&mut computed.default_run, run, theme, mark);
    }
}

fn apply_spacing(computed: &mut ComputedParagraph, spacing: &Spacing) {
    if let Some(before) = spacing.before {
        computed.space_before_pt = f64::from(before.value()) / 20.0;
    }
    if let Some(after) = spacing.after {
        computed.space_after_pt = f64::from(after.value()) / 20.0;
    }
    // `w:beforeLines`/`w:afterLines` take precedence over the twips value
    // in the same element (ISO/IEC 29500-1 §17.3.1.33).
    if let Some(lines) = spacing.before_lines {
        computed.space_before_pt = f64::from(lines.max(0)) / 100.0 * SPACING_LINE_PT;
    }
    if let Some(lines) = spacing.after_lines {
        computed.space_after_pt = f64::from(lines.max(0)) / 100.0 * SPACING_LINE_PT;
    }
    if let Some(auto) = spacing.before_autospacing {
        computed.before_autospacing = auto;
    }
    if let Some(auto) = spacing.after_autospacing {
        computed.after_autospacing = auto;
    }
    if let Some(rule) = spacing.line_rule {
        computed.line_rule = rule;
    }
    if let Some(line) = spacing.line {
        computed.line_pt = Some(f64::from(line.value()) / 20.0);
    }
}

/// Layout gap from one paragraph border edge: `w:space` plus the band the
/// edge's lines take, in points.
fn border_pad_pt(border: Option<&ResolvedBorder>) -> f64 {
    border.map_or(0.0, |border| border.space_pt + border.band_pt())
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

/// How a toggle property combines with the value inherited so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToggleMode {
    /// Style hierarchy: `On` flips the inherited bit (ISO/IEC 29500-1 §17.7.3).
    Cascade,
    /// Direct formatting states the bit. `On` does not flip a mark or a style.
    Direct,
}

/// Merges run properties from a style or from document defaults.
///
/// Toggle properties XOR. Direct formatting goes through `apply_direct_run_props`.
pub fn apply_run_props(computed: &mut ComputedRun, props: &RunProperties, theme: Option<&Theme>) {
    apply_run_props_mode(computed, props, theme, ToggleMode::Cascade);
}

/// Merges direct `rPr` (`w:r` or the paragraph mark) without toggling.
fn apply_direct_run_props(
    computed: &mut ComputedRun,
    props: &RunProperties,
    theme: Option<&Theme>,
) {
    apply_run_props_mode(computed, props, theme, ToggleMode::Direct);
}

/// Merges `props` onto `computed`.
fn apply_run_props_mode(
    computed: &mut ComputedRun,
    props: &RunProperties,
    theme: Option<&Theme>,
    mode: ToggleMode,
) {
    if let Some(fonts) = &props.fonts {
        apply_fonts(computed, fonts, theme);
    }
    apply_toggle(&mut computed.bold, props.bold, mode);
    apply_toggle(&mut computed.italic, props.italic, mode);
    if let Some(underline) = props.underline {
        computed.underline = (underline != Underline::None).then_some(underline);
    }
    if let Some(color) = props
        .underline_theme
        .as_deref()
        .and_then(|reference| resolve_theme_color(reference, theme))
        .or_else(|| props.underline_color.as_ref().and_then(parse_color))
    {
        computed.underline_color = Some(color);
    } else if props
        .underline_color
        .as_ref()
        .is_some_and(|color| color.as_str().eq_ignore_ascii_case("auto"))
    {
        computed.underline_color = None;
    }
    apply_toggle(&mut computed.strike, props.strike, mode);
    apply_toggle(&mut computed.double_strike, props.double_strike, mode);
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
    if let Some(spacing) = props.spacing {
        computed.spacing_pt = f64::from(spacing.value()) / 20.0;
    }
    if let Some(position) = props.position {
        // `w:position` is in half-points; positive raises the text.
        computed.position_pt = f64::from(position.value()) / 2.0;
    }
    if let Some(shading) = &props.shading {
        computed.shading = shading_fill_color(shading, theme);
    }
    if let Some(scale) = props.scale {
        // `w:w` is a percentage of the normal advance (`100` and `100%`).
        computed.char_scale = f64::from(scale) / 100.0;
    }
    // Complex-script toggles (`w:bCs`/`w:iCs`/`w:szCs`) are a parallel channel.
    // Direct run props: when RTL or a CS face is named, CS toggles assign (so
    // `i=0` + `iCs` still italics). Cascade styles: never XOR Latin On with CS
    // On from the same `rPr` — Clio style 50 has `<w:b/><w:bCs/>` plus `w:cs`
    // and that cancel (On XOR On → Off) dropped Figure-caption bold.
    let complex = props.rtl.is_on()
        || props
            .fonts
            .as_ref()
            .is_some_and(|fonts| fonts.complex_script.is_some());
    if complex {
        match mode {
            ToggleMode::Direct => {
                apply_toggle(&mut computed.bold, props.bold_cs, ToggleMode::Direct);
                apply_toggle(&mut computed.italic, props.italic_cs, ToggleMode::Direct);
                if props.size.is_none() {
                    if let Some(size) = props.size_cs {
                        computed.size_pt = f64::from(size.value()) / 2.0;
                    }
                }
            }
            ToggleMode::Cascade => {
                if props.bold == TriState::Absent {
                    apply_toggle(&mut computed.bold, props.bold_cs, mode);
                }
                if props.italic == TriState::Absent {
                    apply_toggle(&mut computed.italic, props.italic_cs, mode);
                }
                if props.size.is_none() {
                    if let Some(size) = props.size_cs {
                        computed.size_pt = f64::from(size.value()) / 2.0;
                    }
                }
            }
        }
    }
    if let Some(vert) = props.vert_align {
        computed.vert_align = vert;
    }
    apply_toggle(&mut computed.caps, props.caps, mode);
    apply_toggle(&mut computed.small_caps, props.small_caps, mode);
    apply_toggle(&mut computed.vanish, props.vanish, mode);
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
    if let Some(family) = fonts.complex_script.as_ref() {
        // Kept for complex-script characters. It must not replace ascii/hAnsi:
        // a Cyrillic run with `w:cs` still uses the Latin face (A11).
        computed.complex_family = Some(family.to_string());
    } else if let Some(family) = theme_font(fonts.cs_theme.as_deref(), theme) {
        computed.complex_family = Some(family);
    }
    if let Some(family) = fonts.east_asia.as_ref() {
        computed.east_asia_family = Some(family.to_string());
    } else if let Some(family) = theme_font(fonts.east_asia_theme.as_deref(), theme) {
        computed.east_asia_family = Some(family);
    }
}

/// The face a run's text is measured and painted with.
///
/// Latin and Cyrillic stay on ascii/hAnsi (or the theme face that resolved
/// there). The complex-script face is used only when the text has no Latin or
/// Cyrillic letters.
#[must_use]
pub fn chosen_family(run: &ComputedRun, text: &str) -> String {
    let (requested, fallback) = if run.complex_family.is_some() && script_is_complex_only(text) {
        (
            run.complex_family.as_deref().unwrap_or(&run.family),
            run.complex_family_fallback.as_deref(),
        )
    } else if run.east_asia_family.is_some() && script_is_east_asian_only(text) {
        (
            run.east_asia_family.as_deref().unwrap_or(&run.family),
            run.east_asia_family_fallback.as_deref(),
        )
    } else {
        (run.family.as_str(), run.family_fallback.as_deref())
    };
    crate::font::family::known_family(requested)
        .or(fallback)
        .unwrap_or("Carlito")
        .to_owned()
}

/// Extra advance, in px, added after each character of `run`.
///
/// ECMA-376 17.3.2.35: `w:spacing` is pitch inserted after every character
/// before the next is drawn. Layout width and paint `x` lists must use the
/// same value or wrap and WPS glyph origins diverge.
#[must_use]
pub fn spacing_px(run: &ComputedRun, size_px: f64) -> f64 {
    if run.spacing_pt == 0.0 || run.size_pt == 0.0 {
        0.0
    } else {
        run.spacing_pt * (size_px / run.size_pt)
    }
}

/// Whether `w:characterSpacingControl` asks for punctuation compression.
#[must_use]
pub fn compress_punctuation(control: Option<&str>) -> bool {
    matches!(
        control,
        Some("compressPunctuation" | "compressPunctuationAndJapaneseKana")
    )
}

/// Western punctuation that may swallow a following space under compressPunctuation.
#[must_use]
pub fn is_space_compressing_punct(ch: char) -> bool {
    matches!(ch, ',' | ')')
}

/// Plain-space advance factor under `compressPunctuation` for this run.
///
/// Right-aligned tiny captions (Clio style 331 `A,C`) keep ~76% of each
/// inter-word space so the line width matches the spaceless WPS PDF. Center
/// legends (`B,D`) and body runs keep full spaces — a blanket shrink moves
/// center lines both ways and breaks body `modern` hits.
#[must_use]
pub fn plain_space_factor(alignment: Justification, size_pt: f64) -> f64 {
    if size_pt > 0.0 && size_pt <= 5.0 && matches!(alignment, Justification::End) {
        0.76
    } else {
        1.0
    }
}

/// Advance factor for `ch` when punctuation compression is active.
///
/// Collapse spaces after `,` / `)` (`Europe, East`, `(A, B)`) so Clio page-104
/// keeps `For` on the preceding line. Other spaces use `plain_space_factor`.
#[must_use]
pub fn compressed_char_factor(
    prev: Option<char>,
    ch: char,
    _next: Option<char>,
    compress: bool,
    plain_space_factor: f64,
) -> f64 {
    if !compress || !ch.is_whitespace() {
        return 1.0;
    }
    if prev.is_some_and(is_space_compressing_punct) {
        0.0
    } else {
        plain_space_factor.clamp(0.0, 1.0)
    }
}

/// Whether `text` is a complex script, with no Latin or Cyrillic letters.
pub(crate) fn script_is_complex_only(text: &str) -> bool {
    let mut complex = false;
    for ch in text.chars() {
        if is_latin_or_cyrillic(ch) {
            return false;
        }
        if is_complex_script(ch) {
            complex = true;
        }
    }
    complex
}

/// Whether `text` is East Asian (CJK) script, with no Latin or Cyrillic letters.
pub(crate) fn script_is_east_asian_only(text: &str) -> bool {
    let mut east_asian = false;
    for ch in text.chars() {
        if is_latin_or_cyrillic(ch) {
            return false;
        }
        if crate::font::metrics::is_wide_character(ch) {
            east_asian = true;
        }
    }
    east_asian
}

fn is_latin_or_cyrillic(ch: char) -> bool {
    matches!(
        ch,
        '\u{0041}'..='\u{024F}' | '\u{0400}'..='\u{052F}' | '\u{1E00}'..='\u{1EFF}' | '\u{2DE0}'
            ..='\u{2DFF}' | '\u{A640}'..='\u{A69F}'
    )
}

fn is_complex_script(ch: char) -> bool {
    matches!(
        ch,
        '\u{0590}'..='\u{08FF}'
            | '\u{0900}'..='\u{0DFF}'
            | '\u{0E00}'..='\u{0E7F}'
            | '\u{0E80}'..='\u{0EFF}'
            | '\u{1000}'..='\u{109F}'
            | '\u{1780}'..='\u{17FF}'
    )
}

/// Resolves a `w:*Theme` font reference through `theme`.
fn theme_font(reference: Option<&str>, theme: Option<&Theme>) -> Option<String> {
    let reference = reference?;
    // A theme slot can be present but empty (`<a:cs typeface=""/>`): that names
    // no face, so it must not replace the inherited one.
    theme?
        .font(reference)
        .filter(|name| !name.trim().is_empty())
        .map(ToString::to_string)
}

/// Resolves a themed run colour (`w:themeColor`/`themeTint`/`themeShade`).
pub(crate) fn resolve_theme_color(
    reference: &ThemeColorRef,
    theme: Option<&Theme>,
) -> Option<String> {
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
            // Both products are of `u8`s divided by 255, so they land in 0..=255 and
            // the `u8` is exact; the fallback says what would happen if it were
            // not, rather than truncating (AUD-09).
            u8::try_from(u16::from(channel) * u16::from(shade) / 255).unwrap_or(u8::MAX)
        } else if let Some(tint) = tint {
            u8::try_from(255 - (255 - u16::from(channel)) * (255 - u16::from(tint)) / 255)
                .unwrap_or(0)
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
    let red = u8::from_str_radix(digits.get(0..2)?, 16).ok()?;
    let green = u8::from_str_radix(digits.get(2..4)?, 16).ok()?;
    let blue = u8::from_str_radix(digits.get(4..6)?, 16).ok()?;
    Some((red, green, blue))
}

/// Non-toggle on/off: `On`/`Off` override, `Absent` inherits.
fn apply_flag(computed: &mut bool, state: TriState) {
    match state {
        TriState::On => *computed = true,
        TriState::Off => *computed = false,
        TriState::Absent => {}
    }
}

/// Toggle property. Cascade `On` flips; direct `On` sets. `Off` clears either way.
fn apply_toggle(computed: &mut bool, state: TriState, mode: ToggleMode) {
    match (state, mode) {
        (TriState::Absent, _) => {}
        (TriState::Off, _) => *computed = false,
        (TriState::On, ToggleMode::Cascade) => *computed = !*computed,
        (TriState::On, ToggleMode::Direct) => *computed = true,
    }
}

/// The colour a `w:shd` paints, as `#rrggbb`, or `None` for no shading.
///
/// The background is `w:themeFill` (with tint/shade) when the theme resolves
/// it, else `w:fill`. `w:val` lays a pattern in the foreground colour
/// (`w:themeColor`, else `w:color`) over it: `solid` covers it, `pctN` covers
/// N percent, and the hatches cover the share of a tile their lines take. The
/// pattern is painted as that blend rather than as a tile — at text sizes the
/// eye sees the mix, and both backends can fill a rectangle. `auto` is white
/// as a background and black as a foreground, as in Word.
pub(crate) fn shading_fill_color(shading: &Shading, theme: Option<&Theme>) -> Option<String> {
    let fill = shading
        .theme_fill
        .as_ref()
        .and_then(|reference| resolve_theme_color(reference, theme))
        .or_else(|| shading.fill.as_ref().and_then(parse_color));
    let pattern = shading.pattern.as_deref().unwrap_or("clear");
    if pattern == "nil" {
        return None;
    }
    let coverage = shading_pattern_coverage(pattern);
    if coverage == 0 {
        return fill;
    }
    let foreground = shading
        .theme_color
        .as_deref()
        .and_then(|reference| resolve_theme_color(reference, theme))
        .or_else(|| shading.color.as_ref().and_then(parse_color))
        .unwrap_or_else(|| "#000000".to_owned());
    let background = fill.unwrap_or_else(|| "#ffffff".to_owned());
    blend_hex(&background, &foreground, coverage)
}

/// Share of a `w:shd` tile the foreground covers (`ST_Shd`), in thousandths:
/// `0` for `clear` and for values this renderer does not know.
fn shading_pattern_coverage(pattern: &str) -> u32 {
    if pattern == "solid" {
        return 1000;
    }
    if let Some(percent) = pattern
        .strip_prefix("pct")
        .and_then(|value| value.parse::<u32>().ok())
    {
        return percent.min(100) * 10;
    }
    match pattern {
        "horzStripe" | "vertStripe" | "reverseDiagStripe" | "diagStripe" => 500,
        "horzCross" | "diagCross" => 750,
        "thinHorzStripe" | "thinVertStripe" | "thinReverseDiagStripe" | "thinDiagStripe" => 250,
        "thinHorzCross" | "thinDiagCross" => 440,
        _ => 0,
    }
}

/// Mixes two `#rrggbb` colours: `per_mille` thousandths of `top` over `base`.
fn blend_hex(base: &str, top: &str, per_mille: u32) -> Option<String> {
    let (br, bg, bb) = parse_hex(base)?;
    let (tr, tg, tb) = parse_hex(top)?;
    let amount = per_mille.min(1000);
    let mix = |b: u8, t: u8| -> u8 {
        let value = (u32::from(b) * (1000 - amount) + u32::from(t) * amount + 500) / 1000;
        u8::try_from(value).unwrap_or(u8::MAX)
    };
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        mix(br, tr),
        mix(bg, tg),
        mix(bb, tb)
    ))
}

/// Whether a `#rrggbb` colour is dark enough that Word draws `auto` text on it
/// in white.
fn is_dark(color: &str) -> bool {
    parse_hex(color).is_some_and(|(red, green, blue)| {
        let luminance = 0.299 * f64::from(red) + 0.587 * f64::from(green) + 0.114 * f64::from(blue);
        luminance < 128.0
    })
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
            font_table: None,
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
                font_table: None,
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
            revision: None,
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
            aliases: None,
            based_on: None,
            next: None,
            link: None,
            is_default: false,
            custom_style: false,
            auto_redefine: false,
            semi_hidden: false,
            hidden: false,
            q_format: false,
            locked: false,
            unhide_when_used: false,
            ui_priority: None,
            table: strict_ooxml_wml::model::props::TableProperties::default(),
            row: strict_ooxml_wml::model::props::RowProperties::default(),
            cell: strict_ooxml_wml::model::props::CellProperties::default(),
            paragraph: ParagraphProperties::default(),
            run: RunProperties {
                bold: TriState::On,
                ..RunProperties::default()
            },
            conditions: Vec::new(),
            based_on_chain: Vec::new(),
            location: location(),
        };
        base.run.size = Some(strict_ooxml_wml::model::values::HalfPoints(28));
        let mut child = base.clone();
        child.id = strict_ooxml_wml::model::StyleId::new("Child");
        child.based_on = Some(strict_ooxml_wml::model::StyleId::new("Base"));
        child.based_on_chain = vec![strict_ooxml_wml::model::StyleId::new("Base")];
        // XOR toggles (AUD-44): only set what this style adds; keep bold Absent
        // so Base's `w:b` is not flipped off by a cloned On.
        child.run.bold = TriState::Absent;
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
            revision: None,
            location: location(),
        };
        let computed = compute_paragraph(&document, &para);
        assert!(computed.default_run.bold, "inherited from Base");
        assert!(computed.default_run.italic, "from Child");
        assert_eq!(computed.default_run.size_pt, 14.0, "from Base");
        assert!(computed.default_run.underline.is_some(), "from pPr/rPr");
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
            revision: None,
            location: location(),
        };
        let computed = compute_run(&document, &para, &run);
        assert_eq!(computed.color.as_deref(), Some("#ff0000"));
        assert_eq!(super::apply_caps("ab", &computed), "ab");
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
