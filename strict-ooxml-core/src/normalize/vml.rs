//! Legacy VML → `DrawingML` (stage T7 of `TZ-STRICT-OOXML-RUST` §10).
//!
//! Strict has no `w:pict` and no VML at all, so everything under it is content
//! the pipeline used to throw away. Four classes are converted, and the fifth is
//! named and dropped:
//!
//! | The shape contains | It becomes |
//! |---|---|
//! | `v:imagedata/@r:id` | `wp:inline` / `wp:anchor` + `pic:pic` |
//! | `v:textbox/w:txbxContent` | `wps:wsp` with `wps:txbx/w:txbxContent` |
//! | nothing, and the element is `v:rect`, `v:roundrect`, `v:oval` | `wps:wsp` with that `a:prstGeom` |
//! | `v:line`, or a straight connector | `wps:wsp` with `a:prstGeom/@prst="line"` in its box |
//! | `w10:wrap` + `position:absolute` | a `wp:anchor` with that wrap and that position |
//! | `v:path` (a freeform) | its frame, with the **geometry declined and named** |
//! | `o:OLEObject` | its preview raster only — the object itself has no Strict equivalent |
//!
//! # Why `r:id` does not change, which is the whole reason this is affordable
//!
//! The conversion runs at the byte seam, **before the model exists** — forced, not
//! chosen: by the time a parser runs there is no `w:pict` left, because a Strict
//! package may not contain one. That would normally mean rebuilding every
//! relationship, which is not a thing a streaming rewriter can do.
//!
//! It does not have to. `v:imagedata/@r:id` and `a:blip/@r:embed` both name a
//! relationship **of the part that contains them**, the relationship type is the
//! same (`.../relationships/image`), and the normalizer has already rewritten that
//! type's URI to its Strict form before the body is read. So the identifier is
//! reused verbatim and no relationship is touched.
//!
//! # The two position spellings, and reading only one loses half
//!
//! A floating VML shape says where it is in one of two ways:
//!
//! - `mso-position-horizontal:center;…` — an **alignment**, with a base
//!   (`mso-position-horizontal-relative`);
//! - `margin-left:68.05pt;margin-top:9.95pt` — an **offset**, with no base.
//!
//! Five of the ten floating objects in the corpus use one and five the other.
//! Reading only the `mso-` pair — which is what the picture path did at first —
//! writes `wp:posOffset` of zero for the other five and puts them in the corner of
//! the page. **The census could not see it**: a shape in the wrong place is not a
//! schema violation, it is a page that looks wrong. Hence [`VmlStyle::margin_left`]
//! and [`VmlStyle::margin_top`], and the unit test that holds both spellings.
//!
//! # `wps` is a vendor namespace, on purpose
//!
//! A shape becomes `wps:wsp` inside `a:graphicData`, and `wps` is
//! `http://schemas.microsoft.com/office/word/2010/wordprocessingShape` — not in
//! ECMA-376, identical in both families. That is ADR-0014's `XS-18`/`XS-19` debt,
//! kept deliberately: the alternative is dropping a shape, and a shape is content.
//! The XSD gate files these in its `extension` basket for the same reason it files
//! `w14:paraId` there — conformance is defined after MCE, so a `graphicData` payload
//! from a namespace the standard does not declare is not a schema defect.

use std::collections::BTreeMap;

use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::XmlVersion;

use crate::error::SourceLocation;
use crate::normalize::report::{LossRecord, NormalizationReport, Severity};
use crate::normalize::tables::VML_NAMESPACES;
use crate::normalize::transitional::PartContext;
use crate::normalize::vml_paint::{self, VmlPaint};

/// Strict `drawingml/main`.
const NS_A: &str = "http://purl.oclc.org/ooxml/drawingml/main";
/// Strict `drawingml/wordprocessingDrawing`.
const NS_WP: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";
/// Strict `drawingml/picture`.
const NS_PIC: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
/// `urn:schemas-microsoft-com:office:word` — where `w10:wrap` and
/// `w10:anchorlock` live: VML's positioning vocabulary under a namespace of its
/// own, which is why `is_drawable_namespace` lists it beside the VML families.
const WORD_NS: &str = "urn:schemas-microsoft-com:office:word";
/// The `a:graphicData/@uri` of a picture payload.
const URI_PICTURE: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
/// The `a:graphicData/@uri` of a shape payload. Vendor; see the module docs.
const URI_SHAPE: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
/// Transitional `drawingml/2006/main`, which is what a part that already carries
/// `DrawingML` binds `a` to. VML documents predate `DrawingML`, so the prefixes a
/// converted shape needs are usually absent entirely, and a part that does have
/// them has them under the Transitional URIs.
const NS_TRANSITIONAL_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// Transitional `drawingml/2006/wordprocessingDrawing`.
const NS_TRANSITIONAL_WP: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
/// Transitional `drawingml/2006/picture`.
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
///
/// `wps` is included because a VML text box / freeform becomes `wps:wsp`
/// (AUD-37). The URI is vendor and identical in both families.
pub const REQUIRED_NAMESPACES: [(&str, &str, &str); 4] = [
    ("a", NS_A, NS_TRANSITIONAL_A),
    ("wp", NS_WP, NS_TRANSITIONAL_WP),
    ("pic", NS_PIC, NS_TRANSITIONAL_PIC),
    ("wps", URI_SHAPE, URI_SHAPE),
];

/// A shape that is not a picture: where it is, how big, and what it is called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmlShape {
    /// `v:shape/@id`, for `wp:docPr/@name` or `wps:cNvPr/@name`.
    pub name: String,
    /// `v:shape/@alt`, for `@descr`.
    pub description: Option<String>,
    /// The parsed `style` string.
    pub style: VmlStyle,
    /// Fill and outline, with VML's defaults.
    pub paint: VmlPaint,
    /// The `a:prstGeom` this shape is: `rect`, `roundRect`, `ellipse`, `line`.
    /// `None` keeps the frame without geometry (a freeform).
    pub preset: Option<&'static str>,
    /// The text of a WordArt shape (`v:textpath`), which becomes a text box.
    pub word_art: Option<VmlWordArt>,
}

/// `v:textpath`: a string drawn as the shape, as Word's text watermark is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmlWordArt {
    /// `@string`.
    pub text: String,
    /// `font-family` of its `style`, without quotes.
    pub font: Option<String>,
    /// `font-size` in half-points.
    pub size: Option<u32>,
    /// The text colour: the shape's fill.
    pub color: String,
}

/// A `v:shape` that turned out to be a picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmlPicture {
    /// The frame, as every other class has one.
    pub shape: VmlShape,
    /// The relationship id `v:imagedata/@r:id` carried, reused as
    /// `a:blip/@r:embed`.
    pub rel_id: String,
}

