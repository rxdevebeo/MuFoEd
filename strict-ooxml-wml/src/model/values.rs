//! Enumerated Strict values and unit newtypes.
//!
//! Every enum exposes `from_strict` (strict lexical parsing) and `as_str`. An
//! unknown value yields `None`; the parser then applies the schema default and
//! records an `InvalidEnumValue` entry in the support model (STAGE-2 §7.4).

use std::fmt;
use std::sync::Arc;

/// Defines a copyable enum with strict lexical parsing.
macro_rules! strict_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $lit:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// Parses the Strict lexical value, returning `None` if unknown.
            #[must_use]
            pub fn from_strict(value: &str) -> Option<Self> {
                match value {
                    $( $lit => Some(Self::$variant), )+
                    _ => None,
                }
            }

            /// Returns the Strict lexical value of this variant.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $lit, )+
                }
            }
        }
    };
}

strict_enum! {
    /// `ST_Jc` — paragraph alignment (direction-neutral Strict values).
    pub enum Justification {
        /// `start`.
        Start => "start",
        /// `end`.
        End => "end",
        /// `center`.
        Center => "center",
        /// `both`.
        Both => "both",
        /// `distribute`.
        Distribute => "distribute",
        /// `justify`.
        Justify => "justify",
        /// `mediumKashida`.
        MediumKashida => "mediumKashida",
        /// `highKashida`.
        HighKashida => "highKashida",
        /// `lowKashida`.
        LowKashida => "lowKashida",
        /// `thaiDistribute`.
        ThaiDistribute => "thaiDistribute",
    }
}

strict_enum! {
    /// `ST_Underline` — underline style.
    pub enum Underline {
        /// `single`.
        Single => "single",
        /// `words`.
        Words => "words",
        /// `double`.
        Double => "double",
        /// `thick`.
        Thick => "thick",
        /// `dotted`.
        Dotted => "dotted",
        /// `dottedHeavy`.
        DottedHeavy => "dottedHeavy",
        /// `dash`.
        Dash => "dash",
        /// `dashedHeavy`.
        DashedHeavy => "dashedHeavy",
        /// `dashLong`.
        DashLong => "dashLong",
        /// `dashLongHeavy`.
        DashLongHeavy => "dashLongHeavy",
        /// `dotDash`.
        DotDash => "dotDash",
        /// `dashDotHeavy`.
        DashDotHeavy => "dashDotHeavy",
        /// `dotDotDash`.
        DotDotDash => "dotDotDash",
        /// `dashDotDotHeavy`.
        DashDotDotHeavy => "dashDotDotHeavy",
        /// `wave`.
        Wave => "wave",
        /// `wavyHeavy`.
        WavyHeavy => "wavyHeavy",
        /// `wavyDouble`.
        WavyDouble => "wavyDouble",
        /// `none`.
        None => "none",
    }
}

strict_enum! {
    /// `ST_VerticalAlignRun` — vertical run alignment.
    pub enum VertAlign {
        /// `baseline`.
        Baseline => "baseline",
        /// `superscript`.
        Superscript => "superscript",
        /// `subscript`.
        Subscript => "subscript",
    }
}

strict_enum! {
    /// `ST_HighlightColor` — text highlight colour.
    pub enum Highlight {
        /// `black`.
        Black => "black",
        /// `blue`.
        Blue => "blue",
        /// `cyan`.
        Cyan => "cyan",
        /// `green`.
        Green => "green",
        /// `magenta`.
        Magenta => "magenta",
        /// `red`.
        Red => "red",
        /// `yellow`.
        Yellow => "yellow",
        /// `white`.
        White => "white",
        /// `darkBlue`.
        DarkBlue => "darkBlue",
        /// `darkCyan`.
        DarkCyan => "darkCyan",
        /// `darkGreen`.
        DarkGreen => "darkGreen",
        /// `darkMagenta`.
        DarkMagenta => "darkMagenta",
        /// `darkRed`.
        DarkRed => "darkRed",
        /// `darkYellow`.
        DarkYellow => "darkYellow",
        /// `darkGray`.
        DarkGray => "darkGray",
        /// `lightGray`.
        LightGray => "lightGray",
        /// `none`.
        None => "none",
    }
}

