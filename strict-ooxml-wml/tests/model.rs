#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Coverage tests for the model layer: enums, units, tables, indexes and
//! accessors.

mod common;

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::{Block, GridCol, OpaqueBlock, Paragraph, Table};
use strict_ooxml_wml::model::document::{Body, Document, DocumentSource};
use strict_ooxml_wml::model::drawing::{
    BlipRef, DocPr, Drawing, DrawingKind, Extent, InlineDrawing, MediaIndex, MediaItem, MediaKind,
    Picture,
};
use strict_ooxml_wml::model::ids::{AbstractNumId, Ilvl, NumId, ParaId, StyleId, TextId};
use strict_ooxml_wml::model::inline::{
    BookmarkId, CommentId, OpaqueInline, Run, RunContent, TextNode,
};
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, LevelOverride, Num, NumberingTable};
use strict_ooxml_wml::model::props::{NumPr, ParagraphProperties, Section};
use strict_ooxml_wml::model::settings::{DocumentZoom, Settings, Zoom};
use strict_ooxml_wml::model::styles::{Style, StyleTable};
use strict_ooxml_wml::model::support::{SupportModel, SupportStatus};
use strict_ooxml_wml::model::values::{
    Border, BorderStyle, BreakKind, Color, DocGridType, EighthsPoint, Emu, FieldCharType,
    HalfPoints, HeightRule, Highlight, Justification, LineNumberRestart, LineSpacingRule,
    PageOrientation, SectionType, Shading, Space, StyleType, TabAlignment, TabLeader, TableLayout,
    TextDirection, ThemeColor, TriState, Twips, Underline, UnderlineSpec, VertAlign, VerticalJc,
    VerticalMerge, Width, WidthKind,
};
use strict_ooxml_wml::parse::ParseOptions;

use common::{document_parts, parse_parts};

fn location() -> SourceLocation {
    SourceLocation::new(PartId::new("/word/document.xml"), 1, 1, 0)
}

