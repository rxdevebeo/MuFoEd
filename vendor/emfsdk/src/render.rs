use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder};
use thiserror::Error;
use ttf_parser::{Face, OutlineBuilder};

use crate::bitmap::{
  BitmapCompression, DeviceIndependentBitmap, DibColorTable, DibColorUsage, DibHeader,
};
use crate::common::{Reader, SdkEnumValue};
use crate::emfplus::{
  EmfPlusBitmapPayload, EmfPlusBrushData, EmfPlusBrushRef, EmfPlusDrawArcData,
  EmfPlusDrawImageData, EmfPlusDrawImagePointsData, EmfPlusDrawPointsData,
  EmfPlusDrawRectShapeData, EmfPlusDrawStringData, EmfPlusFillPieData, EmfPlusFillRectShapeData,
  EmfPlusFontObject, EmfPlusImageData, EmfPlusImageObject, EmfPlusObjectAssembler,
  EmfPlusObjectData, EmfPlusObjectRecordData, EmfPlusPathObject, EmfPlusPathPointType,
  EmfPlusPathPointTypeFlags, EmfPlusPathPointTypeValue, EmfPlusPathPointTypes, EmfPlusPenObject,
  EmfPlusPointData, EmfPlusRecord, EmfPlusRecordData, EmfPlusRecordType,
  EmfPlusRotateWorldTransformData, EmfPlusScaleWorldTransformData,
  EmfPlusTranslateWorldTransformData,
};
use crate::wmf::{
  WmfBrushStyle, WmfEscapeData, WmfMetafileRef, WmfPenLineStyle, WmfRecordData,
  WmfTernaryRasterOperationCode,
};

// record ids. The byte offsets below are the EMR_STRETCHDIBITS /
// EMR_SETDIBITSTODEVICE record layout fields.
const EMF_HEADER_SIZE: usize = 108;
const EMF_RECORD_HEADER_SIZE: usize = 8;
const EMR_EOF: u32 = 14;
const EMR_POLYBEZIER: u32 = 2;
const EMR_POLYGON: u32 = 3;
const EMR_POLYLINE: u32 = 4;
const EMR_POLYBEZIER_TO: u32 = 5;
const EMR_POLYLINE_TO: u32 = 6;
const EMR_POLYPOLYLINE: u32 = 7;
const EMR_POLYPOLYGON: u32 = 8;
const EMR_SET_WINDOW_EXT_EX: u32 = 9;
const EMR_SET_WINDOW_ORG_EX: u32 = 10;
const EMR_SET_VIEWPORT_EXT_EX: u32 = 11;
const EMR_SET_VIEWPORT_ORG_EX: u32 = 12;
const EMR_SET_PIXEL_V: u32 = 15;
const EMR_SET_TEXT_COLOR: u32 = 24;
const EMR_MOVE_TO_EX: u32 = 27;
const EMR_SAVE_DC: u32 = 33;
const EMR_RESTORE_DC: u32 = 34;
const EMR_SET_WORLD_TRANSFORM: u32 = 35;
const EMR_MODIFY_WORLD_TRANSFORM: u32 = 36;
const EMR_SELECT_OBJECT: u32 = 37;
const EMR_CREATE_PEN: u32 = 38;
const EMR_CREATE_BRUSH_INDIRECT: u32 = 39;
const EMR_DELETE_OBJECT: u32 = 40;
const EMR_ELLIPSE: u32 = 42;
const EMR_RECTANGLE: u32 = 43;
const EMR_ROUND_RECT: u32 = 44;
const EMR_ARC: u32 = 45;
const EMR_CHORD: u32 = 46;
const EMR_PIE: u32 = 47;
const EMR_LINE_TO: u32 = 54;
const EMR_SET_DIBITS_TO_DEVICE: u32 = 80;
const EMR_STRETCH_DIBITS: u32 = 81;
const EMR_EXT_CREATE_FONT_INDIRECT_W: u32 = 82;
const EMR_EXT_TEXTOUT_A: u32 = 83;
const EMR_EXT_TEXTOUT_W: u32 = 84;
const EMR_POLYBEZIER16: u32 = 85;
const EMR_POLYGON16: u32 = 86;
const EMR_POLYLINE16: u32 = 87;
const EMR_POLYBEZIER_TO16: u32 = 88;
const EMR_POLYLINE_TO16: u32 = 89;
const EMR_POLYPOLYLINE16: u32 = 90;
const EMR_POLYPOLYGON16: u32 = 91;
const EMR_EXT_CREATE_PEN: u32 = 95;
const EMR_BITMAP_DEST_X_OFFSET: usize = 24;
const EMR_BITMAP_DEST_Y_OFFSET: usize = 28;
const EMR_BITMAP_SOURCE_WIDTH_OFFSET: usize = 40;
const EMR_BITMAP_SOURCE_HEIGHT_OFFSET: usize = 44;
const EMR_BITMAP_INFO_OFFSET_OFFSET: usize = 48;
const EMR_BITMAP_INFO_SIZE_OFFSET: usize = 52;
const EMR_BITMAP_BITS_OFFSET_OFFSET: usize = 56;
const EMR_BITMAP_BITS_SIZE_OFFSET: usize = 60;
const EMR_STRETCH_DIBITS_ROP_OFFSET: usize = 68;
const EMR_STRETCH_DIBITS_DEST_WIDTH_OFFSET: usize = 72;
const EMR_STRETCH_DIBITS_DEST_HEIGHT_OFFSET: usize = 76;
const ENHMETA_STOCK_OBJECT: u32 = 0x8000_0000;
const WHITE_BRUSH: u32 = ENHMETA_STOCK_OBJECT;
const BLACK_BRUSH: u32 = ENHMETA_STOCK_OBJECT | 4;
const NULL_BRUSH: u32 = ENHMETA_STOCK_OBJECT | 5;
const WHITE_PEN: u32 = ENHMETA_STOCK_OBJECT | 6;
const BLACK_PEN: u32 = ENHMETA_STOCK_OBJECT | 7;
const NULL_PEN: u32 = ENHMETA_STOCK_OBJECT | 8;
const MWT_IDENTITY: u32 = 1;
const MWT_LEFTMULTIPLY: u32 = 2;
const MWT_RIGHTMULTIPLY: u32 = 3;
const MWT_SET: u32 = 4;
const EMR_COMMENT: u32 = 70;
const EMR_COMMENT_EMFPLUS: u32 = 0x2B46_4D45;
const LOGFONT_FACE_NAME_CHARS: usize = 32;
// StrictLib patch: GDI text alignment ([MS-WMF] 2.1.2.18 TextAlignmentMode).
const TA_UPDATECP: u16 = 0x0001;
const TA_RIGHT: u16 = 0x0002;
const TA_CENTER: u16 = 0x0006;
const TA_BOTTOM: u16 = 0x0008;
const TA_BASELINE: u16 = 0x0018;
const EMR_SET_TEXT_ALIGN: u32 = 22;
// StrictLib patch: EMR_SETMAPMODE ([MS-EMF] 2.3.11.19).
const EMR_SET_MAP_MODE: u32 = 17;
const MM_TEXT: u32 = 1;
const MM_LOMETRIC: u32 = 2;
const MM_HIMETRIC: u32 = 3;
const MM_LOENGLISH: u32 = 4;
const MM_HIENGLISH: u32 = 5;
const MM_TWIPS: u32 = 6;
const MM_ISOTROPIC: u32 = 7;
const MM_ANISOTROPIC: u32 = 8;
// StrictLib patch: ExtTextOut carries a (dx, dy) pair per character.
const ETO_PDY: u32 = 0x2000;
// values and keeps DIB scanlines aligned to four bytes.
const BITMAPINFOHEADER_SIZE: usize = 40;
const BITMAP_WIDTH_OFFSET: usize = 4;
const BITMAP_HEIGHT_OFFSET: usize = 8;
const BITMAP_PLANES_OFFSET: usize = 12;
const BITMAP_BIT_COUNT_OFFSET: usize = 14;
const BITMAP_COMPRESSION_OFFSET: usize = 16;
const DIB_PLANES: u16 = 1;
const DIB_BIT_COUNT_24: u16 = 24;
const DIB_BIT_COUNT_32: u16 = 32;
const RGB_BYTES_PER_PIXEL: usize = 3;
const BGRA_BYTES_PER_PIXEL: usize = 4;
const DIB_ROW_ALIGNMENT_BYTES: usize = 4;
const BI_RGB: u32 = 0;
const BI_JPEG: u32 = 4;
const BI_PNG: u32 = 5;
const DEFAULT_RENDER_WIDTH: usize = 1024;
const DEFAULT_RENDER_HEIGHT: usize = 768;
const DEFAULT_MAX_PIXELS: usize = 16_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedMetafile {
  pub data: Vec<u8>,
  pub content_type: &'static str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderOptions {
  pub target_width_px: Option<u32>,
  pub target_height_px: Option<u32>,
  pub max_pixels: Option<u32>,
}

impl RenderOptions {
  fn resolved_canvas_size(self, natural_width: usize, natural_height: usize) -> (usize, usize) {
    let width = self
      .target_width_px
      .map(|value| value.max(1) as usize)
      .unwrap_or(natural_width.max(1));
    let height = self
      .target_height_px
      .map(|value| value.max(1) as usize)
      .unwrap_or(natural_height.max(1));
    clamp_canvas_size(width, height, self.max_pixels)
  }
}

#[derive(Debug, Error)]
pub enum RenderError {
  #[error("{0}")]
  Invalid(String),
}

impl From<String> for RenderError {
  fn from(value: String) -> Self {
    Self::Invalid(value)
  }
}

pub type RenderResult<T> = std::result::Result<T, RenderError>;

pub fn decode_metafile_as_raster(
  data: &[u8],
  content_type: Option<&str>,
) -> RenderResult<Option<DecodedMetafile>> {
  decode_metafile_as_raster_with_options(data, content_type, RenderOptions::default())
}

pub fn decode_metafile_as_raster_with_options(
  data: &[u8],
  content_type: Option<&str>,
  options: RenderOptions,
) -> RenderResult<Option<DecodedMetafile>> {
  if !looks_like_metafile(data, content_type) {
    return Ok(None);
  }

  if let Some(raster) = decode_emf_as_raster(data, options)? {
    return Ok(Some(raster));
  }

  if let Some(raster) = decode_wmf_as_raster(data, options)? {
    return Ok(Some(raster));
  }

  Ok(None)
}

#[derive(Clone, Debug)]
pub struct MetafileTextRun {
  pub text: String,
  pub x: f32,
  pub y: f32,
  pub font_size: Option<f32>,
}

pub fn extract_metafile_text_runs(data: &[u8], content_type: Option<&str>) -> Vec<MetafileTextRun> {
  if !looks_like_metafile(data, content_type) || !is_emf(data) {
    return Vec::new();
  }

  let mut state = match EmfTextState::new(data) {
    Ok(state) => state,
    Err(_) => return Vec::new(),
  };
  let mut runs = Vec::new();
  let mut pos = emf_header_size(data);
  while pos + EMF_RECORD_HEADER_SIZE <= data.len() {
    let Ok(record_type) = read_u32(data, pos) else {
      break;
    };
    let Ok(record_size) = read_u32(data, pos + 4) else {
      break;
    };
    let record_size = record_size as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || pos + record_size > data.len() {
      break;
    }

    match record_type {
      EMR_SET_WINDOW_ORG_EX if record_size >= 16 => {
        state.window_org_x = read_i32(data, pos + 8).unwrap_or(state.window_org_x);
        state.window_org_y = read_i32(data, pos + 12).unwrap_or(state.window_org_y);
      }
      EMR_SET_WINDOW_EXT_EX if record_size >= 16 => {
        state.window_ext_x = read_i32(data, pos + 8)
          .unwrap_or(state.window_ext_x)
          .abs()
          .max(1);
        state.window_ext_y = read_i32(data, pos + 12)
          .unwrap_or(state.window_ext_y)
          .abs()
          .max(1);
      }
      EMR_SET_VIEWPORT_ORG_EX if record_size >= 16 => {
        state.viewport_org_x = read_i32(data, pos + 8).unwrap_or(state.viewport_org_x);
        state.viewport_org_y = read_i32(data, pos + 12).unwrap_or(state.viewport_org_y);
      }
      EMR_SET_VIEWPORT_EXT_EX if record_size >= 16 => {
        state.viewport_ext_x = read_i32(data, pos + 8).unwrap_or(state.viewport_ext_x);
        state.viewport_ext_y = read_i32(data, pos + 12).unwrap_or(state.viewport_ext_y);
      }
      EMR_SET_WORLD_TRANSFORM if record_size >= 32 => {
        if let Ok(transform) = read_xform(data, pos + 8) {
          state.world_transform = transform;
        }
      }
      EMR_MODIFY_WORLD_TRANSFORM if record_size >= 36 => {
        if let (Ok(transform), Ok(mode)) = (read_xform(data, pos + 8), read_u32(data, pos + 32)) {
          state.world_transform = match mode {
            MWT_IDENTITY => EmfTransform::identity(),
            MWT_LEFTMULTIPLY => transform.multiply(state.world_transform),
            MWT_RIGHTMULTIPLY => state.world_transform.multiply(transform),
            MWT_SET => transform,
            _ => state.world_transform,
          };
        }
      }
      EMR_EXT_CREATE_FONT_INDIRECT_W if record_size >= 104 => {
        if let Some((object_id, font)) = read_logfont_object(data, pos, record_size)
          && object_id & ENHMETA_STOCK_OBJECT == 0
        {
          state.fonts.insert(object_id, font);
        }
      }
      EMR_SELECT_OBJECT if record_size >= 12 => {
        let object_id = read_u32(data, pos + 8).unwrap_or(0);
        if state.fonts.contains_key(&object_id) {
          state.current_font = Some(object_id);
        }
      }
      EMR_DELETE_OBJECT if record_size >= 12 => {
        let object_id = read_u32(data, pos + 8).unwrap_or(0);
        state.fonts.remove(&object_id);
        if state.current_font == Some(object_id) {
          state.current_font = None;
        }
      }
      EMR_EXT_TEXTOUT_W => {
        if let Some(text) = extract_emr_ext_text_out_w(data, pos, record_size)
          && !text.trim().is_empty()
          && let Some(run) = state.text_run(data, pos, record_size, text)
        {
          runs.push(run);
        }
      }
      EMR_EXT_TEXTOUT_A => {
        if let Some(text) = extract_emr_ext_text_out_a(data, pos, record_size)
          && !text.trim().is_empty()
          && let Some(run) = state.text_run(data, pos, record_size, text)
        {
          runs.push(run);
        }
      }
      EMR_EOF => break,
      _ => {}
    }

    pos += record_size;
  }

  runs
}

pub fn looks_like_metafile(data: &[u8], content_type: Option<&str>) -> bool {
  matches!(
    content_type,
    Some("image/x-wmf" | "image/wmf" | "image/x-emf" | "image/emf" | "application/x-msmetafile")
  ) || is_emf(data)
    || crate::wmf::looks_like_wmf(data)
}

fn decode_emf_as_raster(
  data: &[u8],
  options: RenderOptions,
) -> Result<Option<DecodedMetafile>, String> {
  if !is_emf(data) {
    return Ok(None);
  }
  let mut pos = emf_header_size(data);
  let mut bitmap_record = None;
  let mut bitmap_count = 0usize;
  let mut needs_vector_replay = false;

  while pos + EMF_RECORD_HEADER_SIZE <= data.len() {
    let record_type = read_u32(data, pos)?;
    let record_size = read_u32(data, pos + 4)? as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || pos + record_size > data.len() {
      return Err(format!(
        "invalid EMF record at offset {pos}: type=0x{record_type:08x} size={record_size}"
      ));
    }

    if matches!(record_type, EMR_SET_DIBITS_TO_DEVICE | EMR_STRETCH_DIBITS) {
      bitmap_count += 1;
      bitmap_record = Some((record_type, pos, record_size));
      if bitmap_count > 1 {
        needs_vector_replay = true;
      }
    } else if emf_record_needs_vector_replay(record_type) {
      needs_vector_replay = true;
    }

    pos += record_size;
    if record_type == EMR_EOF {
      break;
    }
  }

  if needs_vector_replay {
    return decode_vector_emf_as_png(data, options).map(Some);
  }

  let (record_type, record_offset, record_size) = match bitmap_record {
    Some(record) => record,
    None => return decode_vector_emf_as_png(data, options).map(Some),
  };
  decode_bitmap_record_as_raster(data, record_type, record_offset, record_size).map(Some)
}

fn emf_record_needs_vector_replay(record_type: u32) -> bool {
  matches!(
    record_type,
    EMR_POLYBEZIER
      | EMR_POLYGON
      | EMR_POLYLINE
      | EMR_POLYBEZIER_TO
      | EMR_POLYLINE_TO
      | EMR_POLYPOLYLINE
      | EMR_POLYPOLYGON
      | EMR_SET_PIXEL_V
      | EMR_MOVE_TO_EX
      | EMR_ELLIPSE
      | EMR_RECTANGLE
      | EMR_ROUND_RECT
      | EMR_ARC
      | EMR_CHORD
      | EMR_PIE
      | EMR_LINE_TO
      | EMR_COMMENT
      | EMR_EXT_TEXTOUT_A
      | EMR_EXT_TEXTOUT_W
      | EMR_POLYBEZIER16
      | EMR_POLYGON16
      | EMR_POLYLINE16
      | EMR_POLYBEZIER_TO16
      | EMR_POLYLINE_TO16
      | EMR_POLYPOLYLINE16
      | EMR_POLYPOLYGON16
  )
}

#[derive(Clone, Copy, Debug)]
struct EmfColor {
  r: u8,
  g: u8,
  b: u8,
}

impl EmfColor {
  fn not(self) -> Self {
    Self {
      r: !self.r,
      g: !self.g,
      b: !self.b,
    }
  }

  fn and(self, other: Self) -> Self {
    Self {
      r: self.r & other.r,
      g: self.g & other.g,
      b: self.b & other.b,
    }
  }

  fn or(self, other: Self) -> Self {
    Self {
      r: self.r | other.r,
      g: self.g | other.g,
      b: self.b | other.b,
    }
  }

  fn xor(self, other: Self) -> Self {
    Self {
      r: self.r ^ other.r,
      g: self.g ^ other.g,
      b: self.b ^ other.b,
    }
  }
}

#[derive(Clone, Copy, Debug)]
struct EmfPoint {
  x: i32,
  y: i32,
}

#[derive(Clone, Copy, Debug)]
struct EmfTransform {
  m11: f32,
  m12: f32,
  m21: f32,
  m22: f32,
  dx: f32,
  dy: f32,
}

impl EmfTransform {
  fn identity() -> Self {
    Self {
      m11: 1.0,
      m12: 0.0,
      m21: 0.0,
      m22: 1.0,
      dx: 0.0,
      dy: 0.0,
    }
  }

  fn apply(self, point: EmfPoint) -> (f32, f32) {
    let x = point.x as f32;
    let y = point.y as f32;
    (
      x * self.m11 + y * self.m21 + self.dx,
      x * self.m12 + y * self.m22 + self.dy,
    )
  }

  fn multiply(self, other: Self) -> Self {
    Self {
      m11: self.m11 * other.m11 + self.m12 * other.m21,
      m12: self.m11 * other.m12 + self.m12 * other.m22,
      m21: self.m21 * other.m11 + self.m22 * other.m21,
      m22: self.m21 * other.m12 + self.m22 * other.m22,
      dx: self.dx * other.m11 + self.dy * other.m21 + other.dx,
      dy: self.dx * other.m12 + self.dy * other.m22 + other.dy,
    }
  }
}

#[derive(Clone, Copy, Debug)]
struct EmfPen {
  color: EmfColor,
  width: usize,
}

#[derive(Clone, Debug)]
struct EmfFont {
  height: i32,
  // StrictLib patch: the full LOGFONT, for drawing.
  text: TextFont,
}

/// StrictLib patch: the LOGFONT fields a text run is drawn with. Sizes stay in
/// logical units; `EmfVectorState::draw_text` maps them onto the canvas the
/// same way it maps coordinates.
#[derive(Clone, Copy, Debug)]
struct TextFont {
  /// `lfHeight`: negative is the em height, positive the cell height, zero the
  /// device default.
  height: i32,
  weight: u16,
  italic: bool,
  char_set: u8,
  face: [u16; LOGFONT_FACE_NAME_CHARS],
}

impl TextFont {
  const DEFAULT_CHARSET: u8 = 1;

  fn with_height(height: i32) -> Self {
    Self {
      height,
      weight: 400,
      italic: false,
      char_set: Self::DEFAULT_CHARSET,
      face: [0; LOGFONT_FACE_NAME_CHARS],
    }
  }

  fn family(&self) -> String {
    let end = self.face.iter().position(|unit| *unit == 0).unwrap_or(self.face.len());
    String::from_utf16_lossy(&self.face[..end]).trim().to_string()
  }

  /// The Symbol face: its codes are Adobe Symbol positions, not text.
  fn is_symbol(&self) -> bool {
    self.family().eq_ignore_ascii_case("Symbol")
  }

  /// Decodes a WMF / `EMR_EXTTEXTOUTA` byte string in the font's charset,
  /// one character per byte so the `dx` array still lines up.
  fn decode_bytes(&self, bytes: &[u8]) -> String {
    let bytes = &bytes[..bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len())];
    if self.is_symbol() {
      return bytes.iter().map(|byte| symbol_to_unicode(u32::from(*byte))).collect();
    }
    let encoding = match self.char_set {
      0 | 1 => encoding_rs::WINDOWS_1252,
      161 => encoding_rs::WINDOWS_1253,
      162 => encoding_rs::WINDOWS_1254,
      163 => encoding_rs::WINDOWS_1258,
      177 => encoding_rs::WINDOWS_1255,
      178 => encoding_rs::WINDOWS_1256,
      186 => encoding_rs::WINDOWS_1257,
      204 => encoding_rs::WINDOWS_1251,
      238 => encoding_rs::WINDOWS_1250,
      _ => return bytes.iter().map(|byte| char::from(*byte)).collect(),
    };
    encoding.decode_without_bom_handling(bytes).0.into_owned()
  }

  /// Maps a UTF-16 run drawn in the Symbol face: Word writes its codes either
  /// as they are or in the U+F020..U+F0FF private-use block.
  fn map_unicode(&self, text: &str) -> String {
    if !self.is_symbol() {
      return text.to_string();
    }
    text
      .chars()
      .map(|ch| match u32::from(ch) {
        code @ 0x20..=0xFF => symbol_to_unicode(code),
        code @ 0xF020..=0xF0FF => symbol_to_unicode(code - 0xF000),
        _ => ch,
      })
      .collect()
  }
}