strict_enum! {
    /// `ST_Border` — border line style.
    pub enum BorderStyle {
        /// `nil`.
        Nil => "nil",
        /// `none`.
        None => "none",
        /// `single`.
        Single => "single",
        /// `thick`.
        Thick => "thick",
        /// `double`.
        Double => "double",
        /// `dotted`.
        Dotted => "dotted",
        /// `dashed`.
        Dashed => "dashed",
        /// `dotDash`.
        DotDash => "dotDash",
        /// `dotDotDash`.
        DotDotDash => "dotDotDash",
        /// `triple`.
        Triple => "triple",
        /// `thinThickSmallGap`.
        ThinThickSmallGap => "thinThickSmallGap",
        /// `thickThinSmallGap`.
        ThickThinSmallGap => "thickThinSmallGap",
        /// `thinThickThinSmallGap`.
        ThinThickThinSmallGap => "thinThickThinSmallGap",
        /// `thinThickMediumGap`.
        ThinThickMediumGap => "thinThickMediumGap",
        /// `thickThinMediumGap`.
        ThickThinMediumGap => "thickThinMediumGap",
        /// `thinThickThinMediumGap`.
        ThinThickThinMediumGap => "thinThickThinMediumGap",
        /// `thinThickLargeGap`.
        ThinThickLargeGap => "thinThickLargeGap",
        /// `thickThinLargeGap`.
        ThickThinLargeGap => "thickThinLargeGap",
        /// `thinThickThinLargeGap`.
        ThinThickThinLargeGap => "thinThickThinLargeGap",
        /// `wave`.
        Wave => "wave",
        /// `doubleWave`.
        DoubleWave => "doubleWave",
        /// `dashSmallGap`.
        DashSmallGap => "dashSmallGap",
        /// `dashDotStroked`.
        DashDotStroked => "dashDotStroked",
        /// `threeDEmboss`.
        ThreeDEmboss => "threeDEmboss",
        /// `threeDEngrave`.
        ThreeDEngrave => "threeDEngrave",
        /// `outset`.
        Outset => "outset",
        /// `inset`.
        Inset => "inset",
    }
}

strict_enum! {
    /// `ST_LineSpacingRule` — line-spacing rule.
    pub enum LineSpacingRule {
        /// `auto`.
        Auto => "auto",
        /// `exact`.
        Exact => "exact",
        /// `atLeast`.
        AtLeast => "atLeast",
    }
}

strict_enum! {
    /// `ST_TabJc` — tab-stop alignment.
    pub enum TabAlignment {
        /// `start`.
        Start => "start",
        /// `end`.
        End => "end",
        /// `center`.
        Center => "center",
        /// `clear`.
        Clear => "clear",
        /// `decimal`.
        Decimal => "decimal",
        /// `bar`.
        Bar => "bar",
        /// `num`.
        Num => "num",
    }
}

impl TabAlignment {
    /// Parses `ST_TabJc`, also accepting the legacy `left`/`right` synonyms that
    /// occur in real-world markup (mapped to `start`/`end`).
    #[must_use]
    pub fn from_lexical(value: &str) -> Option<Self> {
        match value {
            "left" => Some(Self::Start),
            "right" => Some(Self::End),
            _ => Self::from_strict(value),
        }
    }
}

strict_enum! {
    /// `ST_TabTlc` — tab leader.
    pub enum TabLeader {
        /// `none`.
        None => "none",
        /// `dot`.
        Dot => "dot",
        /// `hyphen`.
        Hyphen => "hyphen",
        /// `underscore`.
        Underscore => "underscore",
        /// `heavy`.
        Heavy => "heavy",
        /// `middleDot`.
        MiddleDot => "middleDot",
    }
}