/// What a buffered `w:pict` / `w:object` subtree turned out to be.
///
/// The difference between the classes is what a reader sees on the page, and
/// `docs/transitional-to-strict-audit.md` §12's line — "the VML shape language has
/// no equivalent in `a:custGeom`" — is true of exactly one of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `pic:pic`: an image.
    Picture(VmlPicture),
    /// `wps:wsp` with `wps:txbx/w:txbxContent`: text in a frame.
    ///
    /// The content is queued back through the event loop rather than copied, and
    /// that is the same reason the `mc:Fallback` branch is: `w:txbxContent` is
    /// ordinary WML that has not met T1–T5 yet.
    TextBox(VmlShape),
    /// `wps:wsp` with the frame's preset (`rect`, `roundRect`, `ellipse`,
    /// `line`), its fill and its outline.
    Rectangle(VmlShape),
    /// A frame whose **geometry** this module declines: a freeform `v:path`.
    ///
    /// The frame is worth keeping and the geometry is not, because a frame with a
    /// `prstGeom` is a visible box in the right place while a frame with a
    /// **guessed** path is a shape in the right place drawn wrong. Both freeform
    /// shapes in the corpus are `r` (relative arc) paths with empty argument slots
    /// — see [`path_is_convertible`].
    Freeform(VmlShape),
}

impl Shape {
    /// The frame every class has.
    ///
    /// Uniform on purpose: the `wp:inline`-or-`wp:anchor` choice, the extent and
    /// the position are properties of **where and how big the shape is**, and all
    /// four classes answer that question the same way. Only the payload inside
    /// `a:graphicData` differs.
    #[must_use]
    pub fn frame(&self) -> &VmlShape {
        match self {
            Self::Picture(picture) => &picture.shape,
            Self::TextBox(shape) | Self::Rectangle(shape) | Self::Freeform(shape) => shape,
        }
    }
}

/// How a VML floating object wraps text.
///
/// `w10:wrap/@type` is the same vocabulary `wp:wrap*` uses, with one difference
/// that matters: `type="none"` is VML's default and is written by **omitting**
/// the element, so an absent `w10:wrap` and `w10:wrap/@type="none"` mean the same
/// thing and both arrive here as [`Wrap::None`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// `wp:wrapNone`.
    None,
    /// `wp:wrapSquare`.
    Square,
    /// `wp:wrapTight` — and it needs a `wp:wrapPolygon`.
    Tight,
    /// `wp:wrapThrough` — and it needs a `wp:wrapPolygon`.
    Through,
    /// `wp:wrapTopAndBottom`.
    TopAndBottom,
}

impl Wrap {
    /// The element name for this wrap.
    #[must_use]
    pub fn element(self) -> &'static str {
        match self {
            Self::None => "wp:wrapNone",
            Self::Square => "wp:wrapSquare",
            Self::Tight => "wp:wrapTight",
            Self::Through => "wp:wrapThrough",
            Self::TopAndBottom => "wp:wrapTopAndBottom",
        }
    }

    /// Reads a `w10:wrap/@type`.
    #[must_use]
    pub fn from_type(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "square" => Some(Self::Square),
            "tight" => Some(Self::Tight),
            "through" => Some(Self::Through),
            "topandbottom" => Some(Self::TopAndBottom),
            _ => None,
        }
    }
}

/// Whether a `v:shape/@path` is one this module converts.
///
/// **False for both freeform shapes in the corpus**, and the reason is specific
/// rather than general: VML's path grammar has `r` (a relative arc) and allows
/// **empty** argument slots, and the two shapes are
/// `m665994,l,,,7199r665994,l665994,xe` — an arc whose arguments are partly
/// absent. Writing that as `m`/`l`/`c`/`x` would draw a straight line where the
/// producer drew an arc, and `a:custGeom` has no arc command at all.
///
/// So the frame is converted and the geometry is not, and the loss is named: a
/// named missing path is a defect somebody can decide about; a guessed one is a
/// document that looks wrong and says nothing.
#[must_use]
pub fn path_is_convertible(path: &str) -> bool {
    !path.is_empty()
        && path
            .chars()
            .filter(char::is_ascii_alphabetic)
            .all(|letter| matches!(letter, 'm' | 'l' | 'c' | 'x' | 'e' | 'n' | 'f' | 's'))
}

/// The properties of a VML `style` string this conversion needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VmlStyle {
    /// `width` in points.
    pub width_pt: Option<String>,
    /// `height` in points.
    pub height_pt: Option<String>,
    /// `position` was `absolute`.
    pub absolute: bool,
    /// `z-index`. A negative value means "behind the text", which is what every
    /// watermark has.
    pub z_index: Option<i64>,
    /// `margin-left` in points: a position with no alignment.
    pub margin_left: Option<String>,
    /// `margin-top` in points.
    pub margin_top: Option<String>,
    /// `mso-position-horizontal-relative`, e.g. `margin`.
    pub horizontal_relative: Option<String>,
    /// `mso-position-horizontal`: `center`, `left`, `right`, or an offset.
    pub horizontal: Option<String>,
    /// `mso-position-vertical-relative`, e.g. `margin`.
    pub vertical_relative: Option<String>,
    /// `mso-position-vertical`: `center`, `top`, `bottom`, or an offset.
    pub vertical: Option<String>,
    /// `flip:x`.
    pub flip_h: bool,
    /// `flip:y`.
    pub flip_v: bool,
    /// `rotation` in degrees, clockwise, as written.
    pub rotation: Option<String>,
}

impl VmlStyle {
    /// Parses a VML `style` attribute: `name:value;name:value`.
    ///
    /// Textual and total. A property that is not there is `None`; the widths and
    /// the offsets keep their text because a CSS length carries a unit and the
    /// conversion to EMU happens once, where the report can name a value it could
    /// not read.
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
                "margin-left" => out.margin_left = Some(value),
                "margin-top" => out.margin_top = Some(value),
                "mso-position-horizontal-relative" => out.horizontal_relative = Some(value),
                "mso-position-horizontal" => out.horizontal = Some(value),
                "mso-position-vertical-relative" => out.vertical_relative = Some(value),
                "mso-position-vertical" => out.vertical = Some(value),
                "rotation" => out.rotation = Some(value.trim_end_matches("deg").to_owned()),
                "flip" => {
                    out.flip_h = value.split_whitespace().any(|axis| axis == "x");
                    out.flip_v = value.split_whitespace().any(|axis| axis == "y");
                }
                _ => {}
            }
        }
        out
    }

    /// `ST_RelFromH`: `margin`, `column`, `page`, `character`, `leftMargin`,
    /// `rightMargin`, `insideMargin`, `outsideMargin`.
    ///
    /// VML spells the margin variants with hyphens and has no bare `margin`, so the
    /// value is **mapped** into the schema's vocabulary rather than copied. A
    /// watermark centred on `margin` falls through to `page`, which is what a
    /// centred picture on a sheet wants: on a mirrored-margins page the text
    /// margin is not the page centre, and a watermark centred on the text margin is
    /// not a watermark.
    fn relative_h(&self) -> &'static str {
        match self.horizontal_relative.as_deref() {
            // VML's `text` is the column the anchor paragraph is set in.
            Some("column" | "text") => "column",
            Some("character") => "character",
            Some("left-margin") => "leftMargin",
            Some("right-margin") => "rightMargin",
            Some("inside-margin") => "insideMargin",
            Some("outside-margin") => "outsideMargin",
            // `page`, and everything this does not know: see the note above on why
            // a centred watermark wants the sheet and not the text margin.
            _ => "page",
        }
    }

    /// `ST_RelFromV`: `margin`, `page`, `paragraph`, `line`, `topMargin`,
    /// `bottomMargin`, `insideMargin`, `outsideMargin`.
    fn relative_v(&self) -> &'static str {
        match self.vertical_relative.as_deref() {
            // VML's `text` is the anchor paragraph.
            Some("paragraph" | "text") => "paragraph",
            Some("line") => "line",
            Some("top-margin") => "topMargin",
            Some("bottom-margin") => "bottomMargin",
            Some("inside-margin") => "insideMargin",
            Some("outside-margin") => "outsideMargin",
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
    Some(to_emu(number * per_point * EMU_PER_POINT))
}