#[test]
#[allow(clippy::too_many_lines)]
fn enum_lexical_round_trip() {
    macro_rules! check {
        ($ty:ty, $($value:expr),+ $(,)?) => {{
            $( let value = $value;
               let text = value.as_str();
               assert_eq!(<$ty>::from_strict(text), Some(value)); )+
        }};
    }
    check!(
        Justification,
        Justification::Start,
        Justification::End,
        Justification::Center,
        Justification::Both,
        Justification::Distribute,
        Justification::Justify,
        Justification::MediumKashida,
        Justification::HighKashida,
        Justification::LowKashida,
        Justification::ThaiDistribute,
    );
    check!(
        Underline,
        Underline::Single,
        Underline::Words,
        Underline::Double,
        Underline::Thick,
        Underline::Dotted,
        Underline::DottedHeavy,
        Underline::Dash,
        Underline::DashedHeavy,
        Underline::DashLong,
        Underline::DashLongHeavy,
        Underline::DotDash,
        Underline::DashDotHeavy,
        Underline::DotDotDash,
        Underline::DashDotDotHeavy,
        Underline::Wave,
        Underline::WavyHeavy,
        Underline::WavyDouble,
        Underline::None,
    );
    check!(
        VertAlign,
        VertAlign::Baseline,
        VertAlign::Superscript,
        VertAlign::Subscript,
    );
    check!(
        Highlight,
        Highlight::Black,
        Highlight::Blue,
        Highlight::Cyan,
        Highlight::Green,
        Highlight::Magenta,
        Highlight::Red,
        Highlight::Yellow,
        Highlight::White,
        Highlight::DarkBlue,
        Highlight::DarkCyan,
        Highlight::DarkGreen,
        Highlight::DarkMagenta,
        Highlight::DarkRed,
        Highlight::DarkYellow,
        Highlight::DarkGray,
        Highlight::LightGray,
        Highlight::None,
    );
    check!(
        BorderStyle,
        BorderStyle::Nil,
        BorderStyle::None,
        BorderStyle::Single,
        BorderStyle::Thick,
        BorderStyle::Double,
        BorderStyle::Dotted,
        BorderStyle::Dashed,
        BorderStyle::DotDash,
        BorderStyle::DotDotDash,
        BorderStyle::Triple,
        BorderStyle::ThinThickSmallGap,
        BorderStyle::ThickThinSmallGap,
        BorderStyle::ThinThickThinSmallGap,
        BorderStyle::ThinThickMediumGap,
        BorderStyle::ThickThinMediumGap,
        BorderStyle::ThinThickThinMediumGap,
        BorderStyle::ThinThickLargeGap,
        BorderStyle::ThickThinLargeGap,
        BorderStyle::ThinThickThinLargeGap,
        BorderStyle::Wave,
        BorderStyle::DoubleWave,
        BorderStyle::DashSmallGap,
        BorderStyle::DashDotStroked,
        BorderStyle::ThreeDEmboss,
        BorderStyle::ThreeDEngrave,
        BorderStyle::Outset,
        BorderStyle::Inset,
    );
    check!(
        LineSpacingRule,
        LineSpacingRule::Auto,
        LineSpacingRule::Exact,
        LineSpacingRule::AtLeast,
    );
    check!(
        TabAlignment,
        TabAlignment::Start,
        TabAlignment::End,
        TabAlignment::Center,
        TabAlignment::Clear,
        TabAlignment::Decimal,
        TabAlignment::Bar,
        TabAlignment::Num,
    );
    check!(
        TabLeader,
        TabLeader::None,
        TabLeader::Dot,
        TabLeader::Hyphen,
        TabLeader::Underscore,
        TabLeader::Heavy,
        TabLeader::MiddleDot,
    );
    check!(
        SectionType,
        SectionType::NextPage,
        SectionType::NextColumn,
        SectionType::Continuous,
        SectionType::EvenPage,
        SectionType::OddPage,
    );
    check!(
        PageOrientation,
        PageOrientation::Portrait,
        PageOrientation::Landscape,
    );
    check!(
        DocGridType,
        DocGridType::Default,
        DocGridType::Lines,
        DocGridType::LinesAndChars,
        DocGridType::SnapToChars,
    );
    check!(
        FieldCharType,
        FieldCharType::Begin,
        FieldCharType::Separate,
        FieldCharType::End,
    );
    check!(
        StyleType,
        StyleType::Paragraph,
        StyleType::Character,
        StyleType::Table,
        StyleType::Numbering,
    );
    check!(TableLayout, TableLayout::Fixed, TableLayout::Autofit);
    check!(
        HeightRule,
        HeightRule::Auto,
        HeightRule::AtLeast,
        HeightRule::Exact,
    );
    check!(
        VerticalMerge,
        VerticalMerge::Restart,
        VerticalMerge::Continue,
    );
    check!(
        TextDirection,
        TextDirection::LrTb,
        TextDirection::TbRl,
        TextDirection::BtLr,
        TextDirection::LrTbV,
        TextDirection::TbRlV,
        TextDirection::TbLrV,
    );
    check!(
        VerticalJc,
        VerticalJc::Top,
        VerticalJc::Center,
        VerticalJc::Bottom,
    );
    check!(
        LineNumberRestart,
        LineNumberRestart::NewPage,
        LineNumberRestart::NewSection,
        LineNumberRestart::Continuous,
    );
    check!(
        BreakKind,
        BreakKind::Page,
        BreakKind::Column,
        BreakKind::TextWrapping,
    );
    assert_eq!(Justification::from_strict("bogus"), None);
    assert_eq!(BreakKind::default(), BreakKind::TextWrapping);
}

#[test]
fn value_newtypes_and_defaults() {
    assert_eq!(Twips(20).value(), 20);
    assert_eq!(HalfPoints(28).value(), 28);
    assert_eq!(EighthsPoint(8).value(), 8);
    assert_eq!(Emu(914_400).value(), 914_400);
    assert_eq!(Color::new("FF0000").to_string(), "FF0000");
    assert_eq!(Color::new("auto").as_str(), "auto");
    assert_eq!(ThemeColor::new("accent1").as_str(), "accent1");
    assert_eq!(TriState::from_strict("on"), Some(TriState::On));
    assert_eq!(TriState::from_strict("0"), Some(TriState::Off));
    assert_eq!(TriState::from_strict("maybe"), None);
    assert!(TriState::On.is_on());
    assert!(!TriState::Off.is_on());
    assert_eq!(TriState::default(), TriState::Absent);
    assert_eq!(Space::from_strict("preserve"), Some(Space::Preserve));
    assert_eq!(Space::from_strict("nope"), None);
    assert_eq!(Space::default(), Space::Default);
    assert_eq!(WidthKind::from_strict("pct"), Some(WidthKind::Pct));
    assert_eq!(WidthKind::from_strict("x"), None);
    assert_eq!(WidthKind::default(), WidthKind::Dxa);
    assert_eq!(
        Width::default(),
        Width {
            kind: WidthKind::Dxa,
            value: None
        }
    );
    let border = Border {
        style: Some(BorderStyle::Single),
        size: Some(EighthsPoint(4)),
        color: Some(Color::new("auto")),
        space: Some(0),
        shadow: true,
        frame: false,
    };
    assert_eq!(border.size.unwrap().value(), 4);
    assert_eq!(Shading::default(), Shading::default());
    assert_eq!(
        DocumentZoom::from_strict("bestFit"),
        Some(DocumentZoom::BestFit)
    );
    assert_eq!(DocumentZoom::from_strict("x"), None);
    let underline = UnderlineSpec {
        style: Some(Underline::Single),
        color: None,
    };
    assert!(underline.style.is_some());
}