/// StrictLib patch: Adobe Symbol encoding to Unicode. Codes the face leaves
/// undefined come back as they are.
fn symbol_to_unicode(code: u32) -> char {
  const HIGH: [u16; 95] = [
    0x20AC, 0x03D2, 0x2032, 0x2264, 0x2044, 0x221E, 0x0192, 0x2663, // A0..A7
    0x2666, 0x2665, 0x2660, 0x2194, 0x2190, 0x2191, 0x2192, 0x2193, // A8..AF
    0x00B0, 0x00B1, 0x2033, 0x2265, 0x00D7, 0x221D, 0x2202, 0x2022, // B0..B7
    0x00F7, 0x2260, 0x2261, 0x2248, 0x2026, 0x23D0, 0x23AF, 0x21B5, // B8..BF
    0x2135, 0x2111, 0x211C, 0x2118, 0x2297, 0x2295, 0x2205, 0x2229, // C0..C7
    0x222A, 0x2283, 0x2287, 0x2284, 0x2282, 0x2286, 0x2208, 0x2209, // C8..CF
    0x2220, 0x2207, 0x00AE, 0x00A9, 0x2122, 0x220F, 0x221A, 0x22C5, // D0..D7
    0x00AC, 0x2227, 0x2228, 0x21D4, 0x21D0, 0x21D1, 0x21D2, 0x21D3, // D8..DF
    0x25CA, 0x27E8, 0x00AE, 0x00A9, 0x2122, 0x2211, 0x239B, 0x239C, // E0..E7
    0x239D, 0x23A1, 0x23A2, 0x23A3, 0x23A7, 0x23A8, 0x23A9, 0x23AA, // E8..EF
    0x00F0, 0x27E9, 0x222B, 0x2320, 0x23AE, 0x2321, 0x239E, 0x239F, // F0..F7
    0x23A0, 0x23A4, 0x23A5, 0x23A6, 0x23AB, 0x23AC, 0x23AD, // F8..FE
  ];
  let mapped = match code {
    0x22 => 0x2200,
    0x24 => 0x2203,
    0x27 => 0x220B,
    0x2A => 0x2217,
    0x2D => 0x2212,
    0x40 => 0x2245,
    0x41..=0x5A => u32::from(b"\x91\x92\xA7\x94\x95\xA6\x93\x97\x99\xD1\x9A\x9B\x9C\x9D\x9F\xA0\x98\xA1\xA3\xA4\xA5\xC2\xA9\x9E\xA8\x96"[(code - 0x41) as usize]) + 0x0300,
    0x5C => 0x2234,
    0x5E => 0x22A5,
    0x60 => 0x203E,
    0x61..=0x7A => u32::from(b"\xB1\xB2\xC7\xB4\xB5\xC6\xB3\xB7\xB9\xD5\xBA\xBB\xBC\xBD\xBF\xC0\xB8\xC1\xC3\xC4\xC5\xD6\xC9\xBE\xC8\xB6"[(code - 0x61) as usize]) + 0x0300,
    0x7E => 0x223C,
    0xA0..=0xFE => u32::from(HIGH[(code - 0xA0) as usize]),
    _ => code,
  };
  char::from_u32(mapped).unwrap_or('\u{FFFD}')
}

#[derive(Clone, Debug)]
struct EmfTextState {
  width: usize,
  height: usize,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  world_transform: EmfTransform,
  fonts: std::collections::HashMap<u32, EmfFont>,
  current_font: Option<u32>,
}

impl EmfTextState {
  fn new(data: &[u8]) -> Result<Self, String> {
    let left = read_i32(data, 8)?;
    let top = read_i32(data, 12)?;
    let right = read_i32(data, 16)?;
    let bottom = read_i32(data, 20)?;
    let width = (right - left + 1).max(1) as usize;
    let height = (bottom - top + 1).max(1) as usize;

    Ok(Self {
      width,
      height,
      window_org_x: 0,
      window_org_y: 0,
      window_ext_x: width as i32,
      window_ext_y: height as i32,
      viewport_org_x: 0,
      viewport_org_y: 0,
      viewport_ext_x: width as i32,
      viewport_ext_y: height as i32,
      world_transform: EmfTransform::identity(),
      fonts: std::collections::HashMap::new(),
      current_font: None,
    })
  }

  fn map_point(&self, point: EmfPoint) -> (f32, f32) {
    let (x, y) = self.world_transform.apply(point);
    let scale_x = self.viewport_ext_x as f32 / self.window_ext_x.max(1) as f32;
    let scale_y = self.viewport_ext_y as f32 / self.window_ext_y.max(1) as f32;
    (
      self.viewport_org_x as f32 + (x - self.window_org_x as f32) * scale_x,
      self.viewport_org_y as f32 + (y - self.window_org_y as f32) * scale_y,
    )
  }

  fn map_height(&self, height: i32) -> f32 {
    let (_, y0) = self.map_point(EmfPoint { x: 0, y: 0 });
    let (_, y1) = self.map_point(EmfPoint {
      x: 0,
      y: height.abs(),
    });
    (y1 - y0).abs()
  }

  fn text_run(
    &self,
    data: &[u8],
    record_offset: usize,
    record_size: usize,
    text: String,
  ) -> Option<MetafileTextRun> {
    let text_record = ext_text_record(data, record_offset, record_size)?;
    let (x, y) = self.map_point(EmfPoint {
      x: text_record.x,
      y: text_record.y,
    });
    let font_size = self
      .current_font
      .and_then(|id| self.fonts.get(&id))
      .map(|font| self.map_height(font.height));
    Some(MetafileTextRun {
      text,
      x: x / self.width.max(1) as f32,
      y: y / self.height.max(1) as f32,
      font_size: font_size.map(|height| height / self.height.max(1) as f32),
    })
  }
}

#[derive(Clone, Debug)]
struct EmfVectorState {
  width: usize,
  height: usize,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  // StrictLib patch: the page space → device mapping follows GDI. `map_mode`
  // selects how the window/viewport extents apply (signed: a negative extent
  // flips the axis); device units then go to the canvas through the header's
  // bounds, whose origin need not be (0, 0) (WQ-022, WQ-033).
  map_mode: u32,
  device_px_per_mm: (f32, f32),
  bounds_origin: (i32, i32),
  device_to_canvas: (f32, f32),
  world_transform: EmfTransform,
  brush_colors: std::collections::HashMap<u32, EmfColor>,
  pens: std::collections::HashMap<u32, EmfPen>,
  fonts: std::collections::HashMap<u32, EmfFont>,
  current_brush: Option<EmfColor>,
  current_pen: Option<EmfPen>,
  current_font: Option<u32>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  clip_rect: Option<(i32, i32, i32, i32)>,
  clip_mask: Option<Vec<bool>>,
  saved_states: Vec<EmfVectorSnapshot>,
  emf_plus_objects: Vec<Option<EmfPlusRenderObject>>,
  emf_plus_object_assembler: EmfPlusObjectAssembler,
  font_cache: RenderFontCache,
  // StrictLib patch: SetTextAlign / EMR_SETTEXTALIGN.
  text_align: u16,
  rgb: Vec<u8>,
}

#[derive(Clone, Debug)]
struct EmfVectorSnapshot {
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  map_mode: u32,
  world_transform: EmfTransform,
  current_brush: Option<EmfColor>,
  current_pen: Option<EmfPen>,
  current_font: Option<u32>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  clip_rect: Option<(i32, i32, i32, i32)>,
  clip_mask: Option<Vec<bool>>,
}

#[derive(Clone, Debug)]
enum EmfPlusRenderObject {
  Brush(Option<EmfPlusRenderBrush>),
  Pen(Option<EmfPen>),
  Path(Vec<EmfPoint>),
  Region(Vec<EmfPoint>),
  Image(RasterPixels),
  Font(EmfPlusFontObject),
  Unsupported,
}

#[derive(Clone, Debug)]
enum EmfPlusRenderBrush {
  Solid(EmfColor),
  Hatch {
    fore: EmfColor,
    back: EmfColor,
    style: u32,
  },
  LinearGradient {
    rect: (f32, f32, f32, f32),
    start: EmfColor,
    end: EmfColor,
  },
  PathGradient {
    center: (f32, f32),
    center_color: EmfColor,
    surround: EmfColor,
  },
  Texture(RasterPixels),
}

impl EmfPlusRenderBrush {
  fn representative_color(&self) -> EmfColor {
    match self {
      Self::Solid(color) => *color,
      Self::Hatch { fore, .. } => *fore,
      Self::LinearGradient { start, end, .. } => average_color(*start, *end),
      Self::PathGradient {
        center_color,
        surround,
        ..
      } => average_color(*center_color, *surround),
      Self::Texture(image) => average_image_color(image),
    }
  }

  fn color_at(&self, x: i32, y: i32) -> EmfColor {
    match self {
      Self::Solid(color) => *color,
      Self::Hatch { fore, back, style } => {
        let period = 4 + (style % 4) as i32;
        if (x + y).rem_euclid(period) == 0 {
          *fore
        } else {
          *back
        }
      }
      Self::LinearGradient { rect, start, end } => {
        let span = (rect.2 - rect.0).abs().max(1.0);
        let t = ((x as f32 - rect.0) / span).clamp(0.0, 1.0);
        lerp_color(*start, *end, t)
      }
      Self::PathGradient {
        center,
        center_color,
        surround,
      } => {
        let distance = ((x as f32 - center.0).hypot(y as f32 - center.1) / 256.0).clamp(0.0, 1.0);
        lerp_color(*center_color, *surround, distance)
      }
      Self::Texture(image) => {
        if image.width == 0 || image.height == 0 {
          return EmfColor { r: 0, g: 0, b: 0 };
        }
        let tx = x.rem_euclid(image.width as i32) as usize;
        let ty = y.rem_euclid(image.height as i32) as usize;
        let offset = (ty * image.width + tx) * RGB_BYTES_PER_PIXEL;
        EmfColor {
          r: image.rgb[offset],
          g: image.rgb[offset + 1],
          b: image.rgb[offset + 2],
        }
      }
    }
  }
}

/// StrictLib patch: system fonts are scanned once per process instead of once
/// per metafile, the face follows the run's LOGFONT (family, weight, italic),
/// and a glyph the face lacks comes from a fallback chain instead of sending
/// the whole run to the 5x7 bitmap font.
#[derive(Clone, Debug, Default)]
struct RenderFontCache;

struct LoadedFace {
  data: Vec<u8>,
  index: u32,
}

type FaceKey = (String, u16, bool);

static FONT_DB: std::sync::OnceLock<fontdb::Database> = std::sync::OnceLock::new();
static FACES: std::sync::OnceLock<
  std::sync::Mutex<std::collections::HashMap<FaceKey, Option<std::sync::Arc<LoadedFace>>>>,
> = std::sync::OnceLock::new();

/// Faces asked for a glyph the run's own face does not carry, in order.
const FALLBACK_FAMILIES: &[&str] = &[
  "Times New Roman",
  "Cambria Math",
  "Segoe UI Symbol",
  "STIX Two Math",
  "Noto Sans Math",
  "DejaVu Serif",
  "DejaVu Sans",
  "Liberation Serif",
  "Arial Unicode MS",
  "Arial",
];

fn font_db() -> &'static fontdb::Database {
  FONT_DB.get_or_init(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    db
  })
}

/// A face of `family` (empty: the generic serif, then sans-serif) closest to
/// `weight` / `italic`, cached for the process.
fn load_face(family: &str, weight: u16, italic: bool) -> Option<std::sync::Arc<LoadedFace>> {
  let key = (family.to_ascii_lowercase(), weight, italic);
  let faces = FACES.get_or_init(Default::default);
  if let Some(hit) = faces.lock().ok()?.get(&key) {
    return hit.clone();
  }
  let db = font_db();
  let families = if family.is_empty() {
    vec![fontdb::Family::Serif, fontdb::Family::SansSerif]
  } else {
    vec![fontdb::Family::Name(family)]
  };
  let query = fontdb::Query {
    families: &families,
    weight: fontdb::Weight(weight),
    style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
    ..fontdb::Query::default()
  };
  let loaded = db
    .query(&query)
    .and_then(|id| {
      db.with_face_data(id, |data, index| LoadedFace {
        data: data.to_vec(),
        index,
      })
    })
    .map(std::sync::Arc::new);
  faces.lock().ok()?.insert(key, loaded.clone());
  loaded
}

/// The face a run starts from: its own family unless that is Symbol (whose
/// codes are mapped to Unicode first), then the generic families, then the
/// fallback chain.
fn primary_face(font: &TextFont) -> Option<std::sync::Arc<LoadedFace>> {
  let family = if font.is_symbol() { String::new() } else { font.family() };
  load_face(&family, font.weight, font.italic)
    .or_else(|| load_face("", font.weight, font.italic))
    .or_else(|| {
      FALLBACK_FAMILIES
        .iter()
        .find_map(|name| load_face(name, font.weight, font.italic))
    })
}

/// A glyph of `ch` from the run's face or the first fallback that has it,
/// with its outline (em units) and advance (em units of its own face).
fn find_glyph(primary: &Face<'_>, ch: char, font: &TextFont) -> Option<(Vec<Vec<(f32, f32)>>, f32, f32)> {
  fn outline(face: &Face<'_>, ch: char) -> Option<(Vec<Vec<(f32, f32)>>, f32, f32)> {
    let glyph_id = face.glyph_index(ch)?;
    let mut builder = GlyphOutlineCollector::default();
    // A glyph without an outline (a space) still has an advance.
    let _ = face.outline_glyph(glyph_id, &mut builder);
    builder.finish_current();
    let advance = f32::from(face.glyph_hor_advance(glyph_id).unwrap_or(0));
    Some((builder.contours, advance, f32::from(face.units_per_em())))
  }
  if let Some(found) = outline(primary, ch) {
    return Some(found);
  }
  FALLBACK_FAMILIES.iter().find_map(|name| {
    let loaded = load_face(name, font.weight, font.italic)?;
    let face = Face::parse(&loaded.data, loaded.index).ok()?;
    outline(&face, ch)
  })
}

#[derive(Clone, Debug)]
struct RenderedGlyph {
  contours: Vec<Vec<(f32, f32)>>,
}

impl RenderFontCache {
  fn load() -> Self {
    Self
  }

  /// StrictLib patch: lays a run out at device `(x, y)` placed by `align`
  /// (`TA_*`). `height` is the mapped `lfHeight` (sign kept: negative is the em
  /// height, positive the cell height); `dx` are device advances. Returns the
  /// glyph outlines and the run's device advance; `None` only when no font is
  /// installed at all.
  #[allow(clippy::too_many_arguments)]
  fn render_text(
    &self,
    text: &str,
    x: f32,
    y: f32,
    height: f32,
    font: &TextFont,
    align: u16,
    dx: &[f32],
  ) -> Option<(Vec<RenderedGlyph>, f32)> {
    let loaded = primary_face(font)?;
    let face = Face::parse(&loaded.data, loaded.index).ok()?;
    let units_per_em = f32::from(face.units_per_em());
    if units_per_em <= 0.0 {
      return None;
    }
    let (ascent, descent) = face
      .tables()
      .os2
      .map(|os2| (f32::from(os2.windows_ascender()), f32::from(os2.windows_descender()).abs()))
      .filter(|(ascent, descent)| ascent + descent > 0.0)
      .unwrap_or((f32::from(face.ascender()), f32::from(face.descender()).abs()));
    let em = if height < 0.0 {
      -height
    } else {
      height * units_per_em / (ascent + descent).max(1.0)
    }
    .max(1.0);
    let scale = em / units_per_em;

    let mut laid_out = Vec::new();
    let mut cursor = 0.0f32;
    for (index, ch) in text.chars().enumerate() {
      if ch == '\n' || ch == '\r' {
        continue;
      }
      let (contours, advance) = match find_glyph(&face, ch, font) {
        Some((contours, advance, glyph_upem)) => {
          let glyph_scale = em / glyph_upem.max(1.0);
          let contours = contours
            .into_iter()
            .filter(|contour| contour.len() >= 3)
            .map(|contour| {
              contour
                .into_iter()
                .map(|(px, py)| (px * glyph_scale, py * glyph_scale))
                .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
          (contours, advance * glyph_scale)
        }
        None => (Vec::new(), em * 0.5),
      };
      laid_out.push((cursor, contours));
      cursor += dx.get(index).copied().unwrap_or(advance);
    }

    let origin_x = match align & TA_CENTER {
      TA_CENTER => x - cursor / 2.0,
      TA_RIGHT => x - cursor,
      _ => x,
    };
    let baseline_y = match align & TA_BASELINE {
      TA_BASELINE => y,
      TA_BOTTOM => y - descent * scale,
      _ => y + ascent * scale,
    };
    let glyphs = laid_out
      .into_iter()
      .map(|(offset, contours)| RenderedGlyph {
        contours: contours
          .into_iter()
          .map(|contour| {
            contour
              .into_iter()
              .map(|(px, py)| (origin_x + offset + px, baseline_y - py))
              .collect()
          })
          .collect(),
      })
      .collect();
    Some((glyphs, cursor))
  }
}

#[derive(Default)]
struct GlyphOutlineCollector {
  contours: Vec<Vec<(f32, f32)>>,
  current: Vec<(f32, f32)>,
  current_pos: (f32, f32),
}

impl GlyphOutlineCollector {
  fn finish_current(&mut self) {
    if self.current.len() >= 3 {
      self.contours.push(std::mem::take(&mut self.current));
    } else {
      self.current.clear();
    }
  }
}

impl OutlineBuilder for GlyphOutlineCollector {
  fn move_to(&mut self, x: f32, y: f32) {
    self.finish_current();
    self.current_pos = (x, y);
    self.current.push((x, y));
  }

  fn line_to(&mut self, x: f32, y: f32) {
    self.current_pos = (x, y);
    self.current.push((x, y));
  }

  fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
    let (x0, y0) = self.current_pos;
    for step in 1..=12 {
      let t = step as f32 / 12.0;
      let mt = 1.0 - t;
      self.current.push((
        mt * mt * x0 + 2.0 * mt * t * x1 + t * t * x,
        mt * mt * y0 + 2.0 * mt * t * y1 + t * t * y,
      ));
    }
    self.current_pos = (x, y);
  }

  fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
    let (x0, y0) = self.current_pos;
    for step in 1..=16 {
      let t = step as f32 / 16.0;
      let mt = 1.0 - t;
      self.current.push((
        mt.powi(3) * x0 + 3.0 * mt.powi(2) * t * x1 + 3.0 * mt * t.powi(2) * x2 + t.powi(3) * x,
        mt.powi(3) * y0 + 3.0 * mt.powi(2) * t * y1 + 3.0 * mt * t.powi(2) * y2 + t.powi(3) * y,
      ));
    }
    self.current_pos = (x, y);
  }

  fn close(&mut self) {
    self.finish_current();
  }
}

impl EmfVectorState {
  fn new_with_options(data: &[u8], options: RenderOptions) -> Result<Self, String> {
    let left = read_i32(data, 8)?;
    let top = read_i32(data, 12)?;
    let right = read_i32(data, 16)?;
    let bottom = read_i32(data, 20)?;
    let natural_width = (right - left + 1).max(1) as usize;
    let natural_height = (bottom - top + 1).max(1) as usize;
    let (width, height) = options.resolved_canvas_size(natural_width, natural_height);
    // szlDevice / szlMillimeters: the reference device's resolution, which the
    // metric map modes need.
    let px_per_mm = |device: usize, millimeters: usize| {
      match (read_i32(data, device), read_i32(data, millimeters)) {
        (Ok(px), Ok(mm)) if px > 0 && mm > 0 => px as f32 / mm as f32,
        _ => 96.0 / 25.4,
      }
    };

    Ok(Self {
      width,
      height,
      window_org_x: 0,
      window_org_y: 0,
      window_ext_x: 1,
      window_ext_y: 1,
      viewport_org_x: 0,
      viewport_org_y: 0,
      viewport_ext_x: 1,
      viewport_ext_y: 1,
      map_mode: MM_TEXT,
      device_px_per_mm: (px_per_mm(72, 80), px_per_mm(76, 84)),
      bounds_origin: (left, top),
      device_to_canvas: (
        width as f32 / natural_width as f32,
        height as f32 / natural_height as f32,
      ),
      world_transform: EmfTransform::identity(),
      brush_colors: std::collections::HashMap::new(),
      pens: std::collections::HashMap::new(),
      fonts: std::collections::HashMap::new(),
      current_brush: None,
      current_pen: Some(EmfPen {
        color: EmfColor { r: 0, g: 0, b: 0 },
        width: 1,
      }),
      current_font: None,
      current_pos: EmfPoint { x: 0, y: 0 },
      text_color: EmfColor { r: 0, g: 0, b: 0 },
      clip_rect: None,
      clip_mask: None,
      saved_states: Vec::new(),
      emf_plus_objects: Vec::new(),
      emf_plus_object_assembler: EmfPlusObjectAssembler::default(),
      font_cache: RenderFontCache::load(),
      text_align: 0,
      rgb: vec![255; width * height * RGB_BYTES_PER_PIXEL],
    })
  }

  fn map_point(&self, point: EmfPoint) -> (f32, f32) {
    let (x, y) = self.world_transform.apply(point);
    let (scale_x, scale_y) = self.page_scale();
    let device_x = self.viewport_org_x as f32 + (x - self.window_org_x as f32) * scale_x;
    let device_y = self.viewport_org_y as f32 + (y - self.window_org_y as f32) * scale_y;
    (
      (device_x - self.bounds_origin.0 as f32) * self.device_to_canvas.0,
      (device_y - self.bounds_origin.1 as f32) * self.device_to_canvas.1,
    )
  }

  /// StrictLib patch: device units per page-space unit under the current map
  /// mode ([MS-EMF] 2.1.21 MapMode). The extents keep their signs.
  fn page_scale(&self) -> (f32, f32) {
    let ratio = |viewport: i32, window: i32| {
      if window == 0 {
        1.0
      } else {
        viewport as f32 / window as f32
      }
    };
    let metric = |units_per_mm: f32| {
      (
        self.device_px_per_mm.0 / units_per_mm,
        -self.device_px_per_mm.1 / units_per_mm,
      )
    };
    match self.map_mode {
      MM_LOMETRIC => metric(10.0),
      MM_HIMETRIC => metric(100.0),
      MM_LOENGLISH => metric(100.0 / 25.4),
      MM_HIENGLISH => metric(1000.0 / 25.4),
      MM_TWIPS => metric(1440.0 / 25.4),
      MM_ISOTROPIC => {
        let x = ratio(self.viewport_ext_x, self.window_ext_x);
        let y = ratio(self.viewport_ext_y, self.window_ext_y);
        let scale = x.abs().min(y.abs());
        (scale.copysign(x), scale.copysign(y))
      }
      MM_ANISOTROPIC => (
        ratio(self.viewport_ext_x, self.window_ext_x),
        ratio(self.viewport_ext_y, self.window_ext_y),
      ),
      _ => (1.0, 1.0),
    }
  }