strict_enum! {
    /// `ST_SectionMark` — section break type.
    pub enum SectionType {
        /// `nextPage`.
        NextPage => "nextPage",
        /// `nextColumn`.
        NextColumn => "nextColumn",
        /// `continuous`.
        Continuous => "continuous",
        /// `evenPage`.
        EvenPage => "evenPage",
        /// `oddPage`.
        OddPage => "oddPage",
    }
}

strict_enum! {
    /// `ST_PageOrientation` — page orientation.
    pub enum PageOrientation {
        /// `portrait`.
        Portrait => "portrait",
        /// `landscape`.
        Landscape => "landscape",
    }
}

strict_enum! {
    /// `ST_DocGrid` — document grid type.
    pub enum DocGridType {
        /// `default`.
        Default => "default",
        /// `lines`.
        Lines => "lines",
        /// `linesAndChars`.
        LinesAndChars => "linesAndChars",
        /// `snapToChars`.
        SnapToChars => "snapToChars",
    }
}

strict_enum! {
    /// `ST_FldCharType` — complex field character type.
    pub enum FieldCharType {
        /// `begin`.
        Begin => "begin",
        /// `separate`.
        Separate => "separate",
        /// `end`.
        End => "end",
    }
}

strict_enum! {
    /// `ST_StyleType` — kind of a style definition.
    pub enum StyleType {
        /// `paragraph`.
        Paragraph => "paragraph",
        /// `character`.
        Character => "character",
        /// `table`.
        Table => "table",
        /// `numbering`.
        Numbering => "numbering",
    }
}

strict_enum! {
    /// `ST_TblLayoutType` — table layout algorithm.
    pub enum TableLayout {
        /// `fixed`.
        Fixed => "fixed",
        /// `autofit`.
        Autofit => "autofit",
    }
}

strict_enum! {
    /// `ST_HeightRule` — row-height rule.
    pub enum HeightRule {
        /// `auto`.
        Auto => "auto",
        /// `atLeast`.
        AtLeast => "atLeast",
        /// `exact`.
        Exact => "exact",
    }
}

strict_enum! {
    /// `ST_Merge` — vertically merged cell state.
    pub enum VerticalMerge {
        /// `restart`.
        Restart => "restart",
        /// `continue`.
        Continue => "continue",
    }
}

strict_enum! {
    /// `ST_TextDirection` - cell/paragraph text flow direction.
    ///
    /// The variant names are the **Transitional** spellings, because they read,
    /// and the lexical values are the **Strict** ones, because those are what
    /// `ST_TextDirection` enumerates: `tb`, `rl`, `lr`, `tbV`, `rlV`, `lrV`
    /// (`wml.xsd`, and ECMA-376 Part 1 §17.18.93). The two families name the
    /// same six directions.
    ///
    /// The writer used to emit the Transitional spelling, which is a value the
    /// Strict type does not have, so every written `w:textDirection` was rejected
    /// by the schema; and the reader matched none of the six Strict values, so a
    /// real Strict document's direction was recorded as an unknown enum and then
    /// written back in the spelling the schema rejects (`XS-26`).
    pub enum TextDirection {
        /// `tb` — top to bottom, left to right (Transitional `lrTb`).
        LrTb => "tb",
        /// `rl` — top to bottom, right to left (Transitional `tbRl`).
        TbRl => "rl",
        /// `lr` — bottom to top, left to right (Transitional `btLr`).
        BtLr => "lr",
        /// `tbV` — rotated (Transitional `lrTbV`).
        LrTbV => "tbV",
        /// `rlV` — rotated (Transitional `tbRlV`).
        TbRlV => "rlV",
        /// `lrV` — rotated (Transitional `tbLrV`).
        TbLrV => "lrV",
    }
}