#[test]
fn identifier_newtypes() {
    assert_eq!(StyleId::new("A").to_string(), "A");
    assert_eq!(ParaId::new("0A1B").as_str(), "0A1B");
    assert_eq!(TextId::new("77777777").as_str(), "77777777");
    assert_eq!(Ilvl(3).to_string(), "3");
    assert_eq!(NumId(5).0, 5);
    assert_eq!(AbstractNumId(1), AbstractNumId(1));
}

#[test]
fn media_index_behaviour() {
    let mut index = MediaIndex::new();
    assert!(index.is_empty());
    let part = PartId::new("/word/media/image1.png");
    let item = MediaItem {
        part: part.clone(),
        content_type: Some(Arc::from("image/png")),
        kind: MediaKind::Png,
    };
    let first = index.insert(item.clone());
    let second = index.insert(item);
    assert_eq!(first, second);
    assert_eq!(index.len(), 1);
    assert_eq!(index.get(&part).unwrap().kind, MediaKind::Png);
    assert_eq!(index.iter().count(), 1);
    assert!(index.get(&PartId::new("/none")).is_none());
    assert_eq!(MediaKind::from_content_type("image/jpeg"), MediaKind::Jpeg);
    assert_eq!(MediaKind::from_content_type("image/gif"), MediaKind::Gif);
    assert_eq!(MediaKind::from_content_type("image/bmp"), MediaKind::Bmp);
    assert_eq!(MediaKind::from_content_type("image/tiff"), MediaKind::Tiff);
    assert_eq!(MediaKind::from_content_type("image/x-emf"), MediaKind::Emf);
    assert_eq!(MediaKind::from_content_type("image/x-wmf"), MediaKind::Wmf);
    assert_eq!(
        MediaKind::from_content_type("image/svg+xml"),
        MediaKind::Svg
    );
    assert_eq!(
        MediaKind::from_content_type("application/octet-stream"),
        MediaKind::Other
    );
    assert_eq!(MediaKind::from_extension("PNG"), MediaKind::Png);
    assert_eq!(MediaKind::from_extension("jpg"), MediaKind::Jpeg);
    assert_eq!(MediaKind::from_extension("jpeg"), MediaKind::Jpeg);
    assert_eq!(MediaKind::from_extension("gif"), MediaKind::Gif);
    assert_eq!(MediaKind::from_extension("bmp"), MediaKind::Bmp);
    assert_eq!(MediaKind::from_extension("tif"), MediaKind::Tiff);
    assert_eq!(MediaKind::from_extension("tiff"), MediaKind::Tiff);
    assert_eq!(MediaKind::from_extension("emf"), MediaKind::Emf);
    assert_eq!(MediaKind::from_extension("wmf"), MediaKind::Wmf);
    assert_eq!(MediaKind::from_extension("svg"), MediaKind::Svg);
    assert_eq!(MediaKind::from_extension("weird"), MediaKind::Other);
}

#[test]
fn style_table_behaviour() {
    let mut table = StyleTable::new();
    assert!(table.is_empty());
    let style = Style {
        id: StyleId::new("A"),
        style_type: StyleType::Paragraph,
        name: Some(Arc::from("Alpha")),
        based_on: None,
        next: None,
        link: None,
        is_default: true,
        hidden: false,
        ui_priority: Some(1),
        table: strict_ooxml_wml::model::props::TableProperties::default(),
        paragraph: ParagraphProperties::default(),
        run: strict_ooxml_wml::model::props::RunProperties::default(),
        based_on_chain: Vec::new(),
        location: location(),
    };
    table.insert(style.clone());
    // Re-inserting replaces in place.
    table.insert(style);
    assert_eq!(table.len(), 1);
    assert!(table.contains(&StyleId::new("A")));
    assert_eq!(
        table.get(&StyleId::new("A")).unwrap().name.as_deref(),
        Some("Alpha")
    );
    assert!(table.get_mut(&StyleId::new("A")).is_some());
    assert!(table.get_mut(&StyleId::new("Z")).is_none());
    assert_eq!(table.ids().count(), 1);
    assert_eq!(table.iter().count(), 1);
    assert_eq!(
        table.default_for(StyleType::Paragraph).unwrap().as_str(),
        "A"
    );
    assert!(table.default_for(StyleType::Table).is_none());
}