/// Rounds a length to EMU, saturating at the type bounds.
///
/// The shape is the same as `decimal_to_i32` in `strict-ooxml-wml`, and the
/// **i32 bounds are the point** rather than a copy of its types: `f64::from` on an
/// `i32` is lossless, so the guard is exact and the cast inside it is a rounding
/// rather than a truncation. An `i64` bound would have to be written
/// `i64::MAX as f64`, which is a different number from `i64::MAX` and a precision
/// loss to say so — and `ST_PositiveCoordinate` is an `xsd:long`, so the temptation
/// is real.
///
/// `i32::MAX` EMU is 2 147 483 647 EMU ≈ 2 300 000 points ≈ 32 000 km: twenty-six
/// times the circumference of the Earth. No shape has that extent, so the clamp
/// cannot change one.
// cast_possible_truncation is allowed for this module for the reason the
// doc comment gives: the guard on both sides of the cast is the check, and
// decimal_to_i32 in strict-ooxml-wml has the same shape for the same reason.
#[allow(clippy::cast_possible_truncation)]
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

/// The shape a buffered `w:pict` / `w:object` subtree holds, and the wrap it asked
/// for.
///
/// The classification is by **what the shape contains**, not by its `type` label,
/// because the labels are advisory — `type="#_x0000_t75"` is how Word names the
/// picture shape and a producer that omits it produces the same thing — while the
/// contents are what decide what gets drawn.
///
/// `v:shapetype` is a **definition**: every shape in a document is preceded by the
/// `v:shapetype` that names it, so a subtree whose only shape is that definition
/// holds no drawable thing.
#[must_use]
pub(crate) fn classify(subtree: &[Event<'static>], context: &PartContext) -> Option<(Shape, Wrap)> {
    let mut frame: Option<VmlShape> = None;
    let mut element = String::new();
    let mut rel_id: Option<String> = None;
    let mut has_textbox = false;
    let mut path: Option<String> = None;
    let mut wrap = Wrap::None;
    let mut shape_attributes = BTreeMap::new();
    let mut fill = None;
    let mut stroke = None;
    let mut text_path = None;
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
        let drawable = matches!(local, "shape" | "rect" | "roundrect" | "oval" | "line");
        if frame.is_none() && is_drawable_namespace(uri) && drawable {
            let attributes = attributes_of(start.attributes().flatten());
            let mut style = VmlStyle::parse(attributes.get("style").map_or("", String::as_str));
            if local == "line" {
                line_box(&mut style, &attributes);
            }
            frame = Some(VmlShape {
                name: attributes
                    .get("id")
                    .cloned()
                    .unwrap_or_else(|| "Shape".to_owned()),
                description: attributes.get("alt").cloned().filter(|alt| !alt.is_empty()),
                style,
                paint: VmlPaint::default(),
                preset: preset_of(local, &attributes),
                word_art: None,
            });
            element.clear();
            element.push_str(local);
            path = attributes.get("path").cloned();
            shape_attributes = attributes;
            continue;
        }
        if frame.is_none() {
            continue;
        }
        match local {
            "imagedata" => {
                let attributes = attributes_of(start.attributes().flatten());
                if let Some(id) = attributes.get("id") {
                    rel_id = Some(id.clone());
                }
            }
            "textbox" => has_textbox = true,
            "textpath" if is_vml(uri) && text_path.is_none() => {
                text_path = Some(attributes_of(start.attributes().flatten()));
            }
            "fill" if is_vml(uri) && fill.is_none() => {
                fill = Some(attributes_of(start.attributes().flatten()));
            }
            "stroke" if is_vml(uri) && stroke.is_none() => {
                stroke = Some(attributes_of(start.attributes().flatten()));
            }
            // `w10:wrap` is the positioning vocabulary of VML, in its own
            // namespace, and it is what says how text goes round this shape. It is
            // a **translation of a name**: `w10:wrap/@type` and `wp:wrap*` share a
            // vocabulary.
            "wrap" if uri == WORD_NS => {
                let attributes = attributes_of(start.attributes().flatten());
                if let Some(kind) = attributes.get("type") {
                    wrap = Wrap::from_type(kind).unwrap_or(Wrap::None);
                }
            }
            _ => {}
        }
    }
    let shape = frame?;
    let found = Found {
        attributes: shape_attributes,
        fill,
        stroke,
        text_path,
        rel_id,
        has_textbox,
        path,
    };
    settle(shape, found).map(|shape| (shape, wrap))
}

/// What `classify` saw under the frame.
struct Found {
    attributes: Attributes,
    fill: Option<Attributes>,
    stroke: Option<Attributes>,
    text_path: Option<Attributes>,
    rel_id: Option<String>,
    has_textbox: bool,
    path: Option<String>,
}

/// The class a frame is, from what was under it; `None` stays `None` for a
/// shape this converts to nothing (an unknown `v:shape` without a preset).
fn settle(mut shape: VmlShape, found: Found) -> Option<Shape> {
    shape.paint = vml_paint::paint(&found.attributes, found.fill.as_ref(), found.stroke.as_ref());
    // A picture outranks the frame's other contents: a `v:shape` carrying both an
    // `r:id` and a text box is an OLE object whose *preview* is that image, and the
    // preview is what the reader saw.
    if let Some(rel_id) = found.rel_id {
        return Some(Shape::Picture(VmlPicture { shape, rel_id }));
    }
    if found.has_textbox {
        shape.preset = shape.preset.or(Some("rect"));
        return Some(Shape::TextBox(shape));
    }
    let art = found
        .text_path
        .as_ref()
        .and_then(|path| word_art(path, &shape.paint));
    if let Some(art) = art {
        // The string is the shape: the box itself is neither filled nor drawn.
        shape.word_art = Some(art);
        shape.preset = Some("rect");
        shape.paint = VmlPaint {
            fill: None,
            stroke: None,
        };
        return Some(Shape::TextBox(shape));
    }
    if let Some(path) = found.path {
        return Some(if path_is_convertible(&path) {
            shape.preset = shape.preset.or(Some("rect"));
            Shape::Rectangle(shape)
        } else {
            shape.preset = None;
            Shape::Freeform(shape)
        });
    }
    shape.preset.is_some().then_some(Shape::Rectangle(shape))
}

/// The text, face, size and colour of a `v:textpath`.
fn word_art(path: &BTreeMap<String, String>, paint: &VmlPaint) -> Option<VmlWordArt> {
    let text = path.get("string").filter(|text| !text.trim().is_empty())?.clone();
    let mut font = None;
    let mut size = None;
    for declaration in path.get("style").map_or("", String::as_str).split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches(|c: char| c == '"' || c == '\'');
        match name.trim() {
            "font-family" => font = Some(value.to_owned()),
            "font-size" => {
                size = points_of(value)
                    .filter(|points| (1.0..=1600.0).contains(points))
                    .map(|points| format!("{:.0}", points * 2.0))
                    .and_then(|half| half.parse().ok());
            }
            _ => {}
        }
    }
    let color = paint
        .fill
        .as_ref()
        .map_or_else(|| "C0C0C0".to_owned(), |fill| fill.rgb.clone());
    Some(VmlWordArt {
        text,
        font,
        size,
        color,
    })
}