strict_enum! {
    /// `ST_VerticalJc` — vertical justification of a table cell or section.
    pub enum VerticalJc {
        /// `top`.
        Top => "top",
        /// `center`.
        Center => "center",
        /// `bottom`.
        Bottom => "bottom",
    }
}

strict_enum! {
    /// `ST_LineNumberRestart` — line-number restart mode.
    pub enum LineNumberRestart {
        /// `newPage`.
        NewPage => "newPage",
        /// `newSection`.
        NewSection => "newSection",
        /// `continuous`.
        Continuous => "continuous",
    }
}

strict_enum! {
    /// `ST_BrType` — break kind (with `textWrapping` as the default).
    pub enum BreakKind {
        /// `page`.
        Page => "page",
        /// `column`.
        Column => "column",
        /// `textWrapping`.
        TextWrapping => "textWrapping",
    }
}

#[allow(clippy::derivable_impls)]
impl Default for BreakKind {
    fn default() -> Self {
        Self::TextWrapping
    }
}

/// A tri-state toggle for inheritable on/off properties (`ST_OnOff`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TriState {
    /// Explicitly enabled.
    On,
    /// Explicitly disabled.
    Off,
    /// Absent (inherit).
    #[default]
    Absent,
}

impl TriState {
    /// Parses a Strict on/off lexical value.
    #[must_use]
    pub fn from_strict(value: &str) -> Option<Self> {
        match value {
            "true" | "on" | "1" => Some(Self::On),
            "false" | "off" | "0" => Some(Self::Off),
            _ => None,
        }
    }

    /// Returns `true` only for [`TriState::On`].
    #[must_use]
    pub const fn is_on(self) -> bool {
        matches!(self, Self::On)
    }
}

/// `xml:space` handling for text runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Space {
    /// `default` — whitespace may be collapsed.
    #[default]
    Default,
    /// `preserve` — whitespace is significant.
    Preserve,
}

impl Space {
    /// Parses an `xml:space` value.
    #[must_use]
    pub fn from_strict(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "preserve" => Some(Self::Preserve),
            _ => None,
        }
    }
}

/// A measurement in twentieths of a point (1/20 pt).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Twips(pub i32);

impl Twips {
    /// Returns the raw twip value.
    #[must_use]
    pub const fn value(self) -> i32 {
        self.0
    }
}

/// A measurement in half-points (1/2 pt), used for font sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct HalfPoints(pub i32);

impl HalfPoints {
    /// Returns the raw half-point value.
    #[must_use]
    pub const fn value(self) -> i32 {
        self.0
    }
}

/// A measurement in eighths of a point, used for border widths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct EighthsPoint(pub u16);

impl EighthsPoint {
    /// Returns the raw eighths-of-a-point value.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// A measurement in English Metric Units (EMU), used by DrawingML extents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Emu(pub i64);

impl Emu {
    /// Returns the raw EMU value.
    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }
}

/// A colour value: `auto` or a six-digit hex triplet.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Color(Arc<str>);

impl Color {
    /// Creates a colour from its raw value.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    /// Returns the raw value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A theme colour reference (`w:themeColor/@w:val`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThemeColor(Arc<str>);

impl ThemeColor {
    /// Creates a theme colour reference from its raw value.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    /// Returns the raw value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A theme-colour reference with optional tint/shade (`w:color` theme attrs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeColorRef {
    /// Theme colour slot (`w:themeColor`), for example `accent1`.
    pub color: ThemeColor,
    /// Tint applied to the colour (`w:themeTint`), a hex byte.
    pub tint: Option<Arc<str>>,
    /// Shade applied to the colour (`w:themeShade`), a hex byte.
    pub shade: Option<Arc<str>>,
}

/// A shaded fill (`w:shd`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Shading {
    /// Pattern value (`w:val`), for example `clear`.
    pub pattern: Option<Arc<str>>,
    /// Pattern colour (`w:color`).
    pub color: Option<Color>,
    /// Background fill (`w:fill`).
    pub fill: Option<Color>,
    /// Theme reference for the pattern colour (`w:themeColor` + tint/shade).
    pub theme_color: Option<ThemeColorRef>,
    /// Theme reference for the fill (`w:themeFill` + themeFillTint/Shade).
    pub theme_fill: Option<ThemeColorRef>,
}

