//! Legacy VML pictures → `DrawingML` (stage T7 of `TZ-STRICT-OOXML-RUST` §10).
//!
//! Strict has no `w:pict` and no VML at all, so a `v:shape` with a `v:imagedata`
//! is content the whole pipeline used to throw away: the normalizer dropped the
//! `w:pict` subtree and reported it (`T7.vml`, `Severity::Lossy`), the model never
//! saw a picture, and the writer wrote a document with a hole where the picture
//! was. The audit named this the highest-value item on the queue for exactly that
//! reason — it is the only one that gives a document back **something visible**.
//!
//! # Why this is here and not in the parser
//!
//! The conversion has to happen at the byte seam, before the model exists, and
//! that is forced rather than chosen: by the time the parser runs there is no
//! `w:pict` left, because a Strict package may not contain one. So the `DrawingML`
//! *authored* here rather than *modelled* and regenerated — which is why this
//! module emits markup instead of building a value, and why it is deliberately
//! the narrowest of the four VML shapes rather than a general converter.
//! # What is converted, and what is not
//!
//! | Shape | What it is | What happens |
//! |---|---|---|
//! | `v:shape` with `v:imagedata/@r:id` | a picture — the only VML shape that is purely an image | **converted** to `wp:inline` or `wp:anchor` + `pic:pic` |
//! | `v:shape[@type="#_x0000_t202"]` with `v:textbox` | a text box: a shape plus `w:txbxContent` | not converted; dropped and recorded |
//! | `v:rect`, `v:oval`, `v:line`, freeform `v:path` | drawn geometry | not converted; dropped and recorded |
//! | `o:OLEObject` | an executable object | not convertible by definition (`docs/transitional-to-strict-audit.md` §12) |
//!
//! The picture case is the one worth building, and it is affordable **because
//! `r:id` does not have to change**: `v:imagedata/@r:id` and `a:blip/@r:embed` both
//! name a relationship of the part that contains them, the relationship type is the
//! same (`.../relationships/image`), and the normalizer has already rewritten that
//! relationship's type URI to its Strict form by the time the body is read. So the
//! conversion needs no relationship surgery at all, which is the whole reason it can
//! live at the byte seam.
//!
//! # `wp:inline` or `wp:anchor`
//!
//! The audit said "1:1 into `wp:inline`", and for a `position:static` shape that is
//! right. It is wrong for the shape that actually occurs most: the corpus's
//! pictures are all watermarks, and a watermark says
//! `style="position:absolute;…;z-index:-251657216;mso-position-horizontal:center;
//! mso-position-vertical:center"`. An `mso-position-horizontal:center` on an
//! absolutely positioned shape is an *anchor*, and writing it as an inline image
//! would move the watermark into the text flow and centre it on the line instead of
//! on the page — a visible layout change dressed up as a conversion. So the
//! decision reads the style string:
//!
//! * `position:absolute` (or any `mso-position-*` present) → `wp:anchor`, with
//!   `behindDoc` from the sign of `z-index` and the alignment from the
//!   `mso-position-*` pair;
//! * otherwise → `wp:inline`.
//!
//! The style string is a small CSS subset and is parsed for the eight properties
//! this needs. A property that is absent or unparseable is `None` rather than a
//! guess, and the caller decides what a `None` means — for the extent it means
//! "record a zero extent", because `wp:extent` is `use="required"` and something
//! has to be written, and because a picture that draws at no size is a real defect
//! rather than a default worth hiding.
//!
//! # The shape of what is emitted
//!
//! Exactly the shape `strict-ooxml-write/src/drawing.rs` writes, for two reasons:
//! a converted anchor and a written one are then the same thing, and the schema
//! sees one sequence rather than two. `CT_Anchor`'s attribute set is not optional
//! the way the prose suggests — `simplePos`, `relativeHeight`, `behindDoc`,
//! `locked`, `layoutInCell` and `allowOverlap` are all required — and its children
//! are a fixed `xsd:sequence`.

// `to_emu` turns a CSS length into an integer EMU measure. The conversion is
// range-checked on both sides by the guard inside it and saturates at the bounds,
// so the cast is a rounding rather than a truncation — the same arrangement
// `strict-ooxml-core/src/opc/zip/write.rs` uses for its ZIP offsets, and the same
// reason: the checks are the point and the cast only satisfies the type system
// afterwards.
#![allow(clippy::cast_possible_truncation)]

use std::collections::BTreeMap;