/// The paragraph a WordArt text box holds, as Transitional `w:` events: the
/// caller writes them through the same rewrite as any text box content.
#[must_use]
pub(crate) fn word_art_content(art: &VmlWordArt) -> Vec<Event<'static>> {
    let mut out = vec![
        Event::Start(el("w:p", &[])),
        Event::Start(el("w:pPr", &[])),
        Event::Empty(el("w:jc", &[("w:val", "center")])),
        Event::End(BytesEnd::new("w:pPr").into_owned()),
        Event::Start(el("w:r", &[])),
        Event::Start(el("w:rPr", &[])),
    ];
    if let Some(font) = art.font.as_deref() {
        out.push(Event::Empty(el(
            "w:rFonts",
            &[("w:ascii", font), ("w:hAnsi", font), ("w:cs", font)],
        )));
    }
    out.push(Event::Empty(el("w:color", &[("w:val", art.color.as_str())])));
    if let Some(size) = art.size {
        let size = size.to_string();
        out.push(Event::Empty(el("w:sz", &[("w:val", size.as_str())])));
        out.push(Event::Empty(el("w:szCs", &[("w:val", size.as_str())])));
    }
    out.push(Event::End(BytesEnd::new("w:rPr").into_owned()));
    out.push(Event::Start(el("w:t", &[("xml:space", "preserve")])));
    out.push(Event::Text(BytesText::new(&art.text).into_owned()));
    out.push(Event::End(BytesEnd::new("w:t").into_owned()));
    out.push(Event::End(BytesEnd::new("w:r").into_owned()));
    out.push(Event::End(BytesEnd::new("w:p").into_owned()));
    out
}

/// The `a:prstGeom` a VML element is: its own kind, or a straight connector
/// (`o:connectortype`, the `_x0000_t32` shape type) drawn as a line.
fn preset_of(local: &str, attributes: &BTreeMap<String, String>) -> Option<&'static str> {
    match local {
        "rect" => Some("rect"),
        "roundrect" => Some("roundRect"),
        "oval" => Some("ellipse"),
        "line" => Some("line"),
        "shape"
            if attributes.contains_key("connectortype")
                || attributes
                    .get("type")
                    .is_some_and(|kind| kind == "#_x0000_t32") =>
        {
            Some("line")
        }
        _ => None,
    }
}

/// A `v:line`'s box from its `from`/`to` points: the offset is the nearer
/// corner and the size the distance, and a line drawn right-to-left or upwards
/// is the same box flipped.
fn line_box(style: &mut VmlStyle, attributes: &BTreeMap<String, String>) {
    let point = |name: &str| {
        let (x, y) = attributes.get(name)?.split_once(',')?;
        Some((points_of(x)?, points_of(y)?))
    };
    let (Some((x1, y1)), Some((x2, y2))) = (point("from"), point("to")) else {
        return;
    };
    let base_x = style
        .margin_left
        .as_deref()
        .and_then(points_of)
        .unwrap_or(0.0);
    let base_y = style
        .margin_top
        .as_deref()
        .and_then(points_of)
        .unwrap_or(0.0);
    style.margin_left = Some(format_pt(base_x + x1.min(x2)));
    style.margin_top = Some(format_pt(base_y + y1.min(y2)));
    style.width_pt = Some(format_pt((x2 - x1).abs()));
    style.height_pt = Some(format_pt((y2 - y1).abs()));
    style.absolute = true;
    style.flip_h ^= x2 < x1;
    style.flip_v ^= y2 < y1;
}

/// One member of a `v:group` - a text box, a line or a preset shape - already
/// mapped into the group's point frame. A member that is not a text box has no
/// `content`.
///
/// `classify` keeps the first `v:shape` and treats every later sibling as part of
/// that one shape. A group is a diagram: the later siblings are the labels, and
/// their `w:sz` is the text. Each label becomes its own text box. Lines and the
/// group's own geometry stay out of this list.
pub(crate) struct GroupedTextBox {
    /// The label, with width, height and offsets in points.
    pub shape: Shape,
    /// How text wraps the group. Every label inherits it.
    pub wrap: Wrap,
    /// The children of this label's `w:txbxContent`.
    pub content: Vec<Event<'static>>,
}

/// Text boxes of every `v:group` in `subtree`.
///
/// Child `left`/`top`/`width`/`height` are group coordinates (`coordorigin` +
/// `coordsize`), not points. They are scaled into the group's `style` width and
/// height, and the group's horizontal/vertical relative bases are copied so the
/// label anchors where the group does.
#[must_use]
pub(crate) fn grouped_text_boxes(
    subtree: &[Event<'static>],
    context: &PartContext,
) -> Vec<GroupedTextBox> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut stack: Vec<OpenGroup> = Vec::new();
    let mut child: Option<Vec<Event<'static>>> = None;
    let mut child_depth = 0i32;
    for event in subtree {
        match event {
            Event::Start(start) => {
                depth += 1;
                if let Some(buffer) = child.as_mut() {
                    child_depth += 1;
                    buffer.push(event.clone());
                    continue;
                }
                let Some((local, uri)) = qualified(start, context) else {
                    continue;
                };
                if is_vml(&uri) && local == "group" {
                    if let Some(frame) =
                        group_frame(start).or_else(|| stack.last().map(|group| group.frame.clone()))
                    {
                        stack.push(OpenGroup {
                            depth,
                            frame,
                            wrap: Wrap::None,
                        });
                    }
                    continue;
                }
                if stack.last().is_some_and(|group| group.depth == depth - 1)
                    && is_vml(&uri)
                    && is_group_member(&local)
                {
                    child = Some(vec![event.clone()]);
                    child_depth = 1;
                    continue;
                }
                if let Some(group) = stack.last_mut() {
                    if local == "wrap" && uri == WORD_NS {
                        let attributes = attributes_of(start.attributes().flatten());
                        if let Some(kind) = attributes.get("type") {
                            group.wrap = Wrap::from_type(kind).unwrap_or(Wrap::None);
                        }
                    }
                }
            }
            Event::Empty(start) => {
                if let Some(buffer) = child.as_mut() {
                    buffer.push(event.clone());
                    continue;
                }
                let Some((local, uri)) = qualified(start, context) else {
                    continue;
                };
                let direct = stack.last().is_some_and(|group| group.depth == depth);
                if is_vml(&uri) && is_group_member(&local) && direct {
                    if let Some(group) = stack.last() {
                        if let Some(grouped) =
                            finish_grouped(std::slice::from_ref(event), &group.frame, group.wrap)
                        {
                            out.push(grouped);
                        }
                    }
                    continue;
                }
                if let Some(group) = stack.last_mut() {
                    if local == "wrap" && uri == WORD_NS {
                        let attributes = attributes_of(start.attributes().flatten());
                        if let Some(kind) = attributes.get("type") {
                            group.wrap = Wrap::from_type(kind).unwrap_or(Wrap::None);
                        }
                    }
                }
            }
            Event::End(_) => {
                if let Some(buffer) = child.as_mut() {
                    buffer.push(event.clone());
                    child_depth -= 1;
                    if child_depth == 0 {
                        if let Some(group) = stack.last() {
                            if let Some(grouped) = finish_grouped(buffer, &group.frame, group.wrap)
                            {
                                out.push(grouped);
                            }
                        }
                        child = None;
                    }
                }
                if stack.last().is_some_and(|group| group.depth == depth) {
                    stack.pop();
                }
                depth -= 1;
            }
            _ => {
                if let Some(buffer) = child.as_mut() {
                    buffer.push(event.clone());
                }
            }
        }
    }
    out
}