/// A single border edge (`w:top`, `w:left`, ...).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Border {
    /// Line style (`w:val`).
    pub style: Option<BorderStyle>,
    /// Line width in eighths of a point (`w:sz`).
    pub size: Option<EighthsPoint>,
    /// Border colour (`w:color`).
    pub color: Option<Color>,
    /// Theme colour reference (`w:themeColor` + tint/shade).
    pub theme_color: Option<ThemeColorRef>,
    /// Space between border and text, in points (`w:space`).
    pub space: Option<u16>,
    /// Whether the border is in the shadow (`w:shadow`).
    pub shadow: bool,
    /// Whether the border is a frame border (`w:frame`).
    pub frame: bool,
}

/// The set of borders belonging to a paragraph or table cell.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Borders {
    /// Top border.
    pub top: Option<Border>,
    /// Bottom border.
    pub bottom: Option<Border>,
    /// Start (leading) border.
    pub start: Option<Border>,
    /// End (trailing) border.
    pub end: Option<Border>,
    /// Inside horizontal border (tables).
    pub inside_horizontal: Option<Border>,
    /// Inside vertical border (tables).
    pub inside_vertical: Option<Border>,
}

/// The set of fonts for the four script classes (`w:rFonts`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Fonts {
    /// ASCII font.
    pub ascii: Option<Arc<str>>,
    /// High-ANSI font.
    pub h_ansi: Option<Arc<str>>,
    /// East-Asian font.
    pub east_asia: Option<Arc<str>>,
    /// Complex-script font.
    pub complex_script: Option<Arc<str>>,
    /// Font hint (`w:hint`).
    pub hint: Option<Arc<str>>,
    /// Theme ASCII font reference (`w:asciiTheme`), for example `minorHAnsi`.
    pub ascii_theme: Option<Arc<str>>,
    /// Theme high-ANSI font reference (`w:hAnsiTheme`).
    pub h_ansi_theme: Option<Arc<str>>,
    /// Theme East-Asian font reference (`w:eastAsiaTheme`).
    pub east_asia_theme: Option<Arc<str>>,
    /// Theme complex-script font reference (`w:cstheme`).
    pub cs_theme: Option<Arc<str>>,
}

impl Fonts {
    /// Applies a later `w:rFonts` in the same `w:rPr`.
    ///
    /// `CT_RPr` allows one `w:rFonts`. Some producers emit a second element that
    /// names only the complex-script face. The schema-legal reading is one
    /// element: each attribute the later element sets replaces that slot, and
    /// every slot it does not mention stays.
    pub fn overlay(&mut self, later: Self) {
        fn take(slot: &mut Option<Arc<str>>, next: Option<Arc<str>>) {
            if next.is_some() {
                *slot = next;
            }
        }
        take(&mut self.ascii, later.ascii);
        take(&mut self.h_ansi, later.h_ansi);
        take(&mut self.east_asia, later.east_asia);
        take(&mut self.complex_script, later.complex_script);
        take(&mut self.hint, later.hint);
        take(&mut self.ascii_theme, later.ascii_theme);
        take(&mut self.h_ansi_theme, later.h_ansi_theme);
        take(&mut self.east_asia_theme, later.east_asia_theme);
        take(&mut self.cs_theme, later.cs_theme);
    }
}