#[test]
fn numbering_table_behaviour() {
    let mut table = NumberingTable::new();
    assert!(table.is_empty());
    let mut level = Level::new(Ilvl(0));
    level.start = Some(1);
    level.format = Some(Arc::from("decimal"));
    level.text = Some(Arc::from("%1."));
    level.is_legal = true;
    let abstract_num = AbstractNum {
        id: AbstractNumId(0),
        multi_level_type: Some(Arc::from("hybridMultilevel")),
        num_style_link: None,
        style_link: None,
        levels: vec![level],
        location: location(),
    };
    table.insert_abstract(abstract_num.clone());
    table.insert_abstract(abstract_num);
    let num = Num {
        num_id: NumId(5),
        abstract_num_id: AbstractNumId(0),
        overrides: vec![LevelOverride {
            ilvl: Ilvl(0),
            start_override: Some(3),
            level: None,
        }],
        location: location(),
    };
    table.insert_num(num.clone());
    table.insert_num(num);
    assert_eq!(table.len(), 1);
    assert!(table.abstract_num(AbstractNumId(0)).is_some());
    assert!(table.num(NumId(5)).is_some());
    assert!(table.resolved_abstract(NumId(5)).is_some());
    assert!(table.resolved_abstract(NumId(99)).is_none());
    assert_eq!(table.abstracts().count(), 1);
    assert_eq!(table.nums().count(), 1);
    let abstract_num = table.abstract_num(AbstractNumId(0)).unwrap();
    assert!(abstract_num.level(Ilvl(0)).is_some());
    assert!(abstract_num.level(Ilvl(8)).is_none());
}

#[test]
fn support_model_behaviour() {
    let mut model = SupportModel::new();
    assert!(model.is_empty());
    model.record("w:a", SupportStatus::Supported, None, None);
    model.record(
        "w:a",
        SupportStatus::Partial,
        Some("m".to_owned()),
        Some(location()),
    );
    model.record("w:b", SupportStatus::Unsupported, None, None);
    model.record("w:c", SupportStatus::Ignored, None, None);
    assert_eq!(model.len(), 3);
    assert_eq!(model.get("w:a").unwrap().status, SupportStatus::Partial);
    assert_eq!(model.get("w:a").unwrap().count, 2);
    assert_eq!(model.count_with_status(SupportStatus::Unsupported), 1);
    assert_eq!(model.iter().count(), 3);
    let mut other = SupportModel::new();
    other.record("w:a", SupportStatus::Unsupported, None, None);
    other.record("w:d", SupportStatus::Supported, None, None);
    model.merge(other);
    assert_eq!(model.get("w:a").unwrap().status, SupportStatus::Unsupported);
    assert!(model.get("w:d").is_some());
    assert_eq!(SupportStatus::Supported.as_str(), "supported");
    assert_eq!(SupportStatus::Partial.as_str(), "partial");
    assert_eq!(SupportStatus::Unsupported.as_str(), "unsupported");
    assert_eq!(SupportStatus::Ignored.as_str(), "ignored");
    let summary = model.debug_summary();
    assert!(summary.contains("support:"));
    assert!(summary.contains("w:a"));
}

#[test]
fn block_and_inline_accessors() {
    let paragraph = Paragraph {
        props: ParagraphProperties::default(),
        inlines: vec![
            strict_ooxml_wml::model::inline::Inline::Run(Run {
                props: strict_ooxml_wml::model::props::RunProperties::default(),
                content: vec![RunContent::Text(TextNode {
                    text: "x".to_owned(),
                    space: Space::Default,
                })],
                location: location(),
            }),
            strict_ooxml_wml::model::inline::Inline::BookmarkStart(BookmarkId::new("b")),
        ],
        rsids: strict_ooxml_wml::model::values::Rsids::default(),
        para_id: Some(ParaId::new("1")),
        text_id: None,
        location: location(),
    };
    let block = Block::Paragraph(paragraph);
    assert!(block.as_paragraph().is_some());
    assert!(block.as_table().is_none());
    assert_eq!(
        block.as_paragraph().unwrap().inlines[0]
            .as_run()
            .unwrap()
            .content
            .len(),
        1
    );
    assert!(block.as_paragraph().unwrap().inlines[1].as_run().is_none());

    let opaque = OpaqueBlock {
        namespace: Arc::from("urn:x"),
        local: Arc::from("thing"),
        attributes: Vec::new(),
        location: location(),
    };
    assert_eq!(opaque.feature_id(), "w:thing");
    let opaque_no_ns = OpaqueBlock {
        namespace: Arc::from(""),
        local: Arc::from("bare"),
        attributes: Vec::new(),
        location: location(),
    };
    assert_eq!(opaque_no_ns.feature_id(), "bare");
    let inline_opaque = OpaqueInline {
        namespace: Arc::from("urn:x"),
        local: Arc::from("widget"),
        attributes: Vec::new(),
        location: location(),
    };
    assert_eq!(inline_opaque.feature_id(), "w:widget");
}