  /// StrictLib patch: the clip and bounds checks of `set_pixel`, shared with
  /// `blend_pixel`.
  fn pixel_offset(&self, x: i32, y: i32) -> Option<usize> {
    if x < 0 || y < 0 {
      return None;
    }
    if let Some((left, top, right, bottom)) = self.clip_rect
      && (x < left || x >= right || y < top || y >= bottom)
    {
      return None;
    }
    let (x, y) = (x as usize, y as usize);
    if x >= self.width || y >= self.height {
      return None;
    }
    if let Some(mask) = &self.clip_mask
      && !mask[y * self.width + x]
    {
      return None;
    }
    Some((y * self.width + x) * RGB_BYTES_PER_PIXEL)
  }

  fn set_pixel(&mut self, x: i32, y: i32, color: EmfColor) {
    let Some(offset) = self.pixel_offset(x, y) else {
      return;
    };
    self.rgb[offset] = color.r;
    self.rgb[offset + 1] = color.g;
    self.rgb[offset + 2] = color.b;
  }

  fn draw_rgb_image(
    &mut self,
    dest_x: i32,
    dest_y: i32,
    dest_width: i32,
    dest_height: i32,
    image: &RasterPixels,
  ) {
    let (mapped_left, mapped_top) = self.map_point(EmfPoint {
      x: dest_x,
      y: dest_y,
    });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: dest_x + dest_width,
      y: dest_y + dest_height,
    });
    let left = mapped_left.min(mapped_right).round() as i32;
    let top = mapped_top.min(mapped_bottom).round() as i32;
    let right = mapped_left.max(mapped_right).round() as i32;
    let bottom = mapped_top.max(mapped_bottom).round() as i32;
    let width = (right - left).max(1);
    let height = (bottom - top).max(1);

    for y in 0..height {
      let src_y = (y as usize * image.height) / height as usize;
      for x in 0..width {
        let src_x = (x as usize * image.width) / width as usize;
        let src_offset = (src_y * image.width + src_x) * RGB_BYTES_PER_PIXEL;
        self.set_pixel(
          left + x,
          top + y,
          EmfColor {
            r: image.rgb[src_offset],
            g: image.rgb[src_offset + 1],
            b: image.rgb[src_offset + 2],
          },
        );
      }
    }
  }

  fn draw_rgb_image_with_rop(
    &mut self,
    dest_x: i32,
    dest_y: i32,
    dest_width: i32,
    dest_height: i32,
    image: &RasterPixels,
    rop: WmfTernaryRasterOperationCode,
  ) {
    let (mapped_left, mapped_top) = self.map_point(EmfPoint {
      x: dest_x,
      y: dest_y,
    });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: dest_x + dest_width,
      y: dest_y + dest_height,
    });
    let left = mapped_left.min(mapped_right).round() as i32;
    let top = mapped_top.min(mapped_bottom).round() as i32;
    let right = mapped_left.max(mapped_right).round() as i32;
    let bottom = mapped_top.max(mapped_bottom).round() as i32;
    let width = (right - left).max(1);
    let height = (bottom - top).max(1);

    for y in 0..height {
      let src_y = (y as usize * image.height) / height as usize;
      for x in 0..width {
        let dest_x = left + x;
        let dest_y = top + y;
        let src_x = (x as usize * image.width) / width as usize;
        let src_offset = (src_y * image.width + src_x) * RGB_BYTES_PER_PIXEL;
        let src = EmfColor {
          r: image.rgb[src_offset],
          g: image.rgb[src_offset + 1],
          b: image.rgb[src_offset + 2],
        };
        if let Some(color) = self.apply_raster_op(dest_x, dest_y, src, rop) {
          self.set_pixel(dest_x, dest_y, color);
        }
      }
    }
  }

  fn fill_rect_with_rop(
    &mut self,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    rop: WmfTernaryRasterOperationCode,
  ) {
    let Some(brush) = self.current_brush else {
      return;
    };
    let (mapped_left, mapped_top) = self.map_point(EmfPoint { x: left, y: top });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: right,
      y: bottom,
    });
    let left = mapped_left.min(mapped_right).round().max(0.0) as i32;
    let top = mapped_top.min(mapped_bottom).round().max(0.0) as i32;
    let right = mapped_left.max(mapped_right).round().min(self.width as f32) as i32;
    let bottom = mapped_top
      .max(mapped_bottom)
      .round()
      .min(self.height as f32) as i32;
    for y in top..bottom {
      for x in left..right {
        if let Some(color) = self.apply_raster_op(x, y, brush, rop) {
          self.set_pixel(x, y, color);
        }
      }
    }
  }

  fn apply_raster_op(
    &self,
    x: i32,
    y: i32,
    src: EmfColor,
    rop: WmfTernaryRasterOperationCode,
  ) -> Option<EmfColor> {
    let dest = self.pixel_color(x, y).unwrap_or(EmfColor {
      r: 255,
      g: 255,
      b: 255,
    });
    let pattern = self.current_brush.unwrap_or(src);
    let color = match rop {
      WmfTernaryRasterOperationCode::BLACKNESS => EmfColor { r: 0, g: 0, b: 0 },
      WmfTernaryRasterOperationCode::WHITENESS => EmfColor {
        r: 255,
        g: 255,
        b: 255,
      },
      WmfTernaryRasterOperationCode::DSTINVERT => dest.not(),
      WmfTernaryRasterOperationCode::NOTSRCCOPY => src.not(),
      WmfTernaryRasterOperationCode::SRCCOPY => src,
      WmfTernaryRasterOperationCode::SRCPAINT => src.or(dest),
      WmfTernaryRasterOperationCode::SRCAND => src.and(dest),
      WmfTernaryRasterOperationCode::SRCINVERT => src.xor(dest),
      WmfTernaryRasterOperationCode::SRCERASE => src.and(dest.not()),
      WmfTernaryRasterOperationCode::MERGECOPY => src.and(pattern),
      WmfTernaryRasterOperationCode::MERGEPAINT => src.not().or(dest),
      WmfTernaryRasterOperationCode::PATCOPY => pattern,
      WmfTernaryRasterOperationCode::PATINVERT => pattern.xor(dest),
      WmfTernaryRasterOperationCode::PATPAINT => pattern.or(src.not()).or(dest),
      WmfTernaryRasterOperationCode::D => return None,
      _ => return None,
    };
    Some(color)
  }

  fn pixel_color(&self, x: i32, y: i32) -> Option<EmfColor> {
    if x < 0 || y < 0 {
      return None;
    }
    let (x, y) = (x as usize, y as usize);
    if x >= self.width || y >= self.height {
      return None;
    }
    let offset = (y * self.width + x) * RGB_BYTES_PER_PIXEL;
    Some(EmfColor {
      r: self.rgb[offset],
      g: self.rgb[offset + 1],
      b: self.rgb[offset + 2],
    })
  }

  /// StrictLib patch: a length in logical units, as device pixels.
  fn map_length(&self, dx: i32, dy: i32) -> f32 {
    let (x0, y0) = self.map_point(EmfPoint { x: 0, y: 0 });
    let (x1, y1) = self.map_point(EmfPoint { x: dx, y: dy });
    (x1 - x0).hypot(y1 - y0)
  }

  /// StrictLib patch: draws a run at logical `(x, y)`. The font height and the
  /// `dx` advances are logical units mapped like coordinates (the font height
  /// used to be taken as device pixels, so a metafile whose logical unit is
  /// smaller than a pixel drew its text many times too large); `align` places
  /// the run (`TA_*`). Returns the run's advance in logical units, for
  /// `TA_UPDATECP`.
  #[allow(clippy::too_many_arguments)]
  fn draw_text(
    &mut self,
    x: i32,
    y: i32,
    text: &str,
    color: EmfColor,
    font: &TextFont,
    align: u16,
    dx: &[i32],
  ) -> i32 {
    let (mapped_x, mapped_y) = self.map_point(EmfPoint { x, y });
    let logical_height = if font.height == 0 { -12 } else { font.height };
    let device_height = self.map_length(0, logical_height.abs()).max(1.0) * logical_height.signum() as f32;
    let unit_x = self.map_length(1000, 0) / 1000.0;
    let device_dx = dx.iter().map(|advance| *advance as f32 * unit_x).collect::<Vec<_>>();
    if let Some((glyphs, advance)) = self.font_cache.render_text(
      text,
      mapped_x,
      mapped_y,
      device_height,
      font,
      align,
      &device_dx,
    ) {
      for glyph in glyphs {
        self.fill_device_contours(&glyph.contours, color);
      }
      return if unit_x > 0.0 {
        (advance / unit_x).round() as i32
      } else {
        0
      };
    }

    let scale = ((device_height.abs() as usize).max(7) / 7).max(1);
    let mut cursor_x = mapped_x.round() as i32;
    let baseline_y = mapped_y.round() as i32;
    for ch in text.chars() {
      if ch.is_whitespace() {
        cursor_x += (4 * scale) as i32;
        continue;
      }
      draw_glyph_5x7(
        self,
        cursor_x,
        baseline_y - (7 * scale) as i32,
        ch,
        color,
        scale,
      );
      cursor_x += (6 * scale) as i32;
    }
    if unit_x > 0.0 {
      ((cursor_x as f32 - mapped_x) / unit_x).round() as i32
    } else {
      0
    }
  }

  fn fill_device_contours(&mut self, contours: &[Vec<(f32, f32)>], color: EmfColor) {
    if contours.is_empty() {
      return;
    }
    let mut top = self.height as i32;
    let mut bottom = 0i32;
    for contour in contours {
      for (_, y) in contour {
        top = top.min(y.floor() as i32);
        bottom = bottom.max(y.ceil() as i32);
      }
    }
    top = top.clamp(0, self.height as i32);
    bottom = bottom.clamp(0, self.height as i32);
    // StrictLib patch: anti-aliased coverage - SUBSAMPLES scanlines per pixel
    // row and the exact horizontal overlap of every span - instead of one
    // sample at the pixel centre, which dropped a horizontal stroke thinner
    // than a pixel (a minus sign, a fraction bar) unless it happened to cross
    // that centre, and widened every span to whole pixels.
    const SUBSAMPLES: usize = 4;
    let width = self.width;
    let mut coverage = vec![0.0f32; width];
    let mut intersections = Vec::new();
    for y in top..bottom {
      coverage.fill(0.0);
      let mut touched = false;
      for sample in 0..SUBSAMPLES {
        let scan_y = y as f32 + (sample as f32 + 0.5) / SUBSAMPLES as f32;
        intersections.clear();
        for contour in contours {
          for index in 0..contour.len() {
            let (x1, y1) = contour[index];
            let (x2, y2) = contour[(index + 1) % contour.len()];
            if (y1 <= scan_y && y2 > scan_y) || (y2 <= scan_y && y1 > scan_y) {
              let t = (scan_y - y1) / (y2 - y1);
              intersections.push(x1 + t * (x2 - x1));
            }
          }
        }
        intersections.sort_by(|a, b| a.total_cmp(b));
        for pair in intersections.chunks_exact(2) {
          let start = pair[0].clamp(0.0, width as f32);
          let end = pair[1].clamp(0.0, width as f32);
          if end <= start {
            continue;
          }
          touched = true;
          for x in (start.floor() as usize)..(end.ceil() as usize).min(width) {
            let overlap = end.min(x as f32 + 1.0) - start.max(x as f32);
            coverage[x] += overlap.max(0.0) / SUBSAMPLES as f32;
          }
        }
      }
      if !touched {
        continue;
      }
      for (x, amount) in coverage.iter().enumerate() {
        if *amount > 0.0 {
          self.blend_pixel(x as i32, y, color, amount.min(1.0));
        }
      }
    }
  }

  /// StrictLib patch: `set_pixel` with partial coverage.
  fn blend_pixel(&mut self, x: i32, y: i32, color: EmfColor, alpha: f32) {
    if alpha >= 0.999 {
      self.set_pixel(x, y, color);
      return;
    }
    let Some(offset) = self.pixel_offset(x, y) else {
      return;
    };
    let mix = |under: u8, over: u8| (f32::from(under) + (f32::from(over) - f32::from(under)) * alpha).round() as u8;
    self.rgb[offset] = mix(self.rgb[offset], color.r);
    self.rgb[offset + 1] = mix(self.rgb[offset + 1], color.g);
    self.rgb[offset + 2] = mix(self.rgb[offset + 2], color.b);
  }

  fn fill_arc_segment(
    &mut self,
    rect: (i32, i32, i32, i32),
    start_angle: f32,
    sweep_angle: f32,
    pie: bool,
  ) {
    let (left, top, right, bottom) = rect;
    let points = arc_segment_points(left, top, right, bottom, start_angle, sweep_angle, pie);
    if pie {
      self.fill_polygon(&points);
      self.draw_polyline(&points, true);
    } else {
      self.draw_polyline(&points, false);
    }
  }

  fn save_state(&mut self) {
    self.saved_states.push(EmfVectorSnapshot {
      window_org_x: self.window_org_x,
      window_org_y: self.window_org_y,
      window_ext_x: self.window_ext_x,
      window_ext_y: self.window_ext_y,
      viewport_org_x: self.viewport_org_x,
      viewport_org_y: self.viewport_org_y,
      viewport_ext_x: self.viewport_ext_x,
      viewport_ext_y: self.viewport_ext_y,
      map_mode: self.map_mode,
      world_transform: self.world_transform,
      current_brush: self.current_brush,
      current_pen: self.current_pen,
      current_font: self.current_font,
      current_pos: self.current_pos,
      text_color: self.text_color,
      clip_rect: self.clip_rect,
      clip_mask: self.clip_mask.clone(),
    });
  }

  fn restore_state(&mut self) {
    let Some(saved) = self.saved_states.pop() else {
      return;
    };
    self.window_org_x = saved.window_org_x;
    self.window_org_y = saved.window_org_y;
    self.window_ext_x = saved.window_ext_x;
    self.window_ext_y = saved.window_ext_y;
    self.viewport_org_x = saved.viewport_org_x;
    self.viewport_org_y = saved.viewport_org_y;
    self.viewport_ext_x = saved.viewport_ext_x;
    self.viewport_ext_y = saved.viewport_ext_y;
    self.map_mode = saved.map_mode;
    self.world_transform = saved.world_transform;
    self.current_brush = saved.current_brush;
    self.current_pen = saved.current_pen;
    self.current_font = saved.current_font;
    self.current_pos = saved.current_pos;
    self.text_color = saved.text_color;
    self.clip_rect = saved.clip_rect;
    self.clip_mask = saved.clip_mask;
  }

  fn set_clip_rect_logical(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
    let (x1, y1) = self.map_point(EmfPoint { x: left, y: top });
    let (x2, y2) = self.map_point(EmfPoint {
      x: right,
      y: bottom,
    });
    let rect = (
      x1.min(x2).floor().max(0.0) as i32,
      y1.min(y2).floor().max(0.0) as i32,
      x1.max(x2).ceil().min(self.width as f32) as i32,
      y1.max(y2).ceil().min(self.height as f32) as i32,
    );
    self.set_clip_rect_device(rect, 0);
  }

  fn set_clip_rect_device(&mut self, rect: (i32, i32, i32, i32), combine_mode: u8) {
    let next = (
      rect.0.clamp(0, self.width as i32),
      rect.1.clamp(0, self.height as i32),
      rect.2.clamp(0, self.width as i32),
      rect.3.clamp(0, self.height as i32),
    );
    if combine_mode == 0 {
      self.clip_rect = Some(next);
      self.clip_mask = None;
      return;
    }
    if combine_mode == 1 && self.clip_mask.is_none() {
      self.clip_rect = Some(match self.clip_rect {
        Some(current) => intersect_rects(current, next),
        None => next,
      });
      return;
    }
    let mask = self.rect_clip_mask(next);
    self.combine_clip_mask(mask, combine_mode);
  }

  fn set_clip_points_logical(&mut self, points: &[EmfPoint], combine_mode: u8) {
    let mapped = points
      .iter()
      .map(|point| self.map_point(*point))
      .collect::<Vec<_>>();
    if let Some(rect) = axis_aligned_clip_rect(&mapped, self.width, self.height) {
      self.set_clip_rect_device(rect, combine_mode);
      return;
    }
    let mask = self.polygon_mask(&mapped);
    self.combine_clip_mask(mask, combine_mode);
  }

  fn offset_clip(&mut self, dx: f32, dy: f32) {
    if let Some((left, top, right, bottom)) = self.clip_rect {
      self.clip_rect = Some((
        (left as f32 + dx).round() as i32,
        (top as f32 + dy).round() as i32,
        (right as f32 + dx).round() as i32,
        (bottom as f32 + dy).round() as i32,
      ));
    }
    if let Some(mask) = self.clip_mask.take() {
      let mut shifted = vec![false; mask.len()];
      let dx = dx.round() as i32;
      let dy = dy.round() as i32;
      for y in 0..self.height {
        for x in 0..self.width {
          if !mask[y * self.width + x] {
            continue;
          }
          let nx = x as i32 + dx;
          let ny = y as i32 + dy;
          if nx >= 0 && ny >= 0 && nx < self.width as i32 && ny < self.height as i32 {
            shifted[ny as usize * self.width + nx as usize] = true;
          }
        }
      }
      self.clip_mask = Some(shifted);
      self.update_clip_rect_from_mask();
    }
  }

  fn combine_clip_mask(&mut self, next: Vec<bool>, combine_mode: u8) {
    let current = match self.clip_mask.take() {
      Some(mask) => Some(mask),
      None => self.clip_rect.map(|rect| self.rect_clip_mask(rect)),
    };
    let mask = match (current, combine_mode) {
      (_, 0) => Some(next),
      (None, 1) => Some(next),
      (Some(current), 1) => Some(
        current
          .into_iter()
          .zip(next)
          .map(|(left, right)| left && right)
          .collect(),
      ),
      (None, 2) => None,
      (Some(current), 2) => Some(
        current
          .into_iter()
          .zip(next)
          .map(|(left, right)| left || right)
          .collect(),
      ),
      (None, 3) => Some(next),
      (Some(current), 3) => Some(
        current
          .into_iter()
          .zip(next)
          .map(|(left, right)| left ^ right)
          .collect(),
      ),
      (None, 4) => None,
      (Some(current), 4) => Some(
        current
          .into_iter()
          .zip(next)
          .map(|(left, right)| left && !right)
          .collect(),
      ),
      (None, 5) => Some(next.into_iter().map(|value| !value).collect()),
      (Some(current), 5) => Some(
        current
          .into_iter()
          .zip(next)
          .map(|(left, right)| right && !left)
          .collect(),
      ),
      (_, _) => Some(next),
    };
    self.clip_mask = mask;
    self.update_clip_rect_from_mask();
  }

  fn rect_clip_mask(&self, rect: (i32, i32, i32, i32)) -> Vec<bool> {
    let mut mask = vec![false; self.width * self.height];
    for y in rect.1.max(0) as usize..rect.3.max(0) as usize {
      let row = y * self.width;
      for x in rect.0.max(0) as usize..rect.2.max(0) as usize {
        mask[row + x] = true;
      }
    }
    mask
  }

  fn update_clip_rect_from_mask(&mut self) {
    let Some(mask) = &self.clip_mask else {
      self.clip_rect = None;
      return;
    };
    let mut left = self.width;
    let mut top = self.height;
    let mut right = 0usize;
    let mut bottom = 0usize;
    for y in 0..self.height {
      for x in 0..self.width {
        if !mask[y * self.width + x] {
          continue;
        }
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + 1);
        bottom = bottom.max(y + 1);
      }
    }
    self.clip_rect = (right > left && bottom > top).then_some((
      left as i32,
      top as i32,
      right as i32,
      bottom as i32,
    ));
  }

  fn polygon_mask(&self, mapped: &[(f32, f32)]) -> Vec<bool> {
    let mut mask = vec![false; self.width * self.height];
    if mapped.len() < 3 {
      return mask;
    }
    visit_polygon_scanline_spans(mapped, self.width, self.height, |y, start, end| {
      for x in start..end {
        mask[y * self.width + x] = true;
      }
    });
    mask
  }

  fn fill_polygon(&mut self, points: &[EmfPoint]) {
    let Some(color) = self.current_brush else {
      return;
    };
    if points.len() < 3 {
      return;
    }

    let mapped = points
      .iter()
      .map(|point| self.map_point(*point))
      .collect::<Vec<_>>();
    let width = self.width;
    let height = self.height;
    visit_polygon_scanline_spans(&mapped, width, height, |y, start, end| {
      for x in start..end {
        self.set_pixel(x as i32, y as i32, color);
      }
    });
  }

  fn fill_polygon_with_emf_plus_brush(&mut self, points: &[EmfPoint], brush: &EmfPlusRenderBrush) {
    if points.len() < 3 {
      return;
    }

    let mapped = points
      .iter()
      .map(|point| self.map_point(*point))
      .collect::<Vec<_>>();
    let width = self.width;
    let height = self.height;
    visit_polygon_scanline_spans(&mapped, width, height, |y, start, end| {
      for x in start..end {
        self.set_pixel(x as i32, y as i32, brush.color_at(x as i32, y as i32));
      }
    });
  }

  fn draw_polyline(&mut self, points: &[EmfPoint], closed: bool) {
    let Some(pen) = self.current_pen else {
      return;
    };
    if points.len() < 2 {
      return;
    }
    for pair in points.windows(2) {
      self.draw_line(pair[0], pair[1], pen);
    }
    if closed {
      self.draw_line(points[points.len() - 1], points[0], pen);
    }
  }

  fn draw_line(&mut self, a: EmfPoint, b: EmfPoint, pen: EmfPen) {
    let (x0, y0) = self.map_point(a);
    let (x1, y1) = self.map_point(b);
    let mut x0 = x0.round() as i32;
    let mut y0 = y0.round() as i32;
    let x1 = x1.round() as i32;
    let y1 = y1.round() as i32;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
      self.set_pen_pixel(x0, y0, pen);
      if x0 == x1 && y0 == y1 {
        break;
      }
      let e2 = 2 * error;
      if e2 >= dy {
        error += dy;
        x0 += sx;
      }
      if e2 <= dx {
        error += dx;
        y0 += sy;
      }
    }
  }

  fn set_pen_pixel(&mut self, x: i32, y: i32, pen: EmfPen) {
    // StrictLib patch: a pen width is logical units, mapped like a length; a
    // width that maps under a pixel (cosmetic pens included) is one pixel.
    let logical = i32::try_from(pen.width).unwrap_or(i32::MAX);
    let device = self.map_length(logical, 0).max(self.map_length(0, logical));
    let radius = (device.round().max(1.0) as i32) / 2;
    for yy in y - radius..=y + radius {
      for xx in x - radius..=x + radius {
        self.set_pixel(xx, yy, pen.color);
      }
    }
  }

  fn fill_rect(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
    let points = [
      EmfPoint { x: left, y: top },
      EmfPoint { x: right, y: top },
      EmfPoint {
        x: right,
        y: bottom,
      },
      EmfPoint { x: left, y: bottom },
    ];
    self.fill_polygon(&points);
    self.draw_polyline(&points, true);
  }

  fn fill_ellipse(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
    let steps = 72;
    let cx = (left + right) as f32 / 2.0;
    let cy = (top + bottom) as f32 / 2.0;
    let rx = (right - left).abs() as f32 / 2.0;
    let ry = (bottom - top).abs() as f32 / 2.0;
    let mut points = Vec::with_capacity(steps);
    for index in 0..steps {
      let theta = index as f32 * std::f32::consts::TAU / steps as f32;
      points.push(EmfPoint {
        x: (cx + theta.cos() * rx).round() as i32,
        y: (cy + theta.sin() * ry).round() as i32,
      });
    }
    self.fill_polygon(&points);
    self.draw_polyline(&points, true);
  }

  fn fill_ellipse_with_emf_plus_brush(
    &mut self,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    brush: &EmfPlusRenderBrush,
  ) {
    let steps = 72;
    let cx = (left + right) as f32 / 2.0;
    let cy = (top + bottom) as f32 / 2.0;
    let rx = (right - left).abs() as f32 / 2.0;
    let ry = (bottom - top).abs() as f32 / 2.0;
    let mut points = Vec::with_capacity(steps);
    for index in 0..steps {
      let theta = index as f32 * std::f32::consts::TAU / steps as f32;
      points.push(EmfPoint {
        x: (cx + theta.cos() * rx).round() as i32,
        y: (cy + theta.sin() * ry).round() as i32,
      });
    }
    self.fill_polygon_with_emf_plus_brush(&points, brush);
    self.draw_polyline(&points, true);
  }

  fn select_object(&mut self, object_id: u32) {
    match object_id {
      WHITE_BRUSH => {
        self.current_brush = Some(EmfColor {
          r: 255,
          g: 255,
          b: 255,
        })
      }
      BLACK_BRUSH => self.current_brush = Some(EmfColor { r: 0, g: 0, b: 0 }),
      NULL_BRUSH => self.current_brush = None,
      WHITE_PEN => {
        self.current_pen = Some(EmfPen {
          color: EmfColor {
            r: 255,
            g: 255,
            b: 255,
          },
          width: 1,
        })
      }
      BLACK_PEN => {
        self.current_pen = Some(EmfPen {
          color: EmfColor { r: 0, g: 0, b: 0 },
          width: 1,
        })
      }
      NULL_PEN => self.current_pen = None,
      _ => {
        if let Some(brush) = self.brush_colors.get(&object_id).copied() {
          self.current_brush = Some(brush);
        }
        if let Some(pen) = self.pens.get(&object_id).copied() {
          self.current_pen = Some(pen);
        }
        if self.fonts.contains_key(&object_id) {
          self.current_font = Some(object_id);
        }
      }
    }
  }
}