/// A `v:group` child that becomes a shape of its own.
fn is_group_member(local: &str) -> bool {
    matches!(local, "shape" | "rect" | "oval" | "roundrect" | "line")
}

struct OpenGroup {
    depth: i32,
    frame: GroupFrame,
    wrap: Wrap,
}

#[derive(Clone)]
struct GroupFrame {
    origin_x: f64,
    origin_y: f64,
    coord_w: f64,
    coord_h: f64,
    width_pt: f64,
    height_pt: f64,
    style: VmlStyle,
}

fn qualified(start: &BytesStart<'_>, context: &PartContext) -> Option<(String, String)> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let (prefix, local) = name.split_once(':')?;
    let uri = context.uri_for(prefix.as_bytes()).unwrap_or("").to_owned();
    Some((local.to_owned(), uri))
}

fn group_frame(start: &BytesStart<'_>) -> Option<GroupFrame> {
    let attributes = attributes_of(start.attributes().flatten());
    let style = VmlStyle::parse(attributes.get("style").map_or("", String::as_str));
    let width_pt = style.width_pt.as_deref().and_then(points_of)?;
    let height_pt = style.height_pt.as_deref().and_then(points_of)?;
    let (coord_w, coord_h) = attributes
        .get("coordsize")
        .and_then(|value| pair(value))
        .unwrap_or((width_pt, height_pt));
    if coord_w == 0.0 || coord_h == 0.0 {
        return None;
    }
    let (origin_x, origin_y) = attributes
        .get("coordorigin")
        .and_then(|value| pair(value))
        .unwrap_or((0.0, 0.0));
    Some(GroupFrame {
        origin_x,
        origin_y,
        coord_w,
        coord_h,
        width_pt,
        height_pt,
        style,
    })
}

fn finish_grouped(
    events: &[Event<'static>],
    frame: &GroupFrame,
    wrap: Wrap,
) -> Option<GroupedTextBox> {
    let (Event::Start(start) | Event::Empty(start)) = events.first()? else {
        return None;
    };
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let local = name.rsplit(':').next().unwrap_or_default().to_owned();
    let attributes = attributes_of(start.attributes().flatten());
    let content = textbox_children(events);
    let preset = match &content {
        Some(_) => preset_of(&local, &attributes).or(Some("rect")),
        None => Some(preset_of(&local, &attributes)?),
    };
    let style_text = attributes.get("style").map_or("", String::as_str);
    let numbers = style_numbers(style_text);
    let child_style = VmlStyle::parse(style_text);
    let (mut flip_h, mut flip_v) = (child_style.flip_h, child_style.flip_v);
    let (left, top, width, height) = if local == "line" {
        let (x1, y1) = attributes.get("from").and_then(|value| pair(value))?;
        let (x2, y2) = attributes.get("to").and_then(|value| pair(value))?;
        flip_h ^= x2 < x1;
        flip_v ^= y2 < y1;
        (x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs())
    } else {
        (
            numbers.get("left").copied().unwrap_or(frame.origin_x),
            numbers.get("top").copied().unwrap_or(frame.origin_y),
            numbers.get("width").copied().unwrap_or(frame.coord_w),
            numbers.get("height").copied().unwrap_or(frame.coord_h),
        )
    };
    let scale_x = frame.width_pt / frame.coord_w;
    let scale_y = frame.height_pt / frame.coord_h;
    // The group's own offset, which its members sit inside.
    let base_x = frame
        .style
        .margin_left
        .as_deref()
        .and_then(points_of)
        .unwrap_or(0.0);
    let base_y = frame
        .style
        .margin_top
        .as_deref()
        .and_then(points_of)
        .unwrap_or(0.0);
    let mut style = frame.style.clone();
    style.absolute = true;
    style.horizontal = None;
    style.vertical = None;
    style.flip_h = flip_h;
    style.flip_v = flip_v;
    style.width_pt = Some(format_pt(width * scale_x));
    style.height_pt = Some(format_pt(height * scale_y));
    style.margin_left = Some(format_pt(base_x + (left - frame.origin_x) * scale_x));
    style.margin_top = Some(format_pt(base_y + (top - frame.origin_y) * scale_y));
    let (fill, stroke) = paint_children(events);
    let shape = VmlShape {
        name: attributes
            .get("id")
            .cloned()
            .unwrap_or_else(|| "Shape".to_owned()),
        description: attributes.get("alt").cloned().filter(|alt| !alt.is_empty()),
        style,
        paint: vml_paint::paint(&attributes, fill.as_ref(), stroke.as_ref()),
        preset,
        word_art: None,
    };
    Some(match content {
        Some(content) => GroupedTextBox {
            shape: Shape::TextBox(shape),
            wrap,
            content,
        },
        None => GroupedTextBox {
            shape: Shape::Rectangle(shape),
            wrap,
            content: Vec::new(),
        },
    })
}

/// The attributes of a member's own `v:fill` and `v:stroke`.
/// A start tag's attributes, keyed by local name.
type Attributes = BTreeMap<String, String>;

fn paint_children(events: &[Event<'static>]) -> (Option<Attributes>, Option<Attributes>) {
    let mut fill = None;
    let mut stroke = None;
    for event in events.iter().skip(1) {
        let (Event::Start(start) | Event::Empty(start)) = event else {
            continue;
        };
        match start.name().as_ref() {
            b"v:fill" if fill.is_none() => fill = Some(attributes_of(start.attributes().flatten())),
            b"v:stroke" if stroke.is_none() => {
                stroke = Some(attributes_of(start.attributes().flatten()));
            }
            _ => {}
        }
    }
    (fill, stroke)
}

fn textbox_children(events: &[Event<'static>]) -> Option<Vec<Event<'static>>> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut seen = false;
    for event in events {
        match event {
            Event::Start(start) => {
                let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
                if depth == 0 {
                    if !name.ends_with(":txbxContent") {
                        continue;
                    }
                    depth = 1;
                    seen = true;
                    continue;
                }
                depth += 1;
                out.push(event.clone());
            }
            Event::End(_) => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(out);
                }
                out.push(event.clone());
            }
            _ => {
                if depth > 0 {
                    out.push(event.clone());
                }
            }
        }
    }
    seen.then_some(out)
}

fn pair(value: &str) -> Option<(f64, f64)> {
    let (left, right) = value.split_once(',')?;
    Some((left.trim().parse().ok()?, right.trim().parse().ok()?))
}

fn style_numbers(style: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for declaration in style.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        if !matches!(name.as_str(), "left" | "top" | "width" | "height") {
            continue;
        }
        let number: String = value
            .trim()
            .chars()
            .take_while(|character| character.is_ascii_digit() || matches!(character, '.' | '-'))
            .collect();
        if let Ok(parsed) = number.parse() {
            out.insert(name, parsed);
        }
    }
    out
}