use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use crate::error::SourceLocation;
use crate::normalize::report::{LossRecord, NormalizationReport, Severity};
use crate::normalize::transitional::PartContext;

/// Strict `drawingml/main`.
const NS_A: &str = "http://purl.oclc.org/ooxml/drawingml/main";
/// Strict `drawingml/wordprocessingDrawing`.
const NS_WP: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";
/// Strict `drawingml/picture`.
const NS_PIC: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
/// The `a:graphicData/@uri` value for a picture payload.
const URI_PICTURE: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
/// Transitional `drawingml/2006/main`, which is what a part that already carries
/// `DrawingML` binds `a` to. VML documents predate `DrawingML`, so the prefixes a
/// converted picture needs are usually absent entirely, and a part that does have
/// them has them under the Transitional URIs.
const NS_TRANSITIONAL_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_TRANSITIONAL_WP: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_TRANSITIONAL_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

/// EMU per point. One inch is 914 400 EMU and one inch is 72 points.
const EMU_PER_POINT: f64 = 914_400.0 / 72.0;

/// The namespaces a converted `w:drawing` needs declared, as
/// `(prefix, strict uri, transitional uri)`.
///
/// The Transitional spellings are here for the same reason as everywhere else this
/// pipeline touches them: a part that *does* declare `a`/`wp`/`pic` declares them
/// with the Transitional URIs, and adding a Strict declaration for a prefix that is
/// already bound is not a duplicate declaration — it is a second, conflicting
/// binding for the same prefix, which is a hard XML error. So the caller checks
/// what the prefix is already bound to and rewrites the existing declaration
/// rather than adding one.
pub const REQUIRED_NAMESPACES: [(&str, &str, &str); 3] = [
    ("a", NS_A, NS_TRANSITIONAL_A),
    ("wp", NS_WP, NS_TRANSITIONAL_WP),
    ("pic", NS_PIC, NS_TRANSITIONAL_PIC),
];

/// A `v:shape` that turned out to be a picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmlPicture {
    /// The relationship id `v:imagedata/@r:id` carried, reused as `a:blip/@r:embed`.
    pub rel_id: String,
    /// `v:shape/@id`, for `wp:docPr/@name`.
    pub name: String,
    /// `v:shape/@alt`, for `wp:docPr/@descr`.
    pub description: Option<String>,
    /// The parsed `style` string.
    pub style: VmlStyle,
}

/// The properties of a VML `style` string this conversion needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VmlStyle {
    /// `width`, in points.
    pub width_pt: Option<String>,
    /// `height`, in points.
    pub height_pt: Option<String>,
    /// `position` was `absolute`.
    pub absolute: bool,
    /// `z-index`. A negative value means "behind the text", which is what every
    /// watermark has.
    pub z_index: Option<i64>,
    /// `mso-position-horizontal-relative`, e.g. `margin`.
    pub horizontal_relative: Option<String>,
    /// `mso-position-horizontal`: `center`, `left`, `right`, or an offset.
    pub horizontal: Option<String>,
    /// `mso-position-vertical-relative`, e.g. `margin`.
    pub vertical_relative: Option<String>,
    /// `mso-position-vertical`: `center`, `top`, `bottom`, or an offset.
    pub vertical: Option<String>,
}

impl VmlStyle {
    /// Parses a VML `style` attribute: `name:value;name:value`.
    ///
    /// Textual and total. A property that is not there is `None`; the widths keep
    /// their text because a CSS length carries a unit and the conversion to EMU
    /// happens once, where the report can name a width it could not read.
    #[must_use]
    pub fn parse(style: &str) -> Self {
        let mut out = Self::default();
        for declaration in style.split(';') {
            let Some((name, value)) = declaration.split_once(':') else {
                continue;
            };
            let value = value.trim().to_ascii_lowercase();
            match name.trim().to_ascii_lowercase().as_str() {
                "width" => out.width_pt = Some(value),
                "height" => out.height_pt = Some(value),
                "position" => out.absolute = value == "absolute",
                "z-index" => out.z_index = value.parse().ok(),
                "mso-position-horizontal-relative" => out.horizontal_relative = Some(value),
                "mso-position-horizontal" => out.horizontal = Some(value),
                "mso-position-vertical-relative" => out.vertical_relative = Some(value),
                "mso-position-vertical" => out.vertical = Some(value),
                _ => {}
            }
        }
        out
    }