/// Paragraph spacing (`w:spacing`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Spacing {
    /// Space before, in twips.
    pub before: Option<Twips>,
    /// Space after, in twips.
    pub after: Option<Twips>,
    /// Line spacing measured per `rule`.
    pub line: Option<Twips>,
    /// Line-spacing rule.
    pub line_rule: Option<LineSpacingRule>,
    /// Space before, in hundredths of a line (`w:beforeLines`).
    pub before_lines: Option<i32>,
    /// Space after, in hundredths of a line (`w:afterLines`).
    pub after_lines: Option<i32>,
    /// `w:afterAutospacing`. `None` means the attribute was absent.
    pub after_autospacing: Option<bool>,
    /// `w:beforeAutospacing`. `None` means the attribute was absent.
    pub before_autospacing: Option<bool>,
}

/// Paragraph indentation (`w:ind`), direction-neutral.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Indentation {
    /// Leading indentation, in twips.
    pub start: Option<Twips>,
    /// Trailing indentation, in twips.
    pub end: Option<Twips>,
    /// First-line indentation, in twips.
    pub first_line: Option<Twips>,
    /// Hanging indentation, in twips.
    pub hanging: Option<Twips>,
    /// Leading indentation in character units.
    pub start_chars: Option<i32>,
    /// Trailing indentation in character units.
    pub end_chars: Option<i32>,
    /// First-line indentation in character units.
    pub first_line_chars: Option<i32>,
    /// Hanging indentation in character units.
    pub hanging_chars: Option<i32>,
}

/// A custom tab stop (`w:tab` inside `w:tabs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabStop {
    /// Position in twips.
    pub position: Twips,
    /// Alignment.
    pub alignment: TabAlignment,
    /// Leader.
    pub leader: Option<TabLeader>,
}

/// A run's underlining (`w:u`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct UnderlineSpec {
    /// Underline style.
    pub style: Option<Underline>,
    /// Underline colour.
    pub color: Option<HighlightOrColor>,
}

/// A colour that may be a highlight name or a raw colour value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlightOrColor {
    /// A named highlight colour.
    Highlight(Highlight),
    /// A raw colour value.
    Color(Color),
}

/// Run revision identifiers and paragraph revision identifiers.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Rsids {
    /// `w:rsidR` — revision id of the run.
    pub run: Option<Arc<str>>,
    /// `w:rsidRDefault` — default revision id of a paragraph.
    pub run_default: Option<Arc<str>>,
    /// `w:rsidP` — paragraph revision id.
    pub paragraph: Option<Arc<str>>,
    /// `w:rsidDel` — deletion revision id.
    pub deleted: Option<Arc<str>>,
    /// `w:rsidTr` — table row revision id.
    pub table_row: Option<Arc<str>>,
}

/// Width specification shared by tables and cells (`w:tblW`, `w:tcW`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Width {
    /// Width kind.
    pub kind: WidthKind,
    /// Value in the unit implied by `kind` (twips or fiftieths of a percent).
    pub value: Option<i32>,
}

/// Discriminator for a [`Width`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum WidthKind {
    /// Width measured in twips.
    #[default]
    Dxa,
    /// Width measured in fiftieths of a percent.
    Pct,
    /// Automatic width.
    Auto,
    /// No width (`nil`).
    Nil,
}

impl WidthKind {
    /// Parses a Strict width type.
    #[must_use]
    pub fn from_strict(value: &str) -> Option<Self> {
        match value {
            "dxa" => Some(Self::Dxa),
            "pct" => Some(Self::Pct),
            "auto" => Some(Self::Auto),
            "nil" => Some(Self::Nil),
            _ => None,
        }
    }
}