fn decode_vector_emf_as_png(
  data: &[u8],
  options: RenderOptions,
) -> Result<DecodedMetafile, String> {
  let mut state = EmfVectorState::new_with_options(data, options)?;
  let mut pos = emf_header_size(data);

  while pos + EMF_RECORD_HEADER_SIZE <= data.len() {
    let record_type = read_u32(data, pos)?;
    let record_size = read_u32(data, pos + 4)? as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || pos + record_size > data.len() {
      return Err(format!(
        "invalid EMF record at offset {pos}: type=0x{record_type:08x} size={record_size}"
      ));
    }

    match record_type {
      EMR_SET_WINDOW_ORG_EX if record_size >= 16 => {
        state.window_org_x = read_i32(data, pos + 8)?;
        state.window_org_y = read_i32(data, pos + 12)?;
      }
      EMR_SET_WINDOW_EXT_EX if record_size >= 16 => {
        state.window_ext_x = read_i32(data, pos + 8)?;
        state.window_ext_y = read_i32(data, pos + 12)?;
      }
      EMR_SET_MAP_MODE if record_size >= 12 => {
        state.map_mode = read_u32(data, pos + 8)?;
      }
      EMR_SET_VIEWPORT_ORG_EX if record_size >= 16 => {
        state.viewport_org_x = read_i32(data, pos + 8)?;
        state.viewport_org_y = read_i32(data, pos + 12)?;
      }
      EMR_SET_VIEWPORT_EXT_EX if record_size >= 16 => {
        state.viewport_ext_x = read_i32(data, pos + 8)?;
        state.viewport_ext_y = read_i32(data, pos + 12)?;
      }
      EMR_SET_PIXEL_V if record_size >= 20 => {
        let (x, y) = state.map_point(EmfPoint {
          x: read_i32(data, pos + 8)?,
          y: read_i32(data, pos + 12)?,
        });
        state.set_pixel(
          x.round() as i32,
          y.round() as i32,
          read_color_ref(data, pos + 16)?,
        );
      }
      EMR_MOVE_TO_EX if record_size >= 16 => {
        state.current_pos = EmfPoint {
          x: read_i32(data, pos + 8)?,
          y: read_i32(data, pos + 12)?,
        };
      }
      EMR_LINE_TO if record_size >= 16 => {
        let next = EmfPoint {
          x: read_i32(data, pos + 8)?,
          y: read_i32(data, pos + 12)?,
        };
        state.draw_polyline(&[state.current_pos, next], false);
        state.current_pos = next;
      }
      EMR_SET_TEXT_COLOR if record_size >= 12 => {
        state.text_color = read_color_ref(data, pos + 8)?;
      }
      EMR_SAVE_DC => state.save_state(),
      EMR_RESTORE_DC => state.restore_state(),
      EMR_SET_WORLD_TRANSFORM if record_size >= 32 => {
        state.world_transform = read_xform(data, pos + 8)?;
      }
      EMR_MODIFY_WORLD_TRANSFORM if record_size >= 36 => {
        let transform = read_xform(data, pos + 8)?;
        let mode = read_u32(data, pos + 32)?;
        state.world_transform = match mode {
          MWT_IDENTITY => EmfTransform::identity(),
          MWT_LEFTMULTIPLY => transform.multiply(state.world_transform),
          MWT_RIGHTMULTIPLY => state.world_transform.multiply(transform),
          MWT_SET => transform,
          _ => state.world_transform,
        };
      }
      EMR_CREATE_PEN if record_size >= 28 => {
        let object_id = read_u32(data, pos + 8)?;
        if object_id & ENHMETA_STOCK_OBJECT == 0 {
          let width = read_i32(data, pos + 16)?.unsigned_abs().max(1) as usize;
          state.pens.insert(
            object_id,
            EmfPen {
              color: read_color_ref(data, pos + 24)?,
              width,
            },
          );
        }
      }
      EMR_CREATE_BRUSH_INDIRECT if record_size >= 24 => {
        let object_id = read_u32(data, pos + 8)?;
        state
          .brush_colors
          .insert(object_id, read_color_ref(data, pos + 16)?);
      }
      EMR_EXT_CREATE_PEN if record_size >= 56 => {
        let object_id = read_u32(data, pos + 8)?;
        if object_id & ENHMETA_STOCK_OBJECT == 0 {
          let width = read_u32(data, pos + 32)?.max(1) as usize;
          state.pens.insert(
            object_id,
            EmfPen {
              color: read_color_ref(data, pos + 40)?,
              width,
            },
          );
        }
      }
      EMR_EXT_CREATE_FONT_INDIRECT_W if record_size >= 104 => {
        if let Some((object_id, font)) = read_logfont_object(data, pos, record_size)
          && object_id & ENHMETA_STOCK_OBJECT == 0
        {
          state.fonts.insert(object_id, font);
        }
      }
      EMR_SELECT_OBJECT if record_size >= 12 => {
        state.select_object(read_u32(data, pos + 8)?);
      }
      EMR_DELETE_OBJECT if record_size >= 12 => {
        let object_id = read_u32(data, pos + 8)?;
        state.brush_colors.remove(&object_id);
        state.pens.remove(&object_id);
        state.fonts.remove(&object_id);
        if state.current_font == Some(object_id) {
          state.current_font = None;
        }
      }
      EMR_POLYGON if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_POLYBEZIER if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&flatten_bezier_sequence(&points), false);
        }
      }
      EMR_POLYBEZIER_TO if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&flatten_bezier_sequence(&sequence), false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYGON16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_POLYBEZIER16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&flatten_bezier_sequence(&points), false);
        }
      }
      EMR_POLYBEZIER_TO16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&flatten_bezier_sequence(&sequence), false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYLINE if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYLINE_TO if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&sequence, false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYLINE16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYLINE_TO16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&sequence, false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYPOLYLINE if record_size >= 36 => {
        for points in read_poly_polygons_i32(data, pos, record_size)? {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYPOLYGON if record_size >= 36 => {
        for points in read_poly_polygons_i32(data, pos, record_size)? {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_POLYPOLYLINE16 if record_size >= 36 => {
        for points in read_poly_polygons_i16(data, pos, record_size)? {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYPOLYGON16 if record_size >= 36 => {
        for points in read_poly_polygons_i16(data, pos, record_size)? {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_RECTANGLE if record_size >= 24 => {
        state.fill_rect(
          read_i32(data, pos + 8)?,
          read_i32(data, pos + 12)?,
          read_i32(data, pos + 16)?,
          read_i32(data, pos + 20)?,
        );
      }
      EMR_ROUND_RECT if record_size >= 32 => {
        state.fill_rect(
          read_i32(data, pos + 8)?,
          read_i32(data, pos + 12)?,
          read_i32(data, pos + 16)?,
          read_i32(data, pos + 20)?,
        );
      }
      EMR_ELLIPSE if record_size >= 24 => {
        state.fill_ellipse(
          read_i32(data, pos + 8)?,
          read_i32(data, pos + 12)?,
          read_i32(data, pos + 16)?,
          read_i32(data, pos + 20)?,
        );
      }
      EMR_ARC if record_size >= 40 => {
        let rect = emf_arc_rect(data, pos)?;
        state.fill_arc_segment(
          rect,
          angle_from_emf_arc_point(rect, read_i32(data, pos + 24)?, read_i32(data, pos + 28)?),
          sweep_from_emf_arc_points(data, pos, rect)?,
          false,
        );
      }
      EMR_CHORD | EMR_PIE if record_size >= 40 => {
        let rect = emf_arc_rect(data, pos)?;
        state.fill_arc_segment(
          rect,
          angle_from_emf_arc_point(rect, read_i32(data, pos + 24)?, read_i32(data, pos + 28)?),
          sweep_from_emf_arc_points(data, pos, rect)?,
          true,
        );
      }
      EMR_SET_TEXT_ALIGN if record_size >= 12 => {
        state.text_align = read_u32(data, pos + 8)? as u16;
      }
      EMR_EXT_TEXTOUT_W | EMR_EXT_TEXTOUT_A => {
        // StrictLib patch: the selected LOGFONT, the dx array, Symbol codes
        // and TA_UPDATECP.
        let font = emf_current_font(&state);
        let text = if record_type == EMR_EXT_TEXTOUT_W {
          extract_emr_ext_text_out_w(data, pos, record_size).map(|text| font.map_unicode(&text))
        } else {
          ext_text_record(data, pos, record_size).and_then(|record| {
            let start = pos.checked_add(record.string_offset)?;
            let bytes = data.get(start..start.checked_add(record.characters)?)?;
            Some(font.decode_bytes(bytes))
          })
        };
        if let Some(text) = text
          && let Some(text_record) = ext_text_record(data, pos, record_size)
        {
          let dx = emf_text_dx(data, pos, record_size, text_record.characters);
          let align = state.text_align;
          let (x, y) = if align & TA_UPDATECP != 0 {
            (state.current_pos.x, state.current_pos.y)
          } else {
            (text_record.x, text_record.y)
          };
          let color = state.text_color;
          let advance = state.draw_text(x, y, &text, color, &font, align, &dx);
          if align & TA_UPDATECP != 0 {
            state.current_pos.x += advance;
          }
        }
      }
      EMR_SET_DIBITS_TO_DEVICE | EMR_STRETCH_DIBITS => {
        if let Some(target) = emf_bitmap_draw_target(data, pos, record_type, record_size)? {
          let raster = decode_bitmap_record_as_raster(data, record_type, pos, record_size)?;
          if let Some(image) = decoded_raster_to_rgb(&raster)? {
            if let Some(rop) = target.raster_operation {
              state.draw_rgb_image_with_rop(
                target.dest_x,
                target.dest_y,
                target.dest_width,
                target.dest_height,
                &image,
                rop,
              );
            } else {
              state.draw_rgb_image(
                target.dest_x,
                target.dest_y,
                target.dest_width,
                target.dest_height,
                &image,
              );
            }
          }
        }
      }
      EMR_COMMENT if record_size >= 16 => {
        process_emf_plus_comment(data, pos, record_size, &mut state)?;
      }
      EMR_EOF => break,
      _ => {}
    }

    pos += record_size;
  }

  Ok(DecodedMetafile {
    data: rgb_to_png(&state.rgb, state.width as u32, state.height as u32)?,
    content_type: "image/png",
  })
}

#[derive(Clone, Debug)]
struct RasterPixels {
  width: usize,
  height: usize,
  rgb: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
struct EmfBitmapDrawTarget {
  dest_x: i32,
  dest_y: i32,
  dest_width: i32,
  dest_height: i32,
  raster_operation: Option<WmfTernaryRasterOperationCode>,
}

#[derive(Clone, Copy, Debug)]
struct WmfSavedState {
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  current_brush: Option<EmfColor>,
  current_pen: Option<EmfPen>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  current_font: TextFont,
}

#[derive(Clone, Copy, Debug)]
enum WmfRenderObject {
  Pen(Option<EmfPen>),
  Brush(Option<EmfColor>),
  Font(TextFont),
  Unsupported,
}

struct WmfRenderState {
  canvas: EmfVectorState,
  objects: Vec<Option<WmfRenderObject>>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  current_font: TextFont,
  saved: Vec<WmfSavedState>,
}

impl WmfRenderState {
  fn new(metafile: &WmfMetafileRef<'_>, options: RenderOptions) -> Result<Self, String> {
    let (window_org_x, window_org_y, window_ext_x, window_ext_y) = wmf_initial_window(metafile);
    let natural_width = window_ext_x.unsigned_abs().max(1) as usize;
    let natural_height = window_ext_y.unsigned_abs().max(1) as usize;
    let (width, height) = options.resolved_canvas_size(natural_width, natural_height);
    let object_count = metafile.header.number_of_objects as usize;

    Ok(Self {
      canvas: EmfVectorState {
        width,
        height,
        window_org_x,
        window_org_y,
        window_ext_x: window_ext_x.abs().max(1),
        window_ext_y: window_ext_y.abs().max(1),
        viewport_org_x: 0,
        viewport_org_y: 0,
        viewport_ext_x: width as i32,
        viewport_ext_y: height as i32,
        // A WMF has no reference device: the window maps straight onto the
        // canvas.
        map_mode: MM_ANISOTROPIC,
        device_px_per_mm: (96.0 / 25.4, 96.0 / 25.4),
        bounds_origin: (0, 0),
        device_to_canvas: (1.0, 1.0),
        world_transform: EmfTransform::identity(),
        brush_colors: std::collections::HashMap::new(),
        pens: std::collections::HashMap::new(),
        fonts: std::collections::HashMap::new(),
        current_brush: Some(EmfColor {
          r: 255,
          g: 255,
          b: 255,
        }),
        current_pen: Some(EmfPen {
          color: EmfColor { r: 0, g: 0, b: 0 },
          width: 1,
        }),
        current_font: None,
        current_pos: EmfPoint { x: 0, y: 0 },
        text_color: EmfColor { r: 0, g: 0, b: 0 },
        clip_rect: None,
        clip_mask: None,
        saved_states: Vec::new(),
        emf_plus_objects: Vec::new(),
        emf_plus_object_assembler: EmfPlusObjectAssembler::default(),
        font_cache: RenderFontCache::load(),
        text_align: 0,
        rgb: vec![255; width * height * RGB_BYTES_PER_PIXEL],
      },
      objects: vec![None; object_count],
      current_pos: EmfPoint { x: 0, y: 0 },
      text_color: EmfColor { r: 0, g: 0, b: 0 },
      current_font: TextFont::with_height(12),
      saved: Vec::new(),
    })
  }

  /// StrictLib patch: draws a WMF text record in the selected font. With
  /// `TA_UPDATECP` the run starts at the current position (set by `MoveTo`,
  /// which is how MathType places every run) and moves it; the record's own
  /// point is ignored.
  fn draw_text_run(&mut self, x: i32, y: i32, bytes: &[u8], dx: &[i32]) {
    let font = self.current_font;
    let text = font.decode_bytes(bytes);
    let align = self.canvas.text_align;
    let update_cp = align & TA_UPDATECP != 0;
    let (x, y) = if update_cp {
      (self.current_pos.x, self.current_pos.y)
    } else {
      (x, y)
    };
    let dx = &dx[..dx.len().min(text.chars().count())];
    let advance = self.canvas.draw_text(x, y, &text, self.text_color, &font, align, dx);
    if update_cp {
      self.current_pos.x += advance;
    }
  }

  fn insert_object(&mut self, object: WmfRenderObject) {
    if let Some(slot) = self.objects.iter_mut().find(|slot| slot.is_none()) {
      *slot = Some(object);
    } else {
      self.objects.push(Some(object));
    }
  }

  fn save_dc(&mut self) {
    self.saved.push(WmfSavedState {
      window_org_x: self.canvas.window_org_x,
      window_org_y: self.canvas.window_org_y,
      window_ext_x: self.canvas.window_ext_x,
      window_ext_y: self.canvas.window_ext_y,
      viewport_org_x: self.canvas.viewport_org_x,
      viewport_org_y: self.canvas.viewport_org_y,
      viewport_ext_x: self.canvas.viewport_ext_x,
      viewport_ext_y: self.canvas.viewport_ext_y,
      current_brush: self.canvas.current_brush,
      current_pen: self.canvas.current_pen,
      current_pos: self.current_pos,
      text_color: self.text_color,
      current_font: self.current_font,
    });
  }

  fn restore_dc(&mut self) {
    let Some(saved) = self.saved.pop() else {
      return;
    };
    self.canvas.window_org_x = saved.window_org_x;
    self.canvas.window_org_y = saved.window_org_y;
    self.canvas.window_ext_x = saved.window_ext_x;
    self.canvas.window_ext_y = saved.window_ext_y;
    self.canvas.viewport_org_x = saved.viewport_org_x;
    self.canvas.viewport_org_y = saved.viewport_org_y;
    self.canvas.viewport_ext_x = saved.viewport_ext_x;
    self.canvas.viewport_ext_y = saved.viewport_ext_y;
    self.canvas.current_brush = saved.current_brush;
    self.canvas.current_pen = saved.current_pen;
    self.current_pos = saved.current_pos;
    self.text_color = saved.text_color;
    self.current_font = saved.current_font;
  }

  fn select_object(&mut self, index: u16) {
    let Some(Some(object)) = self.objects.get(index as usize).copied() else {
      return;
    };
    match object {
      WmfRenderObject::Pen(pen) => self.canvas.current_pen = pen,
      WmfRenderObject::Brush(brush) => self.canvas.current_brush = brush,
      WmfRenderObject::Font(font) => self.current_font = font,
      WmfRenderObject::Unsupported => {}
    }
  }

  fn delete_object(&mut self, index: u16) {
    if let Some(slot) = self.objects.get_mut(index as usize) {
      *slot = None;
    }
  }
}

fn decode_wmf_as_raster(
  data: &[u8],
  options: RenderOptions,
) -> Result<Option<DecodedMetafile>, String> {
  if !crate::wmf::looks_like_wmf(data) {
    return Ok(None);
  }

  let metafile = WmfMetafileRef::from_bytes(data).map_err(|err| err.to_string())?;
  let mut state = WmfRenderState::new(&metafile, options)?;

  for record in metafile.records() {
    let parsed = record.parse_data().map_err(|err| err.to_string())?;
    match parsed {
      WmfRecordData::Eof(_) => break,
      WmfRecordData::SaveDc => state.save_dc(),
      WmfRecordData::RestoreDc(_) => state.restore_dc(),
      WmfRecordData::SetWindowOrg(value) => {
        state.canvas.window_org_x = i32::from(value.x);
        state.canvas.window_org_y = i32::from(value.y);
      }
      WmfRecordData::SetWindowExt(value) => {
        state.canvas.window_ext_x = i32::from(value.x).abs().max(1);
        state.canvas.window_ext_y = i32::from(value.y).abs().max(1);
      }
      WmfRecordData::SetViewportOrg(value) => {
        state.canvas.viewport_org_x = i32::from(value.x);
        state.canvas.viewport_org_y = i32::from(value.y);
      }
      WmfRecordData::SetViewportExt(value) => {
        state.canvas.viewport_ext_x = i32::from(value.x).abs().max(1);
        state.canvas.viewport_ext_y = i32::from(value.y).abs().max(1);
      }
      WmfRecordData::IntersectClipRect(value) => {
        state.canvas.set_clip_rect_logical(
          i32::from(value.left),
          i32::from(value.top),
          i32::from(value.right),
          i32::from(value.bottom),
        );
      }
      WmfRecordData::ExcludeClipRect(_) => {}
      WmfRecordData::SetTextColor(value) => {
        state.text_color = color_ref_to_emf(value.color);
      }
      WmfRecordData::OffsetWindowOrg(value) => {
        state.canvas.window_org_x += i32::from(value.x);
        state.canvas.window_org_y += i32::from(value.y);
      }
      WmfRecordData::OffsetViewportOrg(value) => {
        state.canvas.viewport_org_x += i32::from(value.x);
        state.canvas.viewport_org_y += i32::from(value.y);
      }
      WmfRecordData::CreatePenIndirect(value) => {
        let line_style = WmfPenLineStyle::from_raw(value.pen.pen_line_style_raw());
        let pen = if line_style == Some(WmfPenLineStyle::Null) {
          None
        } else {
          Some(EmfPen {
            color: color_ref_to_emf(value.pen.color_ref),
            width: i32::from(value.pen.width.x).unsigned_abs().max(1) as usize,
          })
        };
        state.insert_object(WmfRenderObject::Pen(pen));
      }
      WmfRecordData::CreateBrushIndirect(value) => {
        let brush = match value.brush_style_kind() {
          Some(WmfBrushStyle::Null) => None,
          _ => Some(color_ref_to_emf(value.color_ref)),
        };
        state.insert_object(WmfRenderObject::Brush(brush));
      }
      WmfRecordData::CreateFontIndirect(value) => {
        // StrictLib patch: the whole LOGFONT, not only its height.
        let mut face = [0u16; LOGFONT_FACE_NAME_CHARS];
        for (unit, byte) in face.iter_mut().zip(value.face_name.iter().take_while(|byte| **byte != 0)) {
          *unit = u16::from(*byte);
        }
        state.insert_object(WmfRenderObject::Font(TextFont {
          height: i32::from(value.height),
          weight: u16::try_from(value.weight.clamp(0, 1000)).unwrap_or(400),
          italic: value.italic != 0,
          char_set: value.char_set,
          face,
        }));
      }
      WmfRecordData::SetTextAlign(value) => state.canvas.text_align = value.value,
      WmfRecordData::CreatePalette(_)
      | WmfRecordData::CreatePatternBrush(_)
      | WmfRecordData::CreateRegion(_)
      | WmfRecordData::DibCreatePatternBrush(_) => {
        state.insert_object(WmfRenderObject::Unsupported);
      }
      WmfRecordData::SelectObject(value) => state.select_object(value.index),
      WmfRecordData::DeleteObject(value) => state.delete_object(value.index),
      WmfRecordData::MoveTo(value) => {
        state.current_pos = EmfPoint {
          x: i32::from(value.x),
          y: i32::from(value.y),
        };
      }
      WmfRecordData::LineTo(value) => {
        let next = EmfPoint {
          x: i32::from(value.x),
          y: i32::from(value.y),
        };
        state
          .canvas
          .draw_polyline(&[state.current_pos, next], false);
        state.current_pos = next;
      }
      WmfRecordData::SetPixel(value) => {
        let (x, y) = state.canvas.map_point(EmfPoint {
          x: i32::from(value.x),
          y: i32::from(value.y),
        });
        state.canvas.set_pixel(
          x.round() as i32,
          y.round() as i32,
          color_ref_to_emf(value.color),
        );
      }
      WmfRecordData::Polygon(value) => {
        let points = value
          .points
          .iter()
          .map(|point| EmfPoint {
            x: i32::from(point.x),
            y: i32::from(point.y),
          })
          .collect::<Vec<_>>();
        state.canvas.fill_polygon(&points);
        state.canvas.draw_polyline(&points, true);
      }
      WmfRecordData::Polyline(value) => {
        let points = value
          .points
          .iter()
          .map(|point| EmfPoint {
            x: i32::from(point.x),
            y: i32::from(point.y),
          })
          .collect::<Vec<_>>();
        state.canvas.draw_polyline(&points, false);
      }
      WmfRecordData::PolyPolygon(value) => {
        let mut cursor = 0usize;
        for count in value.points_per_polygon {
          let end = cursor
            .saturating_add(count as usize)
            .min(value.points.len());
          let points = value.points[cursor..end]
            .iter()
            .map(|point| EmfPoint {
              x: i32::from(point.x),
              y: i32::from(point.y),
            })
            .collect::<Vec<_>>();
          state.canvas.fill_polygon(&points);
          state.canvas.draw_polyline(&points, true);
          cursor = end;
        }
      }
      WmfRecordData::Rectangle(value) => state.canvas.fill_rect(
        i32::from(value.left),
        i32::from(value.top),
        i32::from(value.right),
        i32::from(value.bottom),
      ),
      WmfRecordData::RoundRect(value) => state.canvas.fill_rect(
        i32::from(value.left),
        i32::from(value.top),
        i32::from(value.right),
        i32::from(value.bottom),
      ),
      WmfRecordData::Ellipse(value) => state.canvas.fill_ellipse(
        i32::from(value.left),
        i32::from(value.top),
        i32::from(value.right),
        i32::from(value.bottom),
      ),
      WmfRecordData::Arc(value) => state.canvas.fill_arc_segment(
        (
          i32::from(value.left),
          i32::from(value.top),
          i32::from(value.right),
          i32::from(value.bottom),
        ),
        angle_from_arc_point(value, value.x_radial_1, value.y_radial_1),
        sweep_from_arc_points(value),
        false,
      ),
      WmfRecordData::Chord(value) | WmfRecordData::Pie(value) => state.canvas.fill_arc_segment(
        (
          i32::from(value.left),
          i32::from(value.top),
          i32::from(value.right),
          i32::from(value.bottom),
        ),
        angle_from_arc_point(value, value.x_radial_1, value.y_radial_1),
        sweep_from_arc_points(value),
        true,
      ),
      WmfRecordData::TextOut(value) => {
        state.draw_text_run(
          i32::from(value.x_start),
          i32::from(value.y_start),
          &value.string,
          &[],
        );
      }
      WmfRecordData::ExtTextOut(value) => {
        let stride = if value.options.contains(crate::wmf::WmfExtTextOutOptions::PDY) { 2 } else { 1 };
        let dx = value.dx.iter().step_by(stride).map(|advance| i32::from(*advance)).collect::<Vec<_>>();
        state.draw_text_run(i32::from(value.x), i32::from(value.y), &value.string, &dx);
      }
      WmfRecordData::PatBlt(value) => {
        state.canvas.fill_rect_with_rop(
          i32::from(value.x_left),
          i32::from(value.y_left),
          i32::from(value.x_left) + i32::from(value.width),
          i32::from(value.y_left) + i32::from(value.height),
          value.raster_operation_code(),
        );
      }
      WmfRecordData::StretchDib(value) => {
        if let Some(color_usage) = value.color_usage_kind()
          && let Some(image) = packed_dib_to_rgb(&value.dib, color_usage)?
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.dest_width),
            i32::from(value.dest_height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::SetDibToDev(value) => {
        if let Some(color_usage) = value.color_usage_kind()
          && let Some(image) = packed_dib_to_rgb(&value.dib, color_usage)?
        {
          state.canvas.draw_rgb_image(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.width),
            i32::from(value.height),
            &image,
          );
        }
      }
      WmfRecordData::DibBitBlt(value) => {
        if let Some(bytes) = value.target.source_bytes()
          && let Some(image) = packed_dib_to_rgb(bytes, DibColorUsage::RgbColors)?
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.width),
            i32::from(value.height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::DibStretchBlt(value) => {
        if let Some(bytes) = value.target.source_bytes()
          && let Some(image) = packed_dib_to_rgb(bytes, DibColorUsage::RgbColors)?
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.dest_width),
            i32::from(value.dest_height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::BitBlt(value) => {
        if let Some(bytes) = value.target.source_bytes()
          && let Ok(Some(image)) = bitmap16_to_rgb(bytes)
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.width),
            i32::from(value.height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::StretchBlt(value) => {
        if let Some(bytes) = value.target.source_bytes()
          && let Ok(Some(image)) = bitmap16_to_rgb(bytes)
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.dest_width),
            i32::from(value.dest_height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::Escape(value) => {
        if let Ok(WmfEscapeData::EnhancedMetafile {
          enhanced_metafile_data,
          ..
        }) = value.typed_data()
          && let Some(raster) = decode_emf_as_raster(enhanced_metafile_data, options)?
          && let Some(image) = decoded_raster_to_rgb(&raster)?
        {
          state.canvas.draw_rgb_image(
            state.canvas.window_org_x,
            state.canvas.window_org_y,
            state.canvas.window_ext_x,
            state.canvas.window_ext_y,
            &image,
          );
        }
      }
      _ => {}
    }
  }

  Ok(Some(DecodedMetafile {
    data: rgb_to_png(
      &state.canvas.rgb,
      state.canvas.width as u32,
      state.canvas.height as u32,
    )?,
    content_type: "image/png",
  }))
}

fn wmf_initial_window(metafile: &WmfMetafileRef<'_>) -> (i32, i32, i32, i32) {
  if let Some(placeable) = &metafile.placeable_header {
    return (
      i32::from(placeable.left),
      i32::from(placeable.top),
      placeable.bounding_box_width().abs().max(1),
      placeable.bounding_box_height().abs().max(1),
    );
  }

  let mut org_x = 0;
  let mut org_y = 0;
  let mut ext_x = DEFAULT_RENDER_WIDTH as i32;
  let mut ext_y = DEFAULT_RENDER_HEIGHT as i32;
  for record in metafile.records() {
    match record.parse_data() {
      Ok(WmfRecordData::SetWindowOrg(value)) => {
        org_x = i32::from(value.x);
        org_y = i32::from(value.y);
      }
      Ok(WmfRecordData::SetWindowExt(value)) => {
        ext_x = i32::from(value.x).abs().max(1);
        ext_y = i32::from(value.y).abs().max(1);
        break;
      }
      Ok(WmfRecordData::Eof(_)) => break,
      _ => {}
    }
  }
  (org_x, org_y, ext_x, ext_y)
}

fn emf_bitmap_draw_target(
  data: &[u8],
  record_offset: usize,
  record_type: u32,
  record_size: usize,
) -> Result<Option<EmfBitmapDrawTarget>, String> {
  let min_size = match record_type {
    EMR_SET_DIBITS_TO_DEVICE => EMR_BITMAP_BITS_SIZE_OFFSET + 4,
    EMR_STRETCH_DIBITS => EMR_STRETCH_DIBITS_DEST_HEIGHT_OFFSET + 4,
    _ => return Ok(None),
  };
  if record_size < min_size {
    return Ok(None);
  }

  let dest_x = read_i32(data, record_offset + EMR_BITMAP_DEST_X_OFFSET)?;
  let dest_y = read_i32(data, record_offset + EMR_BITMAP_DEST_Y_OFFSET)?;
  let (dest_width, dest_height, raster_operation) = match record_type {
    EMR_SET_DIBITS_TO_DEVICE => (
      read_i32(data, record_offset + EMR_BITMAP_SOURCE_WIDTH_OFFSET)?,
      read_i32(data, record_offset + EMR_BITMAP_SOURCE_HEIGHT_OFFSET)?,
      None,
    ),
    EMR_STRETCH_DIBITS => (
      read_i32(data, record_offset + EMR_STRETCH_DIBITS_DEST_WIDTH_OFFSET)?,
      read_i32(data, record_offset + EMR_STRETCH_DIBITS_DEST_HEIGHT_OFFSET)?,
      Some(emf_ternary_raster_operation(read_u32(
        data,
        record_offset + EMR_STRETCH_DIBITS_ROP_OFFSET,
      )?)),
    ),
    _ => unreachable!(),
  };
  if dest_width == 0 || dest_height == 0 {
    return Ok(None);
  }

  Ok(Some(EmfBitmapDrawTarget {
    dest_x,
    dest_y,
    dest_width,
    dest_height,
    raster_operation,
  }))
}

fn emf_ternary_raster_operation(raw: u32) -> WmfTernaryRasterOperationCode {
  WmfTernaryRasterOperationCode::from_raw(((raw >> 16) & 0xff) as u8)
}

fn decode_bitmap_record_as_raster(
  data: &[u8],
  record_type: u32,
  record_offset: usize,
  _record_size: usize,
) -> Result<DecodedMetafile, String> {
  let (off_bmi_src, cb_bmi_src, off_bits_src, cb_bits_src) = match record_type {
    EMR_STRETCH_DIBITS => (
      read_u32(data, record_offset + EMR_BITMAP_INFO_OFFSET_OFFSET)? as usize,
      read_u32(data, record_offset + EMR_BITMAP_INFO_SIZE_OFFSET)? as usize,
      read_u32(data, record_offset + EMR_BITMAP_BITS_OFFSET_OFFSET)? as usize,
      read_u32(data, record_offset + EMR_BITMAP_BITS_SIZE_OFFSET)? as usize,
    ),
    EMR_SET_DIBITS_TO_DEVICE => (
      read_u32(data, record_offset + EMR_BITMAP_INFO_OFFSET_OFFSET)? as usize,
      read_u32(data, record_offset + EMR_BITMAP_INFO_SIZE_OFFSET)? as usize,
      read_u32(data, record_offset + EMR_BITMAP_BITS_OFFSET_OFFSET)? as usize,
      read_u32(data, record_offset + EMR_BITMAP_BITS_SIZE_OFFSET)? as usize,
    ),
    _ => {
      return Err(format!(
        "unsupported EMF bitmap record type 0x{record_type:08x}"
      ));
    }
  };

  let bmi_start = record_offset + off_bmi_src;
  let bits_start = record_offset + off_bits_src;
  let bmi_end = bmi_start
    .checked_add(cb_bmi_src)
    .ok_or_else(|| "bitmap info range overflows".to_string())?;
  let bits_end = bits_start
    .checked_add(cb_bits_src)
    .ok_or_else(|| "bitmap bits range overflows".to_string())?;
  if bmi_end > data.len() || bits_end > data.len() {
    return Err("EMF bitmap record points outside the file".into());
  }
  if cb_bmi_src < BITMAPINFOHEADER_SIZE {
    return Err("EMF bitmap info header is too small".into());
  }

  let header_size = read_u32(data, bmi_start)? as usize;
  if header_size < BITMAPINFOHEADER_SIZE {
    return Err(format!("unsupported BITMAPINFOHEADER size: {header_size}"));
  }

  let width = read_i32(data, bmi_start + BITMAP_WIDTH_OFFSET)?;
  let height = read_i32(data, bmi_start + BITMAP_HEIGHT_OFFSET)?;
  let planes = read_u16(data, bmi_start + BITMAP_PLANES_OFFSET)?;
  let bit_count = read_u16(data, bmi_start + BITMAP_BIT_COUNT_OFFSET)?;
  let compression = read_u32(data, bmi_start + BITMAP_COMPRESSION_OFFSET)?;

  if planes != DIB_PLANES {
    return Err(format!("unsupported DIB planes value: {planes}"));
  }

  let bits = &data[bits_start..bits_end];
  match compression {
    BI_JPEG => Ok(DecodedMetafile {
      data: bits.to_vec(),
      content_type: "image/jpeg",
    }),
    BI_PNG => Ok(DecodedMetafile {
      data: bits.to_vec(),
      content_type: "image/png",
    }),
    BI_RGB => dib_to_png(bits, width, height, bit_count),
    other => Err(format!("unsupported DIB compression: {other}")),
  }
}

fn dib_to_png(
  bits: &[u8],
  width: i32,
  height: i32,
  bit_count: u16,
) -> Result<DecodedMetafile, String> {
  if width <= 0 || height == 0 {
    return Err(format!("unsupported DIB size {width}x{height}"));
  }

  let width = width as usize;
  let top_down = height < 0;
  let height_abs = height.unsigned_abs() as usize;

  let bytes_per_pixel = match bit_count {
    DIB_BIT_COUNT_24 => RGB_BYTES_PER_PIXEL,
    DIB_BIT_COUNT_32 => BGRA_BYTES_PER_PIXEL,
    other => return Err(format!("unsupported BI_RGB bit depth: {other}")),
  };
  let row_stride = (width * bytes_per_pixel).next_multiple_of(DIB_ROW_ALIGNMENT_BYTES);
  let required_size = row_stride
    .checked_mul(height_abs)
    .ok_or_else(|| "bitmap dimensions overflow".to_string())?;
  if bits.len() < required_size {
    return Err(format!(
      "bitmap payload is truncated: need {required_size} bytes, got {}",
      bits.len()
    ));
  }

  let mut rgb = vec![0u8; width * height_abs * RGB_BYTES_PER_PIXEL];
  for row in 0..height_abs {
    let src_row = if top_down { row } else { height_abs - 1 - row };
    let src_offset = src_row * row_stride;
    let dest_offset = row * width * RGB_BYTES_PER_PIXEL;
    let src = &bits[src_offset..src_offset + row_stride];
    let dest = &mut rgb[dest_offset..dest_offset + width * RGB_BYTES_PER_PIXEL];

    match bit_count {
      DIB_BIT_COUNT_24 => {
        for col in 0..width {
          let src_pixel =
            &src[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      DIB_BIT_COUNT_32 => {
        for col in 0..width {
          let src_pixel =
            &src[col * BGRA_BYTES_PER_PIXEL..col * BGRA_BYTES_PER_PIXEL + BGRA_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      _ => unreachable!(),
    }
  }

  Ok(DecodedMetafile {
    data: rgb_to_png(&rgb, width as u32, height_abs as u32)?,
    content_type: "image/png",
  })
}

fn process_emf_plus_comment(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
  state: &mut EmfVectorState,
) -> Result<(), String> {
  // EMR_COMMENT_EMFPLUS chunks as a stream of 12-byte EMF+ record headers.
  let data_size = read_u32(data, record_offset + 8)? as usize;
  let comment_identifier = read_u32(data, record_offset + 12)?;
  if comment_identifier != EMR_COMMENT_EMFPLUS || data_size < 4 {
    return Ok(());
  }
  let mut cursor = record_offset + 16;
  let end = record_offset
    .checked_add(12)
    .and_then(|offset| offset.checked_add(data_size))
    .map(|end| end.min(record_offset + record_size))
    .ok_or_else(|| "EMF+ comment range overflows".to_string())?;
  while cursor + 12 <= end {
    let size = read_u32(data, cursor + 4)? as usize;
    if size < 12 || cursor + size > end {
      break;
    }
    let record_bytes = &data[cursor..cursor + size];
    let mut reader = Reader::new(std::io::Cursor::new(record_bytes));
    if let Ok(record) = EmfPlusRecord::read_from(&mut reader, record_bytes.len() as u64) {
      if record.record_kind() == Some(EmfPlusRecordType::Object) {
        if let Ok(fragment) = record.into_object_fragment() {
          process_emf_plus_object(fragment, state)?;
        }
      } else if let Ok(parsed) = record.parse_data() {
        process_emf_plus_record(parsed, state)?;
      }
    }
    cursor += size;
  }
  Ok(())
}

fn process_emf_plus_record(
  record: EmfPlusRecordData<'_>,
  state: &mut EmfVectorState,
) -> Result<(), String> {
  match record {
    EmfPlusRecordData::Object(value) => process_emf_plus_object(value, state)?,
    EmfPlusRecordData::Clear(value) => {
      let color = emf_plus_argb_to_color(value.color);
      for y in 0..state.height {
        for x in 0..state.width {
          state.set_pixel(x as i32, y as i32, color);
        }
      }
    }
    EmfPlusRecordData::FillRects(value) => {
      let Some(brush) = emf_plus_brush_ref(value.brush, state) else {
        return Ok(());
      };
      for rect in value.rects {
        let (left, top, right, bottom) = emf_plus_rect_bounds(rect);
        state.fill_polygon_with_emf_plus_brush(&rect_points(left, top, right, bottom), &brush);
      }
    }
    EmfPlusRecordData::DrawRects(value) => {
      let old = state.current_pen;
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        state.current_pen = Some(pen);
        for rect in value.rects {
          let (left, top, right, bottom) = emf_plus_rect_bounds(rect);
          state.fill_rect(left, top, right, bottom);
        }
      }
      state.current_pen = old;
    }
    EmfPlusRecordData::FillPolygon(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
        let points = emf_plus_points_to_emf_points(&value.points);
        state.fill_polygon_with_emf_plus_brush(&points, &brush);
      }
    }
    EmfPlusRecordData::DrawLines(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        let points = emf_plus_points_to_emf_points(&value.points);
        state.draw_polyline(&points, value.close_shape);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::FillEllipse(value) => fill_emf_plus_rect_shape(value, state),
    EmfPlusRecordData::DrawEllipse(value) => draw_emf_plus_rect_shape(value, state),
    EmfPlusRecordData::FillPie(value) => fill_emf_plus_pie(value, state),
    EmfPlusRecordData::DrawPie(value) | EmfPlusRecordData::DrawArc(value) => {
      draw_emf_plus_arc(value, state)
    }
    EmfPlusRecordData::FillRegion(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state)
        && let Some(points) = emf_plus_region_points(value.object_id, state)
      {
        state.fill_polygon_with_emf_plus_brush(&points, &brush);
      }
    }
    EmfPlusRecordData::FillPath(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state)
        && let Some(points) = emf_plus_path_points(value.object_id, state)
      {
        state.fill_polygon_with_emf_plus_brush(&points, &brush);
      }
    }
    EmfPlusRecordData::DrawPath(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state)
        && let Some(points) = emf_plus_path_points(value.object_id, state)
      {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        state.draw_polyline(&points, true);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::FillClosedCurve(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
        let points = emf_plus_points_to_emf_points(&value.points);
        let points = flatten_cardinal_curve(&points, value.tension, true);
        state.fill_polygon_with_emf_plus_brush(&points, &brush);
        state.draw_polyline(&points, true);
      }
    }
    EmfPlusRecordData::DrawClosedCurve(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        let points = emf_plus_points_to_emf_points(&value.points);
        let points = flatten_cardinal_curve(&points, value.tension, true);
        state.draw_polyline(&points, true);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::DrawCurve(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        let points = emf_plus_points_to_emf_points(&value.points);
        let start = value.offset as usize;
        let end = start
          .saturating_add(value.num_segments as usize + 1)
          .min(points.len());
        let points = if start < end {
          flatten_cardinal_curve(&points[start..end], value.tension, false)
        } else {
          points
        };
        state.draw_polyline(&points, false);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::StrokeFillPath => {
      let paths = state
        .emf_plus_objects
        .iter()
        .filter_map(|object| match object {
          Some(EmfPlusRenderObject::Path(points)) => Some(points.clone()),
          _ => None,
        })
        .collect::<Vec<_>>();
      for points in paths {
        state.fill_polygon(&points);
        state.draw_polyline(&points, true);
      }
    }
    EmfPlusRecordData::DrawBeziers(value) => draw_emf_plus_beziers(value, state),
    EmfPlusRecordData::DrawImage(value) => draw_emf_plus_image(value, state),
    EmfPlusRecordData::DrawImagePoints(value) => draw_emf_plus_image_points(value, state),
    EmfPlusRecordData::DrawString(value) => draw_emf_plus_string(value, state),
    EmfPlusRecordData::DrawDriverString(value) => {
      if let Some(color) = emf_plus_brush_ref_to_color(value.brush, state)
        && let Some(first) = value.glyph_positions.first()
      {
        let text = value
          .glyphs
          .iter()
          .filter_map(|glyph| char::from_u32(u32::from(*glyph)))
          .collect::<String>();
        state.draw_text(
          first.x as i32,
          first.y as i32,
          &text,
          color,
          &TextFont::with_height(-12),
          TA_BASELINE,
          &[],
        );
      }
    }
    EmfPlusRecordData::Save(_)
    | EmfPlusRecordData::BeginContainer(_)
    | EmfPlusRecordData::BeginContainerNoParams(_) => state.save_state(),
    EmfPlusRecordData::Restore(_) | EmfPlusRecordData::EndContainer(_) => state.restore_state(),
    EmfPlusRecordData::SetClipRect(value) => {
      let (left, top, right, bottom) = emf_plus_rectf_bounds(value.clip_rect);
      let points = [
        EmfPoint { x: left, y: top },
        EmfPoint { x: right, y: top },
        EmfPoint {
          x: right,
          y: bottom,
        },
        EmfPoint { x: left, y: bottom },
      ];
      state.set_clip_points_logical(&points, value.combine_mode);
    }
    EmfPlusRecordData::SetClipPath(value) => {
      if let Some(points) = emf_plus_path_points(value.object_id, state) {
        state.set_clip_points_logical(&points, value.combine_mode);
      }
    }
    EmfPlusRecordData::SetClipRegion(value) => {
      if let Some(points) = emf_plus_region_points(value.object_id, state) {
        state.set_clip_points_logical(&points, value.combine_mode);
      }
    }
    EmfPlusRecordData::OffsetClip(value) => {
      state.offset_clip(value.dx, value.dy);
    }
    EmfPlusRecordData::ResetClip => {
      state.clip_rect = None;
      state.clip_mask = None;
    }
    EmfPlusRecordData::SetWorldTransform(value) => {
      state.world_transform = xform_to_transform(value);
    }
    EmfPlusRecordData::ResetWorldTransform => state.world_transform = EmfTransform::identity(),
    EmfPlusRecordData::MultiplyWorldTransform(value) => {
      multiply_emf_plus_transform(xform_to_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::TranslateWorldTransform(value) => {
      multiply_emf_plus_transform(translate_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::ScaleWorldTransform(value) => {
      multiply_emf_plus_transform(scale_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::RotateWorldTransform(value) => {
      multiply_emf_plus_transform(rotate_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::SetPageTransform(value)
      if value.page_scale.is_finite() && value.page_scale > 0.0 =>
    {
      multiply_emf_plus_transform(
        EmfTransform {
          m11: value.page_scale,
          m22: value.page_scale,
          ..EmfTransform::identity()
        },
        true,
        state,
      );
    }
    EmfPlusRecordData::SetTsGraphics(value) => {
      state.world_transform = xform_to_transform(value.world_to_device);
    }
    _ => {}
  }
  Ok(())
}

fn process_emf_plus_object(
  value: EmfPlusObjectRecordData,
  state: &mut EmfVectorState,
) -> Result<(), String> {
  match state.emf_plus_object_assembler.push(value) {
    Ok(Some(complete)) => process_complete_emf_plus_object(complete, state),
    Ok(None) => {}
    Err(_) => {
      state.emf_plus_object_assembler = EmfPlusObjectAssembler::default();
    }
  }
  Ok(())
}

fn process_complete_emf_plus_object(value: EmfPlusObjectRecordData, state: &mut EmfVectorState) {
  let object = match value.parse_object_data() {
    Ok(EmfPlusObjectData::Brush(brush)) => {
      EmfPlusRenderObject::Brush(emf_plus_brush_object(&brush))
    }
    Ok(EmfPlusObjectData::Pen(pen)) => EmfPlusRenderObject::Pen(emf_plus_pen_object(&pen)),
    Ok(EmfPlusObjectData::Path(path)) => {
      EmfPlusRenderObject::Path(emf_plus_path_object_points(&path))
    }
    Ok(EmfPlusObjectData::Region(region)) => {
      EmfPlusRenderObject::Region(emf_plus_region_object_points(&region))
    }
    Ok(EmfPlusObjectData::Image(image)) => match emf_plus_image_object_to_rgb(&image) {
      Ok(Some(image)) => EmfPlusRenderObject::Image(image),
      _ => EmfPlusRenderObject::Unsupported,
    },
    Ok(EmfPlusObjectData::Font(font)) => EmfPlusRenderObject::Font(font),
    _ => EmfPlusRenderObject::Unsupported,
  };
  let index = value.object_id as usize;
  if state.emf_plus_objects.len() <= index {
    state.emf_plus_objects.resize(index + 1, None);
  }
  state.emf_plus_objects[index] = Some(object);
}

fn emf_plus_brush_object(brush: &crate::emfplus::EmfPlusBrushObject) -> Option<EmfPlusRenderBrush> {
  match brush.parse_brush_data().ok()? {
    EmfPlusBrushData::Solid(value) => Some(EmfPlusRenderBrush::Solid(emf_plus_argb_to_color(
      value.solid_color,
    ))),
    EmfPlusBrushData::Hatch(value) => Some(EmfPlusRenderBrush::Hatch {
      fore: emf_plus_argb_to_color(value.fore_color),
      back: emf_plus_argb_to_color(value.back_color),
      style: value.hatch_style,
    }),
    EmfPlusBrushData::LinearGradient(value) => Some(EmfPlusRenderBrush::LinearGradient {
      rect: (
        value.rect.x,
        value.rect.y,
        value.rect.x + value.rect.width,
        value.rect.y + value.rect.height,
      ),
      start: emf_plus_argb_to_color(value.start_color),
      end: emf_plus_argb_to_color(value.end_color),
    }),
    EmfPlusBrushData::PathGradient(value) => Some(EmfPlusRenderBrush::PathGradient {
      center: (value.center_point.x, value.center_point.y),
      center_color: emf_plus_argb_to_color(value.center_color),
      surround: value
        .surrounding_colors
        .first()
        .copied()
        .map(emf_plus_argb_to_color)
        .unwrap_or_else(|| emf_plus_argb_to_color(value.center_color)),
    }),
    EmfPlusBrushData::Texture(value) => value
      .parse_optional_data()
      .ok()?
      .image_object
      .as_ref()
      .and_then(|image| emf_plus_image_object_to_rgb(image).ok().flatten())
      .map(EmfPlusRenderBrush::Texture),
    EmfPlusBrushData::Unknown { .. } => None,
  }
}

fn emf_plus_pen_object(pen: &EmfPlusPenObject) -> Option<EmfPen> {
  let payload = pen.parse_pen_payload().ok()?;
  let brush = payload.brush_object.as_ref()?;
  Some(EmfPen {
    color: emf_plus_brush_object(brush)?.representative_color(),
    width: payload.pen_data.pen_width.round().max(1.0) as usize,
  })
}

fn emf_plus_brush_ref(
  brush: EmfPlusBrushRef,
  state: &EmfVectorState,
) -> Option<EmfPlusRenderBrush> {
  match brush {
    EmfPlusBrushRef::Color(color) => Some(EmfPlusRenderBrush::Solid(emf_plus_argb_to_color(color))),
    EmfPlusBrushRef::ObjectId(id) => match state.emf_plus_objects.get(id as usize)? {
      Some(EmfPlusRenderObject::Brush(brush)) => brush.clone(),
      Some(EmfPlusRenderObject::Pen(Some(pen))) => Some(EmfPlusRenderBrush::Solid(pen.color)),
      _ => None,
    },
  }
}

fn emf_plus_brush_ref_to_color(brush: EmfPlusBrushRef, state: &EmfVectorState) -> Option<EmfColor> {
  emf_plus_brush_ref(brush, state).map(|brush| brush.representative_color())
}

fn emf_plus_pen(id: u8, state: &EmfVectorState) -> Option<EmfPen> {
  match state.emf_plus_objects.get(id as usize)? {
    Some(EmfPlusRenderObject::Pen(pen)) => *pen,
    Some(EmfPlusRenderObject::Brush(Some(brush))) => Some(EmfPen {
      color: brush.representative_color(),
      width: 1,
    }),
    _ => None,
  }
}

fn emf_plus_argb_to_color(color: crate::EmfPlusArgb) -> EmfColor {
  EmfColor {
    r: color.red,
    g: color.green,
    b: color.blue,
  }
}

fn lerp_color(start: EmfColor, end: EmfColor, t: f32) -> EmfColor {
  let t = t.clamp(0.0, 1.0);
  EmfColor {
    r: (start.r as f32 + (end.r as f32 - start.r as f32) * t).round() as u8,
    g: (start.g as f32 + (end.g as f32 - start.g as f32) * t).round() as u8,
    b: (start.b as f32 + (end.b as f32 - start.b as f32) * t).round() as u8,
  }
}

fn average_color(a: EmfColor, b: EmfColor) -> EmfColor {
  EmfColor {
    r: ((u16::from(a.r) + u16::from(b.r)) / 2) as u8,
    g: ((u16::from(a.g) + u16::from(b.g)) / 2) as u8,
    b: ((u16::from(a.b) + u16::from(b.b)) / 2) as u8,
  }
}

fn average_image_color(image: &RasterPixels) -> EmfColor {
  if image.rgb.is_empty() {
    return EmfColor { r: 0, g: 0, b: 0 };
  }
  let mut r = 0u64;
  let mut g = 0u64;
  let mut b = 0u64;
  let mut count = 0u64;
  for pixel in image.rgb.chunks_exact(RGB_BYTES_PER_PIXEL) {
    r += u64::from(pixel[0]);
    g += u64::from(pixel[1]);
    b += u64::from(pixel[2]);
    count += 1;
  }
  EmfColor {
    r: (r / count) as u8,
    g: (g / count) as u8,
    b: (b / count) as u8,
  }
}

fn emf_plus_rect_bounds(rect: crate::EmfPlusRect) -> (i32, i32, i32, i32) {
  match rect {
    crate::EmfPlusRect::Compressed(rect) => (
      i32::from(rect.x),
      i32::from(rect.y),
      i32::from(rect.x) + i32::from(rect.width),
      i32::from(rect.y) + i32::from(rect.height),
    ),
    crate::EmfPlusRect::Float(rect) => emf_plus_rectf_bounds(rect),
  }
}

fn emf_plus_rectf_bounds(rect: crate::RectF) -> (i32, i32, i32, i32) {
  (
    rect.x.round() as i32,
    rect.y.round() as i32,
    (rect.x + rect.width).round() as i32,
    (rect.y + rect.height).round() as i32,
  )
}

fn fill_emf_plus_rect_shape(value: EmfPlusFillRectShapeData, state: &mut EmfVectorState) {
  if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    state.fill_ellipse_with_emf_plus_brush(left, top, right, bottom, &brush);
  }
}

fn draw_emf_plus_rect_shape(value: EmfPlusDrawRectShapeData, state: &mut EmfVectorState) {
  if let Some(pen) = emf_plus_pen(value.pen_id, state) {
    let old_brush = state.current_brush;
    let old_pen = state.current_pen;
    state.current_brush = None;
    state.current_pen = Some(pen);
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    state.fill_ellipse(left, top, right, bottom);
    state.current_brush = old_brush;
    state.current_pen = old_pen;
  }
}

fn fill_emf_plus_pie(value: EmfPlusFillPieData, state: &mut EmfVectorState) {
  if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    let points = arc_segment_points(
      left,
      top,
      right,
      bottom,
      value.start_angle,
      value.sweep_angle,
      true,
    );
    state.fill_polygon_with_emf_plus_brush(&points, &brush);
    state.draw_polyline(&points, true);
  }
}

fn draw_emf_plus_arc(value: EmfPlusDrawArcData, state: &mut EmfVectorState) {
  if let Some(pen) = emf_plus_pen(value.pen_id, state) {
    let old = state.current_pen;
    state.current_pen = Some(pen);
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    state.fill_arc_segment(
      (left, top, right, bottom),
      value.start_angle,
      value.sweep_angle,
      false,
    );
    state.current_pen = old;
  }
}

fn arc_segment_points(
  left: i32,
  top: i32,
  right: i32,
  bottom: i32,
  start_angle: f32,
  sweep_angle: f32,
  pie: bool,
) -> Vec<EmfPoint> {
  let steps = ((sweep_angle.abs() / 5.0).ceil() as usize).clamp(6, 144);
  let cx = (left + right) as f32 / 2.0;
  let cy = (top + bottom) as f32 / 2.0;
  let rx = (right - left).abs() as f32 / 2.0;
  let ry = (bottom - top).abs() as f32 / 2.0;
  let mut points = Vec::with_capacity(steps + usize::from(pie) + 1);
  if pie {
    points.push(EmfPoint {
      x: cx.round() as i32,
      y: cy.round() as i32,
    });
  }
  for index in 0..=steps {
    let angle = (start_angle + sweep_angle * index as f32 / steps as f32).to_radians();
    points.push(EmfPoint {
      x: (cx + angle.cos() * rx).round() as i32,
      y: (cy + angle.sin() * ry).round() as i32,
    });
  }
  points
}

fn draw_emf_plus_beziers(value: EmfPlusDrawPointsData, state: &mut EmfVectorState) {
  if let Some(pen) = emf_plus_pen(value.pen_id, state) {
    let old = state.current_pen;
    state.current_pen = Some(pen);
    let points = emf_plus_points_to_emf_points(&value.points);
    let flattened = flatten_bezier_sequence(&points);
    state.draw_polyline(&flattened, false);
    state.current_pen = old;
  }
}

fn emf_plus_points_to_emf_points(points: &EmfPlusPointData) -> Vec<EmfPoint> {
  match points {
    EmfPlusPointData::Relative(points) => {
      let mut current = EmfPoint { x: 0, y: 0 };
      points
        .iter()
        .map(|point| {
          current.x += i32::from(point.x);
          current.y += i32::from(point.y);
          current
        })
        .collect()
    }
    EmfPlusPointData::Compressed(points) => points
      .iter()
      .map(|point| EmfPoint {
        x: i32::from(point.x),
        y: i32::from(point.y),
      })
      .collect(),
    EmfPlusPointData::Float(points) => points
      .iter()
      .map(|point| EmfPoint {
        x: point.x.round() as i32,
        y: point.y.round() as i32,
      })
      .collect(),
  }
}

fn emf_plus_path_object_points(path: &EmfPlusPathObject) -> Vec<EmfPoint> {
  let points = emf_plus_points_to_emf_points(&path.points);
  let types = expanded_path_point_types(&path.point_types);
  if types.is_empty() {
    return points;
  }
  let mut result = Vec::with_capacity(points.len());
  let mut index = 0usize;
  while index < points.len() && index < types.len() {
    let point = points[index];
    let point_type = types[index];
    if point_type.path_point_type() == Some(EmfPlusPathPointType::Bezier)
      && index + 2 < points.len()
      && let Some(start) = result.last().copied()
    {
      result.extend(sample_cubic_bezier(
        start,
        points[index],
        points[index + 1],
        points[index + 2],
      ));
      index += 3;
      continue;
    }
    result.push(point);
    if point_type
      .path_point_flags()
      .contains(EmfPlusPathPointTypeFlags::CLOSE_SUBPATH)
      && let Some(first) = result.first().copied()
    {
      result.push(first);
    }
    index += 1;
  }
  result
}

fn flatten_bezier_sequence(points: &[EmfPoint]) -> Vec<EmfPoint> {
  let Some(first) = points.first().copied() else {
    return Vec::new();
  };
  let mut result = vec![first];
  let mut index = 1usize;
  while index + 2 < points.len() {
    let start = *result.last().unwrap_or(&first);
    result.extend(sample_cubic_bezier(
      start,
      points[index],
      points[index + 1],
      points[index + 2],
    ));
    index += 3;
  }
  result.extend_from_slice(&points[index..]);
  result
}

fn sample_cubic_bezier(p0: EmfPoint, p1: EmfPoint, p2: EmfPoint, p3: EmfPoint) -> Vec<EmfPoint> {
  let chord = ((p3.x - p0.x).unsigned_abs() + (p3.y - p0.y).unsigned_abs()) as usize;
  let control = ((p1.x - p0.x).unsigned_abs()
    + (p1.y - p0.y).unsigned_abs()
    + (p2.x - p3.x).unsigned_abs()
    + (p2.y - p3.y).unsigned_abs()) as usize;
  let steps = ((chord + control) / 16).clamp(8, 64);
  (1..=steps)
    .map(|step| {
      let t = step as f32 / steps as f32;
      let mt = 1.0 - t;
      let x = mt.powi(3) * p0.x as f32
        + 3.0 * mt.powi(2) * t * p1.x as f32
        + 3.0 * mt * t.powi(2) * p2.x as f32
        + t.powi(3) * p3.x as f32;
      let y = mt.powi(3) * p0.y as f32
        + 3.0 * mt.powi(2) * t * p1.y as f32
        + 3.0 * mt * t.powi(2) * p2.y as f32
        + t.powi(3) * p3.y as f32;
      EmfPoint {
        x: x.round() as i32,
        y: y.round() as i32,
      }
    })
    .collect()
}

fn flatten_cardinal_curve(points: &[EmfPoint], tension: f32, closed: bool) -> Vec<EmfPoint> {
  if points.len() < 2 {
    return points.to_vec();
  }
  let mut result = Vec::new();
  if !closed {
    result.push(points[0]);
  }
  let segment_count = if closed {
    points.len()
  } else {
    points.len() - 1
  };
  let tension = tension.clamp(0.0, 1.0);
  let tangent_scale = (1.0 - tension) / 2.0;
  for index in 0..segment_count {
    let p0 = if index == 0 {
      if closed {
        points[points.len() - 1]
      } else {
        points[0]
      }
    } else {
      points[index - 1]
    };
    let p1 = points[index];
    let p2 = points[(index + 1) % points.len()];
    let p3 = if index + 2 < points.len() {
      points[index + 2]
    } else if closed {
      points[(index + 2) % points.len()]
    } else {
      points[points.len() - 1]
    };
    let distance = ((p2.x - p1.x).unsigned_abs() + (p2.y - p1.y).unsigned_abs()) as usize;
    let steps = (distance / 12).clamp(6, 32);
    for step in 1..=steps {
      let t = step as f32 / steps as f32;
      let t2 = t * t;
      let t3 = t2 * t;
      let m1x = (p2.x - p0.x) as f32 * tangent_scale;
      let m1y = (p2.y - p0.y) as f32 * tangent_scale;
      let m2x = (p3.x - p1.x) as f32 * tangent_scale;
      let m2y = (p3.y - p1.y) as f32 * tangent_scale;
      let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
      let h10 = t3 - 2.0 * t2 + t;
      let h01 = -2.0 * t3 + 3.0 * t2;
      let h11 = t3 - t2;
      result.push(EmfPoint {
        x: (h00 * p1.x as f32 + h10 * m1x + h01 * p2.x as f32 + h11 * m2x).round() as i32,
        y: (h00 * p1.y as f32 + h10 * m1y + h01 * p2.y as f32 + h11 * m2y).round() as i32,
      });
    }
  }
  result
}

fn expanded_path_point_types(types: &EmfPlusPathPointTypes) -> Vec<EmfPlusPathPointTypeValue> {
  match types {
    EmfPlusPathPointTypes::Values(values) => values.clone(),
    EmfPlusPathPointTypes::Rle(values) => {
      let mut expanded = Vec::new();
      for value in values {
        expanded.extend(std::iter::repeat_n(
          value.point_type,
          value.run_count() as usize,
        ));
      }
      expanded
    }
  }
}

fn emf_plus_path_points(id: u8, state: &EmfVectorState) -> Option<Vec<EmfPoint>> {
  match state.emf_plus_objects.get(id as usize)? {
    Some(EmfPlusRenderObject::Path(points)) => Some(points.clone()),
    _ => None,
  }
}

fn emf_plus_region_points(id: u8, state: &EmfVectorState) -> Option<Vec<EmfPoint>> {
  match state.emf_plus_objects.get(id as usize)? {
    Some(EmfPlusRenderObject::Region(points)) => Some(points.clone()),
    _ => None,
  }
}

fn emf_plus_region_object_points(region: &crate::emfplus::EmfPlusRegionObject) -> Vec<EmfPoint> {
  let Ok(nodes) = region.parse_region_nodes() else {
    return Vec::new();
  };
  nodes
    .first()
    .and_then(emf_plus_region_node_points)
    .unwrap_or_default()
}

fn emf_plus_region_node_points(node: &crate::emfplus::EmfPlusRegionNode) -> Option<Vec<EmfPoint>> {
  match &node.data {
    crate::emfplus::EmfPlusRegionNodeData::Rect(rect) => {
      let (left, top, right, bottom) = emf_plus_rectf_bounds(*rect);
      Some(rect_points(left, top, right, bottom))
    }
    crate::emfplus::EmfPlusRegionNodeData::Path(path) => {
      path.path().map(emf_plus_path_object_points)
    }
    crate::emfplus::EmfPlusRegionNodeData::Infinite => Some(rect_points(
      0,
      0,
      DEFAULT_RENDER_WIDTH as i32,
      DEFAULT_RENDER_HEIGHT as i32,
    )),
    crate::emfplus::EmfPlusRegionNodeData::ChildNodes(children) => {
      let left = emf_plus_region_node_points(&children.left);
      let right = emf_plus_region_node_points(&children.right);
      union_point_bounds(left.as_deref(), right.as_deref())
    }
    crate::emfplus::EmfPlusRegionNodeData::Empty
    | crate::emfplus::EmfPlusRegionNodeData::Raw(_) => None,
  }
}

fn union_point_bounds(
  left: Option<&[EmfPoint]>,
  right: Option<&[EmfPoint]>,
) -> Option<Vec<EmfPoint>> {
  let mut points = Vec::new();
  if let Some(left) = left {
    points.extend_from_slice(left);
  }
  if let Some(right) = right {
    points.extend_from_slice(right);
  }
  point_bounds(&points).map(|(left, top, right, bottom)| rect_points(left, top, right, bottom))
}

fn point_bounds(points: &[EmfPoint]) -> Option<(i32, i32, i32, i32)> {
  let first = points.first().copied()?;
  let mut left = first.x;
  let mut top = first.y;
  let mut right = first.x;
  let mut bottom = first.y;
  for point in points.iter().copied().skip(1) {
    left = left.min(point.x);
    top = top.min(point.y);
    right = right.max(point.x);
    bottom = bottom.max(point.y);
  }
  Some((left, top, right, bottom))
}

fn rect_points(left: i32, top: i32, right: i32, bottom: i32) -> Vec<EmfPoint> {
  vec![
    EmfPoint { x: left, y: top },
    EmfPoint { x: right, y: top },
    EmfPoint {
      x: right,
      y: bottom,
    },
    EmfPoint { x: left, y: bottom },
  ]
}

fn draw_emf_plus_image(value: EmfPlusDrawImageData, state: &mut EmfVectorState) {
  let Some(image) = state
    .emf_plus_objects
    .get(value.image_id as usize)
    .and_then(|value| match value {
      Some(EmfPlusRenderObject::Image(image)) => Some(image.clone()),
      _ => None,
    })
  else {
    return;
  };
  let (left, top, right, bottom) = emf_plus_rect_bounds(value.dest_rect);
  state.draw_rgb_image(left, top, right - left, bottom - top, &image);
}

fn draw_emf_plus_image_points(value: EmfPlusDrawImagePointsData, state: &mut EmfVectorState) {
  let Some(image) = state
    .emf_plus_objects
    .get(value.image_id as usize)
    .and_then(|value| match value {
      Some(EmfPlusRenderObject::Image(image)) => Some(image.clone()),
      _ => None,
    })
  else {
    return;
  };
  let points = emf_plus_points_to_emf_points(&value.points);
  let Some(first) = points.first().copied() else {
    return;
  };
  let width = points
    .get(1)
    .map(|point| point.x - first.x)
    .unwrap_or(image.width as i32);
  let height = points
    .get(2)
    .map(|point| point.y - first.y)
    .unwrap_or(image.height as i32);
  state.draw_rgb_image(first.x, first.y, width, height, &image);
}

fn draw_emf_plus_string(value: EmfPlusDrawStringData, state: &mut EmfVectorState) {
  let Some(color) = emf_plus_brush_ref_to_color(value.brush, state) else {
    return;
  };
  let text = value
    .string
    .as_str()
    .map(|text| text.to_string())
    .unwrap_or_default();
  let height = match state.emf_plus_objects.get(value.font_id as usize) {
    Some(Some(EmfPlusRenderObject::Font(font))) => font.em_size.round() as i32,
    _ => value.layout_rect.height.round() as i32,
  }
  .abs()
  .max(7);
  state.draw_text(
    value.layout_rect.x.round() as i32,
    value.layout_rect.y.round() as i32 + height,
    &text,
    color,
    &TextFont::with_height(-height),
    TA_BASELINE,
    &[],
  );
}

fn emf_plus_image_object_to_rgb(
  image: &EmfPlusImageObject,
) -> Result<Option<RasterPixels>, String> {
  match image.parse_image_data().map_err(|err| err.to_string())? {
    EmfPlusImageData::Bitmap(bitmap) => emf_plus_bitmap_to_rgb(&bitmap),
    EmfPlusImageData::Metafile(metafile) => {
      let Some(raster) =
        decode_metafile_as_raster(&metafile.metafile_data, None).map_err(|err| err.to_string())?
      else {
        return Ok(None);
      };
      decoded_raster_to_rgb(&raster)
    }
    EmfPlusImageData::Unknown { .. } => Ok(None),
  }
}

fn emf_plus_bitmap_to_rgb(
  bitmap: &crate::emfplus::EmfPlusBitmapObject,
) -> Result<Option<RasterPixels>, String> {
  match bitmap.parse_bitmap_data().map_err(|err| err.to_string())? {
    EmfPlusBitmapPayload::Compressed(data) => {
      let raster = image::load_from_memory(&data.compressed_image_data)
        .map_err(|err| err.to_string())?
        .to_rgb8();
      let (width, height) = raster.dimensions();
      Ok(Some(RasterPixels {
        width: width as usize,
        height: height as usize,
        rgb: raster.into_raw(),
      }))
    }
    EmfPlusBitmapPayload::Pixel(data) => emf_plus_pixel_bitmap_to_rgb(bitmap, &data.pixel_data),
    EmfPlusBitmapPayload::Unknown { .. } => Ok(None),
  }
}

fn emf_plus_pixel_bitmap_to_rgb(
  bitmap: &crate::emfplus::EmfPlusBitmapObject,
  pixels: &[u8],
) -> Result<Option<RasterPixels>, String> {
  if bitmap.width <= 0 || bitmap.height <= 0 {
    return Ok(None);
  }
  let width = bitmap.width as usize;
  let height = bitmap.height as usize;
  let stride = bitmap.stride.unsigned_abs() as usize;
  let bpp = bitmap.bits_per_pixel();
  let bytes_per_pixel = match bpp {
    24 => 3,
    32 => 4,
    _ => return Ok(None),
  };
  let required = stride
    .checked_mul(height)
    .ok_or_else(|| "EMF+ bitmap dimensions overflow".to_string())?;
  if pixels.len() < required || stride < width * bytes_per_pixel {
    return Err("EMF+ bitmap payload is truncated".to_string());
  }
  let mut rgb = vec![0u8; width * height * RGB_BYTES_PER_PIXEL];
  for row in 0..height {
    let src_row = if bitmap.stride < 0 {
      height - 1 - row
    } else {
      row
    };
    let src = &pixels[src_row * stride..src_row * stride + stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    for col in 0..width {
      let src_pixel = &src[col * bytes_per_pixel..col * bytes_per_pixel + bytes_per_pixel];
      let dest_pixel =
        &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
      dest_pixel[0] = src_pixel[2];
      dest_pixel[1] = src_pixel[1];
      dest_pixel[2] = src_pixel[0];
    }
  }
  Ok(Some(RasterPixels { width, height, rgb }))
}

fn xform_to_transform(value: crate::XForm) -> EmfTransform {
  EmfTransform {
    m11: value.m11,
    m12: value.m12,
    m21: value.m21,
    m22: value.m22,
    dx: value.dx,
    dy: value.dy,
  }
}

fn translate_transform(value: EmfPlusTranslateWorldTransformData) -> EmfTransform {
  EmfTransform {
    dx: value.dx,
    dy: value.dy,
    ..EmfTransform::identity()
  }
}

fn scale_transform(value: EmfPlusScaleWorldTransformData) -> EmfTransform {
  EmfTransform {
    m11: value.sx,
    m22: value.sy,
    ..EmfTransform::identity()
  }
}

fn rotate_transform(value: EmfPlusRotateWorldTransformData) -> EmfTransform {
  let radians = value.angle.to_radians();
  EmfTransform {
    m11: radians.cos(),
    m12: radians.sin(),
    m21: -radians.sin(),
    m22: radians.cos(),
    dx: 0.0,
    dy: 0.0,
  }
}

fn multiply_emf_plus_transform(
  transform: EmfTransform,
  post_multiply: bool,
  state: &mut EmfVectorState,
) {
  state.world_transform = if post_multiply {
    state.world_transform.multiply(transform)
  } else {
    transform.multiply(state.world_transform)
  };
}

fn color_ref_to_emf(value: crate::ColorRef) -> EmfColor {
  EmfColor {
    r: value.red,
    g: value.green,
    b: value.blue,
  }
}

/// StrictLib patch: the selected LOGFONT, not only its height.
fn emf_current_font(state: &EmfVectorState) -> TextFont {
  state
    .current_font
    .and_then(|id| state.fonts.get(&id))
    .map_or(TextFont::with_height(-12), |font| font.text)
}

/// StrictLib patch: the `dx` array of an `EMR_EXTTEXTOUT*` record, one
/// advance per character in logical units; empty when the record has none.
fn emf_text_dx(data: &[u8], record_offset: usize, record_size: usize, characters: usize) -> Vec<i32> {
  const EMRTEXT_OFFSET: usize = 36;
  let Ok(options) = read_u32(data, record_offset + EMRTEXT_OFFSET + 16) else {
    return Vec::new();
  };
  let Ok(dx_offset) = read_u32(data, record_offset + EMRTEXT_OFFSET + 36) else {
    return Vec::new();
  };
  let stride = if options & ETO_PDY != 0 { 2 } else { 1 };
  let dx_offset = dx_offset as usize;
  if dx_offset == 0 || characters == 0 {
    return Vec::new();
  }
  let Some(end) = characters
    .checked_mul(4 * stride)
    .and_then(|bytes| dx_offset.checked_add(bytes))
  else {
    return Vec::new();
  };
  if end > record_size {
    return Vec::new();
  }
  (0..characters)
    .filter_map(|index| read_i32(data, record_offset + dx_offset + index * 4 * stride).ok())
    .collect()
}

fn emf_arc_rect(data: &[u8], record_offset: usize) -> Result<(i32, i32, i32, i32), String> {
  Ok((
    read_i32(data, record_offset + 8)?,
    read_i32(data, record_offset + 12)?,
    read_i32(data, record_offset + 16)?,
    read_i32(data, record_offset + 20)?,
  ))
}

fn angle_from_emf_arc_point(rect: (i32, i32, i32, i32), x: i32, y: i32) -> f32 {
  let (left, top, right, bottom) = rect;
  let cx = (left + right) as f32 / 2.0;
  let cy = (top + bottom) as f32 / 2.0;
  (y as f32 - cy).atan2(x as f32 - cx).to_degrees()
}

fn sweep_from_emf_arc_points(
  data: &[u8],
  record_offset: usize,
  rect: (i32, i32, i32, i32),
) -> Result<f32, String> {
  let start = angle_from_emf_arc_point(
    rect,
    read_i32(data, record_offset + 24)?,
    read_i32(data, record_offset + 28)?,
  );
  let end = angle_from_emf_arc_point(
    rect,
    read_i32(data, record_offset + 32)?,
    read_i32(data, record_offset + 36)?,
  );
  let mut sweep = end - start;
  if sweep <= 0.0 {
    sweep += 360.0;
  }
  Ok(sweep)
}

fn angle_from_arc_point(value: crate::wmf::WmfArcRecord, x: i16, y: i16) -> f32 {
  let cx = (i32::from(value.left) + i32::from(value.right)) as f32 / 2.0;
  let cy = (i32::from(value.top) + i32::from(value.bottom)) as f32 / 2.0;
  (f32::from(y) - cy).atan2(f32::from(x) - cx).to_degrees()
}

fn sweep_from_arc_points(value: crate::wmf::WmfArcRecord) -> f32 {
  let start = angle_from_arc_point(value, value.x_radial_1, value.y_radial_1);
  let end = angle_from_arc_point(value, value.x_radial_2, value.y_radial_2);
  let mut sweep = end - start;
  if sweep <= 0.0 {
    sweep += 360.0;
  }
  sweep
}

fn decoded_raster_to_rgb(raster: &DecodedMetafile) -> Result<Option<RasterPixels>, String> {
  match raster.content_type {
    "image/png" => {
      let image = image::load_from_memory_with_format(&raster.data, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?
        .to_rgb8();
      let (width, height) = image.dimensions();
      Ok(Some(RasterPixels {
        width: width as usize,
        height: height as usize,
        rgb: image.into_raw(),
      }))
    }
    "image/jpeg" => Ok(None),
    _ => Ok(None),
  }
}

fn bitmap16_to_rgb(data: &[u8]) -> Result<Option<RasterPixels>, String> {
  let bitmap = crate::wmf::WmfBitmap16::read_from_slice(data).map_err(|err| err.to_string())?;
  let width = bitmap.header.width.max(1) as usize;
  let height = bitmap.header.height.max(1) as usize;
  let bits_pixel = bitmap.header.bits_pixel;
  let stride = bitmap
    .header
    .computed_width_bytes()
    .map_err(|err| err.to_string())?;
  let required = stride
    .checked_mul(height)
    .ok_or_else(|| "Bitmap16 dimensions overflow".to_string())?;
  if bitmap.bits.len() < required {
    return Err("Bitmap16 bits are truncated".to_string());
  }
  let mut rgb = vec![0u8; width * height * RGB_BYTES_PER_PIXEL];
  for row in 0..height {
    let src_row = height - 1 - row;
    let src = &bitmap.bits[src_row * stride..src_row * stride + stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    match bits_pixel {
      1 => {
        for col in 0..width {
          let bit = (src[col / 8] >> (7 - (col % 8))) & 1;
          let value = if bit == 0 { 0 } else { 255 };
          let offset = col * RGB_BYTES_PER_PIXEL;
          dest[offset] = value;
          dest[offset + 1] = value;
          dest[offset + 2] = value;
        }
      }
      8 => {
        for (col, value) in src.iter().copied().enumerate().take(width) {
          let offset = col * RGB_BYTES_PER_PIXEL;
          dest[offset] = value;
          dest[offset + 1] = value;
          dest[offset + 2] = value;
        }
      }
      24 => {
        for col in 0..width {
          let src_pixel =
            &src[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      _ => return Ok(None),
    }
  }
  Ok(Some(RasterPixels { width, height, rgb }))
}

fn packed_dib_to_rgb(
  data: &[u8],
  color_usage: DibColorUsage,
) -> Result<Option<RasterPixels>, String> {
  let dib =
    DeviceIndependentBitmap::from_packed_slice(data, color_usage).map_err(|err| err.to_string())?;
  device_independent_bitmap_to_rgb(&dib, color_usage)
}

fn device_independent_bitmap_to_rgb(
  dib: &DeviceIndependentBitmap,
  color_usage: DibColorUsage,
) -> Result<Option<RasterPixels>, String> {
  match dib.info.header.compression_kind() {
    Some(BitmapCompression::Png) => {
      let image = image::load_from_memory_with_format(&dib.bits, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?
        .to_rgb8();
      let (width, height) = image.dimensions();
      Ok(Some(RasterPixels {
        width: width as usize,
        height: height as usize,
        rgb: image.into_raw(),
      }))
    }
    Some(BitmapCompression::Jpeg) => Ok(None),
    Some(BitmapCompression::Rgb) => {
      dib_rgb_bits_to_rgb(&dib.info.header, &dib.bits, &dib.info, color_usage).map(Some)
    }
    Some(BitmapCompression::Bitfields) => {
      dib_bitfields_to_rgb(&dib.info.header, &dib.bits, &dib.info).map(Some)
    }
    Some(BitmapCompression::Rle8) | Some(BitmapCompression::Rle4) => {
      dib_rle_to_rgb(&dib.info.header, &dib.bits, &dib.info, color_usage).map(Some)
    }
    _ => Ok(None),
  }
}

fn dib_rgb_bits_to_rgb(
  header: &DibHeader,
  bits: &[u8],
  info: &crate::DibBitmapInfo,
  color_usage: DibColorUsage,
) -> Result<RasterPixels, String> {
  let width = header.width();
  let height = header.height();
  if width <= 0 || height == 0 {
    return Err(format!("unsupported DIB size {width}x{height}"));
  }
  let width = width as usize;
  let height_abs = header.height_abs() as usize;
  let bit_count = header.bit_count();
  let row_stride = header
    .scan_line_stride_bytes()
    .map_err(|err| err.to_string())? as usize;
  let required_size = row_stride
    .checked_mul(height_abs)
    .ok_or_else(|| "DIB dimensions overflow".to_string())?;
  if bits.len() < required_size {
    return Err(format!(
      "DIB payload is truncated: need {required_size} bytes, got {}",
      bits.len()
    ));
  }
  let palette = match bit_count {
    1 | 4 | 8 => match info
      .parse_color_table(color_usage)
      .map_err(|err| err.to_string())?
    {
      DibColorTable::RgbQuads { entries, .. } => entries,
      _ => Vec::new(),
    },
    _ => Vec::new(),
  };
  let mut rgb = vec![0u8; width * height_abs * RGB_BYTES_PER_PIXEL];
  for row in 0..height_abs {
    let src_row = if header.is_top_down() {
      row
    } else {
      height_abs - 1 - row
    };
    let src = &bits[src_row * row_stride..src_row * row_stride + row_stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    match bit_count {
      1 => {
        for col in 0..width {
          let byte = src[col / 8];
          let index = ((byte >> (7 - (col % 8))) & 0x01) as usize;
          write_palette_pixel(dest, col, &palette, index);
        }
      }
      4 => {
        for col in 0..width {
          let byte = src[col / 2];
          let index = if col.is_multiple_of(2) {
            (byte >> 4) as usize
          } else {
            (byte & 0x0f) as usize
          };
          write_palette_pixel(dest, col, &palette, index);
        }
      }
      8 => {
        for (col, index) in src.iter().copied().enumerate().take(width) {
          write_palette_pixel(dest, col, &palette, index as usize);
        }
      }
      24 => {
        for col in 0..width {
          let src_pixel =
            &src[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      32 => {
        for col in 0..width {
          let src_pixel =
            &src[col * BGRA_BYTES_PER_PIXEL..col * BGRA_BYTES_PER_PIXEL + BGRA_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      other => return Err(format!("unsupported BI_RGB bit depth: {other}")),
    }
  }
  Ok(RasterPixels {
    width,
    height: height_abs,
    rgb,
  })
}

fn dib_bitfields_to_rgb(
  header: &DibHeader,
  bits: &[u8],
  info: &crate::DibBitmapInfo,
) -> Result<RasterPixels, String> {
  let width = header.width();
  let height = header.height();
  if width <= 0 || height == 0 {
    return Err(format!("unsupported DIB size {width}x{height}"));
  }
  let width = width as usize;
  let height_abs = header.height_abs() as usize;
  let bit_count = header.bit_count();
  let bytes_per_pixel = match bit_count {
    16 => 2,
    32 => 4,
    other => return Err(format!("unsupported BI_BITFIELDS bit depth: {other}")),
  };
  let row_stride = header
    .scan_line_stride_bytes()
    .map_err(|err| err.to_string())? as usize;
  let required_size = row_stride
    .checked_mul(height_abs)
    .ok_or_else(|| "DIB dimensions overflow".to_string())?;
  if bits.len() < required_size {
    return Err(format!(
      "DIB payload is truncated: need {required_size} bytes, got {}",
      bits.len()
    ));
  }
  let masks = info.bitfield_masks().map_err(|err| err.to_string())?;
  let masks = masks.unwrap_or(match bit_count {
    16 => [0x7C00, 0x03E0, 0x001F],
    32 => [0x00FF_0000, 0x0000_FF00, 0x0000_00FF],
    _ => unreachable!(),
  });
  let mut rgb = vec![0u8; width * height_abs * RGB_BYTES_PER_PIXEL];
  for row in 0..height_abs {
    let src_row = if header.is_top_down() {
      row
    } else {
      height_abs - 1 - row
    };
    let src = &bits[src_row * row_stride..src_row * row_stride + row_stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    for col in 0..width {
      let offset = col * bytes_per_pixel;
      let value = if bytes_per_pixel == 2 {
        u32::from(u16::from_le_bytes([src[offset], src[offset + 1]]))
      } else {
        u32::from_le_bytes([
          src[offset],
          src[offset + 1],
          src[offset + 2],
          src[offset + 3],
        ])
      };
      let dest_pixel =
        &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
      dest_pixel[0] = bitfield_channel(value, masks[0]);
      dest_pixel[1] = bitfield_channel(value, masks[1]);
      dest_pixel[2] = bitfield_channel(value, masks[2]);
    }
  }
  Ok(RasterPixels {
    width,
    height: height_abs,
    rgb,
  })
}

fn bitfield_channel(value: u32, mask: u32) -> u8 {
  if mask == 0 {
    return 0;
  }
  let shift = mask.trailing_zeros();
  let bits = mask.count_ones();
  let raw = (value & mask) >> shift;
  let max = (1u32 << bits) - 1;
  ((raw * 255 + max / 2) / max) as u8
}

fn dib_rle_to_rgb(
  header: &DibHeader,
  bits: &[u8],
  info: &crate::DibBitmapInfo,
  color_usage: DibColorUsage,
) -> Result<RasterPixels, String> {
  let width = header.width();
  let height = header.height();
  if width <= 0 || height == 0 {
    return Err(format!("unsupported DIB size {width}x{height}"));
  }
  let width = width as usize;
  let height_abs = header.height_abs() as usize;
  let palette = match info
    .parse_color_table(color_usage)
    .map_err(|err| err.to_string())?
  {
    DibColorTable::RgbQuads { entries, .. } => entries,
    _ => Vec::new(),
  };
  let indices = match header.compression_kind() {
    Some(BitmapCompression::Rle8) => decode_rle8_indices(bits, width, height_abs)?,
    Some(BitmapCompression::Rle4) => decode_rle4_indices(bits, width, height_abs)?,
    other => return Err(format!("unsupported RLE compression: {other:?}")),
  };
  let mut rgb = vec![0u8; width * height_abs * RGB_BYTES_PER_PIXEL];
  for row in 0..height_abs {
    let src_row = if header.is_top_down() {
      row
    } else {
      height_abs - 1 - row
    };
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    for col in 0..width {
      write_palette_pixel(dest, col, &palette, indices[src_row * width + col] as usize);
    }
  }
  Ok(RasterPixels {
    width,
    height: height_abs,
    rgb,
  })
}

fn decode_rle8_indices(bits: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
  let mut out = vec![0u8; width * height];
  let mut x = 0usize;
  let mut y = 0usize;
  let mut pos = 0usize;
  while pos + 1 < bits.len() && y < height {
    let count = bits[pos];
    let value = bits[pos + 1];
    pos += 2;
    if count != 0 {
      for _ in 0..count {
        if x < width && y < height {
          out[y * width + x] = value;
        }
        x = x.saturating_add(1);
      }
      continue;
    }
    match value {
      0 => {
        x = 0;
        y = y.saturating_add(1);
      }
      1 => break,
      2 if pos + 1 < bits.len() => {
        x = x.saturating_add(bits[pos] as usize);
        y = y.saturating_add(bits[pos + 1] as usize);
        pos += 2;
      }
      n => {
        let n = n as usize;
        if pos + n > bits.len() {
          return Err("RLE8 absolute run is truncated".to_string());
        }
        for value in &bits[pos..pos + n] {
          if x < width && y < height {
            out[y * width + x] = *value;
          }
          x = x.saturating_add(1);
        }
        pos += n + (n % 2);
      }
    }
  }
  Ok(out)
}

fn decode_rle4_indices(bits: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
  let mut out = vec![0u8; width * height];
  let mut x = 0usize;
  let mut y = 0usize;
  let mut pos = 0usize;
  while pos + 1 < bits.len() && y < height {
    let count = bits[pos];
    let value = bits[pos + 1];
    pos += 2;
    if count != 0 {
      let high = value >> 4;
      let low = value & 0x0F;
      for index in 0..count as usize {
        if x < width && y < height {
          out[y * width + x] = if index.is_multiple_of(2) { high } else { low };
        }
        x = x.saturating_add(1);
      }
      continue;
    }
    match value {
      0 => {
        x = 0;
        y = y.saturating_add(1);
      }
      1 => break,
      2 if pos + 1 < bits.len() => {
        x = x.saturating_add(bits[pos] as usize);
        y = y.saturating_add(bits[pos + 1] as usize);
        pos += 2;
      }
      n => {
        let pixel_count = n as usize;
        let byte_count = pixel_count.div_ceil(2);
        if pos + byte_count > bits.len() {
          return Err("RLE4 absolute run is truncated".to_string());
        }
        for index in 0..pixel_count {
          let byte = bits[pos + index / 2];
          let value = if index.is_multiple_of(2) {
            byte >> 4
          } else {
            byte & 0x0F
          };
          if x < width && y < height {
            out[y * width + x] = value;
          }
          x = x.saturating_add(1);
        }
        pos += byte_count + (byte_count % 2);
      }
    }
  }
  Ok(out)
}

fn write_palette_pixel(dest: &mut [u8], col: usize, palette: &[crate::RgbQuad], index: usize) {
  let Some(color) = palette.get(index) else {
    return;
  };
  let dest_pixel =
    &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
  dest_pixel[0] = color.red;
  dest_pixel[1] = color.green;
  dest_pixel[2] = color.blue;
}

fn draw_glyph_5x7(
  state: &mut EmfVectorState,
  x: i32,
  y: i32,
  ch: char,
  color: EmfColor,
  scale: usize,
) {
  let glyph = glyph_5x7(ch);
  for (row, bits) in glyph.iter().copied().enumerate() {
    for col in 0..5 {
      if bits & (1 << (4 - col)) == 0 {
        continue;
      }
      for yy in 0..scale {
        for xx in 0..scale {
          state.set_pixel(
            x + (col * scale + xx) as i32,
            y + (row * scale + yy) as i32,
            color,
          );
        }
      }
    }
  }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
  match ch.to_ascii_uppercase() {
    '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
    '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
    '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
    '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
    '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
    '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
    '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
    '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
    '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
    '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
    'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
    'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
    'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
    'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
    'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
    'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
    'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
    'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
    'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
    'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
    'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
    'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
    'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
    'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
    'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
    'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
    'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
    'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
    'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
    'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
    'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
    'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
    'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
    'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
    'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
    'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
    '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
    '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
    ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
    ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
    '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
    _ => [0x1F, 0x11, 0x15, 0x15, 0x15, 0x11, 0x1F],
  }
}

fn clamp_canvas_size(width: usize, height: usize, max_pixels: Option<u32>) -> (usize, usize) {
  let max_pixels = max_pixels.unwrap_or(DEFAULT_MAX_PIXELS as u32).max(1) as usize;
  match width.checked_mul(height) {
    Some(pixels) if pixels <= max_pixels => (width, height),
    Some(pixels) => {
      let scale = (max_pixels as f64 / pixels as f64).sqrt();
      (
        ((width as f64 * scale).round() as usize).max(1),
        ((height as f64 * scale).round() as usize).max(1),
      )
    }
    None => (DEFAULT_RENDER_WIDTH, DEFAULT_RENDER_HEIGHT),
  }
}

fn visit_polygon_scanline_spans(
  points: &[(f32, f32)],
  width: usize,
  height: usize,
  mut visit: impl FnMut(usize, usize, usize),
) {
  if points.len() < 3 || width == 0 || height == 0 {
    return;
  }

  let mut min_y = f32::INFINITY;
  let mut max_y = f32::NEG_INFINITY;
  for &(_, y) in points {
    if y.is_finite() {
      min_y = min_y.min(y);
      max_y = max_y.max(y);
    }
  }
  if !min_y.is_finite() || !max_y.is_finite() {
    return;
  }

  // A scanline samples at y + 0.5, so rows wholly outside the polygon's
  // vertical bounds cannot contribute. Retaining one floor/ceil boundary
  // row preserves the existing edge rule while avoiding a full-canvas scan
  // for every small metafile polygon.
  let start_y = min_y.floor().max(0.0).min(height as f32) as usize;
  let end_y = max_y.ceil().max(0.0).min(height as f32) as usize;
  let mut intersections = Vec::new();
  for y in start_y..end_y {
    let scan_y = y as f32 + 0.5;
    intersections.clear();
    for index in 0..points.len() {
      let (x1, y1) = points[index];
      let (x2, y2) = points[(index + 1) % points.len()];
      if (y1 <= scan_y && y2 > scan_y) || (y2 <= scan_y && y1 > scan_y) {
        let t = (scan_y - y1) / (y2 - y1);
        intersections.push(x1 + t * (x2 - x1));
      }
    }
    intersections.sort_by(|a, b| a.total_cmp(b));
    for pair in intersections.chunks_exact(2) {
      let start_x = pair[0].floor().max(0.0).min(width as f32) as usize;
      let end_x = pair[1].ceil().max(0.0).min(width as f32) as usize;
      if end_x > start_x {
        visit(y, start_x, end_x);
      }
    }
  }
}

fn axis_aligned_clip_rect(
  points: &[(f32, f32)],
  width: usize,
  height: usize,
) -> Option<(i32, i32, i32, i32)> {
  let points = if points.len() == 5 && points_approximately_equal(points[0], points[4]) {
    &points[..4]
  } else {
    points
  };
  if points.len() != 4 || points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
    return None;
  }
  for index in 0..points.len() {
    let current = points[index];
    let next = points[(index + 1) % points.len()];
    if !approximately_equal(current.0, next.0) && !approximately_equal(current.1, next.1) {
      return None;
    }
  }

  let min_x = points
    .iter()
    .map(|point| point.0)
    .fold(f32::INFINITY, f32::min);
  let max_x = points
    .iter()
    .map(|point| point.0)
    .fold(f32::NEG_INFINITY, f32::max);
  let min_y = points
    .iter()
    .map(|point| point.1)
    .fold(f32::INFINITY, f32::min);
  let max_y = points
    .iter()
    .map(|point| point.1)
    .fold(f32::NEG_INFINITY, f32::max);
  Some((
    min_x.floor().max(0.0).min(width as f32) as i32,
    (min_y - 0.5).ceil().max(0.0).min(height as f32) as i32,
    max_x.ceil().max(0.0).min(width as f32) as i32,
    (max_y - 0.5).ceil().max(0.0).min(height as f32) as i32,
  ))
}

fn points_approximately_equal(left: (f32, f32), right: (f32, f32)) -> bool {
  approximately_equal(left.0, right.0) && approximately_equal(left.1, right.1)
}

fn approximately_equal(left: f32, right: f32) -> bool {
  (left - right).abs() <= 0.001
}

fn intersect_rects(
  left: (i32, i32, i32, i32),
  right: (i32, i32, i32, i32),
) -> (i32, i32, i32, i32) {
  let x1 = left.0.max(right.0);
  let y1 = left.1.max(right.1);
  let x2 = left.2.min(right.2).max(x1);
  let y2 = left.3.min(right.3).max(y1);
  (x1, y1, x2, y2)
}

fn read_poly_polygons_i32(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Result<Vec<Vec<EmfPoint>>, String> {
  let polygon_count = read_u32(data, record_offset + 24)? as usize;
  let total_points = read_u32(data, record_offset + 28)? as usize;
  let counts_offset = record_offset + 32;
  let points_offset = counts_offset
    .checked_add(polygon_count * 4)
    .ok_or_else(|| "EMF polygon counts overflow".to_string())?;
  if points_offset > record_offset + record_size {
    return Ok(Vec::new());
  }
  let mut counts = Vec::with_capacity(polygon_count);
  for index in 0..polygon_count {
    counts.push(read_u32(data, counts_offset + index * 4)? as usize);
  }
  let Some(points) = read_points_i32(data, points_offset, total_points) else {
    return Ok(Vec::new());
  };
  Ok(split_polygons(points, counts))
}

fn read_poly_polygons_i16(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Result<Vec<Vec<EmfPoint>>, String> {
  let polygon_count = read_u32(data, record_offset + 24)? as usize;
  let total_points = read_u32(data, record_offset + 28)? as usize;
  let counts_offset = record_offset + 32;
  let points_offset = counts_offset
    .checked_add(polygon_count * 4)
    .ok_or_else(|| "EMF polygon counts overflow".to_string())?;
  if points_offset > record_offset + record_size {
    return Ok(Vec::new());
  }
  let mut counts = Vec::with_capacity(polygon_count);
  for index in 0..polygon_count {
    counts.push(read_u32(data, counts_offset + index * 4)? as usize);
  }
  let Some(points) = read_points_i16(data, points_offset, total_points) else {
    return Ok(Vec::new());
  };
  Ok(split_polygons(points, counts))
}

fn split_polygons(points: Vec<EmfPoint>, counts: Vec<usize>) -> Vec<Vec<EmfPoint>> {
  let mut polygons = Vec::with_capacity(counts.len());
  let mut cursor = 0usize;
  for count in counts {
    let end = cursor.saturating_add(count).min(points.len());
    polygons.push(points[cursor..end].to_vec());
    cursor = end;
  }
  polygons
}

fn read_points_i32(data: &[u8], offset: usize, count: usize) -> Option<Vec<EmfPoint>> {
  let end = offset.checked_add(count.checked_mul(8)?)?;
  if end > data.len() {
    return None;
  }
  let mut points = Vec::with_capacity(count);
  for index in 0..count {
    let point_offset = offset + index * 8;
    points.push(EmfPoint {
      x: read_i32(data, point_offset).ok()?,
      y: read_i32(data, point_offset + 4).ok()?,
    });
  }
  Some(points)
}

fn read_points_i16(data: &[u8], offset: usize, count: usize) -> Option<Vec<EmfPoint>> {
  let end = offset.checked_add(count.checked_mul(4)?)?;
  if end > data.len() {
    return None;
  }
  let mut points = Vec::with_capacity(count);
  for index in 0..count {
    let point_offset = offset + index * 4;
    points.push(EmfPoint {
      x: i32::from(read_i16(data, point_offset).ok()?),
      y: i32::from(read_i16(data, point_offset + 2).ok()?),
    });
  }
  Some(points)
}

fn read_color_ref(data: &[u8], offset: usize) -> Result<EmfColor, String> {
  let color_ref = read_u32(data, offset)?;
  Ok(EmfColor {
    r: (color_ref & 0xff) as u8,
    g: ((color_ref >> 8) & 0xff) as u8,
    b: ((color_ref >> 16) & 0xff) as u8,
  })
}

fn read_xform(data: &[u8], offset: usize) -> Result<EmfTransform, String> {
  Ok(EmfTransform {
    m11: read_f32(data, offset)?,
    m12: read_f32(data, offset + 4)?,
    m21: read_f32(data, offset + 8)?,
    m22: read_f32(data, offset + 12)?,
    dx: read_f32(data, offset + 16)?,
    dy: read_f32(data, offset + 20)?,
  })
}

fn rgb_to_png(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
  let mut output = Vec::new();
  let encoder = PngEncoder::new(&mut output);
  encoder
    .write_image(rgb, width, height, ColorType::Rgb8.into())
    .map_err(|err| err.to_string())?;
  Ok(output)
}

// StrictLib patch: EMR_HEADER may carry the optional pixel-format and
// OpenGL fields (MS-EMF 2.3.4.2: 88, 100 or 108 bytes, and producers write
// longer ones, e.g. 124 or 156), so its size is read rather than required to be
// 108; the record walk starts at the header's own `nSize` (WQ-027).
const EMF_MIN_HEADER_SIZE: usize = 88;

fn is_emf(data: &[u8]) -> bool {
  data.len() >= EMF_MIN_HEADER_SIZE
    && matches!(read_u32(data, 0), Ok(1))
    && matches!(
      read_u32(data, 4),
      Ok(size) if (EMF_MIN_HEADER_SIZE..=data.len()).contains(&(size as usize)) && size % 4 == 0
    )
}

/// The size of the EMR_HEADER record: where the first record after it starts.
fn emf_header_size(data: &[u8]) -> usize {
  read_u32(data, 4).map_or(EMF_HEADER_SIZE, |size| size as usize)
}

fn extract_emr_ext_text_out_w(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<String> {
  let text = ext_text_record(data, record_offset, record_size)?;
  let byte_len = text.characters.checked_mul(2)?;
  let start = record_offset.checked_add(text.string_offset)?;
  let end = start.checked_add(byte_len)?;
  let bytes = data.get(start..end)?;
  let units = bytes
    .chunks_exact(2)
    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
    .collect::<Vec<_>>();
  Some(
    String::from_utf16_lossy(&units)
      .trim_end_matches('\0')
      .to_string(),
  )
}

fn extract_emr_ext_text_out_a(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<String> {
  let text = ext_text_record(data, record_offset, record_size)?;
  let start = record_offset.checked_add(text.string_offset)?;
  let end = start.checked_add(text.characters)?;
  let bytes = data.get(start..end)?;
  Some(
    bytes
      .iter()
      .take_while(|byte| **byte != 0)
      .map(|byte| char::from(*byte))
      .collect(),
  )
}

#[derive(Clone, Copy, Debug)]
struct ExtTextRecord {
  x: i32,
  y: i32,
  characters: usize,
  string_offset: usize,
}

fn ext_text_record(data: &[u8], record_offset: usize, record_size: usize) -> Option<ExtTextRecord> {
  // with rclBounds, graphics mode, scales, then EMRTEXT. EMRTEXT::offString is
  // relative to the record start.
  const EMRTEXT_OFFSET: usize = 36;
  const EMRTEXT_REFERENCE_X_OFFSET: usize = EMRTEXT_OFFSET;
  const EMRTEXT_REFERENCE_Y_OFFSET: usize = EMRTEXT_OFFSET + 4;
  const EMRTEXT_CHARS_OFFSET: usize = EMRTEXT_OFFSET + 8;
  const EMRTEXT_STRING_OFFSET: usize = EMRTEXT_OFFSET + 12;
  let minimum_size = EMRTEXT_OFFSET + 40;
  if record_size < minimum_size {
    return None;
  }
  let characters = read_u32(data, record_offset + EMRTEXT_CHARS_OFFSET).ok()? as usize;
  let string_offset = read_u32(data, record_offset + EMRTEXT_STRING_OFFSET).ok()? as usize;
  if characters == 0 || string_offset >= record_size {
    return None;
  }
  Some(ExtTextRecord {
    x: read_i32(data, record_offset + EMRTEXT_REFERENCE_X_OFFSET).ok()?,
    y: read_i32(data, record_offset + EMRTEXT_REFERENCE_Y_OFFSET).ok()?,
    characters,
    string_offset,
  })
}

fn read_logfont_object(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<(u32, EmfFont)> {
  // EMR_EXTCREATEFONTINDIRECTW reads an object index followed by LOGFONTW.
  const OBJECT_ID_OFFSET: usize = 8;
  const LOGFONT_OFFSET: usize = 12;
  const LOGFONT_HEIGHT_OFFSET: usize = LOGFONT_OFFSET;
  const LOGFONT_FACE_NAME_OFFSET: usize = LOGFONT_OFFSET + 28;
  let face_end = LOGFONT_FACE_NAME_OFFSET.checked_add(LOGFONT_FACE_NAME_CHARS * 2)?;
  if record_size < face_end {
    return None;
  }
  let object_id = read_u32(data, record_offset + OBJECT_ID_OFFSET).ok()?;
  let height = read_i32(data, record_offset + LOGFONT_HEIGHT_OFFSET).ok()?;
  // StrictLib patch: weight, italic, charset and face name for drawing.
  let mut face = [0u16; LOGFONT_FACE_NAME_CHARS];
  for (index, unit) in face.iter_mut().enumerate() {
    *unit = read_u16(data, record_offset + LOGFONT_FACE_NAME_OFFSET + index * 2).ok()?;
  }
  let text = TextFont {
    height,
    weight: u16::try_from(read_i32(data, record_offset + LOGFONT_OFFSET + 16).ok()?.clamp(0, 1000))
      .unwrap_or(400),
    italic: data.get(record_offset + LOGFONT_OFFSET + 20).is_some_and(|value| *value != 0),
    char_set: data.get(record_offset + LOGFONT_OFFSET + 23).copied().unwrap_or(TextFont::DEFAULT_CHARSET),
    face,
  };
  Some((object_id, EmfFont { height, text }))
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, String> {
  let bytes = data
    .get(offset..offset + 2)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_i16(data: &[u8], offset: usize) -> Result<i16, String> {
  let bytes = data
    .get(offset..offset + 2)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, String> {
  let bytes = data
    .get(offset..offset + 4)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_i32(data: &[u8], offset: usize) -> Result<i32, String> {
  let bytes = data
    .get(offset..offset + 4)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_f32(data: &[u8], offset: usize) -> Result<f32, String> {
  let bytes = data
    .get(offset..offset + 4)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    BitmapSourceBounds, DibColorUsage, EMR_EOF, EMR_HEADER, EmfMetafile, EmfRecord, EmfRecordData,
    EmrBitmapBuffer, EmrStretchDiBits, RectL, SdkEnumValue, SizeL,
  };

  fn minimal_header_record() -> EmfRecord {
    let mut data = vec![0; 100];
    data[8..12].copy_from_slice(&1i32.to_le_bytes());
    data[12..16].copy_from_slice(&1i32.to_le_bytes());
    data[32..36].copy_from_slice(&crate::emf::EMF_SIGNATURE.to_le_bytes());
    EmfRecord::new(EMR_HEADER, data)
  }

  fn eof_record() -> EmfRecord {
    EmfRecord::new(EMR_EOF, vec![0; 12])
  }

  fn bitmap_info(width: i32, height: i32, bit_count: u16, compression: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&bit_count.to_le_bytes());
    bytes.extend_from_slice(&compression.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
  }

  fn stretch_record(bitmap_info: Vec<u8>, bitmap_bits: Vec<u8>) -> EmfRecord {
    EmfRecordData::StretchDiBits(EmrStretchDiBits {
      bounds: RectL::default(),
      dest: crate::PointL { x: 0, y: 0 },
      source: BitmapSourceBounds {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
      },
      color_usage: DibColorUsage::RgbColors.raw(),
      raster_operation: 0x00CC_0020,
      dest_size: SizeL { cx: 2, cy: 2 },
      bitmap: EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info,
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits,
      },
      padding: Vec::new(),
    })
    .to_record()
    .unwrap()
  }

  fn metafile_with(record: EmfRecord) -> Vec<u8> {
    metafile_with_records(vec![record])
  }

  fn metafile_with_records(records: Vec<EmfRecord>) -> Vec<u8> {
    let mut all_records = Vec::with_capacity(records.len() + 2);
    all_records.push(minimal_header_record());
    all_records.extend(records);
    all_records.push(eof_record());
    EmfMetafile {
      records: all_records,
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap()
  }

  fn set_pixel_record(x: i32, y: i32, color_ref: u32) -> EmfRecord {
    let mut data = Vec::new();
    data.extend_from_slice(&x.to_le_bytes());
    data.extend_from_slice(&y.to_le_bytes());
    data.extend_from_slice(&color_ref.to_le_bytes());
    EmfRecord::new(super::EMR_SET_PIXEL_V, data)
  }

  #[test]
  fn polygon_scanlines_are_limited_to_the_visible_vertical_bounds() {
    let points = [(2.0, 10.0), (5.0, 10.0), (5.0, 12.0), (2.0, 12.0)];
    let mut spans = Vec::new();
    visit_polygon_scanline_spans(&points, 20, 10_000, |y, start, end| {
      spans.push((y, start, end));
    });

    assert_eq!(spans, [(10, 2, 5), (11, 2, 5)]);

    spans.clear();
    visit_polygon_scanline_spans(&points, 20, 8, |y, start, end| {
      spans.push((y, start, end));
    });
    assert!(spans.is_empty());
  }

  #[test]
  fn axis_aligned_polygon_clip_uses_the_same_pixel_center_bounds() {
    let points = [(2.2, 10.8), (5.2, 10.8), (5.2, 13.2), (2.2, 13.2)];
    assert_eq!(
      axis_aligned_clip_rect(&points, 20, 20),
      Some((2, 11, 6, 13))
    );

    let rotated = [(2.0, 3.0), (4.0, 2.0), (5.0, 4.0), (3.0, 5.0)];
    assert_eq!(axis_aligned_clip_rect(&rotated, 20, 20), None);
  }

  #[test]
  fn rectangle_clip_intersection_preserves_empty_regions() {
    assert_eq!(intersect_rects((1, 2, 8, 9), (4, 0, 10, 6)), (4, 2, 8, 6));
    assert_eq!(intersect_rects((1, 1, 2, 2), (4, 4, 5, 5)), (4, 4, 4, 4));
  }

  #[test]
  fn decode_emf_embedded_png_bitmap() {
    let emf = metafile_with(stretch_record(
      bitmap_info(2, 2, 0, BI_PNG),
      vec![0x89, b'P', b'N', b'G'],
    ));

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    assert_eq!(decoded.content_type, "image/png");
    assert_eq!(decoded.data, [0x89, b'P', b'N', b'G']);
  }

  #[test]
  fn decode_emf_bi_rgb_bitmap_as_png() {
    let bits = vec![
      0, 0, 255, 0, 255, 0, 0, 0, // bottom row: red, green, padding
      255, 0, 0, 255, 255, 255, 0, 0, // top row: blue, white, padding
    ];
    let emf = metafile_with(stretch_record(bitmap_info(2, 2, 24, BI_RGB), bits));

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    assert_eq!(decoded.content_type, "image/png");
    assert!(decoded.data.starts_with(&[0x89, b'P', b'N', b'G']));
  }

  #[test]
  fn decode_emf_replays_bitmap_with_later_vector_records() {
    let bits = vec![
      255, 255, 255, 255, 255, 255, 0, 0, // bottom row: white, white, padding
      255, 255, 255, 255, 255, 255, 0, 0, // top row: white, white, padding
    ];
    let emf = metafile_with_records(vec![
      stretch_record(bitmap_info(2, 2, 24, BI_RGB), bits),
      set_pixel_record(0, 0, 0x0000_00ff),
    ]);

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    assert_eq!(decoded.content_type, "image/png");
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
  }

  #[test]
  fn non_metafile_returns_none() {
    assert!(
      decode_metafile_as_raster(b"not a metafile", None)
        .unwrap()
        .is_none()
    );
  }
}