    /// `ST_RelFromH`: `margin`, `column`, `page`, `character`, `leftMargin`,
    /// `rightMargin`, `insideMargin`, `outsideMargin`.
    ///
    /// VML spells the margin variants with hyphens and has no bare `margin`, so
    /// the value is **mapped** into the schema's vocabulary rather than copied. A
    /// watermark centred on `margin` falls through to `page`, which is what a
    /// centred picture on a sheet wants: on a mirrored-margins page the text margin
    /// is not the page centre, and a watermark centred on the text margin is not a
    /// watermark.
    fn relative_h(&self) -> &'static str {
        match self.horizontal_relative.as_deref() {
            Some("column") => "column",
            Some("character") => "character",
            Some("left-margin") => "leftMargin",
            Some("right-margin") => "rightMargin",
            Some("inside-margin") => "insideMargin",
            Some("outside-margin") => "outsideMargin",
            // `page` and anything else. VML has no bare `margin` in this position,
            // so the producer either says `page` or says nothing at all, and a
            // centred picture with no base means the sheet.
            _ => "page",
        }
    }

    /// `ST_RelFromV`: `margin`, `page`, `paragraph`, `line`, `topMargin`,
    /// `bottomMargin`, `insideMargin`, `outsideMargin`.
    fn relative_v(&self) -> &'static str {
        match self.vertical_relative.as_deref() {
            Some("paragraph") => "paragraph",
            Some("line") => "line",
            Some("top-margin") => "topMargin",
            Some("bottom-margin") => "bottomMargin",
            Some("inside-margin") => "insideMargin",
            Some("outside-margin") => "outsideMargin",
            // `page`, and the same reasoning as the horizontal axis.
            _ => "page",
        }
    }

    /// `ST_AlignH`: `left`, `right`, `center`, `inside`, `outside`.
    fn align_h(value: &str) -> Option<&'static str> {
        match value {
            "left" => Some("left"),
            "right" => Some("right"),
            "center" => Some("center"),
            "inside" => Some("inside"),
            "outside" => Some("outside"),
            _ => None,
        }
    }

    /// `ST_AlignV`: `top`, `bottom`, `center`, `inside`, `outside`.
    fn align_v(value: &str) -> Option<&'static str> {
        match value {
            "top" => Some("top"),
            "bottom" => Some("bottom"),
            "center" => Some("center"),
            "inside" => Some("inside"),
            "outside" => Some("outside"),
            _ => None,
        }
    }
}

/// A CSS length in EMU: `468.0pt`, `6.5in`, `234mm`, or a bare number in points.
fn emu(value: &str) -> Option<i32> {
    let trimmed = value.trim();
    let split = trimmed
        .find(|character: char| !character.is_ascii_digit() && !matches!(character, '.' | '-'))
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let per_point = match unit.trim() {
        "pt" | "" => 1.0,
        "in" => 72.0,
        "cm" => 72.0 / 2.54,
        "mm" => 7.2 / 2.54,
        "pc" => 12.0,
        "pi" => 15.0,
        _ => return None,
    };
    // Saturating, not truncating, and in the shape `strict-ooxml-wml`'s
    // `decimal_to_i32` uses so the range guard is one the compiler's lints
    // recognise: `as i64` on an `f64` is a truncation, and a picture whose CSS
    // said `width:1e30pt` should draw at the largest size an EMU can hold rather
    // than at whatever the low 64 bits happened to be.
    Some(to_emu(number * per_point * EMU_PER_POINT))
}

/// Rounds a length to EMU, saturating at the type bounds.
///
/// The shape is the same as `decimal_to_i32` in `strict-ooxml-wml`, and the
/// **i32 bounds are the point** rather than a copy of its types: `f64::from` on an
/// `i32` is lossless, so the guard is exact and the cast inside it is a rounding
/// rather than a truncation. An `i64` bound would have to be written
/// `i64::MAX as f64`, which is a different number from `i64::MAX` and a
/// precision loss to say so — and `ST_PositiveCoordinate` is an `xsd:long`, so
/// the temptation is real.
///
/// `i32::MAX` EMU is 2 147 483 647 EMU ≈ 2 300 000 points ≈ 32 000 km: twenty-six
/// times the circumference of the Earth. No picture has that extent, so the clamp
/// cannot change one, and it is recorded rather than assumed by the caller — an
/// unreadable width is reported as an unreadable width
/// ([`extent_of`](self)).
fn to_emu(value: f64) -> i32 {
    let rounded = value.round();
    if rounded >= f64::from(i32::MAX) {
        i32::MAX
    } else if rounded <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        rounded as i32
    }
}