fn points_of(value: &str) -> Option<f64> {
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
    Some(number * per_point)
}

fn format_pt(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{rounded}pt")
}

/// Whether a namespace URI holds VML or the `w10:` vocabulary that positions it.
fn is_drawable_namespace(uri: &str) -> bool {
    is_vml(uri) || uri == WORD_NS
}

/// Whether a namespace URI is one of the VML families.
fn is_vml(uri: &str) -> bool {
    VML_NAMESPACES.contains(&uri)
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
        if let Ok(value) = attribute.normalized_value(XmlVersion::Implicit1_0) {
            out.insert(local, value.into_owned());
        }
    }
    out
}

/// The `w:drawing` events that replace a converted `w:pict` / `w:object`, split
/// around a text box's content.
///
/// The frame is the same for all four classes — `wp:inline` or `wp:anchor`, and the
/// choice is the style's, not the class's — and only the payload inside
/// `a:graphicData` differs.
///
/// **The split is a return value rather than a marker inside one list**, and that
/// is deliberate. A text box's content has to go through T1–T5 like everything else,
/// and writing it through `TransitionalNormalizer::rewrite_event` in place is both
/// simpler and impossible to get wrong. The marker version — emit a placeholder,
/// queue the content, queue the tail — was tried first and it is a trap: the caller
/// has to get three orderings right at once (head, content, tail), and getting one
/// of them wrong emits a well-formed part with the shape's body properties outside
/// the shape and the paragraph one level too deep. A queue is only needed when the
/// events come from *elsewhere* in the stream, which is the `mc:Fallback` case and
/// not this one: the whole text box is already in `subtree`.
#[must_use]
pub fn shape_events(
    shape: &Shape,
    wrap: Wrap,
    doc_pr_id: u32,
    report: &mut NormalizationReport,
    location: &SourceLocation,
) -> (Vec<Event<'static>>, Vec<Event<'static>>) {
    let frame = shape.frame();
    let (cx, cy) = extent_of(&frame.style, report, location, &frame.name);
    let mut head = vec![Event::Start(el("w:drawing", &[]))];
    if frame.style.absolute || frame.style.horizontal.is_some() {
        head.extend(anchor_events(shape, frame, wrap, cx, cy, doc_pr_id));
    } else {
        head.extend(inline_events(shape, frame, cx, cy, doc_pr_id));
    }
    head.push(Event::End(BytesEnd::new("w:drawing").into_owned()));
    (head, Vec::new())
}

/// The events before a text box's content and the events after it.
///
/// Two lists because the content is written **in place**, through the same
/// `TransitionalNormalizer::rewrite_event` as the rest of the part — which is
/// what makes it ordinary WML by the time it lands, and removes any need for a
/// marker, a queue or an ordering the caller has to get right.
#[must_use]
pub fn text_box_shape_events(
    shape: &Shape,
    wrap: Wrap,
    doc_pr_id: u32,
    report: &mut NormalizationReport,
    location: &SourceLocation,
) -> (Vec<Event<'static>>, Vec<Event<'static>>) {
    let (mut head, mut tail) = shape_events(shape, wrap, doc_pr_id, report, location);
    let at = head
        .iter()
        .position(|event| {
            matches!(event, Event::Start(start) if start.name().as_ref() == b"w:txbxContent")
        })
        .map_or(head.len(), |index| index + 1);
    let split = head.split_off(at);
    tail.splice(0..0, split);
    (head, tail)
}

/// `wp:extent` in EMU, and the record when the shape declared no usable size.
///
/// A zero extent is a shape that draws at no size, so it is a real defect and not
/// a default worth hiding; `wp:extent` is `use="required"` on both `CT_Inline` and
/// `CT_Anchor`, so something has to be written either way.
fn extent_of(
    style: &VmlStyle,
    report: &mut NormalizationReport,
    location: &SourceLocation,
    name: &str,
) -> (i64, i64) {
    let width = style.width_pt.as_deref().and_then(emu);
    let height = style.height_pt.as_deref().and_then(emu);
    if width.is_none() || height.is_none() {
        report.record_loss(LossRecord {
            transform_id: "T7.vml-size",
            feature_id: "v:shape/@style".to_owned(),
            reason: format!(
                "the VML shape {name:?} declared no usable width/height (width {:?}, height \
                 {:?}), so a zero extent is written: the frame is carried and will draw at no \
                 size",
                style.width_pt, style.height_pt
            ),
            severity: Severity::Lossy,
            locations: vec![location.clone()],
        });
    }
    (
        i64::from(width.unwrap_or(0)),
        i64::from(height.unwrap_or(0)),
    )
}

fn inline_events(
    class: &Shape,
    shape: &VmlShape,
    cx: i64,
    cy: i64,
    doc_pr_id: u32,
) -> Vec<Event<'static>> {
    let mut out = vec![Event::Start(el(
        "wp:inline",
        &[
            ("distT", "0"),
            ("distB", "0"),
            ("distL", "0"),
            ("distR", "0"),
        ],
    ))];
    out.push(Event::Empty(extent(cx, cy)));
    out.push(Event::Empty(el(
        "wp:effectExtent",
        &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")],
    )));
    out.extend(doc_pr_events(
        &shape.name,
        shape.description.as_deref(),
        doc_pr_id,
    ));
    out.extend(graphic_events(class, cx, cy));
    out.push(Event::End(BytesEnd::new("wp:inline").into_owned()));
    out
}

/// The closing half of a text box: `wps:bodyPr` and the tags that close it.
///
/// `wps:bodyPr` is required after the optional `wps:txbx` in
/// `CT_WordprocessingShape`, so it is always written. VML's `insetmode`/`inset`
/// would become its `*Ins` attributes, and the defaults are left to the consumer:
/// the model carries none for a converted shape, and inventing an inset is a claim
/// about the text layout that nothing supports.
#[must_use]
pub(crate) fn text_box_close() -> Vec<Event<'static>> {
    vec![
        Event::End(BytesEnd::new("w:txbxContent").into_owned()),
        Event::End(BytesEnd::new("wps:txbx").into_owned()),
        Event::Empty(el("wps:bodyPr", &[])),
    ]
}