#[test]
fn document_accessors_and_tables() {
    let body = "<w:tbl><w:tblGrid><w:gridCol w:w=\"100\"/></w:tblGrid>\
<w:tr><w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let document = parse_parts(&document_parts(body, &[])).expect("parse");
    assert_eq!(document.support().len(), document.support.len());
    assert_eq!(document.media().len(), 0);
    assert!(document.support_debug().contains("support:"));
    assert!(document.body.blocks[0].as_table().is_some());
    assert!(document.body.blocks[0].as_paragraph().is_none());

    // A hand-built empty document also exposes the accessors.
    let source = DocumentSource {
        main_document: PartId::new("/word/document.xml"),
        styles: None,
        numbering: None,
        settings: None,
        footnotes: None,
        endnotes: None,
        theme: None,
    };
    let built = Document {
        body: Body::default(),
        styles: StyleTable::new(),
        numbering: NumberingTable::new(),
        footnotes: strict_ooxml_wml::model::NoteTable::new(),
        endnotes: strict_ooxml_wml::model::NoteTable::new(),
        settings: Settings::default(),
        theme: None,
        sections: vec![Section {
            properties: strict_ooxml_wml::model::props::SectionProperties::default(),
            location: location(),
        }],
        headers_footers: Vec::new(),
        media: MediaIndex::new(),
        support: SupportModel::new(),
        source,
    };
    assert!(built.support().is_empty());
    assert!(built.media().is_empty());
    let _ = built.clone();
    let _ = format!("{built:?}");
}

#[test]
fn drawing_and_run_constructors() {
    let blip = BlipRef {
        embed: None,
        link: None,
        resolved: None,
        location: location(),
    };
    let picture = Picture {
        name: Some(Arc::from("p")),
        descr: Some(Arc::from("d")),
        blip: Some(blip),
        extent: Some(Extent {
            cx: Emu(1),
            cy: Emu(2),
        }),
        src_rect: None,
        xfrm: None,
    };
    let inline = InlineDrawing {
        extent: Some(Extent {
            cx: Emu(1),
            cy: Emu(2),
        }),
        doc_pr: Some(DocPr {
            id: Some(1),
            name: Some(Arc::from("n")),
            descr: None,
        }),
        graphic_uri: Some(Arc::from("uri")),
        graphic: Box::new(strict_ooxml_wml::model::Graphic::Picture(picture)),
        location: location(),
    };
    let drawing = Drawing {
        kind: DrawingKind::Inline(inline),
        location: location(),
    };
    assert!(matches!(drawing.kind, DrawingKind::Inline(_)));
    let run = Run {
        props: strict_ooxml_wml::model::props::RunProperties::default(),
        content: vec![RunContent::Tab, RunContent::Break(BreakKind::Page)],
        location: location(),
    };
    assert_eq!(run.content.len(), 2);
    assert_eq!(GridCol::default().width, None);
    assert_eq!(CommentId::new("1").as_str(), "1");
    assert_eq!(NumPr::default().num_id, None);
    let zoom = Zoom {
        percent: Some(100),
        kind: Some(DocumentZoom::FullPage),
    };
    assert_eq!(zoom.percent, Some(100));
    assert_eq!(DocumentZoom::None, DocumentZoom::None);
}

#[test]
fn parse_options_default_is_strict() {
    let options = ParseOptions::default();
    assert_eq!(
        options.conformance,
        strict_ooxml_core::opc::ConformancePolicy::StrictOnly
    );
    let _ = options.limits;
    let _ = Arc::<str>::from("x");
    let _: Table = Table {
        props: strict_ooxml_wml::model::props::TableProperties::default(),
        grid: Vec::new(),
        rows: Vec::new(),
        location: location(),
    };
}