/// The picture a buffered `w:pict` subtree holds, if it holds one.
///
/// The shape is a picture when it is a `v:shape` that carries a `v:imagedata` with
/// an `r:id`. A `type="#_x0000_t75"` check is **not** required and is deliberately
/// absent: that is how Word labels the picture shape, but a producer that omits it
/// produces the same thing, and the audit's own count came from searching for
/// `v:imagedata` rather than for the type. `v:shapetype` is a *definition* — every
/// picture shape in a document is preceded by the `v:shapetype id="_x0000_t75"`
/// that names it — so a subtree whose only shape is that definition has no picture
/// in it.
///
/// `#_x0000_t202` is the text box: a shape plus `w:txbxContent`. Its content is
/// not this module's business, and converting the frame without the text would draw
/// an empty picture where a paragraph of text was.
#[must_use]
pub(crate) fn picture_in(subtree: &[Event<'static>], context: &PartContext) -> Option<VmlPicture> {
    let mut shape: Option<(String, Option<String>, VmlStyle)> = None;
    let mut rel_id: Option<String> = None;
    for event in subtree {
        let start: &BytesStart<'_> = match event {
            Event::Start(start) | Event::Empty(start) => start,
            _ => continue,
        };
        let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
        let Some((prefix, local)) = name.split_once(':') else {
            continue;
        };
        let uri = context.uri_for(prefix.as_bytes()).unwrap_or_default();
        match local {
            "shape" if shape.is_none() && is_vml(uri) => {
                let attributes = attributes_of(start.attributes().flatten());
                let kind = attributes
                    .get("type")
                    .map(String::as_str)
                    .unwrap_or_default();
                if kind.contains("t202") {
                    return None;
                }
                shape = Some((
                    attributes
                        .get("id")
                        .cloned()
                        .unwrap_or_else(|| "Picture".to_owned()),
                    attributes.get("alt").cloned().filter(|alt| !alt.is_empty()),
                    VmlStyle::parse(attributes.get("style").map_or("", String::as_str)),
                ));
            }
            "imagedata" if shape.is_some() => {
                let attributes = attributes_of(start.attributes().flatten());
                if let Some(id) = attributes.get("id") {
                    rel_id = Some(id.clone());
                }
            }
            _ => {}
        }
    }
    let (name, description, style) = shape?;
    Some(VmlPicture {
        rel_id: rel_id?,
        name,
        description,
        style,
    })
}

/// Whether a namespace URI is one of the VML families.
fn is_vml(uri: &str) -> bool {
    uri == "urn:schemas-microsoft-com:vml"
        || uri == "urn:schemas-microsoft-com:office:office"
        || uri == "urn:schemas-microsoft-com:office:word"
}

/// The attributes of a start tag, keyed by **local** name.
///
/// Local, because the conversion is about what the attribute *is* and a producer
/// binds `r` to the relationships namespace under whatever prefix it likes — the
/// same reason `v:shape`'s attributes are read this way.
fn attributes_of<'a>(attributes: impl Iterator<Item = Attribute<'a>>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for attribute in attributes {
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let local = key.rsplit(':').next().unwrap_or(&key).to_owned();
        if let Ok(value) = attribute.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
            out.insert(local, value.into_owned());
        }
    }
    out
}

/// The `w:drawing` events that replace a converted `w:pict`.
///
/// `None` when the shape is not a picture this module can convert, which is the
/// caller's signal to drop the subtree as before.
#[must_use]
pub fn picture_events(
    picture: &VmlPicture,
    doc_pr_id: u32,
    report: &mut NormalizationReport,
    location: &SourceLocation,
) -> Vec<Event<'static>> {
    let (cx, cy) = extent_of(picture, report, location);
    let mut events = vec![Event::Start(el("w:drawing", &[]))];
    if picture.style.absolute || picture.style.horizontal.is_some() {
        events.extend(anchor_events(picture, cx, cy, doc_pr_id));
    } else {
        events.extend(inline_events(picture, cx, cy, doc_pr_id));
    }
    events.push(Event::End(BytesEnd::new("w:drawing").into_owned()));
    events
}