/// The anchor for a `position:absolute` shape.
///
/// `CT_Anchor`'s attribute set is not optional in the way the prose suggests: it
/// has `simplePos`, `relativeHeight`, `behindDoc`, `locked`, `layoutInCell` and
/// `allowOverlap` as **required** booleans/integers, and its children are a fixed
/// `xsd:sequence` (`simplePos`, `positionH`, `positionV`, `extent`, `effectExtent`,
/// one `wrap*`, `docPr`, `cNvGraphicFramePr`, `graphic`). The writer emits the same
/// sequence, so a converted anchor and a written one are the same shape.
fn anchor_events(
    class: &Shape,
    shape: &VmlShape,
    wrap: Wrap,
    cx: i64,
    cy: i64,
    doc_pr_id: u32,
) -> Vec<Event<'static>> {
    let style = &shape.style;
    let behind = style.z_index.is_some_and(|index| index < 0);
    // `relativeHeight` is `ST_WrapDistance`, an unsigned measure, so a negative
    // `z-index` cannot be copied into it. Word's own convention is the unsigned
    // magnitude, which is what a reader compares against the other anchors on the
    // page.
    let relative_height = style.z_index.map_or(0, |index| {
        u32::try_from(index.unsigned_abs()).unwrap_or(u32::MAX)
    });
    let relative_height = relative_height.to_string();
    let mut out = vec![Event::Start(el(
        "wp:anchor",
        &[
            ("distT", "0"),
            ("distB", "0"),
            ("distL", "114300"),
            ("distR", "114300"),
            ("simplePos", "0"),
            ("relativeHeight", relative_height.as_str()),
            ("behindDoc", bool_str(behind)),
            ("locked", "0"),
            ("layoutInCell", "1"),
            ("allowOverlap", "1"),
        ],
    ))];
    out.push(Event::Empty(el("wp:simplePos", &[("x", "0"), ("y", "0")])));
    out.extend(position_axis(
        "wp:positionH",
        style.relative_h(),
        style.horizontal.as_deref(),
        style.margin_left.as_deref().and_then(emu),
    ));
    out.extend(position_axis(
        "wp:positionV",
        style.relative_v(),
        style.vertical.as_deref(),
        style.margin_top.as_deref().and_then(emu),
    ));
    out.push(Event::Empty(extent(cx, cy)));
    out.push(Event::Empty(el(
        "wp:effectExtent",
        &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")],
    )));
    out.extend(wrap_events(wrap, cx, cy));
    out.extend(doc_pr_events(
        &shape.name,
        shape.description.as_deref(),
        doc_pr_id,
    ));
    out.extend(graphic_events(class, cx, cy));
    out.push(Event::End(BytesEnd::new("wp:anchor").into_owned()));
    out
}

fn extent(cx: i64, cy: i64) -> BytesStart<'static> {
    let (cx, cy) = (cx.to_string(), cy.to_string());
    el("wp:extent", &[("cx", cx.as_str()), ("cy", cy.as_str())])
}

/// `wp:positionH` / `wp:positionV`, from whichever of the two VML spellings is
/// present.
///
/// `@relativeFrom` is `use="required"` and the child is a required `xsd:choice` of
/// `wp:align` and `wp:posOffset`, so both are always written — the same two
/// violations per element `XS-21` was about.
///
/// **Alignment first, then offset.** A shape that has both is a producer that
/// wrote both, and the `mso-` pair wins: it names a base *and* an alignment, where
/// `margin-left` is a distance from the page edge that the pair usually
/// contradicts. A shape with only `margin-left` gets the offset, and a shape with
/// neither gets a zero offset — which is the position the element would have had
/// anyway.
fn position_axis(
    name: &'static str,
    relative_from: &'static str,
    align: Option<&str>,
    offset: Option<i32>,
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
        let text = offset.unwrap_or_default().to_string();
        out.push(Event::Start(el("wp:posOffset", &[])));
        out.push(Event::Text(BytesText::from_escaped(text)));
        out.push(Event::End(BytesEnd::new("wp:posOffset").into_owned()));
    }
    out.push(Event::End(BytesEnd::new(name).into_owned()));
    out
}

/// The `wp:wrap*` element the VML `w10:wrap` asked for.
///
/// `CT_WrapTight` and `CT_WrapThrough` require a `wp:wrapPolygon`, and
/// `CT_WrapPath` wants one `wp:start` plus at least two `wp:lineTo` — so an empty
/// polygon is **not** conformant either, and the **frame's own rectangle** goes
/// there instead. A tight wrap around a box is a tight wrap; the two-point default
/// would draw a diagonal, which is a claim about a shape we have no points for
/// (`docs/transitional-to-strict-audit.md` §14.4c).
fn wrap_events(wrap: Wrap, cx: i64, cy: i64) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    match wrap {
        Wrap::None => out.push(Event::Empty(el("wp:wrapNone", &[]))),
        Wrap::Square => {
            out.push(Event::Start(el(
                "wp:wrapSquare",
                &[("wrapText", "bothSides")],
            )));
            out.push(Event::End(BytesEnd::new("wp:wrapSquare").into_owned()));
        }
        Wrap::TopAndBottom => {
            out.push(Event::Start(el("wp:wrapTopBottom", &[])));
            out.push(Event::End(BytesEnd::new("wp:wrapTopBottom").into_owned()));
        }
        Wrap::Tight | Wrap::Through => {
            out.push(Event::Start(el(
                wrap.element(),
                &[("wrapText", "bothSides")],
            )));
            out.push(Event::Start(el("wp:wrapPolygon", &[])));
            out.push(Event::Start(el("wp:start", &[("x", "0"), ("y", "0")])));
            out.push(Event::End(BytesEnd::new("wp:start").into_owned()));
            for (x, y) in [(cx, 0), (cx, cy), (0, cy)] {
                let (x, y) = (x.to_string(), y.to_string());
                out.push(Event::Start(el(
                    "wp:lineTo",
                    &[("x", x.as_str()), ("y", y.as_str())],
                )));
                out.push(Event::End(BytesEnd::new("wp:lineTo").into_owned()));
            }
            out.push(Event::End(BytesEnd::new("wp:wrapPolygon").into_owned()));
            out.push(Event::End(BytesEnd::new(wrap.element()).into_owned()));
        }
    }
    out
}

fn doc_pr_events(name: &str, description: Option<&str>, doc_pr_id: u32) -> Vec<Event<'static>> {
    let id = doc_pr_id.to_string();
    let mut attributes: Vec<(&str, &str)> = vec![("id", id.as_str()), ("name", name)];
    if let Some(description) = description {
        attributes.push(("descr", description));
    }
    vec![Event::Empty(el("wp:docPr", &attributes))]
}

/// The `a:graphic` payload for this class.
///
/// Dispatched on the enum the classifier returned rather than re-derived from the
/// markup, so the two can never disagree about what a shape is.
fn graphic_events(class: &Shape, cx: i64, cy: i64) -> Vec<Event<'static>> {
    match class {
        Shape::Picture(picture) => picture_payload(&picture.shape.name, &picture.rel_id, cx, cy),
        // A freeform keeps its frame and loses its geometry; a text box's content
        // is spliced in by the caller. Both losses are recorded there, where the
        // shape's name is in scope.
        Shape::Rectangle(frame) | Shape::Freeform(frame) => shape_payload(frame, false, cx, cy),
        Shape::TextBox(frame) => shape_payload(frame, true, cx, cy),
    }
}