/// The lexical form of `ST_MeasurementOrPercent` for a fiftieths-of-a-percent
/// width (`Q-E5`, closed 2026-10-01).
///
/// **The arithmetic, which is the whole of the assumption the audit recorded.**
/// `ST_MeasurementOrPercent` in Strict is `union(ST_DecimalNumberOrPercent,
/// s:ST_UniversalMeasure)`, and in Strict `ST_DecimalNumberOrPercent` is
/// `union(s:ST_Percentage)` alone - `s:ST_Percentage`'s pattern is
/// `-?[0-9]+(\.[0-9]+)?%`. The Transitional set had a second branch,
/// `ST_UnqualifiedPercentage`, which is `xsd:integer`, and that is the branch a
/// bare `5000` took. Strict removed it, so the fiftieths reading has no lexical
/// form at all: `5000` is not a value the attribute can hold.
///
/// The remaining question the audit called "a question of behaviour" was what
/// `5000` meant, and it is answerable without a producer: `w:type="pct"` is
/// defined as fiftieths of a percent, so the value is 5000/50 = 100%, and the
/// lexical form carries the `%` the pattern demands. Nothing here depends on how
/// a consumer reads it, because there is no longer a spelling for it to read
/// ambiguously.
///
/// The fiftieths are exact in the other direction too: `i32 / 50` has at most
/// two decimals, and `s:ST_Percentage` admits at most two, so no precision is lost
/// and no value is rejected by the pattern.
#[must_use]
pub fn percent_from_fiftieths(fiftieths: i32) -> String {
    // AUD-45: work on abs via i64 so `i32::MIN` is representable and the sign
    // is applied once (previously `whole` stayed negative → `"--2%"`).
    let negative = fiftieths < 0;
    let abs = i64::from(fiftieths).unsigned_abs();
    let whole = abs / 50;
    let rest = abs % 50;
    let sign = if negative { "-" } else { "" };
    if rest == 0 {
        return format!("{sign}{whole}%");
    }
    // Two decimals, exactly: rest/50 has at most two.
    let hundredths = rest * 2;
    format!("{sign}{whole}.{hundredths:02}%")
}

/// The largest `ST_TextScale` the schema admits, from its own pattern
/// `0*(600|([0-5]?[0-9]?[0-9]))%`.
pub const TEXT_SCALE_MAX: u16 = 600;

/// The lexical form of `ST_TextScale` (`w:w` in a run property bag).
///
/// The pattern requires the `%` sign and admits nothing above 600, so a bare
/// `90` — which is what every Transitional producer writes and what the writer
/// wrote until 2026-10-01 — is not a value the attribute can hold.
/// `RunProperties::scale` holds the percentage as an integer, which is what the
/// `%` form makes the meaning of.
#[must_use]
pub fn text_scale_lexical(percent: u16) -> String {
    format!("{}%", percent.min(TEXT_SCALE_MAX))
}

/// A table row height (`w:trHeight`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RowHeight {
    /// Height in twips.
    pub value: Option<Twips>,
    /// Height rule.
    pub rule: Option<HeightRule>,
}

/// The four cell margins (`w:tblCellMar` / `w:tcMar`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CellMargins {
    /// Top margin in twips.
    pub top: Option<Twips>,
    /// Start margin in twips.
    pub start: Option<Twips>,
    /// Bottom margin in twips.
    pub bottom: Option<Twips>,
    /// End margin in twips.
    pub end: Option<Twips>,
}

/// A table-look bit mask (`w:tblLook`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TableLook {
    /// First row special formatting.
    pub first_row: bool,
    /// Last row special formatting.
    pub last_row: bool,
    /// First column special formatting.
    pub first_column: bool,
    /// Last column special formatting.
    pub last_column: bool,
    /// Do not apply banding to rows.
    pub no_h_band: bool,
    /// Do not apply banding to columns.
    pub no_v_band: bool,
}

#[cfg(test)]
mod percent_tests {
    use super::percent_from_fiftieths;

    #[test]
    fn negative_percents_carry_one_sign() {
        assert_eq!(percent_from_fiftieths(-100), "-2%");
        assert_eq!(percent_from_fiftieths(-75), "-1.50%");
        assert_eq!(percent_from_fiftieths(-25), "-0.50%");
        assert_eq!(percent_from_fiftieths(i32::MIN), "-42949672.96%");
        assert_eq!(percent_from_fiftieths(100), "2%");
    }
}