/// `wp:extent` in EMU, and the record when the shape declared no usable size.
///
/// A zero extent is a picture that draws at no size, so it is a real defect and not
/// a default worth hiding; `wp:extent` is `use="required"` on both `CT_Inline` and
/// `CT_Anchor`, so something has to be written either way.
fn extent_of(
    picture: &VmlPicture,
    report: &mut NormalizationReport,
    location: &SourceLocation,
) -> (i64, i64) {
    let width = picture.style.width_pt.as_deref().and_then(emu);
    let height = picture.style.height_pt.as_deref().and_then(emu);
    if width.is_none() || height.is_none() {
        report.record_loss(LossRecord {
            transform_id: "T7.vml-size",
            feature_id: "v:shape/@style".to_owned(),
            reason: format!(
                "the VML picture {:?} declared no usable width/height (style width {:?}, height \
                 {:?}), so a zero extent is written: the image is carried and will draw at no \
                 size",
                picture.name, picture.style.width_pt, picture.style.height_pt
            ),
            severity: Severity::Lossy,
            locations: vec![location.clone()],
        });
    }
    (width.map_or(0, i64::from), height.map_or(0, i64::from))
}

fn inline_events(picture: &VmlPicture, cx: i64, cy: i64, doc_pr_id: u32) -> Vec<Event<'static>> {
    let mut out = vec![Event::Start(el(
        "wp:inline",
        &[
            ("distT", "0"),
            ("distB", "0"),
            ("distL", "0"),
            ("distR", "0"),
        ],
    ))];
    out.push(Event::Empty(el(
        "wp:extent",
        &[("cx", &cx.to_string()), ("cy", &cy.to_string())],
    )));
    out.push(Event::Empty(el(
        "wp:effectExtent",
        &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")],
    )));
    out.extend(doc_pr_events(picture, doc_pr_id));
    out.extend(graphic_events(picture, cx, cy));
    out.push(Event::End(BytesEnd::new("wp:inline").into_owned()));
    out
}

fn anchor_events(picture: &VmlPicture, cx: i64, cy: i64, doc_pr_id: u32) -> Vec<Event<'static>> {
    let style = &picture.style;
    let behind = style.z_index.is_some_and(|index| index < 0);
    // `relativeHeight` is `ST_WrapDistance`, an unsigned measure, so a negative
    // `z-index` cannot be copied into it. Word's own convention is the unsigned
    // magnitude, which is what a reader compares against the other anchors on the
    // page.
    let relative_height = style.z_index.map_or(0, |index| {
        u32::try_from(index.unsigned_abs()).unwrap_or(u32::MAX)
    });
    let mut out = vec![Event::Start(el(
        "wp:anchor",
        &[
            ("distT", "0"),
            ("distB", "0"),
            // 114 300 EMU is 9 pt, Word's default distance for a floating object.
            ("distL", "114300"),
            ("distR", "114300"),
            ("simplePos", "0"),
            ("relativeHeight", &relative_height.to_string()),
            ("behindDoc", bool_str(behind)),
            ("locked", "0"),
            ("layoutInCell", "1"),
            ("allowOverlap", "1"),
        ],
    ))];
    out.push(Event::Empty(el("wp:simplePos", &[("x", "0"), ("y", "0")])));
    out.extend(position_events(
        "wp:positionH",
        style.relative_h(),
        style.horizontal.as_deref(),
    ));
    out.extend(position_events(
        "wp:positionV",
        style.relative_v(),
        style.vertical.as_deref(),
    ));
    out.push(Event::Empty(el(
        "wp:extent",
        &[("cx", &cx.to_string()), ("cy", &cy.to_string())],
    )));
    out.push(Event::Empty(el(
        "wp:effectExtent",
        &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")],
    )));
    // `CT_Anchor` requires one of the five `wrap*` elements. A watermark's `style`
    // says nothing about wrapping and `wp:wrapNone` is the choice that changes
    // nothing: the shape floats over the page exactly as `position:absolute` with
    // no wrap meant. `CT_WrapNone` is an empty type — every attribute on it is a
    // schema violation (`XS-04`).
    out.push(Event::Empty(el("wp:wrapNone", &[])));
    out.extend(doc_pr_events(picture, doc_pr_id));
    out.extend(graphic_events(picture, cx, cy));
    out.push(Event::End(BytesEnd::new("wp:anchor").into_owned()));
    out
}