fn picture_payload(name: &str, rel_id: &str, cx: i64, cy: i64) -> Vec<Event<'static>> {
    vec![
        Event::Start(el("a:graphic", &[])),
        Event::Start(el("a:graphicData", &[("uri", URI_PICTURE)])),
        Event::Start(el("pic:pic", &[])),
        Event::Start(el("pic:nvPicPr", &[])),
        Event::Empty(el("pic:cNvPr", &[("id", "0"), ("name", name)])),
        Event::Empty(el("pic:cNvPicPr", &[])),
        Event::End(BytesEnd::new("pic:nvPicPr").into_owned()),
        Event::Start(el("pic:blipFill", &[])),
        // **The same `r:id`, not a new relationship.** `v:imagedata/@r:id` and
        // `a:blip/@r:embed` both name a relationship of this part, and the
        // relationship behind it is the same image part with the same
        // `.../relationships/image` type - which the normalizer has already
        // rewritten to its Strict form. Nothing about the relationship changes.
        Event::Empty(el("a:blip", &[("r:embed", rel_id)])),
        Event::Start(el("a:stretch", &[])),
        Event::Empty(el("a:fillRect", &[])),
        Event::End(BytesEnd::new("a:stretch").into_owned()),
        Event::End(BytesEnd::new("pic:blipFill").into_owned()),
        Event::Start(el("pic:spPr", &[])),
        Event::Start(el(
            "a:xfrm",
            &[("rot", "0"), ("flipH", "0"), ("flipV", "0")],
        )),
        Event::Empty(el("a:off", &[("x", "0"), ("y", "0")])),
        Event::Empty(shape_extent(cx, cy)),
        Event::End(BytesEnd::new("a:xfrm").into_owned()),
        Event::End(BytesEnd::new("pic:spPr").into_owned()),
        Event::End(BytesEnd::new("pic:pic").into_owned()),
        Event::End(BytesEnd::new("a:graphicData").into_owned()),
        Event::End(BytesEnd::new("a:graphic").into_owned()),
    ]
}

/// `wps:wsp`, the shape the writer already reads and writes back.
///
/// A shape with geometry gets its VML fill and outline ([`vml_paint`]); a
/// freeform frame, which has none, gets neither.
fn shape_payload(shape: &VmlShape, text_box: bool, cx: i64, cy: i64) -> Vec<Event<'static>> {
    let name = shape.name.as_str();
    let mut c_nv_pr: Vec<(&str, &str)> = vec![("id", "0"), ("name", name)];
    if let Some(description) = shape.description.as_deref() {
        c_nv_pr.push(("descr", description));
    }
    let mut out = vec![
        Event::Start(el("a:graphic", &[])),
        Event::Start(el("a:graphicData", &[("uri", URI_SHAPE)])),
        Event::Start(el("wps:wsp", &[])),
        Event::Start(el("wps:cNvSpPr", &[])),
        Event::Empty(el("a:spLocks", &[])),
        Event::End(BytesEnd::new("wps:cNvSpPr").into_owned()),
        Event::Empty(el("wps:cNvPr", &c_nv_pr)),
        Event::Start(el("wps:spPr", &[])),
        Event::Start(el(
            "a:xfrm",
            &[
                ("rot", rotation(shape.style.rotation.as_deref()).as_str()),
                ("flipH", bool_str(shape.style.flip_h)),
                ("flipV", bool_str(shape.style.flip_v)),
            ],
        )),
        Event::Empty(el("a:off", &[("x", "0"), ("y", "0")])),
        Event::Empty(shape_extent(cx, cy)),
        Event::End(BytesEnd::new("a:xfrm").into_owned()),
    ];
    if let Some(preset) = shape.preset {
        out.push(Event::Start(el("a:prstGeom", &[("prst", preset)])));
        out.push(Event::Empty(el("a:avLst", &[])));
        out.push(Event::End(BytesEnd::new("a:prstGeom").into_owned()));
        out.extend(vml_paint::events(&shape.paint, preset == "line"));
    }
    out.push(Event::End(BytesEnd::new("wps:spPr").into_owned()));
    // `wps:txbx` sits between `wps:spPr` and `wps:bodyPr` in
    // `CT_WordprocessingShape`'s sequence. It is opened **here** and closed by
    // [`text_box_close`] so that the content lands between them — the caller
    // splits this list at `w:txbxContent` and writes the content in place.
    if text_box {
        out.push(Event::Start(el("wps:txbx", &[])));
        out.push(Event::Start(el("w:txbxContent", &[])));
    }
    out.push(Event::End(BytesEnd::new("wps:wsp").into_owned()));
    out.push(Event::End(BytesEnd::new("a:graphicData").into_owned()));
    out.push(Event::End(BytesEnd::new("a:graphic").into_owned()));
    out
}

/// `a:xfrm/@rot`: 60 000ths of a degree in `[0, 21 600 000)`.
fn rotation(degrees: Option<&str>) -> String {
    let degrees = degrees.and_then(|text| text.trim().parse::<f64>().ok());
    let Some(degrees) = degrees.filter(|degrees| degrees.is_finite()) else {
        return "0".to_owned();
    };
    let turned = degrees.rem_euclid(360.0);
    format!("{:.0}", turned * 60_000.0)
}

fn shape_extent(cx: i64, cy: i64) -> BytesStart<'static> {
    let (cx, cy) = (cx.to_string(), cy.to_string());
    el("a:ext", &[("cx", cx.as_str()), ("cy", cy.as_str())])
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
        start.push_attribute(crate::xml::escape::attribute(key, value));
    }
    start
}

#[cfg(test)]
mod tests {
    use super::{classify, path_is_convertible, Shape, Wrap};
    use crate::normalize::transitional::PartContext;
    use crate::part::PartId;

    /// The corpus's two `v:rect`/`v:shape` fixtures, read straight through the
    /// classifier.
    ///
    /// The unit level on purpose: the same two shapes reached the writer as *no
    /// drawing at all* when the classifier was reached through the event loop, and
    /// the difference between "the classifier says no" and "the loop never asked
    /// it" is invisible from the outside.
    #[test]
    fn the_corpus_floating_shapes_classify_and_keep_their_wrap() {
        let source = r#"<w:pict xmlns:w="urn:w" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w10="urn:schemas-microsoft-com:office:word"><v:rect id="a" style="position:absolute;margin-left:68.05pt;width:52.45pt;height:.6pt"><w10:wrap type="topAndBottom"/></v:rect></w:pict>"#;
        let mut context = PartContext::new(PartId::new("/word/document.xml"));
        context.bind("v", "urn:schemas-microsoft-com:vml");
        context.bind("w10", "urn:schemas-microsoft-com:office:word");
        let subtree = events_of(source);
        let (shape, wrap) = classify(&subtree, &context).expect("a v:rect with a wrap is a shape");
        assert_eq!(wrap, Wrap::TopAndBottom);
        assert!(matches!(shape, Shape::Rectangle(_)), "{shape:?}");
        assert_eq!(shape.frame().style.margin_left.as_deref(), Some("68.05pt"));
    }

    #[test]
    fn an_arc_is_not_a_path_this_converts() {
        assert!(!path_is_convertible("m665994,l,,,7199r665994,l665994,xe"));
        assert!(path_is_convertible("m0,0l100,100x"));
    }

    /// The events of `xml`, as the collector produces them.
    fn events_of(xml: &str) -> Vec<quick_xml::events::Event<'static>> {
        use quick_xml::events::Event;
        use quick_xml::Reader;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(false);
        reader.config_mut().expand_empty_elements = false;
        let mut out = Vec::new();
        loop {
            match reader.read_event() {
                Ok(Event::Eof) | Err(_) => break,
                Ok(event) => out.push(match event {
                    Event::Start(s) => Event::Start(s.into_owned()),
                    Event::End(e) => Event::End(e.into_owned()),
                    Event::Empty(s) => Event::Empty(s.into_owned()),
                    Event::Text(t) => Event::Text(t.into_owned()),
                    other => other.into_owned(),
                }),
            }
        }
        out
    }
}