/// `wp:positionH` / `wp:positionV`.
///
/// `@relativeFrom` is `use="required"` and the child is a required `xsd:choice` of
/// `wp:align` and `wp:posOffset`, so both are always written — the same two
/// violations per element `XS-21` was about. An alignment VML uses that
/// `ST_AlignH`/`ST_AlignV` does not have falls through to a zero `wp:posOffset`,
/// which is the position the element would have had anyway.
fn position_events(
    name: &'static str,
    relative_from: &'static str,
    align: Option<&str>,
) -> Vec<Event<'static>> {
    let mut out = vec![Event::Start(el(name, &[("relativeFrom", relative_from)]))];
    let aligned = align.and_then(|value| {
        if name == "wp:positionH" {
            VmlStyle::align_h(value)
        } else {
            VmlStyle::align_v(value)
        }
    });
    if let Some(alignment) = aligned {
        out.push(Event::Start(el("wp:align", &[])));
        out.push(Event::Text(BytesText::new(alignment)));
        out.push(Event::End(BytesEnd::new("wp:align").into_owned()));
    } else {
        out.push(Event::Start(el("wp:posOffset", &[])));
        out.push(Event::Text(BytesText::new("0")));
        out.push(Event::End(BytesEnd::new("wp:posOffset").into_owned()));
    }
    out.push(Event::End(BytesEnd::new(name).into_owned()));
    out
}

fn doc_pr_events(picture: &VmlPicture, doc_pr_id: u32) -> Vec<Event<'static>> {
    let id = doc_pr_id.to_string();
    let name = picture.name.clone();
    let mut attributes: Vec<(&str, &str)> = vec![("id", &id), ("name", &name)];
    if let Some(description) = &picture.description {
        attributes.push(("descr", description));
    }
    vec![Event::Empty(el("wp:docPr", &attributes))]
}

fn graphic_events(picture: &VmlPicture, cx: i64, cy: i64) -> Vec<Event<'static>> {
    vec![
        Event::Start(el("a:graphic", &[])),
        Event::Start(el("a:graphicData", &[("uri", URI_PICTURE)])),
        Event::Start(el("pic:pic", &[])),
        Event::Start(el("pic:nvPicPr", &[])),
        Event::Empty(el("pic:cNvPr", &[("id", "0"), ("name", &picture.name)])),
        Event::Empty(el("pic:cNvPicPr", &[])),
        Event::End(BytesEnd::new("pic:nvPicPr").into_owned()),
        Event::Start(el("pic:blipFill", &[])),
        // **The same `r:id`, not a new relationship.** `v:imagedata/@r:id` and
        // `a:blip/@r:embed` both name a relationship of this part, and the
        // relationship behind it is the same image part with the same
        // `.../relationships/image` type - which the normalizer has already
        // rewritten to its Strict form. Nothing about the relationship changes.
        Event::Empty(el("a:blip", &[("r:embed", &picture.rel_id)])),
        Event::Start(el("a:stretch", &[])),
        Event::Empty(el("a:fillRect", &[])),
        Event::End(BytesEnd::new("a:stretch").into_owned()),
        Event::End(BytesEnd::new("pic:blipFill").into_owned()),
        Event::Start(el("pic:spPr", &[])),
        // `a:xfrm` is a container of `a:off` and `a:ext`; the attributes are on the
        // element itself. The extent repeats `wp:extent` because `CT_Transform2D`
        // requires it, and a picture whose shape extent disagrees with its frame
        // extent is one no renderer agrees on.
        Event::Start(el(
            "a:xfrm",
            &[("rot", "0"), ("flipH", "0"), ("flipV", "0")],
        )),
        Event::Empty(el("a:off", &[("x", "0"), ("y", "0")])),
        Event::Empty(el(
            "a:ext",
            &[("cx", &cx.to_string()), ("cy", &cy.to_string())],
        )),
        Event::End(BytesEnd::new("a:xfrm").into_owned()),
        Event::Start(el("a:prstGeom", &[("prst", "rect")])),
        Event::Empty(el("a:avLst", &[])),
        Event::End(BytesEnd::new("a:prstGeom").into_owned()),
        Event::End(BytesEnd::new("pic:spPr").into_owned()),
        Event::End(BytesEnd::new("pic:pic").into_owned()),
        Event::End(BytesEnd::new("a:graphicData").into_owned()),
        Event::End(BytesEnd::new("a:graphic").into_owned()),
    ]
}

fn bool_str(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
}

/// A start tag with attributes.
///
/// `push_attribute` is infallible in this `quick-xml`, and every name here is a
/// literal identifier or an `r:id` copied verbatim, so there is no runtime
/// condition to handle — which is why this is a function and not a `Result`.
fn el(name: &'static str, attributes: &[(&str, &str)]) -> BytesStart<'static> {
    let mut start = BytesStart::new(name);
    for (key, value) in attributes {
        start.push_attribute((*key, *value));
    }
    start
}
