use bitflags::bitflags;
use emfsdk_derive::{SdkEnum, SdkObject};

use crate::common::{Error, Reader, Result, SdkEnumValue, SdkRead, SdkSize, SdkWrite, Writer};
use crate::string::{SdkEncoding, SdkString};
use crate::types::{EmfPlusArgb, PointF, PointL, PointS, RectF, RectL, XForm};

pub const EMFPLUS_METAFILE_SIGNATURE: u32 = 0xDBC01;
pub const EMFPLUS_BLUR_EFFECT_GUID: [u8; 16] = [
  0xA4, 0x80, 0x3C, 0x63, 0x43, 0x18, 0x2B, 0x48, 0x9E, 0xF2, 0xBE, 0x28, 0x34, 0xC5, 0xFD, 0xD4,
];
pub const EMFPLUS_BRIGHTNESS_CONTRAST_EFFECT_GUID: [u8; 16] = [
  0xE1, 0xDB, 0xA1, 0xD3, 0xC4, 0x8E, 0x17, 0x4C, 0x9F, 0x4C, 0xEA, 0x97, 0xAD, 0x1C, 0x34, 0x3D,
];
pub const EMFPLUS_COLOR_BALANCE_EFFECT_GUID: [u8; 16] = [
  0x7D, 0x59, 0x7E, 0x53, 0x1E, 0x25, 0xDA, 0x48, 0x96, 0x64, 0x29, 0xCA, 0x49, 0x6B, 0x70, 0xF8,
];
pub const EMFPLUS_COLOR_CURVE_EFFECT_GUID: [u8; 16] = [
  0x22, 0x00, 0x6A, 0xDD, 0xE4, 0x58, 0x67, 0x4A, 0x9D, 0x9B, 0xD4, 0x8E, 0xB8, 0x81, 0xA5, 0x3D,
];
pub const EMFPLUS_COLOR_LOOKUP_TABLE_EFFECT_GUID: [u8; 16] = [
  0xA9, 0x72, 0xCE, 0xA7, 0x7F, 0x0F, 0xD7, 0x40, 0xB3, 0xCC, 0xD0, 0xC0, 0x2D, 0x5C, 0x32, 0x12,
];
pub const EMFPLUS_COLOR_MATRIX_EFFECT_GUID: [u8; 16] = [
  0x15, 0x26, 0x8F, 0x71, 0x33, 0x79, 0xE3, 0x40, 0xA5, 0x11, 0x5F, 0x68, 0xFE, 0x14, 0xDD, 0x74,
];
pub const EMFPLUS_HUE_SATURATION_LIGHTNESS_EFFECT_GUID: [u8; 16] = [
  0xC3, 0xD6, 0x2D, 0x8B, 0x07, 0xEB, 0x87, 0x4D, 0xA5, 0xF0, 0x71, 0x08, 0xE2, 0x6A, 0x9C, 0x5F,
];
pub const EMFPLUS_LEVELS_EFFECT_GUID: [u8; 16] = [
  0xEC, 0x54, 0xC3, 0x99, 0x31, 0x2A, 0x3A, 0x4F, 0x8C, 0x34, 0x17, 0xA8, 0x03, 0xB3, 0x3A, 0x25,
];
pub const EMFPLUS_RED_EYE_CORRECTION_EFFECT_GUID: [u8; 16] = [
  0x05, 0x9D, 0xD2, 0x74, 0xA4, 0x69, 0x66, 0x42, 0x95, 0x49, 0x3C, 0xC5, 0x28, 0x36, 0xB6, 0x32,
];
pub const EMFPLUS_SHARPEN_EFFECT_GUID: [u8; 16] = [
  0xEE, 0xF3, 0xCB, 0x63, 0x26, 0xC5, 0x2C, 0x40, 0x8F, 0x71, 0x62, 0xC5, 0x40, 0xBF, 0x51, 0x42,
];
pub const EMFPLUS_TINT_EFFECT_GUID: [u8; 16] = [
  0x00, 0xAF, 0x77, 0x10, 0x48, 0x28, 0x41, 0x44, 0x94, 0x89, 0x44, 0xAD, 0x4C, 0x2D, 0x7A, 0x2C,
];
const EMFPLUS_TS_CLIP_MAX_RECTS: u16 = 0x7FFF;

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusRecordFlags: u16 {
        const OBJECT_ID_MASK = 0x00FF;
        const RELATIVE_POSITION = 0x0800;
        const POST_MULTIPLY = 0x2000;
        const EFFECT = 0x2000;
        const CLOSE_SHAPE = 0x2000;
        const WINDING_FILL = 0x2000;
        const COMPRESSED = 0x4000;
        const SOLID_COLOR = 0x8000;
        const TS_GRAPHICS_PALETTE = 0x0001;
        const TS_GRAPHICS_BASIC_VGA = 0x0002;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusFontStyleFlags: u32 {
        const BOLD = 0x0000_0001;
        const ITALIC = 0x0000_0002;
        const UNDERLINE = 0x0000_0004;
        const STRIKEOUT = 0x0000_0008;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusBrushDataFlags: u32 {
        const PATH = 0x0000_0001;
        const TRANSFORM = 0x0000_0002;
        const PRESET_COLORS = 0x0000_0004;
        const BLEND_FACTORS_H = 0x0000_0008;
        const BLEND_FACTORS_V = 0x0000_0010;
        const FOCUS_SCALES = 0x0000_0040;
        const IS_GAMMA_CORRECTED = 0x0000_0080;
        const DO_NOT_TRANSFORM = 0x0000_0100;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusPenDataFlags: u32 {
        const TRANSFORM = 0x0000_0001;
        const START_CAP = 0x0000_0002;
        const END_CAP = 0x0000_0004;
        const JOIN = 0x0000_0008;
        const MITER_LIMIT = 0x0000_0010;
        const LINE_STYLE = 0x0000_0020;
        const DASHED_LINE_CAP = 0x0000_0040;
        const DASHED_LINE_OFFSET = 0x0000_0080;
        const DASHED_LINE = 0x0000_0100;
        const NON_CENTER = 0x0000_0200;
        const COMPOUND_LINE = 0x0000_0400;
        const CUSTOM_START_CAP = 0x0000_0800;
        const CUSTOM_END_CAP = 0x0000_1000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusCustomLineCapDataFlags: u32 {
        const FILL_PATH = 0x0000_0001;
        const LINE_PATH = 0x0000_0002;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusDriverStringOptionsFlags: u32 {
        const CMAP_LOOKUP = 0x0000_0001;
        const VERTICAL = 0x0000_0002;
        const REALIZED_ADVANCE = 0x0000_0004;
        const LIMIT_SUBPIXEL = 0x0000_0008;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusPathPointTypeFlags: u8 {
        const DASH_MODE = 0x01;
        const PATH_MARKER = 0x02;
        const CLOSE_SUBPATH = 0x08;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusStringFormatFlags: u32 {
        const DIRECTION_RIGHT_TO_LEFT = 0x0000_0001;
        const DIRECTION_VERTICAL = 0x0000_0002;
        const NO_FIT_BLACK_BOX = 0x0000_0004;
        const DISPLAY_FORMAT_CONTROL = 0x0000_0020;
        const NO_FONT_FALLBACK = 0x0000_0400;
        const MEASURE_TRAILING_SPACES = 0x0000_0800;
        const NO_WRAP = 0x0000_1000;
        const LINE_LIMIT = 0x0000_2000;
        const NO_CLIP = 0x0000_4000;
        const BYPASS_GDI = 0x8000_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmfPlusPaletteStyleFlags: u32 {
        const HAS_ALPHA = 0x0000_0001;
        const GRAYSCALE = 0x0000_0002;
        const HALFTONE = 0x0000_0004;
    }
}

impl EmfPlusRecordFlags {
  pub fn header_dual(self) -> bool {
    self.bits() & 0x0001 != 0
  }

  pub fn header_reserved_bits(self) -> u16 {
    self.bits() & !0x0001
  }

  pub fn object_id(self) -> u8 {
    (self.bits() & Self::OBJECT_ID_MASK.bits()) as u8
  }

  pub fn combine_mode_raw(self) -> u8 {
    ((self.bits() >> 8) & 0x0F) as u8
  }

  pub fn combine_mode(self) -> Option<EmfPlusCombineMode> {
    EmfPlusCombineMode::from_raw(u32::from(self.combine_mode_raw()))
  }

  pub fn property_mode_raw(self) -> u8 {
    (self.bits() & 0x00FF) as u8
  }

  pub fn anti_alias_enabled(self) -> bool {
    self.bits() & 0x0001 != 0
  }

  pub fn smoothing_mode_raw(self) -> u8 {
    ((self.bits() >> 1) & 0x7F) as u8
  }

  pub fn smoothing_mode(self) -> Option<EmfPlusSmoothingMode> {
    EmfPlusSmoothingMode::from_raw(u32::from(self.smoothing_mode_raw()))
  }

  pub fn compositing_mode(self) -> Option<EmfPlusCompositingMode> {
    EmfPlusCompositingMode::from_raw(u32::from(self.property_mode_raw()))
  }

  pub fn compositing_quality(self) -> Option<EmfPlusCompositingQuality> {
    EmfPlusCompositingQuality::from_raw(u32::from(self.property_mode_raw()))
  }

  pub fn interpolation_mode(self) -> Option<EmfPlusInterpolationMode> {
    EmfPlusInterpolationMode::from_raw(u32::from(self.property_mode_raw()))
  }

  pub fn pixel_offset_mode(self) -> Option<EmfPlusPixelOffsetMode> {
    EmfPlusPixelOffsetMode::from_raw(u32::from(self.property_mode_raw()))
  }

  pub fn text_contrast(self) -> u16 {
    self.bits() & 0x0FFF
  }

  pub fn text_rendering_hint(self) -> Option<EmfPlusTextRenderingHint> {
    EmfPlusTextRenderingHint::from_raw(u32::from(self.property_mode_raw()))
  }

  pub fn page_unit_raw(self) -> u8 {
    self.property_mode_raw()
  }

  pub fn page_unit(self) -> Option<EmfPlusUnitType> {
    EmfPlusUnitType::from_raw(u32::from(self.page_unit_raw()))
  }

  pub fn ts_graphics_palette_present(self) -> bool {
    self.contains(Self::TS_GRAPHICS_PALETTE)
  }

  pub fn ts_graphics_basic_vga(self) -> bool {
    self.contains(Self::TS_GRAPHICS_BASIC_VGA)
  }

  pub fn object_type_raw(self) -> u8 {
    ((self.bits() >> 8) & 0x7F) as u8
  }

  pub fn object_type(self) -> Option<EmfPlusObjectType> {
    EmfPlusObjectType::from_raw(u16::from(self.object_type_raw()))
  }

  pub fn object_continues(self) -> bool {
    self.bits() & 0x8000 != 0
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum EmfPlusRecordType {
  Header = 0x4001,
  Eof = 0x4002,
  Comment = 0x4003,
  GetDc = 0x4004,
  MultiFormatStart = 0x4005,
  MultiFormatSection = 0x4006,
  MultiFormatEnd = 0x4007,
  Object = 0x4008,
  Clear = 0x4009,
  FillRects = 0x400A,
  DrawRects = 0x400B,
  FillPolygon = 0x400C,
  DrawLines = 0x400D,
  FillEllipse = 0x400E,
  DrawEllipse = 0x400F,
  FillPie = 0x4010,
  DrawPie = 0x4011,
  DrawArc = 0x4012,
  FillRegion = 0x4013,
  FillPath = 0x4014,
  DrawPath = 0x4015,
  FillClosedCurve = 0x4016,
  DrawClosedCurve = 0x4017,
  DrawCurve = 0x4018,
  DrawBeziers = 0x4019,
  DrawImage = 0x401A,
  DrawImagePoints = 0x401B,
  DrawString = 0x401C,
  SetRenderingOrigin = 0x401D,
  SetAntiAliasMode = 0x401E,
  SetTextRenderingHint = 0x401F,
  SetTextContrast = 0x4020,
  SetInterpolationMode = 0x4021,
  SetPixelOffsetMode = 0x4022,
  SetCompositingMode = 0x4023,
  SetCompositingQuality = 0x4024,
  Save = 0x4025,
  Restore = 0x4026,
  BeginContainer = 0x4027,
  BeginContainerNoParams = 0x4028,
  EndContainer = 0x4029,
  SetWorldTransform = 0x402A,
  ResetWorldTransform = 0x402B,
  MultiplyWorldTransform = 0x402C,
  TranslateWorldTransform = 0x402D,
  ScaleWorldTransform = 0x402E,
  RotateWorldTransform = 0x402F,
  SetPageTransform = 0x4030,
  ResetClip = 0x4031,
  SetClipRect = 0x4032,
  SetClipPath = 0x4033,
  SetClipRegion = 0x4034,
  OffsetClip = 0x4035,
  DrawDriverString = 0x4036,
  StrokeFillPath = 0x4037,
  SerializableObject = 0x4038,
  SetTsGraphics = 0x4039,
  SetTsClip = 0x403A,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum EmfPlusGraphicsVersionValue {
  Version1 = 0x0001,
  Version1_1 = 0x0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum EmfPlusObjectType {
  Invalid = 0x0000,
  Brush = 0x0001,
  Pen = 0x0002,
  Path = 0x0003,
  Region = 0x0004,
  Image = 0x0005,
  Font = 0x0006,
  StringFormat = 0x0007,
  ImageAttributes = 0x0008,
  CustomLineCap = 0x0009,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusBrushType {
  SolidColor = 0x0000_0000,
  HatchFill = 0x0000_0001,
  TextureFill = 0x0000_0002,
  PathGradient = 0x0000_0003,
  LinearGradient = 0x0000_0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusCombineMode {
  Replace = 0x0000_0000,
  Intersect = 0x0000_0001,
  Union = 0x0000_0002,
  Xor = 0x0000_0003,
  Exclude = 0x0000_0004,
  Complement = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusCompositingMode {
  SourceOver = 0x0000_0000,
  SourceCopy = 0x0000_0001,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusCompositingQuality {
  Default = 0x0000_0001,
  HighSpeed = 0x0000_0002,
  HighQuality = 0x0000_0003,
  GammaCorrected = 0x0000_0004,
  AssumeLinear = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusHatchStyle {
  Horizontal = 0x0000_0000,
  Vertical = 0x0000_0001,
  ForwardDiagonal = 0x0000_0002,
  BackwardDiagonal = 0x0000_0003,
  LargeGrid = 0x0000_0004,
  DiagonalCross = 0x0000_0005,
  Percent05 = 0x0000_0006,
  Percent10 = 0x0000_0007,
  Percent20 = 0x0000_0008,
  Percent25 = 0x0000_0009,
  Percent30 = 0x0000_000A,
  Percent40 = 0x0000_000B,
  Percent50 = 0x0000_000C,
  Percent60 = 0x0000_000D,
  Percent70 = 0x0000_000E,
  Percent75 = 0x0000_000F,
  Percent80 = 0x0000_0010,
  Percent90 = 0x0000_0011,
  LightDownwardDiagonal = 0x0000_0012,
  LightUpwardDiagonal = 0x0000_0013,
  DarkDownwardDiagonal = 0x0000_0014,
  DarkUpwardDiagonal = 0x0000_0015,
  WideDownwardDiagonal = 0x0000_0016,
  WideUpwardDiagonal = 0x0000_0017,
  LightVertical = 0x0000_0018,
  LightHorizontal = 0x0000_0019,
  NarrowVertical = 0x0000_001A,
  NarrowHorizontal = 0x0000_001B,
  DarkVertical = 0x0000_001C,
  DarkHorizontal = 0x0000_001D,
  DashedDownwardDiagonal = 0x0000_001E,
  DashedUpwardDiagonal = 0x0000_001F,
  DashedHorizontal = 0x0000_0020,
  DashedVertical = 0x0000_0021,
  SmallConfetti = 0x0000_0022,
  LargeConfetti = 0x0000_0023,
  ZigZag = 0x0000_0024,
  Wave = 0x0000_0025,
  DiagonalBrick = 0x0000_0026,
  HorizontalBrick = 0x0000_0027,
  Weave = 0x0000_0028,
  Plaid = 0x0000_0029,
  Divot = 0x0000_002A,
  DottedGrid = 0x0000_002B,
  DottedDiamond = 0x0000_002C,
  Shingle = 0x0000_002D,
  Trellis = 0x0000_002E,
  Sphere = 0x0000_002F,
  SmallGrid = 0x0000_0030,
  SmallCheckerBoard = 0x0000_0031,
  LargeCheckerBoard = 0x0000_0032,
  OutlinedDiamond = 0x0000_0033,
  SolidDiamond = 0x0000_0034,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusBitmapDataType {
  Pixel = 0x0000_0000,
  Compressed = 0x0000_0001,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusCustomLineCapDataType {
  Default = 0x0000_0000,
  AdjustableArrow = 0x0000_0001,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusCurveAdjustment {
  Exposure = 0x0000_0000,
  Density = 0x0000_0001,
  Contrast = 0x0000_0002,
  Highlight = 0x0000_0003,
  Shadow = 0x0000_0004,
  Midtone = 0x0000_0005,
  WhiteSaturation = 0x0000_0006,
  BlackSaturation = 0x0000_0007,
}

pub type EmfPlusCurveAdjustments = EmfPlusCurveAdjustment;
pub type EmfPlusARGB = EmfPlusArgb;
pub type EmfPlusCompressedImage = EmfPlusCompressedImageObject;
pub type EmfPlusFlags = EmfPlusRecordFlags;
pub type EmfPlusMetafileData = EmfPlusMetafileObject;
pub type EmfPlusPathPointTypeRLE = EmfPlusPathPointTypeRle;
pub type EmfPlusPoint = PointS;
pub type EmfPlusPointF = PointF;
pub type EmfPlusRectF = RectF;
pub type EmfPlusTransformMatrix = XForm;

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusCurveChannel {
  All = 0x0000_0000,
  Red = 0x0000_0001,
  Green = 0x0000_0002,
  Blue = 0x0000_0003,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusFilterType {
  None = 0x0000_0000,
  Point = 0x0000_0001,
  Linear = 0x0000_0002,
  Triangle = 0x0000_0003,
  Box = 0x0000_0004,
  PyramidalQuad = 0x0000_0006,
  GaussianQuad = 0x0000_0007,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusDashedLineCapType {
  Flat = 0x0000_0000,
  Round = 0x0000_0002,
  Triangle = 0x0000_0003,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusLineCapType {
  Flat = 0x0000_0000,
  Square = 0x0000_0001,
  Round = 0x0000_0002,
  Triangle = 0x0000_0003,
  NoAnchor = 0x0000_0010,
  SquareAnchor = 0x0000_0011,
  RoundAnchor = 0x0000_0012,
  DiamondAnchor = 0x0000_0013,
  ArrowAnchor = 0x0000_0014,
  AnchorMask = 0x0000_00F0,
  Custom = 0x0000_00FF,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusLineJoinType {
  Miter = 0x0000_0000,
  Bevel = 0x0000_0001,
  Round = 0x0000_0002,
  MiterClipped = 0x0000_0003,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusLineStyle {
  Solid = 0x0000_0000,
  Dash = 0x0000_0001,
  Dot = 0x0000_0002,
  DashDot = 0x0000_0003,
  DashDotDot = 0x0000_0004,
  Custom = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusImageDataType {
  Unknown = 0x0000_0000,
  Bitmap = 0x0000_0001,
  Metafile = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusHotkeyPrefix {
  None = 0x0000_0000,
  Show = 0x0000_0001,
  Hide = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusInterpolationMode {
  Default = 0x0000_0000,
  LowQuality = 0x0000_0001,
  HighQuality = 0x0000_0002,
  Bilinear = 0x0000_0003,
  Bicubic = 0x0000_0004,
  NearestNeighbor = 0x0000_0005,
  HighQualityBilinear = 0x0000_0006,
  HighQualityBicubic = 0x0000_0007,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusMetafileDataType {
  Wmf = 0x0000_0001,
  WmfPlaceable = 0x0000_0002,
  Emf = 0x0000_0003,
  EmfPlusOnly = 0x0000_0004,
  EmfPlusDual = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusObjectClamp {
  RectClamp = 0x0000_0000,
  BitmapClamp = 0x0000_0001,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusPixelFormat {
  Undefined = 0x0000_0000,
  Format1bppIndexed = 0x0003_0101,
  Format4bppIndexed = 0x0003_0402,
  Format8bppIndexed = 0x0003_0803,
  Format16bppGrayScale = 0x0010_1004,
  Format16bppRgb555 = 0x0002_1005,
  Format16bppRgb565 = 0x0002_1006,
  Format16bppArgb1555 = 0x0006_1007,
  Format24bppRgb = 0x0002_1808,
  Format32bppRgb = 0x0002_2009,
  Format32bppArgb = 0x0026_200A,
  Format32bppPArgb = 0x000E_200B,
  Format48bppRgb = 0x0010_300C,
  Format64bppArgb = 0x0034_400D,
  Format64bppPArgb = 0x001A_400E,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmfPlusPixelFormatValue {
  pub raw: u32,
}

impl EmfPlusPixelFormatValue {
  const KNOWN_BITS_MASK: u32 = 0x003F_FFFF;

  pub const fn new(raw: u32) -> Self {
    Self { raw }
  }

  pub fn kind(self) -> Option<EmfPlusPixelFormat> {
    EmfPlusPixelFormat::from_raw(self.raw & Self::KNOWN_BITS_MASK)
  }

  pub const fn index(self) -> u8 {
    (self.raw & 0x0000_00FF) as u8
  }

  pub const fn bits_per_pixel(self) -> u8 {
    ((self.raw & 0x0000_FF00) >> 8) as u8
  }

  pub const fn is_indexed(self) -> bool {
    self.raw & 0x0001_0000 != 0
  }

  pub const fn is_gdi(self) -> bool {
    self.raw & 0x0002_0000 != 0
  }

  pub const fn has_alpha(self) -> bool {
    self.raw & 0x0004_0000 != 0
  }

  pub const fn is_pre_multiplied_alpha(self) -> bool {
    self.raw & 0x0008_0000 != 0
  }

  pub const fn is_extended(self) -> bool {
    self.raw & 0x0010_0000 != 0
  }

  pub const fn is_canonical(self) -> bool {
    self.raw & 0x0020_0000 != 0
  }

  pub const fn reserved_bits(self) -> u32 {
    self.raw & !Self::KNOWN_BITS_MASK
  }
}

impl EmfPlusPixelFormat {
  pub fn pixel_format_index(self) -> u8 {
    self.value().index()
  }

  pub fn bits_per_pixel(self) -> u8 {
    self.value().bits_per_pixel()
  }

  pub fn is_indexed(self) -> bool {
    self.value().is_indexed()
  }

  pub fn is_gdi(self) -> bool {
    self.value().is_gdi()
  }

  pub fn has_alpha(self) -> bool {
    self.value().has_alpha()
  }

  pub fn is_pre_multiplied_alpha(self) -> bool {
    self.value().is_pre_multiplied_alpha()
  }

  pub fn is_extended(self) -> bool {
    self.value().is_extended()
  }

  pub fn is_canonical(self) -> bool {
    self.value().is_canonical()
  }

  pub const fn value(self) -> EmfPlusPixelFormatValue {
    EmfPlusPixelFormatValue { raw: self as u32 }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmfPlusPenAlignment {
  Center = 0x0000_0000,
  Inset = 0x0000_0001,
  Left = 0x0000_0002,
  Outset = 0x0000_0003,
  Right = 0x0000_0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusPixelOffsetMode {
  Default = 0x0000_0000,
  HighSpeed = 0x0000_0001,
  HighQuality = 0x0000_0002,
  None = 0x0000_0003,
  Half = 0x0000_0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusRegionNodeDataType {
  And = 0x0000_0001,
  Or = 0x0000_0002,
  Xor = 0x0000_0003,
  Exclude = 0x0000_0004,
  Complement = 0x0000_0005,
  Rect = 0x1000_0000,
  Path = 0x1000_0001,
  Empty = 0x1000_0002,
  Infinite = 0x1000_0003,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmfPlusPathPointType {
  Start = 0x00,
  Line = 0x01,
  Bezier = 0x03,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusStringAlignment {
  Near = 0x0000_0000,
  Center = 0x0000_0001,
  Far = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusStringDigitSubstitution {
  User = 0x0000_0000,
  None = 0x0000_0001,
  National = 0x0000_0002,
  Traditional = 0x0000_0003,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusStringTrimming {
  None = 0x0000_0000,
  Character = 0x0000_0001,
  Word = 0x0000_0002,
  EllipsisCharacter = 0x0000_0003,
  EllipsisWord = 0x0000_0004,
  EllipsisPath = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusSmoothingMode {
  Default = 0x0000_0000,
  HighSpeed = 0x0000_0001,
  HighQuality = 0x0000_0002,
  None = 0x0000_0003,
  AntiAlias8x4 = 0x0000_0004,
  AntiAlias8x8 = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusTextRenderingHint {
  SystemDefault = 0x0000_0000,
  SingleBitPerPixelGridFit = 0x0000_0001,
  SingleBitPerPixel = 0x0000_0002,
  AntiAliasGridFit = 0x0000_0003,
  AntiAlias = 0x0000_0004,
  ClearTypeGridFit = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusUnitType {
  World = 0x0000_0000,
  Display = 0x0000_0001,
  Pixel = 0x0000_0002,
  Point = 0x0000_0003,
  Inch = 0x0000_0004,
  Document = 0x0000_0005,
  Millimeter = 0x0000_0006,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfPlusWrapMode {
  Tile = 0x0000_0000,
  TileFlipX = 0x0000_0001,
  TileFlipY = 0x0000_0002,
  TileFlipXY = 0x0000_0003,
  Clamp = 0x0000_0004,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emf_plus_graphics_version")]
pub struct EmfPlusGraphicsVersion {
  pub value: u32,
}

impl EmfPlusGraphicsVersion {
  pub fn from_parts(metafile_signature: u32, graphics_version: u16) -> Result<Self> {
    if metafile_signature > 0x000F_FFFF {
      return Err(Error::invalid(
        0,
        "EmfPlusGraphicsVersion MetafileSignature exceeds 20 bits",
      ));
    }
    if graphics_version > 0x0FFF {
      return Err(Error::invalid(
        0,
        "EmfPlusGraphicsVersion GraphicsVersion exceeds 12 bits",
      ));
    }
    Ok(Self {
      value: (metafile_signature << 12) | u32::from(graphics_version),
    })
  }

  pub fn from_graphics_version(graphics_version: EmfPlusGraphicsVersionValue) -> Self {
    Self {
      value: (EMFPLUS_METAFILE_SIGNATURE << 12) | u32::from(graphics_version.raw()),
    }
  }

  pub fn metafile_signature(&self) -> u32 {
    self.value >> 12
  }

  pub fn is_emf_plus_signature(&self) -> bool {
    self.metafile_signature() == EMFPLUS_METAFILE_SIGNATURE
  }

  pub fn graphics_version_raw(&self) -> u16 {
    (self.value & 0x0FFF) as u16
  }

  pub fn graphics_version(&self) -> Option<EmfPlusGraphicsVersionValue> {
    EmfPlusGraphicsVersionValue::from_raw(self.graphics_version_raw())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emf_plus_header_data")]
pub struct EmfPlusHeaderData {
  pub graphics_version: EmfPlusGraphicsVersion,
  pub emf_plus_flags: u32,
  pub logical_dpi_x: u32,
  pub logical_dpi_y: u32,
}

impl EmfPlusHeaderData {
  pub fn video_display(&self) -> bool {
    self.emf_plus_flags & 0x0000_0001 != 0
  }

  pub fn emf_plus_reserved_flags(&self) -> u32 {
    self.emf_plus_flags & !0x0000_0001
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusRectS {
  pub x: i16,
  pub y: i16,
  pub width: i16,
  pub height: i16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EmfPlusRect {
  Compressed(EmfPlusRectS),
  Float(RectF),
}

impl EmfPlusRect {
  pub fn sdk_size(&self) -> u64 {
    match self {
      Self::Compressed(value) => value.sdk_size(),
      Self::Float(value) => value.sdk_size(),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusPointR {
  pub x: i16,
  pub y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusInteger7 {
  pub value: i8,
}

impl TryFrom<i16> for EmfPlusInteger7 {
  type Error = Error;

  fn try_from(value: i16) -> Result<Self> {
    if (-64..=63).contains(&value) {
      Ok(Self { value: value as i8 })
    } else {
      Err(Error::invalid(0, "EmfPlusInteger7 is outside -64..=63"))
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusInteger15 {
  pub value: i16,
}

impl TryFrom<i16> for EmfPlusInteger15 {
  type Error = Error;

  fn try_from(value: i16) -> Result<Self> {
    if (-16_384..=16_383).contains(&value) {
      Ok(Self { value })
    } else {
      Err(Error::invalid(
        0,
        "EmfPlusInteger15 is outside -16384..=16383",
      ))
    }
  }
}

impl EmfPlusPointR {
  pub fn sdk_size(&self) -> u64 {
    emf_plus_integer_size(self.x) + emf_plus_integer_size(self.y)
  }

  pub fn x_integer7(&self) -> Option<EmfPlusInteger7> {
    EmfPlusInteger7::try_from(self.x).ok()
  }

  pub fn y_integer7(&self) -> Option<EmfPlusInteger7> {
    EmfPlusInteger7::try_from(self.y).ok()
  }

  pub fn x_integer15(&self) -> EmfPlusInteger15 {
    EmfPlusInteger15 { value: self.x }
  }

  pub fn y_integer15(&self) -> EmfPlusInteger15 {
    EmfPlusInteger15 { value: self.y }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusPointData {
  Relative(Vec<EmfPlusPointR>),
  Compressed(Vec<PointS>),
  Float(Vec<PointF>),
}

impl EmfPlusPointData {
  pub fn len(&self) -> usize {
    match self {
      Self::Relative(points) => points.len(),
      Self::Compressed(points) => points.len(),
      Self::Float(points) => points.len(),
    }
  }

  pub fn sdk_size(&self) -> u64 {
    match self {
      Self::Relative(points) => points.iter().map(EmfPlusPointR::sdk_size).sum(),
      Self::Compressed(points) => points.iter().map(SdkSize::sdk_size).sum(),
      Self::Float(points) => points.iter().map(SdkSize::sdk_size).sum(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmfPlusBrushRef {
  ObjectId(u32),
  Color(EmfPlusArgb),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFillRectsData {
  pub brush: EmfPlusBrushRef,
  pub rects: Vec<EmfPlusRect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawRectsData {
  pub pen_id: u8,
  pub rects: Vec<EmfPlusRect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawPointsData {
  pub pen_id: u8,
  pub points: EmfPlusPointData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawLinesData {
  pub pen_id: u8,
  pub close_shape: bool,
  pub points: EmfPlusPointData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFillPolygonData {
  pub brush: EmfPlusBrushRef,
  pub points: EmfPlusPointData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawCurveData {
  pub pen_id: u8,
  pub tension: f32,
  pub offset: u32,
  pub num_segments: u32,
  pub points: EmfPlusPointData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusClosedCurveData {
  pub pen_id: u8,
  pub tension: f32,
  pub points: EmfPlusPointData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFillClosedCurveData {
  pub brush: EmfPlusBrushRef,
  pub winding_fill: bool,
  pub tension: f32,
  pub points: EmfPlusPointData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawRectShapeData {
  pub pen_id: u8,
  pub rect: EmfPlusRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFillRectShapeData {
  pub brush: EmfPlusBrushRef,
  pub rect: EmfPlusRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawArcData {
  pub pen_id: u8,
  pub start_angle: f32,
  pub sweep_angle: f32,
  pub rect: EmfPlusRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFillPieData {
  pub brush: EmfPlusBrushRef,
  pub start_angle: f32,
  pub sweep_angle: f32,
  pub rect: EmfPlusRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusBrushObjectData {
  pub object_id: u8,
  pub brush: EmfPlusBrushRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusDrawObjectData {
  pub object_id: u8,
  pub pen_id: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusRawRecordData {
  pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusObjectRecordData {
  pub object_id: u8,
  pub object_type_raw: u8,
  pub continues: bool,
  pub total_object_size: Option<u32>,
  pub object_data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmfPlusObjectAssembler {
  pending: Option<EmfPlusPendingObject>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EmfPlusPendingObject {
  object_id: u8,
  object_type_raw: u8,
  total_object_size: usize,
  object_data: Vec<u8>,
}

impl EmfPlusObjectAssembler {
  pub fn push(
    &mut self,
    fragment: EmfPlusObjectRecordData,
  ) -> Result<Option<EmfPlusObjectRecordData>> {
    validate_emf_plus_object_fragment(&fragment)?;

    if self.pending.is_none() {
      if !fragment.continues {
        fragment.parse_object_data()?;
        return Ok(Some(fragment));
      }
      let total_object_size = fragment.total_object_size.ok_or_else(|| {
        Error::invalid(
          0,
          "EmfPlusObject continued fragment is missing TotalObjectSize",
        )
      })? as usize;
      if fragment.object_data.len() >= total_object_size {
        return Err(Error::invalid(
          0,
          "EmfPlusObject continuation reaches TotalObjectSize before its final fragment",
        ));
      }
      self.pending = Some(EmfPlusPendingObject {
        object_id: fragment.object_id,
        object_type_raw: fragment.object_type_raw,
        total_object_size,
        object_data: fragment.object_data,
      });
      return Ok(None);
    }

    let pending = self.pending.as_mut().expect("pending object checked above");
    if fragment.object_id != pending.object_id
      || fragment.object_type_raw != pending.object_type_raw
    {
      return Err(Error::invalid(
        0,
        "EmfPlusObject continuation ObjectID or ObjectType changed",
      ));
    }
    if fragment.continues
      && fragment.total_object_size.map(|value| value as usize) != Some(pending.total_object_size)
    {
      return Err(Error::invalid(
        0,
        "EmfPlusObject continuation TotalObjectSize changed",
      ));
    }
    pending.object_data.extend_from_slice(&fragment.object_data);
    if pending.object_data.len() > pending.total_object_size {
      return Err(Error::invalid(
        0,
        "EmfPlusObject continuation exceeds TotalObjectSize",
      ));
    }
    if fragment.continues {
      if pending.object_data.len() == pending.total_object_size {
        return Err(Error::invalid(
          0,
          "EmfPlusObject continuation reaches TotalObjectSize before its final fragment",
        ));
      }
      return Ok(None);
    }
    if pending.object_data.len() != pending.total_object_size {
      return Err(Error::invalid(
        0,
        "EmfPlusObject final fragment does not reach TotalObjectSize",
      ));
    }

    let pending = self.pending.take().expect("pending object checked above");
    let complete = EmfPlusObjectRecordData {
      object_id: pending.object_id,
      object_type_raw: pending.object_type_raw,
      continues: false,
      total_object_size: None,
      object_data: pending.object_data,
    };
    complete.parse_object_data()?;
    Ok(Some(complete))
  }

  pub fn finish(&self) -> Result<()> {
    if self.pending.is_some() {
      Err(Error::invalid(
        0,
        "EmfPlusObject continuation is missing its final fragment",
      ))
    } else {
      Ok(())
    }
  }
}

impl EmfPlusObjectRecordData {
  pub fn object_type(&self) -> Option<EmfPlusObjectType> {
    EmfPlusObjectType::from_raw(u16::from(self.object_type_raw))
  }

  pub fn from_typed_data(object_id: u8, data: &EmfPlusObjectData) -> Result<Self> {
    validate_object_id_u8(object_id, "EmfPlusObject ObjectID")?;
    Ok(Self {
      object_id,
      object_type_raw: data.object_type_raw(),
      continues: false,
      total_object_size: None,
      object_data: data.to_bytes()?,
    })
  }

  pub fn set_typed_data(&mut self, data: &EmfPlusObjectData) -> Result<()> {
    self.object_type_raw = data.object_type_raw();
    self.continues = false;
    self.total_object_size = None;
    self.object_data = data.to_bytes()?;
    Ok(())
  }

  pub fn parse_object_data(&self) -> Result<EmfPlusObjectData> {
    if self.continues {
      return Ok(EmfPlusObjectData::Unknown {
        object_type_raw: self.object_type_raw,
        data: self.object_data.clone(),
      });
    }

    let mut reader = Reader::new(std::io::Cursor::new(self.object_data.as_slice()));
    let data_len = self.object_data.len() as u64;
    match self.object_type() {
      Some(EmfPlusObjectType::Brush) if self.object_data.len() >= 8 => Ok(
        EmfPlusObjectData::Brush(read_emf_plus_brush_object(&mut reader, data_len)?),
      ),
      Some(EmfPlusObjectType::CustomLineCap) if self.object_data.len() >= 8 => {
        Ok(EmfPlusObjectData::CustomLineCap(
          read_emf_plus_custom_line_cap_object(&mut reader, data_len)?,
        ))
      }
      Some(EmfPlusObjectType::Font) if self.object_data.len() >= 24 => Ok(EmfPlusObjectData::Font(
        read_emf_plus_font_object(&mut reader, data_len)?,
      )),
      Some(EmfPlusObjectType::Image) if self.object_data.len() >= 8 => Ok(
        EmfPlusObjectData::Image(read_emf_plus_image_object(&mut reader, data_len)?),
      ),
      Some(EmfPlusObjectType::ImageAttributes) if self.object_data.len() == 24 => Ok(
        EmfPlusObjectData::ImageAttributes(EmfPlusImageAttributesObject::read_from(&mut reader)?),
      ),
      Some(EmfPlusObjectType::Path) if self.object_data.len() >= 12 => Ok(EmfPlusObjectData::Path(
        read_emf_plus_path_object(&mut reader, data_len)?,
      )),
      Some(EmfPlusObjectType::Pen) if self.object_data.len() >= 8 => Ok(EmfPlusObjectData::Pen(
        read_emf_plus_pen_object(&mut reader, data_len)?,
      )),
      Some(EmfPlusObjectType::Region) if self.object_data.len() >= 8 => Ok(
        EmfPlusObjectData::Region(read_emf_plus_region_object(&mut reader, data_len)?),
      ),
      Some(EmfPlusObjectType::StringFormat) if self.object_data.len() >= 60 => Ok(
        EmfPlusObjectData::StringFormat(read_emf_plus_string_format_object(&mut reader, data_len)?),
      ),
      Some(EmfPlusObjectType::Invalid) => Err(Error::invalid(
        0,
        "EmfPlusObject ObjectTypeInvalid is not a valid object",
      )),
      Some(_) => Err(Error::invalid(
        0,
        "known EMF+ object data does not match its object type",
      )),
      None => Ok(EmfPlusObjectData::Unknown {
        object_type_raw: self.object_type_raw,
        data: self.object_data.clone(),
      }),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusObjectData {
  Brush(EmfPlusBrushObject),
  CustomLineCap(EmfPlusCustomLineCapObject),
  Font(EmfPlusFontObject),
  Image(EmfPlusImageObject),
  ImageAttributes(EmfPlusImageAttributesObject),
  Path(EmfPlusPathObject),
  Pen(EmfPlusPenObject),
  Region(EmfPlusRegionObject),
  StringFormat(EmfPlusStringFormatObject),
  Unknown { object_type_raw: u8, data: Vec<u8> },
}

impl EmfPlusObjectData {
  pub fn validate_strict(&self) -> Result<()> {
    match self {
      Self::Brush(value) => validate_brush_object_strict(value),
      Self::Image(value) => validate_image_object_strict(value),
      Self::Path(value) => value.validate_strict(),
      Self::Pen(value) => value.validate_strict(),
      _ => Ok(()),
    }
  }

  pub fn object_type_raw(&self) -> u8 {
    match self {
      Self::Brush(_) => EmfPlusObjectType::Brush.raw() as u8,
      Self::CustomLineCap(_) => EmfPlusObjectType::CustomLineCap.raw() as u8,
      Self::Font(_) => EmfPlusObjectType::Font.raw() as u8,
      Self::Image(_) => EmfPlusObjectType::Image.raw() as u8,
      Self::ImageAttributes(_) => EmfPlusObjectType::ImageAttributes.raw() as u8,
      Self::Path(_) => EmfPlusObjectType::Path.raw() as u8,
      Self::Pen(_) => EmfPlusObjectType::Pen.raw() as u8,
      Self::Region(_) => EmfPlusObjectType::Region.raw() as u8,
      Self::StringFormat(_) => EmfPlusObjectType::StringFormat.raw() as u8,
      Self::Unknown {
        object_type_raw, ..
      } => *object_type_raw,
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Brush(value) => value.write_to(writer),
      Self::CustomLineCap(value) => value.write_to(writer),
      Self::Font(value) => value.write_to(writer),
      Self::Image(value) => value.write_to(writer),
      Self::ImageAttributes(value) => value.write_to(writer),
      Self::Path(value) => value.write_to(writer),
      Self::Pen(value) => value.write_to(writer),
      Self::Region(value) => value.write_to(writer),
      Self::StringFormat(value) => value.write_to(writer),
      Self::Unknown {
        object_type_raw,
        data,
      } => {
        validate_unknown_object_data_type(*object_type_raw)?;
        writer.write_all(data)
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusBrushObject {
  pub version: EmfPlusGraphicsVersion,
  pub brush_type: u32,
  pub brush_data: Vec<u8>,
}

impl EmfPlusBrushObject {
  pub fn from_typed_data(version: EmfPlusGraphicsVersion, data: &EmfPlusBrushData) -> Result<Self> {
    let value = Self {
      version,
      brush_type: data.brush_type_raw(),
      brush_data: data.to_bytes()?,
    };
    validate_brush_object(&value)?;
    value.parse_brush_data()?;
    Ok(value)
  }

  pub fn brush_kind(&self) -> Option<EmfPlusBrushType> {
    EmfPlusBrushType::from_raw(self.brush_type)
  }

  pub fn set_typed_brush_data(&mut self, data: &EmfPlusBrushData) -> Result<()> {
    self.brush_type = data.brush_type_raw();
    self.brush_data = data.to_bytes()?;
    validate_brush_object(self)?;
    self.parse_brush_data()?;
    Ok(())
  }

  pub fn parse_brush_data(&self) -> Result<EmfPlusBrushData> {
    let mut reader = Reader::new(std::io::Cursor::new(self.brush_data.as_slice()));
    let data_len = self.brush_data.len() as u64;
    match self.brush_kind() {
      Some(EmfPlusBrushType::SolidColor) if self.brush_data.len() >= 4 => Ok(
        EmfPlusBrushData::Solid(read_emf_plus_solid_brush_data(&mut reader, data_len)?),
      ),
      Some(EmfPlusBrushType::HatchFill) if self.brush_data.len() >= 12 => Ok(
        EmfPlusBrushData::Hatch(read_emf_plus_hatch_brush_data(&mut reader, data_len)?),
      ),
      Some(EmfPlusBrushType::TextureFill) if self.brush_data.len() >= 8 => Ok(
        EmfPlusBrushData::Texture(read_emf_plus_texture_brush_data(&mut reader, data_len)?),
      ),
      Some(EmfPlusBrushType::PathGradient) if self.brush_data.len() >= 24 => {
        Ok(EmfPlusBrushData::PathGradient(
          read_emf_plus_path_gradient_brush_data(&mut reader, data_len)?,
        ))
      }
      Some(EmfPlusBrushType::LinearGradient) if self.brush_data.len() >= 44 => {
        Ok(EmfPlusBrushData::LinearGradient(
          read_emf_plus_linear_gradient_brush_data(&mut reader, data_len)?,
        ))
      }
      Some(_) => Err(Error::invalid(
        0,
        "known EMF+ brush data does not match its brush type",
      )),
      None => Ok(EmfPlusBrushData::Unknown {
        brush_type: self.brush_type,
        data: self.brush_data.clone(),
      }),
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_brush_object(self)?;
    self.version.write_to(writer)?;
    writer.write_u32(self.brush_type)?;
    writer.write_all(&self.brush_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusBrushData {
  Solid(EmfPlusSolidBrushData),
  Hatch(EmfPlusHatchBrushData),
  Texture(EmfPlusTextureBrushData),
  PathGradient(EmfPlusPathGradientBrushData),
  LinearGradient(EmfPlusLinearGradientBrushData),
  Unknown { brush_type: u32, data: Vec<u8> },
}

impl EmfPlusBrushData {
  pub fn brush_type_raw(&self) -> u32 {
    match self {
      Self::Solid(_) => EmfPlusBrushType::SolidColor.raw(),
      Self::Hatch(_) => EmfPlusBrushType::HatchFill.raw(),
      Self::Texture(_) => EmfPlusBrushType::TextureFill.raw(),
      Self::PathGradient(_) => EmfPlusBrushType::PathGradient.raw(),
      Self::LinearGradient(_) => EmfPlusBrushType::LinearGradient.raw(),
      Self::Unknown { brush_type, .. } => *brush_type,
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Solid(value) => value.write_to(writer),
      Self::Hatch(value) => value.write_to(writer),
      Self::Texture(value) => value.write_to(writer),
      Self::PathGradient(value) => value.write_to(writer),
      Self::LinearGradient(value) => value.write_to(writer),
      Self::Unknown { brush_type, data } => {
        validate_unknown_brush_data_type(*brush_type)?;
        writer.write_all(data)
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusSolidBrushData {
  pub solid_color: EmfPlusArgb,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusSolidBrushData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_solid_brush_data(self)?;
    self.solid_color.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusHatchBrushData {
  pub hatch_style: u32,
  pub fore_color: EmfPlusArgb,
  pub back_color: EmfPlusArgb,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusHatchBrushData {
  pub fn hatch_style_kind(&self) -> Option<EmfPlusHatchStyle> {
    EmfPlusHatchStyle::from_raw(self.hatch_style)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_hatch_brush_data(self)?;
    writer.write_u32(self.hatch_style)?;
    self.fore_color.write_to(writer)?;
    self.back_color.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusLinearGradientBrushData {
  pub brush_data_flags: u32,
  pub wrap_mode: i32,
  pub rect: RectF,
  pub start_color: EmfPlusArgb,
  pub end_color: EmfPlusArgb,
  pub reserved1: u32,
  pub reserved2: u32,
  pub optional_data: Vec<u8>,
}

impl EmfPlusLinearGradientBrushData {
  pub fn flags(&self) -> EmfPlusBrushDataFlags {
    EmfPlusBrushDataFlags::from_bits_retain(self.brush_data_flags)
  }

  pub fn wrap_mode_kind(&self) -> Option<EmfPlusWrapMode> {
    wrap_mode_from_i32(self.wrap_mode)
  }

  pub fn parse_optional_data(&self) -> Result<EmfPlusLinearGradientBrushOptionalData> {
    let mut reader = Reader::new(std::io::Cursor::new(self.optional_data.as_slice()));
    read_emf_plus_linear_gradient_optional_data(
      &mut reader,
      self.optional_data.len() as u64,
      self.flags(),
    )
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_linear_gradient_brush_data(self)?;
    writer.write_u32(self.brush_data_flags)?;
    writer.write_i32(self.wrap_mode)?;
    self.rect.write_to(writer)?;
    self.start_color.write_to(writer)?;
    self.end_color.write_to(writer)?;
    writer.write_u32(self.reserved1)?;
    writer.write_u32(self.reserved2)?;
    writer.write_all(&self.optional_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusPathGradientBrushData {
  pub brush_data_flags: u32,
  pub wrap_mode: i32,
  pub center_color: EmfPlusArgb,
  pub center_point: PointF,
  pub surrounding_colors: Vec<EmfPlusArgb>,
  pub boundary_and_optional_data: Vec<u8>,
}

impl EmfPlusPathGradientBrushData {
  pub fn flags(&self) -> EmfPlusBrushDataFlags {
    EmfPlusBrushDataFlags::from_bits_retain(self.brush_data_flags)
  }

  pub fn wrap_mode_kind(&self) -> Option<EmfPlusWrapMode> {
    wrap_mode_from_i32(self.wrap_mode)
  }

  pub fn parse_tail_data(&self) -> Result<EmfPlusPathGradientBrushTailData> {
    let mut reader = Reader::new(std::io::Cursor::new(
      self.boundary_and_optional_data.as_slice(),
    ));
    read_emf_plus_path_gradient_tail_data(
      &mut reader,
      self.boundary_and_optional_data.len() as u64,
      self.flags(),
    )
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_path_gradient_brush_data(self)?;
    writer.write_u32(self.brush_data_flags)?;
    writer.write_i32(self.wrap_mode)?;
    self.center_color.write_to(writer)?;
    self.center_point.write_to(writer)?;
    writer.write_u32(len_to_u32(
      self.surrounding_colors.len(),
      "EMF+ path gradient surrounding colors",
    )?)?;
    for color in &self.surrounding_colors {
      color.write_to(writer)?;
    }
    writer.write_all(&self.boundary_and_optional_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusLinearGradientBrushOptionalData {
  pub transform_matrix: Option<XForm>,
  pub blend_pattern: Option<EmfPlusBlendPattern>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusLinearGradientBrushOptionalData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_linear_gradient_brush_optional_data(self)?;
    if let Some(transform_matrix) = &self.transform_matrix {
      transform_matrix.write_to(writer)?;
    }
    if let Some(blend_pattern) = &self.blend_pattern {
      blend_pattern.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusPathGradientBrushTailData {
  pub boundary_data: Option<EmfPlusBoundaryData>,
  pub optional_data: EmfPlusPathGradientBrushOptionalData,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusPathGradientBrushTailData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_path_gradient_brush_tail_data(self)?;
    if let Some(boundary_data) = &self.boundary_data {
      boundary_data.write_to(writer)?;
    }
    self.optional_data.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusPathGradientBrushOptionalData {
  pub transform_matrix: Option<XForm>,
  pub blend_pattern: Option<EmfPlusBlendPattern>,
  pub focus_scale_data: Option<EmfPlusFocusScaleData>,
}

impl EmfPlusPathGradientBrushOptionalData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_path_gradient_brush_optional_data(self)?;
    if let Some(transform_matrix) = &self.transform_matrix {
      transform_matrix.write_to(writer)?;
    }
    if let Some(blend_pattern) = &self.blend_pattern {
      blend_pattern.write_to(writer)?;
    }
    if let Some(focus_scale_data) = &self.focus_scale_data {
      focus_scale_data.write_to(writer)?;
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusBlendPattern {
  Colors(EmfPlusBlendColors),
  Factors(EmfPlusBlendFactors),
  FactorsHV {
    horizontal: EmfPlusBlendFactors,
    vertical: EmfPlusBlendFactors,
  },
}

impl EmfPlusBlendPattern {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Colors(value) => value.write_to(writer),
      Self::Factors(value) => value.write_to(writer),
      Self::FactorsHV {
        horizontal,
        vertical,
      } => {
        vertical.write_to(writer)?;
        horizontal.write_to(writer)
      }
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusBlendColors {
  pub positions: Vec<f32>,
  pub colors: Vec<EmfPlusArgb>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusBlendColors {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    if self.positions.len() != self.colors.len() {
      return Err(Error::invalid(
        0,
        "EmfPlusBlendColors position and color counts differ",
      ));
    }
    validate_unit_interval_values(&self.positions, "EmfPlusBlendColors positions")?;
    validate_empty_trailing_data(&self.trailing_data, "EmfPlusBlendColors")?;
    writer.write_u32(len_to_u32(
      self.positions.len(),
      "EMF+ blend color positions",
    )?)?;
    for position in &self.positions {
      writer.write_f32(*position)?;
    }
    for color in &self.colors {
      color.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusBlendFactors {
  pub positions: Vec<f32>,
  pub factors: Vec<f32>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusBlendFactors {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    if self.positions.len() != self.factors.len() {
      return Err(Error::invalid(
        0,
        "EmfPlusBlendFactors position and factor counts differ",
      ));
    }
    validate_blend_factors(&self.positions, &self.factors)?;
    validate_empty_trailing_data(&self.trailing_data, "EmfPlusBlendFactors")?;
    writer.write_u32(len_to_u32(
      self.positions.len(),
      "EMF+ blend factor positions",
    )?)?;
    for position in &self.positions {
      writer.write_f32(*position)?;
    }
    for factor in &self.factors {
      writer.write_f32(*factor)?;
    }
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusBoundaryData {
  Path(EmfPlusBoundaryPathData),
  Points(EmfPlusBoundaryPointData),
}

impl EmfPlusBoundaryData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Path(value) => value.write_to(writer),
      Self::Points(value) => value.write_to(writer),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusBoundaryPathData {
  pub path_data: EmfPlusRegionNodePathData,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusBoundaryPathData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_boundary_path_data(self)?;
    self.path_data.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusBoundaryPointData {
  pub points: Vec<PointF>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusBoundaryPointData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_empty_trailing_data(&self.trailing_data, "EmfPlusBoundaryPointData")?;
    writer.write_i32(len_to_i32(self.points.len(), "EMF+ boundary points")?)?;
    for point in &self.points {
      point.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFocusScaleData {
  pub focus_scale_count: u32,
  pub focus_scale_x: f32,
  pub focus_scale_y: f32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusFocusScaleData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_focus_scale_data(self)?;
    writer.write_u32(self.focus_scale_count)?;
    writer.write_f32(self.focus_scale_x)?;
    writer.write_f32(self.focus_scale_y)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusPalette {
  pub palette_style_flags: u32,
  pub entries: Vec<EmfPlusArgb>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusPalette {
  pub fn flags(&self) -> EmfPlusPaletteStyleFlags {
    EmfPlusPaletteStyleFlags::from_bits_retain(self.palette_style_flags)
  }

  pub fn read_from_bytes(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(std::io::Cursor::new(data));
    read_emf_plus_palette(&mut reader, data.len() as u64)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_palette(self)?;
    writer.write_u32(self.palette_style_flags)?;
    writer.write_u32(len_to_u32(self.entries.len(), "EMF+ palette entries")?)?;
    for entry in &self.entries {
      entry.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusTextureBrushData {
  pub brush_data_flags: u32,
  pub wrap_mode: i32,
  pub optional_data: Vec<u8>,
}

impl EmfPlusTextureBrushData {
  pub fn flags(&self) -> EmfPlusBrushDataFlags {
    EmfPlusBrushDataFlags::from_bits_retain(self.brush_data_flags)
  }

  pub fn wrap_mode_kind(&self) -> Option<EmfPlusWrapMode> {
    wrap_mode_from_i32(self.wrap_mode)
  }

  pub fn parse_optional_data(&self) -> Result<EmfPlusTextureBrushOptionalData> {
    read_emf_plus_texture_brush_optional_data(
      &mut Reader::new(std::io::Cursor::new(self.optional_data.as_slice())),
      self.optional_data.len() as u64,
      self.flags(),
    )
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_texture_brush_data(self)?;
    writer.write_u32(self.brush_data_flags)?;
    writer.write_i32(self.wrap_mode)?;
    writer.write_all(&self.optional_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusTextureBrushOptionalData {
  pub transform_matrix: Option<XForm>,
  pub image_object: Option<EmfPlusImageObject>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusTextureBrushOptionalData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_texture_brush_optional_data(self)?;
    if let Some(value) = &self.transform_matrix {
      value.write_to(writer)?;
    }
    if let Some(value) = &self.image_object {
      value.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusCustomLineCapObject {
  pub version: EmfPlusGraphicsVersion,
  pub cap_type: i32,
  pub custom_line_cap_data: Vec<u8>,
}

impl EmfPlusCustomLineCapObject {
  pub fn from_typed_data(
    version: EmfPlusGraphicsVersion,
    data: &EmfPlusCustomLineCapData,
  ) -> Result<Self> {
    let value = Self {
      version,
      cap_type: data.cap_type_raw(),
      custom_line_cap_data: data.to_bytes()?,
    };
    validate_custom_line_cap_object(&value)?;
    value.parse_cap_data()?;
    Ok(value)
  }

  pub fn cap_data_type(&self) -> Option<EmfPlusCustomLineCapDataType> {
    EmfPlusCustomLineCapDataType::from_raw(self.cap_type)
  }

  pub fn set_typed_cap_data(&mut self, data: &EmfPlusCustomLineCapData) -> Result<()> {
    self.cap_type = data.cap_type_raw();
    self.custom_line_cap_data = data.to_bytes()?;
    validate_custom_line_cap_object(self)?;
    self.parse_cap_data()?;
    Ok(())
  }

  pub fn parse_cap_data(&self) -> Result<EmfPlusCustomLineCapData> {
    let mut reader = Reader::new(std::io::Cursor::new(self.custom_line_cap_data.as_slice()));
    let data_len = self.custom_line_cap_data.len() as u64;
    match self.cap_data_type() {
      Some(EmfPlusCustomLineCapDataType::AdjustableArrow)
        if self.custom_line_cap_data.len() >= 52 =>
      {
        Ok(EmfPlusCustomLineCapData::Arrow(
          read_emf_plus_custom_line_cap_arrow_data(&mut reader, data_len)?,
        ))
      }
      Some(EmfPlusCustomLineCapDataType::Default) if self.custom_line_cap_data.len() >= 48 => {
        Ok(EmfPlusCustomLineCapData::Default(
          read_emf_plus_custom_line_cap_default_data(&mut reader, data_len)?,
        ))
      }
      Some(_) => Err(Error::invalid(
        0,
        "known EMF+ custom line cap data does not match its cap type",
      )),
      None => Ok(EmfPlusCustomLineCapData::Unknown {
        cap_type: self.cap_type,
        data: self.custom_line_cap_data.clone(),
      }),
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_custom_line_cap_object(self)?;
    self.version.write_to(writer)?;
    writer.write_i32(self.cap_type)?;
    writer.write_all(&self.custom_line_cap_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusCustomLineCapData {
  Arrow(EmfPlusCustomLineCapArrowData),
  Default(EmfPlusCustomLineCapDefaultData),
  Unknown { cap_type: i32, data: Vec<u8> },
}

impl EmfPlusCustomLineCapData {
  pub fn cap_type_raw(&self) -> i32 {
    match self {
      Self::Arrow(_) => EmfPlusCustomLineCapDataType::AdjustableArrow.raw(),
      Self::Default(_) => EmfPlusCustomLineCapDataType::Default.raw(),
      Self::Unknown { cap_type, .. } => *cap_type,
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Arrow(value) => value.write_to(writer),
      Self::Default(value) => value.write_to(writer),
      Self::Unknown { cap_type, data } => {
        validate_unknown_custom_line_cap_data_type(*cap_type)?;
        writer.write_all(data)
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusCustomLineCapArrowData {
  pub width: f32,
  pub height: f32,
  pub middle_inset: f32,
  pub fill_state: u32,
  pub line_start_cap: u32,
  pub line_end_cap: u32,
  pub line_join: u32,
  pub line_miter_limit: f32,
  pub width_scale: f32,
  pub fill_hot_spot: PointF,
  pub line_hot_spot: PointF,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusCustomLineCapArrowData {
  pub fn line_start_cap_kind(&self) -> Option<EmfPlusLineCapType> {
    i32::try_from(self.line_start_cap)
      .ok()
      .and_then(EmfPlusLineCapType::from_raw)
  }

  pub fn line_end_cap_kind(&self) -> Option<EmfPlusLineCapType> {
    i32::try_from(self.line_end_cap)
      .ok()
      .and_then(EmfPlusLineCapType::from_raw)
  }

  pub fn line_join_kind(&self) -> Option<EmfPlusLineJoinType> {
    i32::try_from(self.line_join)
      .ok()
      .and_then(EmfPlusLineJoinType::from_raw)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_custom_line_cap_arrow_data(self)?;
    writer.write_f32(self.width)?;
    writer.write_f32(self.height)?;
    writer.write_f32(self.middle_inset)?;
    writer.write_u32(self.fill_state)?;
    writer.write_u32(self.line_start_cap)?;
    writer.write_u32(self.line_end_cap)?;
    writer.write_u32(self.line_join)?;
    writer.write_f32(self.line_miter_limit)?;
    writer.write_f32(self.width_scale)?;
    self.fill_hot_spot.write_to(writer)?;
    self.line_hot_spot.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusCustomLineCapDefaultData {
  pub custom_line_cap_data_flags: u32,
  pub base_cap: u32,
  pub base_inset: f32,
  pub stroke_start_cap: u32,
  pub stroke_end_cap: u32,
  pub stroke_join: u32,
  pub stroke_miter_limit: f32,
  pub width_scale: f32,
  pub fill_hot_spot: PointF,
  pub stroke_hot_spot: PointF,
  pub optional_data: Vec<u8>,
}

impl EmfPlusCustomLineCapDefaultData {
  pub fn flags(&self) -> EmfPlusCustomLineCapDataFlags {
    EmfPlusCustomLineCapDataFlags::from_bits_retain(self.custom_line_cap_data_flags)
  }

  pub fn base_cap_kind(&self) -> Option<EmfPlusLineCapType> {
    i32::try_from(self.base_cap)
      .ok()
      .and_then(EmfPlusLineCapType::from_raw)
  }

  pub fn stroke_start_cap_kind(&self) -> Option<EmfPlusLineCapType> {
    i32::try_from(self.stroke_start_cap)
      .ok()
      .and_then(EmfPlusLineCapType::from_raw)
  }

  pub fn stroke_end_cap_kind(&self) -> Option<EmfPlusLineCapType> {
    i32::try_from(self.stroke_end_cap)
      .ok()
      .and_then(EmfPlusLineCapType::from_raw)
  }

  pub fn stroke_join_kind(&self) -> Option<EmfPlusLineJoinType> {
    i32::try_from(self.stroke_join)
      .ok()
      .and_then(EmfPlusLineJoinType::from_raw)
  }

  pub fn parse_optional_data(&self) -> Result<EmfPlusCustomLineCapOptionalData> {
    let mut reader = Reader::new(std::io::Cursor::new(self.optional_data.as_slice()));
    read_emf_plus_custom_line_cap_optional_data(
      &mut reader,
      self.optional_data.len() as u64,
      self.flags(),
    )
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_custom_line_cap_default_data(self)?;
    writer.write_u32(self.custom_line_cap_data_flags)?;
    writer.write_u32(self.base_cap)?;
    writer.write_f32(self.base_inset)?;
    writer.write_u32(self.stroke_start_cap)?;
    writer.write_u32(self.stroke_end_cap)?;
    writer.write_u32(self.stroke_join)?;
    writer.write_f32(self.stroke_miter_limit)?;
    writer.write_f32(self.width_scale)?;
    self.fill_hot_spot.write_to(writer)?;
    self.stroke_hot_spot.write_to(writer)?;
    writer.write_all(&self.optional_data)
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmfPlusCustomLineCapOptionalData {
  pub fill_path: Option<EmfPlusFillPathObject>,
  pub line_path: Option<EmfPlusLinePathObject>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusCustomLineCapOptionalData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_custom_line_cap_optional_data(self)?;
    if let Some(fill_path) = &self.fill_path {
      fill_path.write_to(writer)?;
    }
    if let Some(line_path) = &self.line_path {
      line_path.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFillPathObject {
  pub path_data: EmfPlusRegionNodePathData,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusFillPathObject {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_fill_path_object(self)?;
    self.path_data.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusLinePathObject {
  pub path_data: EmfPlusRegionNodePathData,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusLinePathObject {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_line_path_object(self)?;
    self.path_data.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusFontObject {
  pub version: EmfPlusGraphicsVersion,
  pub em_size: f32,
  pub size_unit: u32,
  pub font_style_flags: i32,
  pub reserved: u32,
  pub family_name: SdkString,
  pub padding: Vec<u8>,
}

impl EmfPlusFontObject {
  pub fn size_unit_kind(&self) -> Option<EmfPlusUnitType> {
    EmfPlusUnitType::from_raw(self.size_unit)
  }

  pub fn font_style(&self) -> EmfPlusFontStyleFlags {
    EmfPlusFontStyleFlags::from_bits_retain(self.font_style_flags as u32)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_font_object(self)?;
    self.version.write_to(writer)?;
    writer.write_f32(self.em_size)?;
    writer.write_u32(self.size_unit)?;
    writer.write_i32(self.font_style_flags)?;
    writer.write_u32(self.reserved)?;
    let family_name = self.family_name.encoded_bytes()?;
    if !family_name.len().is_multiple_of(2) {
      return Err(Error::invalid(
        0,
        "EmfPlusFont FamilyName byte length is odd",
      ));
    }
    writer.write_u32(len_to_u32(
      family_name.len() / 2,
      "EMF+ font family name length",
    )?)?;
    writer.write_all(&family_name)?;
    writer.write_all(&self.padding)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusImageObject {
  pub version: EmfPlusGraphicsVersion,
  pub image_type: u32,
  pub image_data: Vec<u8>,
}

impl EmfPlusImageObject {
  pub fn from_typed_data(version: EmfPlusGraphicsVersion, data: &EmfPlusImageData) -> Result<Self> {
    let value = Self {
      version,
      image_type: data.image_type_raw(),
      image_data: data.to_bytes()?,
    };
    validate_image_object(&value)?;
    value.parse_image_data()?;
    Ok(value)
  }

  pub fn image_data_type(&self) -> Option<EmfPlusImageDataType> {
    EmfPlusImageDataType::from_raw(self.image_type)
  }

  pub fn set_typed_image_data(&mut self, data: &EmfPlusImageData) -> Result<()> {
    self.image_type = data.image_type_raw();
    self.image_data = data.to_bytes()?;
    validate_image_object(self)?;
    self.parse_image_data()?;
    Ok(())
  }

  pub fn parse_image_data(&self) -> Result<EmfPlusImageData> {
    let mut reader = Reader::new(std::io::Cursor::new(self.image_data.as_slice()));
    let data_len = self.image_data.len() as u64;
    match self.image_data_type() {
      Some(EmfPlusImageDataType::Bitmap) if self.image_data.len() >= 20 => Ok(
        EmfPlusImageData::Bitmap(read_emf_plus_bitmap_object(&mut reader, data_len)?),
      ),
      Some(EmfPlusImageDataType::Metafile) if self.image_data.len() >= 8 => Ok(
        EmfPlusImageData::Metafile(read_emf_plus_metafile_object(&mut reader, data_len)?),
      ),
      Some(EmfPlusImageDataType::Unknown) | None => Ok(EmfPlusImageData::Unknown {
        image_type: self.image_type,
        data: self.image_data.clone(),
      }),
      Some(_) => Err(Error::invalid(
        0,
        "known EMF+ image data does not match its image data type",
      )),
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_image_object(self)?;
    self.version.write_to(writer)?;
    writer.write_u32(self.image_type)?;
    writer.write_all(&self.image_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmfPlusImageData {
  Bitmap(EmfPlusBitmapObject),
  Metafile(EmfPlusMetafileObject),
  Unknown { image_type: u32, data: Vec<u8> },
}

impl EmfPlusImageData {
  pub fn image_type_raw(&self) -> u32 {
    match self {
      Self::Bitmap(_) => EmfPlusImageDataType::Bitmap.raw(),
      Self::Metafile(_) => EmfPlusImageDataType::Metafile.raw(),
      Self::Unknown { image_type, .. } => *image_type,
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Bitmap(value) => value.write_to(writer),
      Self::Metafile(value) => value.write_to(writer),
      Self::Unknown { image_type, data } => {
        validate_unknown_image_data_type(*image_type)?;
        writer.write_all(data)
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmfPlusBitmapPayload {
  Pixel(EmfPlusBitmapDataObject),
  Compressed(EmfPlusCompressedImageObject),
  Unknown {
    bitmap_data_type: u32,
    data: Vec<u8>,
  },
}

impl EmfPlusBitmapPayload {
  pub fn bitmap_data_type_raw(&self) -> u32 {
    match self {
      Self::Pixel(_) => EmfPlusBitmapDataType::Pixel.raw(),
      Self::Compressed(_) => EmfPlusBitmapDataType::Compressed.raw(),
      Self::Unknown {
        bitmap_data_type, ..
      } => *bitmap_data_type,
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Pixel(value) => value.write_to(writer),
      Self::Compressed(value) => value.write_to(writer),
      Self::Unknown {
        bitmap_data_type,
        data,
      } => {
        validate_unknown_bitmap_data_type(*bitmap_data_type)?;
        writer.write_all(data)
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusBitmapDataObject {
  pub palette: Option<EmfPlusPalette>,
  pub pixel_data: Vec<u8>,
}

impl EmfPlusBitmapDataObject {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    if let Some(palette) = &self.palette {
      if !palette.trailing_data.is_empty() {
        return Err(Error::invalid(
          0,
          "EmfPlusBitmapData palette must not contain trailing data",
        ));
      }
      palette.write_to(writer)?;
    }
    writer.write_all(&self.pixel_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusCompressedImageObject {
  pub compressed_image_data: Vec<u8>,
}

impl EmfPlusCompressedImageObject {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_all(&self.compressed_image_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusBitmapObject {
  pub width: i32,
  pub height: i32,
  pub stride: i32,
  pub pixel_format: u32,
  pub bitmap_data_type: u32,
  pub bitmap_data: Vec<u8>,
}

impl EmfPlusBitmapObject {
  pub fn from_typed_payload(
    width: i32,
    height: i32,
    stride: i32,
    pixel_format: u32,
    payload: &EmfPlusBitmapPayload,
  ) -> Result<Self> {
    let value = Self {
      width,
      height,
      stride,
      pixel_format,
      bitmap_data_type: payload.bitmap_data_type_raw(),
      bitmap_data: payload.to_bytes()?,
    };
    validate_emf_plus_bitmap_object(&value)?;
    Ok(value)
  }

  pub fn pixel_format_value(&self) -> EmfPlusPixelFormatValue {
    EmfPlusPixelFormatValue::new(self.pixel_format)
  }

  pub fn pixel_format_kind(&self) -> Option<EmfPlusPixelFormat> {
    self.pixel_format_value().kind()
  }

  pub fn pixel_format_index(&self) -> u8 {
    self.pixel_format_value().index()
  }

  pub fn bits_per_pixel(&self) -> u8 {
    self.pixel_format_value().bits_per_pixel()
  }

  pub fn is_indexed_pixel_format(&self) -> bool {
    self.pixel_format_value().is_indexed()
  }

  pub fn is_gdi_pixel_format(&self) -> bool {
    self.pixel_format_value().is_gdi()
  }

  pub fn has_alpha_pixel_format(&self) -> bool {
    self.pixel_format_value().has_alpha()
  }

  pub fn is_pre_multiplied_alpha_pixel_format(&self) -> bool {
    self.pixel_format_value().is_pre_multiplied_alpha()
  }

  pub fn is_extended_pixel_format(&self) -> bool {
    self.pixel_format_value().is_extended()
  }

  pub fn is_canonical_pixel_format(&self) -> bool {
    self.pixel_format_value().is_canonical()
  }

  pub fn bitmap_data_type_kind(&self) -> Option<EmfPlusBitmapDataType> {
    EmfPlusBitmapDataType::from_raw(self.bitmap_data_type)
  }

  pub fn parse_bitmap_data(&self) -> Result<EmfPlusBitmapPayload> {
    read_emf_plus_bitmap_payload(self)
  }

  pub fn set_typed_bitmap_data(&mut self, payload: &EmfPlusBitmapPayload) -> Result<()> {
    self.bitmap_data_type = payload.bitmap_data_type_raw();
    self.bitmap_data = payload.to_bytes()?;
    validate_emf_plus_bitmap_object(self)?;
    Ok(())
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_emf_plus_bitmap_object(self)?;
    writer.write_i32(self.width)?;
    writer.write_i32(self.height)?;
    writer.write_i32(self.stride)?;
    writer.write_u32(self.pixel_format)?;
    writer.write_u32(self.bitmap_data_type)?;
    writer.write_all(&self.bitmap_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusMetafileObject {
  pub metafile_type: u32,
  pub metafile_data: Vec<u8>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusMetafileObject {
  pub fn validate_strict(&self) -> Result<()> {
    validate_metafile_object_strict(self)
  }

  pub fn metafile_data_type_kind(&self) -> Option<EmfPlusMetafileDataType> {
    EmfPlusMetafileDataType::from_raw(self.metafile_type)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_metafile_object(self)?;
    writer.write_u32(self.metafile_type)?;
    writer.write_u32(len_to_u32(self.metafile_data.len(), "EMF+ metafile data")?)?;
    writer.write_all(&self.metafile_data)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emf_plus_image_attributes_object")]
pub struct EmfPlusImageAttributesObject {
  pub version: EmfPlusGraphicsVersion,
  pub reserved1: u32,
  pub wrap_mode: u32,
  pub clamp_color: EmfPlusArgb,
  pub object_clamp: i32,
  pub reserved2: u32,
}

impl EmfPlusImageAttributesObject {
  pub fn wrap_mode_kind(&self) -> Option<EmfPlusWrapMode> {
    EmfPlusWrapMode::from_raw(self.wrap_mode)
  }

  pub fn object_clamp_kind(&self) -> Option<EmfPlusObjectClamp> {
    EmfPlusObjectClamp::from_raw(self.object_clamp)
  }

  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    <Self as SdkRead>::read_from(reader)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    <Self as SdkWrite>::write_to(self, writer)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusPathObject {
  pub version: EmfPlusGraphicsVersion,
  pub path_point_flags: u32,
  pub points: EmfPlusPointData,
  pub point_types: EmfPlusPathPointTypes,
  pub alignment_padding: Vec<u8>,
}

impl EmfPlusPathObject {
  pub fn validate_strict(&self) -> Result<()> {
    validate_path_object_strict(self)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    let point_count = self.points.len();
    validate_path_object(self)?;
    self.version.write_to(writer)?;
    writer.write_u32(len_to_u32(point_count, "EMF+ path point count")?)?;
    writer.write_u32(self.path_point_flags)?;
    write_points(writer, &self.points)?;
    self.point_types.write_to(writer)?;
    writer.write_all(&self.alignment_padding)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmfPlusPathPointTypes {
  Values(Vec<EmfPlusPathPointTypeValue>),
  Rle(Vec<EmfPlusPathPointTypeRle>),
}

impl EmfPlusPathPointTypes {
  pub fn point_count(&self) -> usize {
    match self {
      Self::Values(values) => values.len(),
      Self::Rle(values) => values.iter().map(|value| value.run_count() as usize).sum(),
    }
  }

  pub fn sdk_size(&self) -> Result<usize> {
    match self {
      Self::Values(values) => Ok(values.len()),
      Self::Rle(values) => values
        .len()
        .checked_mul(2)
        .ok_or_else(|| Error::invalid(0, "EMF+ path point type size overflows")),
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_path_point_type_sequence(self)?;
    match self {
      Self::Values(values) => {
        for value in values {
          writer.write_u8(value.value)?;
        }
        Ok(())
      }
      Self::Rle(values) => {
        for value in values {
          writer.write_u8(value.control)?;
          writer.write_u8(value.point_type.value)?;
        }
        Ok(())
      }
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusPathPointTypeValue {
  pub value: u8,
}

impl EmfPlusPathPointTypeValue {
  pub fn new(value: u8) -> Result<Self> {
    validate_path_point_type_byte(value)?;
    Ok(Self { value })
  }

  pub fn path_point_type_raw(self) -> u8 {
    self.value & 0x0F
  }

  pub fn path_point_type(self) -> Option<EmfPlusPathPointType> {
    EmfPlusPathPointType::from_raw(self.path_point_type_raw())
  }

  pub fn path_point_flags(self) -> EmfPlusPathPointTypeFlags {
    EmfPlusPathPointTypeFlags::from_bits_retain(self.value >> 4)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusPathPointTypeRle {
  pub control: u8,
  pub point_type: EmfPlusPathPointTypeValue,
}

impl EmfPlusPathPointTypeRle {
  pub fn new(bezier: bool, run_count: u8, point_type: EmfPlusPathPointTypeValue) -> Result<Self> {
    if run_count == 0 || run_count > 0x3F {
      return Err(Error::invalid(
        0,
        "EmfPlusPathPointTypeRLE RunCount must be in 1..=63",
      ));
    }
    let mut control = 0x40 | run_count;
    if bezier {
      control |= 0x80;
    }
    Ok(Self {
      control,
      point_type,
    })
  }

  pub fn bezier(self) -> bool {
    self.control & 0x80 != 0
  }

  pub fn marker_bit_set(self) -> bool {
    self.control & 0x40 != 0
  }

  pub fn run_count(self) -> u8 {
    self.control & 0x3F
  }

  pub fn path_point_type_raw(self) -> u8 {
    self.point_type.path_point_type_raw()
  }

  pub fn path_point_type(self) -> Option<EmfPlusPathPointType> {
    self.point_type.path_point_type()
  }

  pub fn path_point_flags(self) -> EmfPlusPathPointTypeFlags {
    self.point_type.path_point_flags()
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusPenObject {
  pub version: EmfPlusGraphicsVersion,
  pub pen_type: u32,
  pub pen_data_and_brush_object: Vec<u8>,
}

impl EmfPlusPenObject {
  pub fn validate_strict(&self) -> Result<()> {
    validate_pen_object_strict(self)
  }

  pub fn from_typed_payload(
    version: EmfPlusGraphicsVersion,
    payload: &EmfPlusPenPayload,
  ) -> Result<Self> {
    let value = Self {
      version,
      pen_type: 0,
      pen_data_and_brush_object: payload.to_bytes()?,
    };
    validate_pen_object(&value)?;
    Ok(value)
  }

  pub fn parse_pen_payload(&self) -> Result<EmfPlusPenPayload> {
    let mut reader = Reader::new(std::io::Cursor::new(
      self.pen_data_and_brush_object.as_slice(),
    ));
    let data_len = self.pen_data_and_brush_object.len() as u64;
    let pen_data = read_emf_plus_pen_data(&mut reader, data_len)?;
    let start = reader.position()?;
    if start >= data_len {
      return Err(Error::invalid(start, "EmfPlusPen BrushObject is missing"));
    }
    let remaining = data_len - start;
    if remaining < 8 {
      return Err(Error::invalid(start, "EmfPlusPen BrushObject is truncated"));
    }
    let version = EmfPlusGraphicsVersion {
      value: reader.read_u32()?,
    };
    let brush_type = reader.read_u32()?;
    let brush_data = read_remaining_vec(&mut reader, data_len, "EmfPlusPen BrushObject")?;
    let brush_object = EmfPlusBrushObject {
      version,
      brush_type,
      brush_data,
    };
    if brush_object.brush_kind().is_none() {
      return Err(Error::invalid(0, "EmfPlusBrush Type is invalid"));
    }
    Ok(EmfPlusPenPayload {
      pen_data,
      brush_object: Some(brush_object),
    })
  }

  pub fn set_typed_pen_payload(&mut self, payload: &EmfPlusPenPayload) -> Result<()> {
    self.pen_type = 0;
    self.pen_data_and_brush_object = payload.to_bytes()?;
    validate_pen_object(self)?;
    Ok(())
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_pen_object(self)?;
    writer.write_u32(self.version.value)?;
    writer.write_u32(self.pen_type)?;
    writer.write_all(&self.pen_data_and_brush_object)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusPenPayload {
  pub pen_data: EmfPlusPenData,
  pub brush_object: Option<EmfPlusBrushObject>,
}

impl EmfPlusPenPayload {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    if self.brush_object.is_none() {
      return Err(Error::invalid(0, "EmfPlusPen BrushObject is missing"));
    }
    self.pen_data.write_to(writer)?;
    if let Some(brush_object) = &self.brush_object {
      brush_object.write_to(writer)?;
    }
    Ok(())
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusPenData {
  pub pen_data_flags: u32,
  pub pen_unit: u32,
  pub pen_width: f32,
  pub optional_data: EmfPlusPenOptionalData,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusPenData {
  pub fn flags(&self) -> EmfPlusPenDataFlags {
    EmfPlusPenDataFlags::from_bits_retain(self.pen_data_flags)
  }

  pub fn pen_unit_kind(&self) -> Option<EmfPlusUnitType> {
    EmfPlusUnitType::from_raw(self.pen_unit)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_pen_data(self)?;
    self.optional_data.validate_for_flags(self.flags())?;
    writer.write_u32(self.pen_data_flags)?;
    writer.write_u32(self.pen_unit)?;
    writer.write_f32(self.pen_width)?;
    self.optional_data.write_to(writer)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDashedLineData {
  pub dashed_line_data: Vec<f32>,
}

impl EmfPlusDashedLineData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    write_f32_array_with_u32_count(writer, &self.dashed_line_data, "EMF+ dashed line data")
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusCompoundLineData {
  pub compound_line_data: Vec<f32>,
}

impl EmfPlusCompoundLineData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_increasing_unit_interval_values(&self.compound_line_data, "EmfPlusCompoundLineData")?;
    write_f32_array_with_u32_count(writer, &self.compound_line_data, "EMF+ compound line data")
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusCustomStartCapData {
  pub custom_start_cap: Vec<u8>,
}

impl EmfPlusCustomStartCapData {
  pub fn from_typed_cap(cap: &EmfPlusCustomLineCapObject) -> Result<Self> {
    let value = Self {
      custom_start_cap: cap.to_bytes()?,
    };
    value.parse_custom_start_cap()?;
    Ok(value)
  }

  pub fn parse_custom_start_cap(&self) -> Result<EmfPlusCustomLineCapObject> {
    read_sized_custom_line_cap(&self.custom_start_cap, "EmfPlusCustomStartCapData")
  }

  pub fn set_typed_cap(&mut self, cap: &EmfPlusCustomLineCapObject) -> Result<()> {
    self.custom_start_cap = cap.to_bytes()?;
    self.parse_custom_start_cap()?;
    Ok(())
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.parse_custom_start_cap()?;
    writer.write_u32(len_to_u32(
      self.custom_start_cap.len(),
      "EMF+ custom start cap",
    )?)?;
    writer.write_all(&self.custom_start_cap)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusCustomEndCapData {
  pub custom_end_cap: Vec<u8>,
}

impl EmfPlusCustomEndCapData {
  pub fn from_typed_cap(cap: &EmfPlusCustomLineCapObject) -> Result<Self> {
    let value = Self {
      custom_end_cap: cap.to_bytes()?,
    };
    value.parse_custom_end_cap()?;
    Ok(value)
  }

  pub fn parse_custom_end_cap(&self) -> Result<EmfPlusCustomLineCapObject> {
    read_sized_custom_line_cap(&self.custom_end_cap, "EmfPlusCustomEndCapData")
  }

  pub fn set_typed_cap(&mut self, cap: &EmfPlusCustomLineCapObject) -> Result<()> {
    self.custom_end_cap = cap.to_bytes()?;
    self.parse_custom_end_cap()?;
    Ok(())
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.parse_custom_end_cap()?;
    writer.write_u32(len_to_u32(
      self.custom_end_cap.len(),
      "EMF+ custom end cap",
    )?)?;
    writer.write_all(&self.custom_end_cap)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmfPlusPenOptionalData {
  pub transform_matrix: Option<XForm>,
  pub start_cap: Option<i32>,
  pub end_cap: Option<i32>,
  pub join: Option<i32>,
  pub miter_limit: Option<f32>,
  pub line_style: Option<i32>,
  pub dashed_line_cap_type: Option<i32>,
  pub dash_offset: Option<f32>,
  pub dashed_line_data: Option<EmfPlusDashedLineData>,
  pub pen_alignment: Option<i32>,
  pub compound_line_data: Option<EmfPlusCompoundLineData>,
  pub custom_start_cap_data: Option<EmfPlusCustomStartCapData>,
  pub custom_end_cap_data: Option<EmfPlusCustomEndCapData>,
}

impl EmfPlusPenOptionalData {
  pub fn start_cap_kind(&self) -> Option<EmfPlusLineCapType> {
    self.start_cap.and_then(EmfPlusLineCapType::from_raw)
  }

  pub fn end_cap_kind(&self) -> Option<EmfPlusLineCapType> {
    self.end_cap.and_then(EmfPlusLineCapType::from_raw)
  }

  pub fn join_kind(&self) -> Option<EmfPlusLineJoinType> {
    self.join.and_then(EmfPlusLineJoinType::from_raw)
  }

  pub fn line_style_kind(&self) -> Option<EmfPlusLineStyle> {
    self.line_style.and_then(EmfPlusLineStyle::from_raw)
  }

  pub fn dashed_line_cap_type_kind(&self) -> Option<EmfPlusDashedLineCapType> {
    self
      .dashed_line_cap_type
      .and_then(EmfPlusDashedLineCapType::from_raw)
  }

  pub fn pen_alignment_kind(&self) -> Option<EmfPlusPenAlignment> {
    self.pen_alignment.and_then(EmfPlusPenAlignment::from_raw)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.validate_arrays()?;
    if let Some(value) = &self.transform_matrix {
      value.write_to(writer)?;
    }
    if let Some(value) = self.start_cap {
      writer.write_i32(value)?;
    }
    if let Some(value) = self.end_cap {
      writer.write_i32(value)?;
    }
    if let Some(value) = self.join {
      writer.write_i32(value)?;
    }
    if let Some(value) = self.miter_limit {
      writer.write_f32(value)?;
    }
    if let Some(value) = self.line_style {
      writer.write_i32(value)?;
    }
    if let Some(value) = self.dashed_line_cap_type {
      writer.write_i32(value)?;
    }
    if let Some(value) = self.dash_offset {
      writer.write_f32(value)?;
    }
    if let Some(value) = &self.dashed_line_data {
      value.write_to(writer)?;
    }
    if let Some(value) = self.pen_alignment {
      writer.write_i32(value)?;
    }
    if let Some(value) = &self.compound_line_data {
      value.write_to(writer)?;
    }
    if let Some(value) = &self.custom_start_cap_data {
      value.write_to(writer)?;
    }
    if let Some(value) = &self.custom_end_cap_data {
      value.write_to(writer)?;
    }
    Ok(())
  }

  pub fn validate_for_flags(&self, flags: EmfPlusPenDataFlags) -> Result<()> {
    if flags.contains(EmfPlusPenDataFlags::TRANSFORM) && self.transform_matrix.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData TransformMatrix missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::START_CAP) && self.start_cap.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData StartCap missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::END_CAP) && self.end_cap.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData EndCap missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::JOIN) && self.join.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData Join missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::MITER_LIMIT) && self.miter_limit.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData MiterLimit missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::LINE_STYLE) && self.line_style.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData LineStyle missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::DASHED_LINE_CAP) && self.dashed_line_cap_type.is_none() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData DashedLineCapType missing",
      ));
    }
    if flags.contains(EmfPlusPenDataFlags::DASHED_LINE_OFFSET) && self.dash_offset.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData DashOffset missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::DASHED_LINE) && self.dashed_line_data.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData DashedLineData missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::NON_CENTER) && self.pen_alignment.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData PenAlignment missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::COMPOUND_LINE) && self.compound_line_data.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData CompoundLineData missing"));
    }
    if flags.contains(EmfPlusPenDataFlags::CUSTOM_START_CAP) && self.custom_start_cap_data.is_none()
    {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData CustomStartCapData missing",
      ));
    }
    if flags.contains(EmfPlusPenDataFlags::CUSTOM_END_CAP) && self.custom_end_cap_data.is_none() {
      return Err(Error::invalid(0, "EmfPlusPenData CustomEndCapData missing"));
    }
    if !flags.contains(EmfPlusPenDataFlags::TRANSFORM) && self.transform_matrix.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData TransformMatrix supplied without PenDataTransform",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::START_CAP) && self.start_cap.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData StartCap supplied without PenDataStartCap",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::END_CAP) && self.end_cap.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData EndCap supplied without PenDataEndCap",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::JOIN) && self.join.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData Join supplied without PenDataJoin",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::MITER_LIMIT) && self.miter_limit.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData MiterLimit supplied without PenDataMiterLimit",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::LINE_STYLE) && self.line_style.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData LineStyle supplied without PenDataLineStyle",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::DASHED_LINE_CAP) && self.dashed_line_cap_type.is_some()
    {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData DashedLineCapType supplied without PenDataDashedLineCap",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::DASHED_LINE_OFFSET) && self.dash_offset.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData DashOffset supplied without PenDataDashedLineOffset",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::DASHED_LINE) && self.dashed_line_data.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData DashedLineData supplied without PenDataDashedLine",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::NON_CENTER) && self.pen_alignment.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData PenAlignment supplied without PenDataNonCenter",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::COMPOUND_LINE) && self.compound_line_data.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData CompoundLineData supplied without PenDataCompoundLine",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::CUSTOM_START_CAP)
      && self.custom_start_cap_data.is_some()
    {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData CustomStartCapData supplied without PenDataCustomStartCap",
      ));
    }
    if !flags.contains(EmfPlusPenDataFlags::CUSTOM_END_CAP) && self.custom_end_cap_data.is_some() {
      return Err(Error::invalid(
        0,
        "EmfPlusPenData CustomEndCapData supplied without PenDataCustomEndCap",
      ));
    }
    if self
      .start_cap
      .is_some_and(|_| self.start_cap_kind().is_none())
    {
      return Err(Error::invalid(0, "EmfPlusPenData StartCap is invalid"));
    }
    if self.end_cap.is_some_and(|_| self.end_cap_kind().is_none()) {
      return Err(Error::invalid(0, "EmfPlusPenData EndCap is invalid"));
    }
    if self.join.is_some_and(|_| self.join_kind().is_none()) {
      return Err(Error::invalid(0, "EmfPlusPenData Join is invalid"));
    }
    if self
      .line_style
      .is_some_and(|_| self.line_style_kind().is_none())
    {
      return Err(Error::invalid(0, "EmfPlusPenData LineStyle is invalid"));
    }
    if self
      .pen_alignment
      .is_some_and(|_| self.pen_alignment_kind().is_none())
    {
      return Err(Error::invalid(0, "EmfPlusPenData PenAlignment is invalid"));
    }
    self.validate_arrays()
  }

  fn validate_arrays(&self) -> Result<()> {
    if let Some(value) = &self.compound_line_data {
      validate_increasing_unit_interval_values(
        &value.compound_line_data,
        "EmfPlusCompoundLineData",
      )?;
    }
    if let Some(value) = &self.custom_start_cap_data {
      value.parse_custom_start_cap()?;
    }
    if let Some(value) = &self.custom_end_cap_data {
      value.parse_custom_end_cap()?;
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusRegionObject {
  pub version: EmfPlusGraphicsVersion,
  pub region_node_count: u32,
  pub region_nodes: Vec<u8>,
}

impl EmfPlusRegionObject {
  pub fn parse_region_nodes(&self) -> Result<Vec<EmfPlusRegionNode>> {
    let mut reader = Reader::new(std::io::Cursor::new(self.region_nodes.as_slice()));
    let data_len = self.region_nodes.len() as u64;
    let expected_count = self
      .region_node_count
      .checked_add(1)
      .ok_or_else(|| Error::invalid(0, "EmfPlusRegion node count overflows"))?
      as usize;
    if expected_count == 0 || data_len == 0 {
      return Err(Error::invalid(0, "EmfPlusRegion RegionNode is missing"));
    }
    let max_nodes_by_size = self.region_nodes.len() / 4;
    if expected_count > max_nodes_by_size {
      return Err(Error::invalid(
        0,
        "EmfPlusRegion RegionNodeCount exceeds the node payload size",
      ));
    }
    let node = read_emf_plus_region_node(&mut reader, data_len)?;
    validate_region_node_tree(
      &node,
      expected_count,
      reader.position()?,
      data_len,
      "EmfPlusRegion",
    )?;
    let nodes = vec![node];
    Ok(nodes)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_region_object(self)?;
    self.version.write_to(writer)?;
    writer.write_u32(self.region_node_count)?;
    writer.write_all(&self.region_nodes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusRegionNode {
  pub node_type: u32,
  pub data: EmfPlusRegionNodeData,
}

impl EmfPlusRegionNode {
  pub fn node_type_kind(&self) -> Option<EmfPlusRegionNodeDataType> {
    EmfPlusRegionNodeDataType::from_raw(self.node_type)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_region_node_data_matches_type(self)?;
    writer.write_u32(self.node_type)?;
    self.data.write_to(writer)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusRegionNodeData {
  ChildNodes(Box<EmfPlusRegionNodeChildNodes>),
  Rect(RectF),
  Path(EmfPlusRegionNodePathData),
  Empty,
  Infinite,
  Raw(Vec<u8>),
}

impl EmfPlusRegionNodeData {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::ChildNodes(value) => value.write_to(writer),
      Self::Raw(value) => writer.write_all(value),
      Self::Rect(value) => value.write_to(writer),
      Self::Path(value) => value.write_to(writer),
      Self::Empty | Self::Infinite => Ok(()),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusRegionNodePathData {
  Path(EmfPlusPathObject),
  Raw(Vec<u8>),
}

impl EmfPlusRegionNodePathData {
  pub fn path(&self) -> Option<&EmfPlusPathObject> {
    match self {
      Self::Path(value) => Some(value),
      Self::Raw(_) => None,
    }
  }

  pub fn path_bytes(&self) -> Result<Vec<u8>> {
    match self {
      Self::Path(value) => value.to_bytes(),
      Self::Raw(value) => Ok(value.clone()),
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_region_node_path_data(self, "EmfPlusRegionNodePath")?;
    let path_bytes = self.path_bytes()?;
    writer.write_i32(len_to_i32(path_bytes.len(), "EMF+ region node path")?)?;
    writer.write_all(&path_bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusRegionNodeChildNodes {
  pub left: EmfPlusRegionNode,
  pub right: EmfPlusRegionNode,
}

impl EmfPlusRegionNodeChildNodes {
  pub fn node_count(&self) -> usize {
    self.left.node_count() + self.right.node_count()
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.left.write_to(writer)?;
    self.right.write_to(writer)
  }
}

impl EmfPlusRegionNode {
  pub fn node_count(&self) -> usize {
    match &self.data {
      EmfPlusRegionNodeData::ChildNodes(value) => 1 + value.node_count(),
      _ => 1,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusStringFormatObject {
  pub version: EmfPlusGraphicsVersion,
  pub string_format_flags: u32,
  pub language: u32,
  pub string_alignment: u32,
  pub line_align: u32,
  pub digit_substitution: u32,
  pub digit_language: u32,
  pub first_tab_offset: f32,
  pub hotkey_prefix: i32,
  pub leading_margin: f32,
  pub trailing_margin: f32,
  pub tracking: f32,
  pub trimming: u32,
  pub tab_stops: Vec<f32>,
  pub char_ranges: Vec<EmfPlusCharacterRange>,
  pub trailing_data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusLanguageIdentifier {
  pub raw: u32,
}

impl EmfPlusLanguageIdentifier {
  pub fn from_parts(primary_language_id: u16, sub_language_id: u8) -> Result<Self> {
    if primary_language_id > 0x03FF {
      return Err(Error::invalid(
        0,
        "EmfPlusLanguageIdentifier PrimaryLanguageId exceeds 10 bits",
      ));
    }
    if sub_language_id > 0x3F {
      return Err(Error::invalid(
        0,
        "EmfPlusLanguageIdentifier SubLanguageId exceeds 6 bits",
      ));
    }
    Ok(Self {
      raw: u32::from(primary_language_id) | (u32::from(sub_language_id) << 10),
    })
  }

  pub fn language_id(self) -> u16 {
    self.raw as u16
  }

  pub fn high_word(self) -> u16 {
    (self.raw >> 16) as u16
  }

  pub fn primary_language_id(self) -> u16 {
    self.language_id() & 0x03FF
  }

  pub fn sub_language_id(self) -> u8 {
    ((self.language_id() >> 10) & 0x3F) as u8
  }

  pub fn is_vendor_primary_language_id(self) -> bool {
    (0x0200..=0x03FF).contains(&self.primary_language_id())
  }

  pub fn is_vendor_sub_language_id(self) -> bool {
    (0x20..=0x3F).contains(&self.sub_language_id())
  }

  pub fn is_word_sized(self) -> bool {
    self.high_word() == 0
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusStringFormatData {
  pub tab_stops: Vec<f32>,
  pub char_ranges: Vec<EmfPlusCharacterRange>,
}

impl EmfPlusStringFormatObject {
  pub fn flags(&self) -> EmfPlusStringFormatFlags {
    EmfPlusStringFormatFlags::from_bits_retain(self.string_format_flags)
  }

  pub fn language_identifier(&self) -> EmfPlusLanguageIdentifier {
    EmfPlusLanguageIdentifier { raw: self.language }
  }

  pub fn digit_language_identifier(&self) -> EmfPlusLanguageIdentifier {
    EmfPlusLanguageIdentifier {
      raw: self.digit_language,
    }
  }

  pub fn string_format_data(&self) -> EmfPlusStringFormatData {
    EmfPlusStringFormatData {
      tab_stops: self.tab_stops.clone(),
      char_ranges: self.char_ranges.clone(),
    }
  }

  pub fn tab_stops(&self) -> &[f32] {
    &self.tab_stops
  }

  pub fn char_ranges(&self) -> &[EmfPlusCharacterRange] {
    &self.char_ranges
  }

  pub fn string_alignment_kind(&self) -> Option<EmfPlusStringAlignment> {
    EmfPlusStringAlignment::from_raw(self.string_alignment)
  }

  pub fn line_align_kind(&self) -> Option<EmfPlusStringAlignment> {
    EmfPlusStringAlignment::from_raw(self.line_align)
  }

  pub fn digit_substitution_kind(&self) -> Option<EmfPlusStringDigitSubstitution> {
    EmfPlusStringDigitSubstitution::from_raw(self.digit_substitution)
  }

  pub fn hotkey_prefix_kind(&self) -> Option<EmfPlusHotkeyPrefix> {
    EmfPlusHotkeyPrefix::from_raw(self.hotkey_prefix)
  }

  pub fn trimming_kind(&self) -> Option<EmfPlusStringTrimming> {
    EmfPlusStringTrimming::from_raw(self.trimming)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_string_format_object(self)?;
    self.version.write_to(writer)?;
    writer.write_u32(self.string_format_flags)?;
    writer.write_u32(self.language)?;
    writer.write_u32(self.string_alignment)?;
    writer.write_u32(self.line_align)?;
    writer.write_u32(self.digit_substitution)?;
    writer.write_u32(self.digit_language)?;
    writer.write_f32(self.first_tab_offset)?;
    writer.write_i32(self.hotkey_prefix)?;
    writer.write_f32(self.leading_margin)?;
    writer.write_f32(self.trailing_margin)?;
    writer.write_f32(self.tracking)?;
    writer.write_u32(self.trimming)?;
    writer.write_i32(len_to_i32(
      self.tab_stops.len(),
      "EMF+ string format tab stops",
    )?)?;
    writer.write_i32(len_to_i32(
      self.char_ranges.len(),
      "EMF+ string format character ranges",
    )?)?;
    for tab_stop in &self.tab_stops {
      writer.write_f32(*tab_stop)?;
    }
    for range in &self.char_ranges {
      range.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusCharacterRange {
  pub first: i32,
  pub length: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawImageData {
  pub image_id: u8,
  pub image_attributes_id: u32,
  pub src_unit: i32,
  pub src_rect: RectF,
  pub dest_rect: EmfPlusRect,
}

impl EmfPlusDrawImageData {
  pub fn src_unit_kind(&self) -> Option<EmfPlusUnitType> {
    EmfPlusUnitType::from_raw(self.src_unit as u32)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawImagePointsData {
  pub image_id: u8,
  pub apply_effect: bool,
  pub image_attributes_id: u32,
  pub src_unit: i32,
  pub src_rect: RectF,
  pub points: EmfPlusPointData,
}

impl EmfPlusDrawImagePointsData {
  pub fn src_unit_kind(&self) -> Option<EmfPlusUnitType> {
    EmfPlusUnitType::from_raw(self.src_unit as u32)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawStringData {
  pub font_id: u8,
  pub brush: EmfPlusBrushRef,
  pub format_id: u32,
  pub layout_rect: RectF,
  pub string: SdkString,
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusDrawDriverStringData {
  pub font_id: u8,
  pub brush: EmfPlusBrushRef,
  pub driver_string_options_flags: u32,
  pub glyphs: Vec<u16>,
  pub glyph_positions: Vec<PointF>,
  pub transform_matrix: Option<XForm>,
}

impl EmfPlusDrawDriverStringData {
  pub fn driver_string_options(&self) -> EmfPlusDriverStringOptionsFlags {
    EmfPlusDriverStringOptionsFlags::from_bits_retain(self.driver_string_options_flags)
  }

  pub fn expected_glyph_position_count(&self) -> usize {
    driver_string_glyph_position_count(self.glyphs.len(), self.driver_string_options())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusSerializableObjectData {
  pub object_guid: [u8; 16],
  pub buffer: Vec<u8>,
}

impl EmfPlusSerializableObjectData {
  pub fn effect_kind(&self) -> Option<EmfPlusImageEffectKind> {
    EmfPlusImageEffectKind::from_guid(self.object_guid)
  }

  pub fn parse_effect(&self) -> Result<EmfPlusImageEffect> {
    let mut reader = Reader::new(std::io::Cursor::new(self.buffer.as_slice()));
    read_emf_plus_image_effect(&mut reader, self.buffer.len() as u64, self.object_guid)
  }

  pub fn validate_known_effect_buffer(&self) -> Result<()> {
    if !self.buffer.len().is_multiple_of(4) {
      return Err(Error::invalid(
        0,
        "EmfPlusSerializableObject BufferSize must be 32-bit aligned",
      ));
    }
    if self.effect_kind().is_none() {
      return Err(Error::invalid(
        0,
        "EmfPlusSerializableObject ObjectGUID is not an ImageEffects identifier",
      ));
    }
    self.parse_effect()?;
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmfPlusImageEffectKind {
  Blur,
  BrightnessContrast,
  ColorBalance,
  ColorCurve,
  ColorLookupTable,
  ColorMatrix,
  HueSaturationLightness,
  Levels,
  RedEyeCorrection,
  Sharpen,
  Tint,
}

impl EmfPlusImageEffectKind {
  pub fn from_guid(guid: [u8; 16]) -> Option<Self> {
    match guid {
      EMFPLUS_BLUR_EFFECT_GUID => Some(Self::Blur),
      EMFPLUS_BRIGHTNESS_CONTRAST_EFFECT_GUID => Some(Self::BrightnessContrast),
      EMFPLUS_COLOR_BALANCE_EFFECT_GUID => Some(Self::ColorBalance),
      EMFPLUS_COLOR_CURVE_EFFECT_GUID => Some(Self::ColorCurve),
      EMFPLUS_COLOR_LOOKUP_TABLE_EFFECT_GUID => Some(Self::ColorLookupTable),
      EMFPLUS_COLOR_MATRIX_EFFECT_GUID => Some(Self::ColorMatrix),
      EMFPLUS_HUE_SATURATION_LIGHTNESS_EFFECT_GUID => Some(Self::HueSaturationLightness),
      EMFPLUS_LEVELS_EFFECT_GUID => Some(Self::Levels),
      EMFPLUS_RED_EYE_CORRECTION_EFFECT_GUID => Some(Self::RedEyeCorrection),
      EMFPLUS_SHARPEN_EFFECT_GUID => Some(Self::Sharpen),
      EMFPLUS_TINT_EFFECT_GUID => Some(Self::Tint),
      _ => None,
    }
  }

  pub fn guid(self) -> [u8; 16] {
    match self {
      Self::Blur => EMFPLUS_BLUR_EFFECT_GUID,
      Self::BrightnessContrast => EMFPLUS_BRIGHTNESS_CONTRAST_EFFECT_GUID,
      Self::ColorBalance => EMFPLUS_COLOR_BALANCE_EFFECT_GUID,
      Self::ColorCurve => EMFPLUS_COLOR_CURVE_EFFECT_GUID,
      Self::ColorLookupTable => EMFPLUS_COLOR_LOOKUP_TABLE_EFFECT_GUID,
      Self::ColorMatrix => EMFPLUS_COLOR_MATRIX_EFFECT_GUID,
      Self::HueSaturationLightness => EMFPLUS_HUE_SATURATION_LIGHTNESS_EFFECT_GUID,
      Self::Levels => EMFPLUS_LEVELS_EFFECT_GUID,
      Self::RedEyeCorrection => EMFPLUS_RED_EYE_CORRECTION_EFFECT_GUID,
      Self::Sharpen => EMFPLUS_SHARPEN_EFFECT_GUID,
      Self::Tint => EMFPLUS_TINT_EFFECT_GUID,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusImageEffect {
  Blur(EmfPlusBlurEffect),
  BrightnessContrast(EmfPlusBrightnessContrastEffect),
  ColorBalance(EmfPlusColorBalanceEffect),
  ColorCurve(EmfPlusColorCurveEffect),
  ColorLookupTable(Box<EmfPlusColorLookupTableEffect>),
  ColorMatrix(EmfPlusColorMatrixEffect),
  HueSaturationLightness(EmfPlusHueSaturationLightnessEffect),
  Levels(EmfPlusLevelsEffect),
  RedEyeCorrection(EmfPlusRedEyeCorrectionEffect),
  Sharpen(EmfPlusSharpenEffect),
  Tint(EmfPlusTintEffect),
  Unknown {
    object_guid: [u8; 16],
    buffer: Vec<u8>,
  },
}

impl EmfPlusImageEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Blur(value) => value.write_to(writer),
      Self::BrightnessContrast(value) => value.write_to(writer),
      Self::ColorBalance(value) => value.write_to(writer),
      Self::ColorCurve(value) => value.write_to(writer),
      Self::ColorLookupTable(value) => value.write_to(writer),
      Self::ColorMatrix(value) => value.write_to(writer),
      Self::HueSaturationLightness(value) => value.write_to(writer),
      Self::Levels(value) => value.write_to(writer),
      Self::RedEyeCorrection(value) => value.write_to(writer),
      Self::Sharpen(value) => value.write_to(writer),
      Self::Tint(value) => value.write_to(writer),
      Self::Unknown {
        object_guid,
        buffer,
      } => {
        validate_unknown_image_effect_guid(*object_guid)?;
        writer.write_all(buffer)
      }
    }
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    self.write_to(&mut writer)?;
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusBlurEffect {
  pub blur_radius: f32,
  pub expand_edge: u32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusBlurEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_blur_effect(self)?;
    writer.write_f32(self.blur_radius)?;
    writer.write_u32(self.expand_edge)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusBrightnessContrastEffect {
  pub brightness_level: i32,
  pub contrast_level: i32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusBrightnessContrastEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_brightness_contrast_effect(self)?;
    writer.write_i32(self.brightness_level)?;
    writer.write_i32(self.contrast_level)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusColorBalanceEffect {
  pub cyan_red: i32,
  pub magenta_green: i32,
  pub yellow_blue: i32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusColorBalanceEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_color_balance_effect(self)?;
    writer.write_i32(self.cyan_red)?;
    writer.write_i32(self.magenta_green)?;
    writer.write_i32(self.yellow_blue)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusColorCurveEffect {
  pub curve_adjustment: u32,
  pub curve_channel: u32,
  pub adjustment_intensity: i32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusColorCurveEffect {
  pub fn curve_adjustment_kind(&self) -> Option<EmfPlusCurveAdjustment> {
    EmfPlusCurveAdjustment::from_raw(self.curve_adjustment)
  }

  pub fn curve_channel_kind(&self) -> Option<EmfPlusCurveChannel> {
    EmfPlusCurveChannel::from_raw(self.curve_channel)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_color_curve_effect(self)?;
    writer.write_u32(self.curve_adjustment)?;
    writer.write_u32(self.curve_channel)?;
    writer.write_i32(self.adjustment_intensity)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusColorLookupTableEffect {
  pub blue_lookup_table: [u8; 256],
  pub green_lookup_table: [u8; 256],
  pub red_lookup_table: [u8; 256],
  pub alpha_lookup_table: [u8; 256],
  pub trailing_data: Vec<u8>,
}

impl EmfPlusColorLookupTableEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_color_lookup_table_effect(self)?;
    writer.write_all(&self.blue_lookup_table)?;
    writer.write_all(&self.green_lookup_table)?;
    writer.write_all(&self.red_lookup_table)?;
    writer.write_all(&self.alpha_lookup_table)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusColorMatrixEffect {
  pub matrix: [[f32; 5]; 5],
  pub trailing_data: Vec<u8>,
}

impl EmfPlusColorMatrixEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_color_matrix_effect(self)?;
    for column in self.matrix {
      for value in column {
        writer.write_f32(value)?;
      }
    }
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusHueSaturationLightnessEffect {
  pub hue_level: i32,
  pub saturation_level: i32,
  pub lightness_level: i32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusHueSaturationLightnessEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_hue_saturation_lightness_effect(self)?;
    writer.write_i32(self.hue_level)?;
    writer.write_i32(self.saturation_level)?;
    writer.write_i32(self.lightness_level)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusLevelsEffect {
  pub highlight: i32,
  pub mid_tone: i32,
  pub shadow: i32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusLevelsEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_levels_effect(self)?;
    writer.write_i32(self.highlight)?;
    writer.write_i32(self.mid_tone)?;
    writer.write_i32(self.shadow)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusRedEyeCorrectionEffect {
  pub areas: Vec<RectL>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusRedEyeCorrectionEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_red_eye_correction_effect(self)?;
    writer.write_i32(len_to_i32(self.areas.len(), "EMF+ red eye areas")?)?;
    for area in &self.areas {
      area.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusSharpenEffect {
  pub radius: f32,
  pub amount: f32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusSharpenEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_sharpen_effect(self)?;
    writer.write_f32(self.radius)?;
    writer.write_f32(self.amount)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusTintEffect {
  pub hue: i32,
  pub amount: i32,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusTintEffect {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_tint_effect(self)?;
    writer.write_i32(self.hue)?;
    writer.write_i32(self.amount)?;
    writer.write_all(&self.trailing_data)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmfPlusSetTsGraphicsData {
  pub anti_alias_mode: u8,
  pub text_render_hint: u8,
  pub compositing_mode: u8,
  pub compositing_quality: u8,
  pub render_origin_x: i16,
  pub render_origin_y: i16,
  pub text_contrast: u16,
  pub filter_type: u8,
  pub pixel_offset: u8,
  pub world_to_device: XForm,
  pub palette: Option<EmfPlusPalette>,
}

impl EmfPlusSetTsGraphicsData {
  pub fn anti_alias_mode_kind(&self) -> Option<EmfPlusSmoothingMode> {
    EmfPlusSmoothingMode::from_raw(u32::from(self.anti_alias_mode))
  }

  pub fn anti_alias_smoothing_mode(&self) -> Option<EmfPlusSmoothingMode> {
    self.anti_alias_mode_kind()
  }

  pub fn anti_alias_enabled(&self) -> bool {
    self.anti_alias_mode & 0x01 != 0
  }

  pub fn text_rendering_hint_kind(&self) -> Option<EmfPlusTextRenderingHint> {
    EmfPlusTextRenderingHint::from_raw(u32::from(self.text_render_hint))
  }

  pub fn compositing_mode_kind(&self) -> Option<EmfPlusCompositingMode> {
    EmfPlusCompositingMode::from_raw(u32::from(self.compositing_mode))
  }

  pub fn compositing_quality_kind(&self) -> Option<EmfPlusCompositingQuality> {
    EmfPlusCompositingQuality::from_raw(u32::from(self.compositing_quality))
  }

  pub fn filter_type_kind(&self) -> Option<EmfPlusFilterType> {
    EmfPlusFilterType::from_raw(u32::from(self.filter_type))
  }

  pub fn pixel_offset_mode_kind(&self) -> Option<EmfPlusPixelOffsetMode> {
    EmfPlusPixelOffsetMode::from_raw(u32::from(self.pixel_offset))
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmfPlusSetTsClipRects {
  Rects(Vec<EmfPlusRectS>),
  Compressed(Vec<EmfPlusSetTsClipCompressedRect>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusSetTsClipCompressedRect {
  pub left_delta: i8,
  pub top_delta: i8,
  pub right_delta: i8,
  pub bottom_delta: i8,
}

impl EmfPlusSetTsClipCompressedRect {
  fn read_from_bytes(bytes: [u8; 4]) -> Result<Self> {
    let mut values = [0_i8; 4];
    for (index, byte) in bytes.into_iter().enumerate() {
      if byte & 0x80 == 0 {
        return Err(Error::invalid(
          0,
          "EmfPlusSetTSClip compressed coordinate high bit must be set",
        ));
      }
      values[index] = ((byte << 1) as i8) >> 1;
    }
    Ok(Self {
      left_delta: values[0],
      top_delta: values[1],
      right_delta: values[2],
      bottom_delta: values[3],
    })
  }

  fn to_bytes(self) -> Result<[u8; 4]> {
    let values = [
      self.left_delta,
      self.top_delta,
      self.right_delta,
      self.bottom_delta,
    ];
    let mut bytes = [0_u8; 4];
    for (index, value) in values.into_iter().enumerate() {
      if !(-64..=63).contains(&value) {
        return Err(Error::invalid(
          0,
          "EmfPlusSetTSClip compressed coordinate is outside -64..=63",
        ));
      }
      bytes[index] = 0x80 | ((value as u8) & 0x7F);
    }
    Ok(bytes)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusSetTsClipData {
  pub compressed: bool,
  pub rect_count: u16,
  pub rects: EmfPlusSetTsClipRects,
}

impl EmfPlusSetTsClipData {
  pub fn read_data(flags: u16, data: &[u8]) -> Result<Self> {
    let compressed = flags & 0x8000 != 0;
    let rect_count = flags & 0x7FFF;
    let rect_size = if compressed { 4 } else { 8 };
    let expected_size = usize::from(rect_count)
      .checked_mul(rect_size)
      .ok_or_else(|| Error::invalid(0, "EmfPlusSetTSClip data size overflows"))?;
    if data.len() != expected_size {
      return Err(Error::invalid(
        0,
        "EmfPlusSetTSClip data length does not match NumRects",
      ));
    }

    if compressed {
      let mut rects = Vec::with_capacity(usize::from(rect_count));
      for chunk in data.chunks_exact(4) {
        let bytes = chunk
          .try_into()
          .map_err(|_| Error::invalid(0, "EmfPlusSetTSClip compressed rect is malformed"))?;
        rects.push(EmfPlusSetTsClipCompressedRect::read_from_bytes(bytes)?);
      }
      Ok(Self {
        compressed,
        rect_count,
        rects: EmfPlusSetTsClipRects::Compressed(rects),
      })
    } else {
      let mut reader = Reader::new(std::io::Cursor::new(data));
      let mut rects = Vec::with_capacity(usize::from(rect_count));
      for _ in 0..rect_count {
        rects.push(EmfPlusRectS::read_from(&mut reader)?);
      }
      Ok(Self {
        compressed,
        rect_count,
        rects: EmfPlusSetTsClipRects::Rects(rects),
      })
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_set_ts_clip(self)?;
    match &self.rects {
      EmfPlusSetTsClipRects::Rects(rects) => {
        for rect in rects {
          rect.write_to(writer)?;
        }
        Ok(())
      }
      EmfPlusSetTsClipRects::Compressed(rects) => {
        for rect in rects {
          writer.write_all(&rect.to_bytes()?)?;
        }
        Ok(())
      }
    }
  }

  pub fn sdk_size(&self) -> u64 {
    match &self.rects {
      EmfPlusSetTsClipRects::Rects(rects) => rects.len() as u64 * 8,
      EmfPlusSetTsClipRects::Compressed(rects) => rects.len() as u64 * 4,
    }
  }

  pub fn flags_bits(&self) -> u16 {
    (if self.compressed { 0x8000 } else { 0 }) | (self.rect_count & 0x7FFF)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusTranslateWorldTransformData {
  pub dx: f32,
  pub dy: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusScaleWorldTransformData {
  pub sx: f32,
  pub sy: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusRotateWorldTransformData {
  pub angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusSetPageTransformData {
  pub page_scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmfPlusTransformOrderData<T> {
  pub data: T,
  pub post_multiply: bool,
  pub reserved_flags: u16,
}

pub type EmfPlusMultiplyWorldTransformData = EmfPlusTransformOrderData<XForm>;
pub type EmfPlusTranslateWorldTransformRecordData =
  EmfPlusTransformOrderData<EmfPlusTranslateWorldTransformData>;
pub type EmfPlusScaleWorldTransformRecordData =
  EmfPlusTransformOrderData<EmfPlusScaleWorldTransformData>;
pub type EmfPlusRotateWorldTransformRecordData =
  EmfPlusTransformOrderData<EmfPlusRotateWorldTransformData>;

impl<T> EmfPlusTransformOrderData<T> {
  pub fn from_flags(data: T, flags: EmfPlusRecordFlags) -> Self {
    Self {
      data,
      post_multiply: flags.contains(EmfPlusRecordFlags::POST_MULTIPLY),
      reserved_flags: flags.bits() & !EmfPlusRecordFlags::POST_MULTIPLY.bits(),
    }
  }

  pub fn flags_bits(&self) -> u16 {
    (self.reserved_flags & !EmfPlusRecordFlags::POST_MULTIPLY.bits())
      | if self.post_multiply {
        EmfPlusRecordFlags::POST_MULTIPLY.bits()
      } else {
        0
      }
  }
}

impl<T: SdkSize> SdkSize for EmfPlusTransformOrderData<T> {
  fn sdk_size(&self) -> u64 {
    self.data.sdk_size()
  }
}

impl<T: SdkWrite> EmfPlusTransformOrderData<T> {
  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.data.write_to(writer)
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmfPlusSetClipRectData {
  pub combine_mode: u8,
  pub reserved_flags: u16,
  pub clip_rect: RectF,
}

impl EmfPlusSetClipRectData {
  pub fn combine_mode_kind(&self) -> Option<EmfPlusCombineMode> {
    EmfPlusCombineMode::from_raw(u32::from(self.combine_mode))
  }

  pub fn flags_bits(&self) -> u16 {
    (self.reserved_flags & !0x0F00) | (u16::from(self.combine_mode & 0x0F) << 8)
  }

  pub fn checked_flags_bits(&self) -> Result<u16> {
    if self.combine_mode_kind().is_none() {
      return Err(Error::invalid(
        0,
        "EmfPlusSetClipRect CombineMode is invalid",
      ));
    }
    Ok(self.flags_bits())
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.checked_flags_bits()?;
    self.clip_rect.write_to(writer)
  }
}

impl SdkSize for EmfPlusSetClipRectData {
  fn sdk_size(&self) -> u64 {
    self.clip_rect.sdk_size()
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusClipObjectData {
  pub combine_mode: u8,
  pub object_id: u8,
  pub reserved_flags: u16,
}

impl EmfPlusClipObjectData {
  pub fn combine_mode_kind(&self) -> Option<EmfPlusCombineMode> {
    EmfPlusCombineMode::from_raw(u32::from(self.combine_mode))
  }

  pub fn flags_bits(&self) -> u16 {
    (self.reserved_flags & !0x0FFF)
      | (u16::from(self.combine_mode & 0x0F) << 8)
      | u16::from(self.object_id)
  }

  pub fn checked_flags_bits(&self, name: &str) -> Result<u16> {
    let object_id_name = format!("{name} ObjectID");
    validate_object_id_u8(self.object_id, &object_id_name)?;
    if self.combine_mode_kind().is_none() {
      return Err(Error::invalid(0, format!("{name} CombineMode is invalid")));
    }
    Ok(self.flags_bits())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusSetAntiAliasModeData {
  pub smoothing_mode: u8,
  pub anti_alias: bool,
  pub reserved_flags: u16,
}

impl EmfPlusSetAntiAliasModeData {
  pub fn smoothing_mode_kind(&self) -> Option<EmfPlusSmoothingMode> {
    EmfPlusSmoothingMode::from_raw(u32::from(self.smoothing_mode))
  }

  pub fn flags_bits(&self) -> u16 {
    (self.reserved_flags & !0x00FF)
      | (u16::from(self.smoothing_mode & 0x7F) << 1)
      | u16::from(self.anti_alias)
  }

  pub fn checked_flags_bits(&self) -> Result<u16> {
    if self.smoothing_mode_kind().is_none() {
      return Err(Error::invalid(
        0,
        "EmfPlusSetAntiAliasMode SmoothingMode is invalid",
      ));
    }
    Ok(self.flags_bits())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusU8PropertyData {
  pub value: u8,
  pub reserved_flags: u16,
}

impl EmfPlusU8PropertyData {
  pub fn flags_bits(&self) -> u16 {
    (self.reserved_flags & !0x00FF) | u16::from(self.value)
  }

  pub fn checked_flags_bits(&self, name: &str, valid: bool) -> Result<u16> {
    if !valid {
      return Err(Error::invalid(0, format!("{name} is invalid")));
    }
    Ok(self.flags_bits())
  }

  pub fn compositing_mode(&self) -> Option<EmfPlusCompositingMode> {
    EmfPlusCompositingMode::from_raw(u32::from(self.value))
  }

  pub fn compositing_quality(&self) -> Option<EmfPlusCompositingQuality> {
    EmfPlusCompositingQuality::from_raw(u32::from(self.value))
  }

  pub fn interpolation_mode(&self) -> Option<EmfPlusInterpolationMode> {
    EmfPlusInterpolationMode::from_raw(u32::from(self.value))
  }

  pub fn pixel_offset_mode(&self) -> Option<EmfPlusPixelOffsetMode> {
    EmfPlusPixelOffsetMode::from_raw(u32::from(self.value))
  }

  pub fn text_rendering_hint(&self) -> Option<EmfPlusTextRenderingHint> {
    EmfPlusTextRenderingHint::from_raw(u32::from(self.value))
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusSetTextContrastData {
  pub text_contrast: u16,
  pub reserved_flags: u16,
}

impl EmfPlusSetTextContrastData {
  pub fn flags_bits(&self) -> u16 {
    (self.reserved_flags & !0x0FFF) | (self.text_contrast & 0x0FFF)
  }

  pub fn checked_flags_bits(&self) -> Result<u16> {
    validate_text_contrast(self.text_contrast, "EmfPlusSetTextContrast TextContrast")?;
    Ok(self.flags_bits())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusClearData {
  pub color: EmfPlusArgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusStackIndexData {
  pub stack_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusBeginContainerData {
  pub dest_rect: RectF,
  pub src_rect: RectF,
  pub stack_index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfPlusRecordData<'a> {
  Header(EmfPlusHeaderData),
  Eof,
  Comment(Vec<u8>),
  GetDc,
  MultiFormatStart(EmfPlusRawRecordData),
  MultiFormatSection(EmfPlusRawRecordData),
  MultiFormatEnd(EmfPlusRawRecordData),
  Object(EmfPlusObjectRecordData),
  Clear(EmfPlusClearData),
  FillRects(EmfPlusFillRectsData),
  DrawRects(EmfPlusDrawRectsData),
  FillPolygon(EmfPlusFillPolygonData),
  DrawLines(EmfPlusDrawLinesData),
  FillEllipse(EmfPlusFillRectShapeData),
  DrawEllipse(EmfPlusDrawRectShapeData),
  FillPie(EmfPlusFillPieData),
  DrawPie(EmfPlusDrawArcData),
  DrawArc(EmfPlusDrawArcData),
  FillRegion(EmfPlusBrushObjectData),
  FillPath(EmfPlusBrushObjectData),
  DrawPath(EmfPlusDrawObjectData),
  FillClosedCurve(EmfPlusFillClosedCurveData),
  DrawClosedCurve(EmfPlusClosedCurveData),
  DrawCurve(EmfPlusDrawCurveData),
  DrawBeziers(EmfPlusDrawPointsData),
  DrawImage(EmfPlusDrawImageData),
  DrawImagePoints(EmfPlusDrawImagePointsData),
  DrawString(EmfPlusDrawStringData),
  ResetClip,
  SetClipRect(EmfPlusSetClipRectData),
  SetClipPath(EmfPlusClipObjectData),
  SetClipRegion(EmfPlusClipObjectData),
  OffsetClip(EmfPlusTranslateWorldTransformData),
  SetRenderingOrigin(PointL),
  SetAntiAliasMode(EmfPlusSetAntiAliasModeData),
  SetTextRenderingHint(EmfPlusU8PropertyData),
  SetTextContrast(EmfPlusSetTextContrastData),
  SetInterpolationMode(EmfPlusU8PropertyData),
  SetPixelOffsetMode(EmfPlusU8PropertyData),
  SetCompositingMode(EmfPlusU8PropertyData),
  SetCompositingQuality(EmfPlusU8PropertyData),
  Save(EmfPlusStackIndexData),
  Restore(EmfPlusStackIndexData),
  BeginContainer(EmfPlusBeginContainerData),
  BeginContainerNoParams(EmfPlusStackIndexData),
  EndContainer(EmfPlusStackIndexData),
  SetWorldTransform(XForm),
  ResetWorldTransform,
  MultiplyWorldTransform(EmfPlusMultiplyWorldTransformData),
  TranslateWorldTransform(EmfPlusTranslateWorldTransformRecordData),
  ScaleWorldTransform(EmfPlusScaleWorldTransformRecordData),
  RotateWorldTransform(EmfPlusRotateWorldTransformRecordData),
  SetPageTransform(EmfPlusSetPageTransformData),
  DrawDriverString(EmfPlusDrawDriverStringData),
  StrokeFillPath,
  SerializableObject(EmfPlusSerializableObjectData),
  SetTsGraphics(EmfPlusSetTsGraphicsData),
  SetTsClip(EmfPlusSetTsClipData),
  Unknown(EmfPlusRecordRef<'a>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusRecord {
  pub record_type: u16,
  pub flags: u16,
  pub total_object_size: Option<u32>,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusRecordRef<'a> {
  pub record_type: u16,
  pub flags: u16,
  pub total_object_size: Option<u32>,
  pub data: &'a [u8],
  pub padding: &'a [u8],
}

impl EmfPlusRecordRef<'_> {
  pub fn flags(&self) -> EmfPlusRecordFlags {
    EmfPlusRecordFlags::from_bits_retain(self.flags)
  }

  pub fn record_kind(&self) -> Option<EmfPlusRecordType> {
    EmfPlusRecordType::from_raw(self.record_type)
  }

  pub fn into_owned(self) -> EmfPlusRecord {
    EmfPlusRecord {
      record_type: self.record_type,
      flags: self.flags,
      total_object_size: self.total_object_size,
      data: self.data.to_vec(),
      padding: self.padding.to_vec(),
    }
  }

  pub fn object_fragment(&self) -> Result<EmfPlusObjectRecordData> {
    if self.record_kind() != Some(EmfPlusRecordType::Object) {
      return Err(Error::invalid(0, "EMF+ record is not an EmfPlusObject"));
    }
    let flags = self.flags();
    let fragment = EmfPlusObjectRecordData {
      object_id: flags.object_id(),
      object_type_raw: flags.object_type_raw(),
      continues: flags.object_continues(),
      total_object_size: self.total_object_size,
      object_data: self.data.to_vec(),
    };
    validate_emf_plus_object_fragment(&fragment)?;
    Ok(fragment)
  }
}

impl SdkSize for EmfPlusRecordRef<'_> {
  fn sdk_size(&self) -> u64 {
    12 + u64::from(self.total_object_size.is_some()) * 4
      + self.data.len() as u64
      + self.padding.len() as u64
  }
}

#[derive(Clone, Debug)]
pub struct EmfPlusRecords<'a> {
  bytes: &'a [u8],
  offset: usize,
  remaining: usize,
}

impl<'a> Iterator for EmfPlusRecords<'a> {
  type Item = EmfPlusRecordRef<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.remaining == 0 {
      return None;
    }
    let layout = emf_plus_record_layout(self.bytes, self.offset)
      .expect("validated EMF+ record stream must remain valid");
    self.offset = layout.end;
    self.remaining -= 1;
    Some(EmfPlusRecordRef {
      record_type: layout.record_type,
      flags: layout.flags,
      total_object_size: layout.total_object_size,
      data: &self.bytes[layout.data_start..layout.data_end],
      padding: &self.bytes[layout.data_end..layout.end],
    })
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.remaining, Some(self.remaining))
  }
}

impl ExactSizeIterator for EmfPlusRecords<'_> {}
impl std::iter::FusedIterator for EmfPlusRecords<'_> {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfPlusStreamRef<'a> {
  records_bytes: &'a [u8],
  trailing_data: &'a [u8],
  record_count: usize,
}

impl<'a> EmfPlusStreamRef<'a> {
  pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
    let (records_end, record_count) = scan_emf_plus_records(bytes)?;
    Ok(Self {
      records_bytes: &bytes[..records_end],
      trailing_data: &bytes[records_end..],
      record_count,
    })
  }

  pub fn from_bytes_exact(bytes: &'a [u8]) -> Result<Self> {
    let value = Self::from_bytes(bytes)?;
    if !value.trailing_data.is_empty() {
      return Err(Error::invalid(
        value.records_bytes.len() as u64,
        "EMF+ stream has trailing data smaller than a record header",
      ));
    }
    Ok(value)
  }

  pub fn records(&self) -> EmfPlusRecords<'a> {
    EmfPlusRecords {
      bytes: self.records_bytes,
      offset: 0,
      remaining: self.record_count,
    }
  }

  pub const fn record_count(&self) -> usize {
    self.record_count
  }

  pub const fn trailing_data(&self) -> &'a [u8] {
    self.trailing_data
  }

  pub fn into_owned(self) -> EmfPlusStream {
    EmfPlusStream {
      records: self.records().map(EmfPlusRecordRef::into_owned).collect(),
      trailing_data: self.trailing_data.to_vec(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfPlusStream {
  pub records: Vec<EmfPlusRecord>,
  pub trailing_data: Vec<u8>,
}

impl EmfPlusStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Ok(EmfPlusStreamRef::from_bytes(bytes)?.into_owned())
  }

  pub fn from_bytes_exact(bytes: &[u8]) -> Result<Self> {
    Ok(EmfPlusStreamRef::from_bytes_exact(bytes)?.into_owned())
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let capacity = self
      .records
      .iter()
      .try_fold(self.trailing_data.len(), |capacity, record| {
        let record_size = usize::try_from(record.sdk_size())
          .map_err(|_| Error::invalid(0, "EMF+ record size overflows usize"))?;
        capacity
          .checked_add(record_size)
          .ok_or_else(|| Error::invalid(0, "EMF+ stream serialized size overflows usize"))
      })?;
    let mut writer = Writer::new(Vec::with_capacity(capacity));
    self.write_to_writer(&mut writer)?;
    Ok(writer.into_inner())
  }

  pub fn write_to<W: std::io::Write>(&self, writer: W) -> Result<()> {
    self.write_to_writer(&mut Writer::new(writer))
  }

  fn write_to_writer<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    for record in &self.records {
      record.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }
}

impl SdkWrite for EmfPlusStream {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.write_to_writer(writer)
  }
}

struct EmfPlusRecordLayout {
  record_type: u16,
  flags: u16,
  total_object_size: Option<u32>,
  data_start: usize,
  data_end: usize,
  end: usize,
}

fn emf_plus_record_layout(bytes: &[u8], offset: usize) -> Result<EmfPlusRecordLayout> {
  let header = bytes
    .get(offset..offset.saturating_add(12))
    .ok_or_else(|| Error::invalid(offset as u64, "EMF+ record header is truncated"))?;
  let record_type = u16::from_le_bytes(header[..2].try_into().expect("slice length checked"));
  let flags = u16::from_le_bytes(header[2..4].try_into().expect("slice length checked"));
  let size = u32::from_le_bytes(header[4..8].try_into().expect("slice length checked")) as usize;
  let data_size =
    u32::from_le_bytes(header[8..12].try_into().expect("slice length checked")) as usize;
  if size < 12 {
    return Err(Error::invalid(
      offset as u64,
      "EMF+ record size is smaller than its header",
    ));
  }
  if !size.is_multiple_of(4) {
    return Err(Error::invalid(
      offset as u64,
      "EMF+ record size is not 32-bit aligned",
    ));
  }
  if data_size > size - 12 {
    return Err(Error::invalid(
      offset as u64,
      "EMF+ record data size exceeds record size",
    ));
  }
  let end = offset
    .checked_add(size)
    .ok_or_else(|| Error::invalid(offset as u64, "EMF+ record size overflows"))?;
  if end > bytes.len() {
    return Err(Error::invalid(
      offset as u64,
      "EMF+ record extends past end of stream",
    ));
  }

  let continued_object = record_type == EmfPlusRecordType::Object.raw()
    && EmfPlusRecordFlags::from_bits_retain(flags).object_continues();
  if continued_object && data_size < 4 {
    return Err(Error::invalid(
      offset as u64,
      "continued EmfPlusObject DataSize omits TotalObjectSize",
    ));
  }
  let total_object_size = continued_object.then(|| {
    u32::from_le_bytes(
      bytes[offset + 12..offset + 16]
        .try_into()
        .expect("continued object header length checked"),
    )
  });
  let data_start = offset + 12 + usize::from(continued_object) * 4;
  let data_end = offset + 12 + data_size;
  if let Some(total_object_size) = total_object_size
    && total_object_size < (data_end - data_start) as u32
  {
    return Err(Error::invalid(
      offset as u64,
      "EmfPlusObject TotalObjectSize is smaller than ObjectData",
    ));
  }
  validate_emf_plus_record_padding_len(end - data_end)?;
  Ok(EmfPlusRecordLayout {
    record_type,
    flags,
    total_object_size,
    data_start,
    data_end,
    end,
  })
}

fn scan_emf_plus_records(bytes: &[u8]) -> Result<(usize, usize)> {
  let mut offset = 0usize;
  let mut record_count = 0usize;
  while bytes.len() - offset >= 12 {
    offset = emf_plus_record_layout(bytes, offset)?.end;
    record_count += 1;
  }
  Ok((offset, record_count))
}

impl EmfPlusRecord {
  pub fn flags(&self) -> EmfPlusRecordFlags {
    EmfPlusRecordFlags::from_bits_retain(self.flags)
  }

  pub fn record_kind(&self) -> Option<EmfPlusRecordType> {
    EmfPlusRecordType::from_raw(self.record_type)
  }

  pub fn as_ref(&self) -> EmfPlusRecordRef<'_> {
    EmfPlusRecordRef {
      record_type: self.record_type,
      flags: self.flags,
      total_object_size: self.total_object_size,
      data: &self.data,
      padding: &self.padding,
    }
  }

  pub fn object_fragment(&self) -> Result<EmfPlusObjectRecordData> {
    if self.record_kind() != Some(EmfPlusRecordType::Object) {
      return Err(Error::invalid(0, "EMF+ record is not an EmfPlusObject"));
    }
    let flags = self.flags();
    let fragment = EmfPlusObjectRecordData {
      object_id: flags.object_id(),
      object_type_raw: flags.object_type_raw(),
      continues: flags.object_continues(),
      total_object_size: self.total_object_size,
      object_data: self.data.clone(),
    };
    validate_emf_plus_object_fragment(&fragment)?;
    Ok(fragment)
  }

  pub fn into_object_fragment(self) -> Result<EmfPlusObjectRecordData> {
    if self.record_kind() != Some(EmfPlusRecordType::Object) {
      return Err(Error::invalid(0, "EMF+ record is not an EmfPlusObject"));
    }
    let flags = self.flags();
    let fragment = EmfPlusObjectRecordData {
      object_id: flags.object_id(),
      object_type_raw: flags.object_type_raw(),
      continues: flags.object_continues(),
      total_object_size: self.total_object_size,
      object_data: self.data,
    };
    validate_emf_plus_object_fragment(&fragment)?;
    Ok(fragment)
  }

  pub fn from_continued_object(
    object_id: u8,
    data: &EmfPlusObjectData,
    max_data_size: usize,
  ) -> Result<Vec<Self>> {
    validate_object_id_u8(object_id, "EmfPlusObject ObjectID")?;
    if max_data_size == 0 || !max_data_size.is_multiple_of(4) {
      return Err(Error::invalid(
        0,
        "EmfPlusObject continuation data size must be a nonzero multiple of 4",
      ));
    }
    let object_type_raw = data.object_type_raw();
    if !matches!(
        EmfPlusObjectType::from_raw(u16::from(object_type_raw)),
        Some(value) if value != EmfPlusObjectType::Invalid
    ) {
      return Err(Error::invalid(0, "EmfPlusObject ObjectType is invalid"));
    }
    let object_data = data.to_bytes()?;
    if !object_data.len().is_multiple_of(4) {
      return Err(Error::invalid(
        0,
        "EmfPlusObject ObjectData must be 32-bit aligned",
      ));
    }
    let total_object_size = len_to_u32(object_data.len(), "EMF+ total object size")?;
    let chunk_count = object_data.len().div_ceil(max_data_size);
    let mut records = Vec::with_capacity(chunk_count.max(1));
    for (index, chunk) in object_data.chunks(max_data_size).enumerate() {
      let continues = index + 1 < chunk_count;
      records.push(Self {
        record_type: EmfPlusRecordType::Object.raw(),
        flags: u16::from(object_id)
          | (u16::from(object_type_raw) << 8)
          | if continues { 0x8000 } else { 0 },
        total_object_size: continues.then_some(total_object_size),
        data: chunk.to_vec(),
        padding: Vec::new(),
      });
    }
    if records.is_empty() {
      return Err(Error::invalid(0, "EmfPlusObject ObjectData is empty"));
    }
    Ok(records)
  }

  pub fn parse_data(&self) -> Result<EmfPlusRecordData<'_>> {
    self.as_ref().parse_data()
  }

  pub fn rebuild_typed(&self) -> Result<Self> {
    self.as_ref().rebuild_typed()
  }
}

impl SdkWrite for EmfPlusRecord {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    EmfPlusRecord::write_to(self, writer)
  }
}

impl<'a> EmfPlusRecordRef<'a> {
  pub fn parse_data(self) -> Result<EmfPlusRecordData<'a>> {
    if !self.data.len().is_multiple_of(4) || !self.padding.is_empty() {
      return Err(Error::invalid(
        0,
        "EMF+ typed record requires 32-bit-aligned DataSize with no outer padding",
      ));
    }
    validate_emf_plus_fixed_payload_shape(self.record_kind(), self.data.len(), self.padding.len())?;
    let mut reader = Reader::new(std::io::Cursor::new(self.data));
    let flags = self.flags();

    let data = match self.record_kind() {
      Some(EmfPlusRecordType::Header) => {
        EmfPlusRecordData::Header(read_exact_object(self.data, "EmfPlusHeader")?)
      }
      Some(EmfPlusRecordType::Eof) => {
        ensure_empty_data(self.data, "EmfPlusEndOfFile")?;
        EmfPlusRecordData::Eof
      }
      Some(EmfPlusRecordType::Comment) => EmfPlusRecordData::Comment(self.data.to_vec()),
      Some(EmfPlusRecordType::GetDc) => {
        ensure_empty_data(self.data, "EmfPlusGetDC")?;
        EmfPlusRecordData::GetDc
      }
      Some(EmfPlusRecordType::MultiFormatStart) => {
        EmfPlusRecordData::MultiFormatStart(EmfPlusRawRecordData {
          data: self.data.to_vec(),
        })
      }
      Some(EmfPlusRecordType::MultiFormatSection) => {
        EmfPlusRecordData::MultiFormatSection(EmfPlusRawRecordData {
          data: self.data.to_vec(),
        })
      }
      Some(EmfPlusRecordType::MultiFormatEnd) => {
        EmfPlusRecordData::MultiFormatEnd(EmfPlusRawRecordData {
          data: self.data.to_vec(),
        })
      }
      Some(EmfPlusRecordType::Object) => EmfPlusRecordData::Object(self.object_fragment()?),
      Some(EmfPlusRecordType::Clear) => {
        EmfPlusRecordData::Clear(read_exact_object(self.data, "EmfPlusClear")?)
      }
      Some(EmfPlusRecordType::FillRects) if self.data.len() >= 8 => {
        let brush = if flags.contains(EmfPlusRecordFlags::SOLID_COLOR) {
          EmfPlusBrushRef::Color(EmfPlusArgb::read_from(&mut reader)?)
        } else {
          EmfPlusBrushRef::ObjectId(reader.read_u32()?)
        };
        let count = reader.read_u32()? as usize;
        require_count_at_least(count, 1, "EmfPlusFillRects rect")?;
        let rects = read_rects(&mut reader, count, flags, self.data.len() as u64)?;
        EmfPlusRecordData::FillRects(EmfPlusFillRectsData { brush, rects })
      }
      Some(EmfPlusRecordType::DrawRects) if self.data.len() >= 4 => {
        let count = reader.read_u32()? as usize;
        require_count_at_least(count, 1, "EmfPlusDrawRects rect")?;
        let rects = read_rects(&mut reader, count, flags, self.data.len() as u64)?;
        EmfPlusRecordData::DrawRects(EmfPlusDrawRectsData {
          pen_id: flags.object_id(),
          rects,
        })
      }
      Some(EmfPlusRecordType::FillPolygon) if self.data.len() >= 8 => {
        let brush = read_brush_ref(&mut reader, flags)?;
        let count = reader.read_u32()? as usize;
        require_count_at_least(count, 3, "EmfPlusFillPolygon point")?;
        let points = read_points(&mut reader, count, flags, self.data.len() as u64)?;
        finish_record_point_array(
          &mut reader,
          self.data.len() as u64,
          &points,
          "EmfPlusFillPolygon PointData",
        )?;
        EmfPlusRecordData::FillPolygon(EmfPlusFillPolygonData { brush, points })
      }
      Some(EmfPlusRecordType::DrawLines) if self.data.len() >= 4 => {
        let count = reader.read_u32()? as usize;
        require_count_at_least(count, 2, "EmfPlusDrawLines point")?;
        let points = read_points(&mut reader, count, flags, self.data.len() as u64)?;
        finish_record_point_array(
          &mut reader,
          self.data.len() as u64,
          &points,
          "EmfPlusDrawLines PointData",
        )?;
        EmfPlusRecordData::DrawLines(EmfPlusDrawLinesData {
          pen_id: flags.object_id(),
          close_shape: flags.contains(EmfPlusRecordFlags::CLOSE_SHAPE),
          points,
        })
      }
      Some(EmfPlusRecordType::FillEllipse) if self.data.len() >= 12 => {
        let brush = read_brush_ref(&mut reader, flags)?;
        let rect = read_single_rect(&mut reader, flags, self.data.len() as u64)?;
        EmfPlusRecordData::FillEllipse(EmfPlusFillRectShapeData { brush, rect })
      }
      Some(EmfPlusRecordType::DrawEllipse) if self.data.len() >= 8 => {
        let rect = read_single_rect(&mut reader, flags, self.data.len() as u64)?;
        EmfPlusRecordData::DrawEllipse(EmfPlusDrawRectShapeData {
          pen_id: flags.object_id(),
          rect,
        })
      }
      Some(EmfPlusRecordType::FillPie) if self.data.len() >= 20 => {
        let brush = read_brush_ref(&mut reader, flags)?;
        let start_angle = reader.read_f32()?;
        let sweep_angle = reader.read_f32()?;
        let rect = read_single_rect(&mut reader, flags, self.data.len() as u64)?;
        EmfPlusRecordData::FillPie(EmfPlusFillPieData {
          brush,
          start_angle,
          sweep_angle,
          rect,
        })
      }
      Some(EmfPlusRecordType::DrawPie) if self.data.len() >= 16 => {
        let start_angle = reader.read_f32()?;
        let sweep_angle = reader.read_f32()?;
        let rect = read_single_rect(&mut reader, flags, self.data.len() as u64)?;
        EmfPlusRecordData::DrawPie(EmfPlusDrawArcData {
          pen_id: flags.object_id(),
          start_angle,
          sweep_angle,
          rect,
        })
      }
      Some(EmfPlusRecordType::DrawArc) if self.data.len() >= 16 => {
        let start_angle = reader.read_f32()?;
        let sweep_angle = reader.read_f32()?;
        let rect = read_single_rect(&mut reader, flags, self.data.len() as u64)?;
        EmfPlusRecordData::DrawArc(EmfPlusDrawArcData {
          pen_id: flags.object_id(),
          start_angle,
          sweep_angle,
          rect,
        })
      }
      Some(EmfPlusRecordType::FillRegion) if self.data.len() == 4 => {
        EmfPlusRecordData::FillRegion(EmfPlusBrushObjectData {
          object_id: flags.object_id(),
          brush: read_brush_ref(&mut reader, flags)?,
        })
      }
      Some(EmfPlusRecordType::FillPath) if self.data.len() == 4 => {
        EmfPlusRecordData::FillPath(EmfPlusBrushObjectData {
          object_id: flags.object_id(),
          brush: read_brush_ref(&mut reader, flags)?,
        })
      }
      Some(EmfPlusRecordType::DrawPath) if self.data.len() == 4 => {
        EmfPlusRecordData::DrawPath(EmfPlusDrawObjectData {
          object_id: flags.object_id(),
          pen_id: object_id_u8(reader.read_u32()?, "EmfPlusDrawPath PenId")?,
        })
      }
      Some(EmfPlusRecordType::FillClosedCurve) if self.data.len() >= 12 => {
        let brush = read_brush_ref(&mut reader, flags)?;
        let tension = reader.read_f32()?;
        let count = reader.read_u32()? as usize;
        require_count_at_least(count, 3, "EmfPlusFillClosedCurve point")?;
        let points = read_points(&mut reader, count, flags, self.data.len() as u64)?;
        finish_record_point_array(
          &mut reader,
          self.data.len() as u64,
          &points,
          "EmfPlusFillClosedCurve PointData",
        )?;
        EmfPlusRecordData::FillClosedCurve(EmfPlusFillClosedCurveData {
          brush,
          winding_fill: flags.contains(EmfPlusRecordFlags::WINDING_FILL),
          tension,
          points,
        })
      }
      Some(EmfPlusRecordType::DrawClosedCurve) if self.data.len() >= 8 => {
        let tension = reader.read_f32()?;
        let count = reader.read_u32()? as usize;
        require_count_at_least(count, 3, "EmfPlusDrawClosedCurve point")?;
        let points = read_points(&mut reader, count, flags, self.data.len() as u64)?;
        finish_record_point_array(
          &mut reader,
          self.data.len() as u64,
          &points,
          "EmfPlusDrawClosedCurve PointData",
        )?;
        EmfPlusRecordData::DrawClosedCurve(EmfPlusClosedCurveData {
          pen_id: flags.object_id(),
          tension,
          points,
        })
      }
      Some(EmfPlusRecordType::DrawCurve) if self.data.len() >= 16 => {
        let tension = reader.read_f32()?;
        let offset = reader.read_u32()?;
        let num_segments = reader.read_u32()?;
        let count = reader.read_u32()? as usize;
        require_count_at_least(count, 2, "EmfPlusDrawCurve point")?;
        let points = read_absolute_points(&mut reader, count, flags, self.data.len() as u64)?;
        finish_record_point_array(
          &mut reader,
          self.data.len() as u64,
          &points,
          "EmfPlusDrawCurve PointData",
        )?;
        EmfPlusRecordData::DrawCurve(EmfPlusDrawCurveData {
          pen_id: flags.object_id(),
          tension,
          offset,
          num_segments,
          points,
        })
      }
      Some(EmfPlusRecordType::DrawBeziers) if self.data.len() >= 4 => {
        let count = reader.read_u32()? as usize;
        validate_draw_beziers_point_count(count)?;
        let points = read_points(&mut reader, count, flags, self.data.len() as u64)?;
        finish_record_point_array(
          &mut reader,
          self.data.len() as u64,
          &points,
          "EmfPlusDrawBeziers PointData",
        )?;
        EmfPlusRecordData::DrawBeziers(EmfPlusDrawPointsData {
          pen_id: flags.object_id(),
          points,
        })
      }
      Some(EmfPlusRecordType::DrawImage) if self.data.len() >= 28 => {
        let image_attributes_id = reader.read_u32()?;
        let src_unit = reader.read_i32()?;
        let src_rect = RectF::read_from(&mut reader)?;
        let dest_rect = read_single_rect(&mut reader, flags, self.data.len() as u64)?;
        EmfPlusRecordData::DrawImage(EmfPlusDrawImageData {
          image_id: flags.object_id(),
          image_attributes_id,
          src_unit,
          src_rect,
          dest_rect,
        })
      }
      Some(EmfPlusRecordType::DrawImagePoints) if self.data.len() >= 28 => {
        let image_attributes_id = reader.read_u32()?;
        let src_unit = reader.read_i32()?;
        let src_rect = RectF::read_from(&mut reader)?;
        let count = reader.read_u32()? as usize;
        if count != 3 {
          return Err(Error::invalid(
            0,
            "EmfPlusDrawImagePoints point count must be 3",
          ));
        }
        let points = read_points(&mut reader, count, flags, self.data.len() as u64)?;
        finish_record_point_array(
          &mut reader,
          self.data.len() as u64,
          &points,
          "EmfPlusDrawImagePoints PointData",
        )?;
        EmfPlusRecordData::DrawImagePoints(EmfPlusDrawImagePointsData {
          image_id: flags.object_id(),
          apply_effect: flags.contains(EmfPlusRecordFlags::EFFECT),
          image_attributes_id,
          src_unit,
          src_rect,
          points,
        })
      }
      Some(EmfPlusRecordType::DrawString) if self.data.len() >= 28 => {
        let brush = read_brush_ref(&mut reader, flags)?;
        let format_id = reader.read_u32()?;
        let length = reader.read_u32()? as usize;
        let layout_rect = RectF::read_from(&mut reader)?;
        let string_len = length
          .checked_mul(2)
          .ok_or_else(|| Error::invalid(0, "EmfPlusDrawString string length overflows"))?;
        ensure_remaining(
          &mut reader,
          self.data.len() as u64,
          string_len,
          "EmfPlusDrawString string",
        )?;
        let string = SdkString::raw(reader.read_vec(string_len)?, SdkEncoding::Utf16Le);
        let position = reader.position()? as usize;
        let padding = self
          .data
          .get(position..)
          .ok_or_else(|| Error::invalid(0, "EmfPlusDrawString padding is out of bounds"))?
          .to_vec();
        EmfPlusRecordData::DrawString(EmfPlusDrawStringData {
          font_id: flags.object_id(),
          brush,
          format_id,
          layout_rect,
          string,
          padding,
        })
      }
      Some(EmfPlusRecordType::ResetClip) => {
        ensure_empty_data(self.data, "EmfPlusResetClip")?;
        EmfPlusRecordData::ResetClip
      }
      Some(EmfPlusRecordType::SetClipRect) => {
        EmfPlusRecordData::SetClipRect(EmfPlusSetClipRectData {
          combine_mode: flags.combine_mode_raw(),
          reserved_flags: flags.bits() & !0x0F00,
          clip_rect: read_exact_object(self.data, "EmfPlusSetClipRect")?,
        })
      }
      Some(EmfPlusRecordType::SetClipPath) => {
        ensure_empty_data(self.data, "EmfPlusSetClipPath")?;
        EmfPlusRecordData::SetClipPath(EmfPlusClipObjectData {
          combine_mode: flags.combine_mode_raw(),
          object_id: flags.object_id(),
          reserved_flags: flags.bits() & !0x0FFF,
        })
      }
      Some(EmfPlusRecordType::SetClipRegion) => {
        ensure_empty_data(self.data, "EmfPlusSetClipRegion")?;
        EmfPlusRecordData::SetClipRegion(EmfPlusClipObjectData {
          combine_mode: flags.combine_mode_raw(),
          object_id: flags.object_id(),
          reserved_flags: flags.bits() & !0x0FFF,
        })
      }
      Some(EmfPlusRecordType::OffsetClip) => {
        EmfPlusRecordData::OffsetClip(read_exact_object(self.data, "EmfPlusOffsetClip")?)
      }
      Some(EmfPlusRecordType::SetRenderingOrigin) => EmfPlusRecordData::SetRenderingOrigin(
        read_exact_object(self.data, "EmfPlusSetRenderingOrigin")?,
      ),
      Some(EmfPlusRecordType::SetAntiAliasMode) => {
        ensure_empty_data(self.data, "EmfPlusSetAntiAliasMode")?;
        EmfPlusRecordData::SetAntiAliasMode(EmfPlusSetAntiAliasModeData {
          smoothing_mode: flags.smoothing_mode_raw(),
          anti_alias: flags.anti_alias_enabled(),
          reserved_flags: flags.bits() & !0x00FF,
        })
      }
      Some(EmfPlusRecordType::SetTextRenderingHint) => {
        ensure_empty_data(self.data, "EmfPlusSetTextRenderingHint")?;
        EmfPlusRecordData::SetTextRenderingHint(EmfPlusU8PropertyData {
          value: flags.property_mode_raw(),
          reserved_flags: flags.bits() & !0x00FF,
        })
      }
      Some(EmfPlusRecordType::SetTextContrast) => {
        ensure_empty_data(self.data, "EmfPlusSetTextContrast")?;
        EmfPlusRecordData::SetTextContrast(EmfPlusSetTextContrastData {
          text_contrast: flags.text_contrast(),
          reserved_flags: flags.bits() & !0x0FFF,
        })
      }
      Some(EmfPlusRecordType::SetInterpolationMode) => {
        ensure_empty_data(self.data, "EmfPlusSetInterpolationMode")?;
        EmfPlusRecordData::SetInterpolationMode(EmfPlusU8PropertyData {
          value: flags.property_mode_raw(),
          reserved_flags: flags.bits() & !0x00FF,
        })
      }
      Some(EmfPlusRecordType::SetPixelOffsetMode) => {
        ensure_empty_data(self.data, "EmfPlusSetPixelOffsetMode")?;
        EmfPlusRecordData::SetPixelOffsetMode(EmfPlusU8PropertyData {
          value: flags.property_mode_raw(),
          reserved_flags: flags.bits() & !0x00FF,
        })
      }
      Some(EmfPlusRecordType::SetCompositingMode) => {
        ensure_empty_data(self.data, "EmfPlusSetCompositingMode")?;
        EmfPlusRecordData::SetCompositingMode(EmfPlusU8PropertyData {
          value: flags.property_mode_raw(),
          reserved_flags: flags.bits() & !0x00FF,
        })
      }
      Some(EmfPlusRecordType::SetCompositingQuality) => {
        ensure_empty_data(self.data, "EmfPlusSetCompositingQuality")?;
        EmfPlusRecordData::SetCompositingQuality(EmfPlusU8PropertyData {
          value: flags.property_mode_raw(),
          reserved_flags: flags.bits() & !0x00FF,
        })
      }
      Some(EmfPlusRecordType::Save) => {
        EmfPlusRecordData::Save(read_exact_object(self.data, "EmfPlusSave")?)
      }
      Some(EmfPlusRecordType::Restore) => {
        EmfPlusRecordData::Restore(read_exact_object(self.data, "EmfPlusRestore")?)
      }
      Some(EmfPlusRecordType::BeginContainer) => {
        EmfPlusRecordData::BeginContainer(read_exact_object(self.data, "EmfPlusBeginContainer")?)
      }
      Some(EmfPlusRecordType::BeginContainerNoParams) => EmfPlusRecordData::BeginContainerNoParams(
        read_exact_object(self.data, "EmfPlusBeginContainerNoParams")?,
      ),
      Some(EmfPlusRecordType::EndContainer) => {
        EmfPlusRecordData::EndContainer(read_exact_object(self.data, "EmfPlusEndContainer")?)
      }
      Some(EmfPlusRecordType::SetWorldTransform) => EmfPlusRecordData::SetWorldTransform(
        read_exact_object(self.data, "EmfPlusSetWorldTransform")?,
      ),
      Some(EmfPlusRecordType::ResetWorldTransform) => {
        ensure_empty_data(self.data, "EmfPlusResetWorldTransform")?;
        EmfPlusRecordData::ResetWorldTransform
      }
      Some(EmfPlusRecordType::MultiplyWorldTransform) => {
        EmfPlusRecordData::MultiplyWorldTransform(EmfPlusTransformOrderData::from_flags(
          read_exact_object(self.data, "EmfPlusMultiplyWorldTransform")?,
          flags,
        ))
      }
      Some(EmfPlusRecordType::TranslateWorldTransform) => {
        EmfPlusRecordData::TranslateWorldTransform(EmfPlusTransformOrderData::from_flags(
          read_exact_object(self.data, "EmfPlusTranslateWorldTransform")?,
          flags,
        ))
      }
      Some(EmfPlusRecordType::ScaleWorldTransform) => {
        EmfPlusRecordData::ScaleWorldTransform(EmfPlusTransformOrderData::from_flags(
          read_exact_object(self.data, "EmfPlusScaleWorldTransform")?,
          flags,
        ))
      }
      Some(EmfPlusRecordType::RotateWorldTransform) => {
        EmfPlusRecordData::RotateWorldTransform(EmfPlusTransformOrderData::from_flags(
          read_exact_object(self.data, "EmfPlusRotateWorldTransform")?,
          flags,
        ))
      }
      Some(EmfPlusRecordType::SetPageTransform) => EmfPlusRecordData::SetPageTransform(
        read_exact_object(self.data, "EmfPlusSetPageTransform")?,
      ),
      Some(EmfPlusRecordType::DrawDriverString) if self.data.len() >= 16 => {
        let brush = read_brush_ref(&mut reader, flags)?;
        let driver_string_options_flags = reader.read_u32()?;
        let matrix_present = reader.read_u32()?;
        if matrix_present > 1 {
          return Err(Error::invalid(
            0,
            "EmfPlusDrawDriverString MatrixPresent must be 0 or 1",
          ));
        }
        let glyph_count = reader.read_u32()? as usize;
        let driver_string_options =
          EmfPlusDriverStringOptionsFlags::from_bits_retain(driver_string_options_flags);
        validate_flag_bits(
          driver_string_options_flags,
          EmfPlusDriverStringOptionsFlags::all().bits(),
          "EmfPlusDrawDriverString DriverStringOptionsFlags",
        )?;
        let glyph_bytes = glyph_count
          .checked_mul(2)
          .ok_or_else(|| Error::invalid(0, "EmfPlusDrawDriverString glyph size overflows usize"))?;
        ensure_remaining(
          &mut reader,
          self.data.len() as u64,
          glyph_bytes,
          "EmfPlusDrawDriverString glyphs",
        )?;
        let mut glyphs = Vec::with_capacity(glyph_count);
        for _ in 0..glyph_count {
          glyphs.push(reader.read_u16()?);
        }
        let glyph_position_count =
          driver_string_glyph_position_count(glyph_count, driver_string_options);
        let glyph_position_bytes = glyph_position_count.checked_mul(8).ok_or_else(|| {
          Error::invalid(
            0,
            "EmfPlusDrawDriverString glyph position size overflows usize",
          )
        })?;
        ensure_remaining(
          &mut reader,
          self.data.len() as u64,
          glyph_position_bytes,
          "EmfPlusDrawDriverString glyph positions",
        )?;
        let mut glyph_positions = Vec::with_capacity(glyph_position_count);
        for _ in 0..glyph_position_count {
          glyph_positions.push(PointF::read_from(&mut reader)?);
        }
        let transform_matrix = if matrix_present == 0 {
          None
        } else {
          ensure_remaining(
            &mut reader,
            self.data.len() as u64,
            24,
            "EmfPlusDrawDriverString TransformMatrix",
          )?;
          Some(XForm::read_from(&mut reader)?)
        };
        ensure_reader_end(
          &mut reader,
          self.data.len() as u64,
          "EmfPlusDrawDriverString",
        )?;
        EmfPlusRecordData::DrawDriverString(EmfPlusDrawDriverStringData {
          font_id: flags.object_id(),
          brush,
          driver_string_options_flags,
          glyphs,
          glyph_positions,
          transform_matrix,
        })
      }
      Some(EmfPlusRecordType::StrokeFillPath) => {
        ensure_empty_data(self.data, "EmfPlusStrokeFillPath")?;
        EmfPlusRecordData::StrokeFillPath
      }
      Some(EmfPlusRecordType::SerializableObject) if self.data.len() >= 20 => {
        let object_guid = reader.read_array::<16>()?;
        let buffer_size = reader.read_u32()? as usize;
        ensure_remaining(
          &mut reader,
          self.data.len() as u64,
          buffer_size,
          "EmfPlusSerializableObject Buffer",
        )?;
        let buffer = reader.read_vec(buffer_size)?;
        ensure_reader_end(
          &mut reader,
          self.data.len() as u64,
          "EmfPlusSerializableObject",
        )?;
        EmfPlusRecordData::SerializableObject(EmfPlusSerializableObjectData {
          object_guid,
          buffer,
        })
      }
      Some(EmfPlusRecordType::SetTsGraphics) if self.data.len() >= 36 => {
        let anti_alias_mode = reader.read_u8()?;
        let text_render_hint = reader.read_u8()?;
        let compositing_mode = reader.read_u8()?;
        let compositing_quality = reader.read_u8()?;
        let render_origin_x = reader.read_i16()?;
        let render_origin_y = reader.read_i16()?;
        let text_contrast = reader.read_u16()?;
        let filter_type = reader.read_u8()?;
        let pixel_offset = reader.read_u8()?;
        let world_to_device = XForm::read_from(&mut reader)?;
        let palette = if flags.ts_graphics_palette_present() {
          let palette = read_emf_plus_palette(&mut reader, self.data.len() as u64)?;
          if !palette.trailing_data.is_empty() {
            return Err(Error::invalid(
              0,
              "EmfPlusSetTSGraphics Palette has trailing data",
            ));
          }
          Some(palette)
        } else {
          if reader.position()? != self.data.len() as u64 {
            return Err(Error::invalid(
              0,
              "EmfPlusSetTSGraphics has Palette data without Palette flag",
            ));
          }
          None
        };
        EmfPlusRecordData::SetTsGraphics(EmfPlusSetTsGraphicsData {
          anti_alias_mode,
          text_render_hint,
          compositing_mode,
          compositing_quality,
          render_origin_x,
          render_origin_y,
          text_contrast,
          filter_type,
          pixel_offset,
          world_to_device,
          palette,
        })
      }
      Some(EmfPlusRecordType::SetTsClip) => {
        EmfPlusRecordData::SetTsClip(EmfPlusSetTsClipData::read_data(self.flags, self.data)?)
      }
      Some(_) => {
        return Err(Error::invalid(
          0,
          "known EMF+ record data does not match its record type",
        ));
      }
      None => EmfPlusRecordData::Unknown(self),
    };

    validate_emf_plus_record_data(&data, flags)?;
    Ok(data)
  }

  pub fn rebuild_typed(self) -> Result<EmfPlusRecord> {
    let flags = self.flags();
    let padding = self.padding.to_vec();
    let data = self.parse_data()?;
    let mut record = EmfPlusRecord::from_data(&data, flags)?;
    record.padding = padding;
    Ok(record)
  }
}

impl EmfPlusRecord {
  pub fn from_data(data: &EmfPlusRecordData<'_>, flags: EmfPlusRecordFlags) -> Result<Self> {
    if let EmfPlusRecordData::Object(value) = data
      && value.continues != value.total_object_size.is_some()
    {
      return Err(Error::invalid(
        0,
        "EmfPlusObject continued flag requires TotalObjectSize",
      ));
    }

    let record_flags = data.record_flags(flags)?;
    validate_emf_plus_record_data(data, record_flags)?;

    let record_data_capacity = usize::try_from(data.sdk_size())
      .map_err(|_| Error::invalid(0, "EMF+ record data size overflows usize"))?;
    let mut record_data = Vec::with_capacity(record_data_capacity);
    {
      let mut writer = Writer::new(&mut record_data);
      match data {
        EmfPlusRecordData::Header(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::Eof => {}
        EmfPlusRecordData::Comment(value) => writer.write_all(value)?,
        EmfPlusRecordData::GetDc => {}
        EmfPlusRecordData::MultiFormatStart(value)
        | EmfPlusRecordData::MultiFormatSection(value)
        | EmfPlusRecordData::MultiFormatEnd(value) => writer.write_all(&value.data)?,
        EmfPlusRecordData::Object(value) => writer.write_all(&value.object_data)?,
        EmfPlusRecordData::Clear(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::FillRects(value) => {
          require_count_at_least(value.rects.len(), 1, "EmfPlusFillRects rect")?;
          write_brush_ref(&mut writer, value.brush)?;
          writer.write_u32(len_to_u32(value.rects.len(), "EMF+ rect count")?)?;
          write_rects(&mut writer, &value.rects)?;
        }
        EmfPlusRecordData::DrawRects(value) => {
          require_count_at_least(value.rects.len(), 1, "EmfPlusDrawRects rect")?;
          writer.write_u32(len_to_u32(value.rects.len(), "EMF+ rect count")?)?;
          write_rects(&mut writer, &value.rects)?;
        }
        EmfPlusRecordData::FillPolygon(value) => {
          require_count_at_least(value.points.len(), 3, "EmfPlusFillPolygon point")?;
          write_brush_ref(&mut writer, value.brush)?;
          writer.write_u32(len_to_u32(value.points.len(), "EMF+ point count")?)?;
          write_record_points(&mut writer, &value.points)?;
        }
        EmfPlusRecordData::DrawLines(value) => {
          require_count_at_least(value.points.len(), 2, "EmfPlusDrawLines point")?;
          writer.write_u32(len_to_u32(value.points.len(), "EMF+ point count")?)?;
          write_record_points(&mut writer, &value.points)?;
        }
        EmfPlusRecordData::DrawBeziers(value) => {
          validate_draw_beziers_point_count(value.points.len())?;
          writer.write_u32(len_to_u32(value.points.len(), "EMF+ point count")?)?;
          write_record_points(&mut writer, &value.points)?;
        }
        EmfPlusRecordData::FillEllipse(value) => {
          write_brush_ref(&mut writer, value.brush)?;
          write_rects(&mut writer, std::slice::from_ref(&value.rect))?;
        }
        EmfPlusRecordData::DrawEllipse(value) => {
          write_rects(&mut writer, std::slice::from_ref(&value.rect))?;
        }
        EmfPlusRecordData::FillPie(value) => {
          write_brush_ref(&mut writer, value.brush)?;
          writer.write_f32(value.start_angle)?;
          writer.write_f32(value.sweep_angle)?;
          write_rects(&mut writer, std::slice::from_ref(&value.rect))?;
        }
        EmfPlusRecordData::DrawPie(value) | EmfPlusRecordData::DrawArc(value) => {
          writer.write_f32(value.start_angle)?;
          writer.write_f32(value.sweep_angle)?;
          write_rects(&mut writer, std::slice::from_ref(&value.rect))?;
        }
        EmfPlusRecordData::FillRegion(value) | EmfPlusRecordData::FillPath(value) => {
          write_brush_ref(&mut writer, value.brush)?;
        }
        EmfPlusRecordData::DrawPath(value) => writer.write_u32(u32::from(value.pen_id))?,
        EmfPlusRecordData::FillClosedCurve(value) => {
          require_count_at_least(value.points.len(), 3, "EmfPlusFillClosedCurve point")?;
          write_brush_ref(&mut writer, value.brush)?;
          writer.write_f32(value.tension)?;
          writer.write_u32(len_to_u32(value.points.len(), "EMF+ point count")?)?;
          write_record_points(&mut writer, &value.points)?;
        }
        EmfPlusRecordData::DrawClosedCurve(value) => {
          require_count_at_least(value.points.len(), 3, "EmfPlusDrawClosedCurve point")?;
          writer.write_f32(value.tension)?;
          writer.write_u32(len_to_u32(value.points.len(), "EMF+ point count")?)?;
          write_record_points(&mut writer, &value.points)?;
        }
        EmfPlusRecordData::DrawCurve(value) => {
          require_count_at_least(value.points.len(), 2, "EmfPlusDrawCurve point")?;
          writer.write_f32(value.tension)?;
          writer.write_u32(value.offset)?;
          writer.write_u32(value.num_segments)?;
          writer.write_u32(len_to_u32(value.points.len(), "EMF+ point count")?)?;
          write_record_points(&mut writer, &value.points)?;
        }
        EmfPlusRecordData::DrawImage(value) => {
          writer.write_u32(value.image_attributes_id)?;
          writer.write_i32(value.src_unit)?;
          value.src_rect.write_to(&mut writer)?;
          write_rects(&mut writer, std::slice::from_ref(&value.dest_rect))?;
        }
        EmfPlusRecordData::DrawImagePoints(value) => {
          if value.points.len() != 3 {
            return Err(Error::invalid(
              0,
              "EmfPlusDrawImagePoints point count must be 3",
            ));
          }
          writer.write_u32(value.image_attributes_id)?;
          writer.write_i32(value.src_unit)?;
          value.src_rect.write_to(&mut writer)?;
          writer.write_u32(len_to_u32(value.points.len(), "EMF+ point count")?)?;
          write_record_points(&mut writer, &value.points)?;
        }
        EmfPlusRecordData::DrawString(value) => {
          write_brush_ref(&mut writer, value.brush)?;
          writer.write_u32(value.format_id)?;
          let string_bytes = value.string.encoded_bytes()?;
          if !string_bytes.len().is_multiple_of(2) {
            return Err(Error::invalid(
              0,
              "EmfPlusDrawString UTF-16 byte length is odd",
            ));
          }
          writer.write_u32(len_to_u32(string_bytes.len() / 2, "EMF+ string length")?)?;
          value.layout_rect.write_to(&mut writer)?;
          writer.write_all(&string_bytes)?;
          writer.write_all(&value.padding)?;
        }
        EmfPlusRecordData::ResetClip => {}
        EmfPlusRecordData::SetClipRect(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::SetClipPath(_) => {}
        EmfPlusRecordData::SetClipRegion(_) => {}
        EmfPlusRecordData::OffsetClip(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::SetRenderingOrigin(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::SetAntiAliasMode(_)
        | EmfPlusRecordData::SetTextRenderingHint(_)
        | EmfPlusRecordData::SetTextContrast(_)
        | EmfPlusRecordData::SetInterpolationMode(_)
        | EmfPlusRecordData::SetPixelOffsetMode(_)
        | EmfPlusRecordData::SetCompositingMode(_)
        | EmfPlusRecordData::SetCompositingQuality(_) => {}
        EmfPlusRecordData::Save(value)
        | EmfPlusRecordData::Restore(value)
        | EmfPlusRecordData::BeginContainerNoParams(value)
        | EmfPlusRecordData::EndContainer(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::BeginContainer(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::SetWorldTransform(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::ResetWorldTransform => {}
        EmfPlusRecordData::MultiplyWorldTransform(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::TranslateWorldTransform(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::ScaleWorldTransform(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::RotateWorldTransform(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::SetPageTransform(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::DrawDriverString(value) => {
          write_brush_ref(&mut writer, value.brush)?;
          writer.write_u32(value.driver_string_options_flags)?;
          writer.write_u32(u32::from(value.transform_matrix.is_some()))?;
          writer.write_u32(len_to_u32(value.glyphs.len(), "EMF+ glyph count")?)?;
          if value.glyph_positions.len() != value.expected_glyph_position_count() {
            return Err(Error::invalid(
              0,
              "EmfPlusDrawDriverString glyph position count mismatch",
            ));
          }
          for glyph in &value.glyphs {
            writer.write_u16(*glyph)?;
          }
          for position in &value.glyph_positions {
            position.write_to(&mut writer)?;
          }
          if let Some(transform_matrix) = &value.transform_matrix {
            transform_matrix.write_to(&mut writer)?;
          }
        }
        EmfPlusRecordData::StrokeFillPath => {}
        EmfPlusRecordData::SerializableObject(value) => {
          writer.write_all(&value.object_guid)?;
          writer.write_u32(len_to_u32(value.buffer.len(), "EMF+ serializable buffer")?)?;
          writer.write_all(&value.buffer)?;
        }
        EmfPlusRecordData::SetTsGraphics(value) => {
          writer.write_u8(value.anti_alias_mode)?;
          writer.write_u8(value.text_render_hint)?;
          writer.write_u8(value.compositing_mode)?;
          writer.write_u8(value.compositing_quality)?;
          writer.write_i16(value.render_origin_x)?;
          writer.write_i16(value.render_origin_y)?;
          writer.write_u16(value.text_contrast)?;
          writer.write_u8(value.filter_type)?;
          writer.write_u8(value.pixel_offset)?;
          value.world_to_device.write_to(&mut writer)?;
          if let Some(palette) = &value.palette {
            palette.write_to(&mut writer)?;
          }
        }
        EmfPlusRecordData::SetTsClip(value) => value.write_to(&mut writer)?,
        EmfPlusRecordData::Unknown(record) => {
          validate_unknown_emf_plus_record(record.record_type)?;
          return Ok(EmfPlusRecordRef::into_owned(*record));
        }
      }
    }

    if !record_data.len().is_multiple_of(4) {
      return Err(Error::invalid(
        0,
        "EMF+ typed record DataSize must be 32-bit aligned",
      ));
    }

    Ok(Self {
      record_type: data.record_type(),
      flags: record_flags.bits(),
      total_object_size: match data {
        EmfPlusRecordData::Object(value) => value.total_object_size,
        _ => None,
      },
      data: record_data,
      padding: Vec::new(),
    })
  }

  pub fn read_from<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    stream_len: u64,
  ) -> Result<Self> {
    let offset = reader.position()?;
    let record_type = reader.read_u16()?;
    let flags = reader.read_u16()?;
    let size = reader.read_u32()?;
    let data_size = reader.read_u32()?;
    let record_flags = EmfPlusRecordFlags::from_bits_retain(flags);
    let has_total_object_size =
      record_type == EmfPlusRecordType::Object.raw() && record_flags.object_continues();

    const HEADER_SIZE: u32 = 12;
    if size < HEADER_SIZE {
      return Err(Error::invalid(
        offset,
        "EMF+ record size is smaller than its header",
      ));
    }
    if !size.is_multiple_of(4) {
      return Err(Error::invalid(
        offset,
        "EMF+ record size is not 32-bit aligned",
      ));
    }
    if data_size > size - HEADER_SIZE {
      return Err(Error::invalid(
        offset,
        "EMF+ record data size exceeds record size",
      ));
    }
    if has_total_object_size && data_size < 4 {
      return Err(Error::invalid(
        offset,
        "continued EmfPlusObject DataSize omits TotalObjectSize",
      ));
    }
    let total_object_size = if has_total_object_size {
      Some(reader.read_u32()?)
    } else {
      None
    };
    let object_data_size = data_size - u32::from(has_total_object_size) * 4;
    if let Some(total_object_size) = total_object_size
      && total_object_size < object_data_size
    {
      return Err(Error::invalid(
        offset,
        "EmfPlusObject TotalObjectSize is smaller than ObjectData",
      ));
    }
    let end = offset
      .checked_add(size as u64)
      .ok_or_else(|| Error::invalid(offset, "EMF+ record size overflows"))?;
    if end > stream_len {
      return Err(Error::invalid(
        offset,
        "EMF+ record extends past end of stream",
      ));
    }

    let data = reader.read_vec(object_data_size as usize)?;
    let padding_len = size as usize - HEADER_SIZE as usize - data_size as usize;
    validate_emf_plus_record_padding_len(padding_len)?;
    let padding = reader.read_vec(padding_len)?;

    Ok(Self {
      record_type,
      flags,
      total_object_size,
      data,
      padding,
    })
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    let has_continued_object_header =
      self.record_type == EmfPlusRecordType::Object.raw() && self.flags().object_continues();
    if has_continued_object_header != self.total_object_size.is_some() {
      return Err(Error::invalid(
        writer.position().unwrap_or(0),
        "EmfPlusObject continued header and TotalObjectSize disagree",
      ));
    }
    if let Some(total_object_size) = self.total_object_size
      && (total_object_size as usize) < self.data.len()
    {
      return Err(Error::invalid(
        writer.position().unwrap_or(0),
        "EmfPlusObject TotalObjectSize is smaller than ObjectData",
      ));
    }
    validate_emf_plus_record_padding_len(self.padding.len())?;

    let record_data_size = self
      .data
      .len()
      .checked_add(usize::from(self.total_object_size.is_some()) * 4)
      .ok_or_else(|| Error::invalid(writer.position().unwrap_or(0), "EMF+ record is too large"))?;
    let payload_size = record_data_size
      .checked_add(self.padding.len())
      .ok_or_else(|| Error::invalid(writer.position().unwrap_or(0), "EMF+ record is too large"))?;
    const HEADER_SIZE: usize = 12;
    let size = payload_size
      .checked_add(HEADER_SIZE)
      .ok_or_else(|| Error::invalid(writer.position().unwrap_or(0), "EMF+ record is too large"))?;
    if size > u32::MAX as usize {
      return Err(Error::invalid(
        writer.position()?,
        "EMF+ record size exceeds u32::MAX",
      ));
    }
    if !size.is_multiple_of(4) {
      return Err(Error::invalid(
        writer.position()?,
        "EMF+ record size must be 32-bit aligned",
      ));
    }
    writer.write_u16(self.record_type)?;
    writer.write_u16(self.flags)?;
    writer.write_u32(size as u32)?;
    writer.write_u32(record_data_size as u32)?;
    if let Some(total_object_size) = self.total_object_size {
      writer.write_u32(total_object_size)?;
    }
    writer.write_all(&self.data)?;
    writer.write_all(&self.padding)
  }
}

impl SdkSize for EmfPlusRecord {
  fn sdk_size(&self) -> u64 {
    12 + u64::from(self.total_object_size.is_some()) * 4
      + self.data.len() as u64
      + self.padding.len() as u64
  }
}

impl EmfPlusRecordData<'_> {
  pub fn validate_strict(&self) -> Result<()> {
    match self {
      Self::Object(value) if !value.continues => value.parse_object_data()?.validate_strict(),
      Self::DrawImage(value) => {
        validate_draw_image_src_unit(value.src_unit, "EmfPlusDrawImage SrcUnit")
      }
      Self::DrawImagePoints(value) => {
        validate_draw_image_src_unit(value.src_unit, "EmfPlusDrawImagePoints SrcUnit")?;
        validate_object_id_u32(
          value.image_attributes_id,
          "EmfPlusDrawImagePoints ImageAttributesID",
        )
      }
      Self::DrawString(value) => {
        validate_object_id_u32(value.format_id, "EmfPlusDrawString FormatID")
      }
      Self::SetCompositingQuality(value) if value.compositing_quality().is_none() => {
        Err(Error::invalid(
          0,
          "EmfPlusSetCompositingQuality CompositingQuality is invalid",
        ))
      }
      _ => Ok(()),
    }
  }

  pub fn record_type(&self) -> u16 {
    match self {
      Self::Header(_) => EmfPlusRecordType::Header.raw(),
      Self::Eof => EmfPlusRecordType::Eof.raw(),
      Self::Comment(_) => EmfPlusRecordType::Comment.raw(),
      Self::GetDc => EmfPlusRecordType::GetDc.raw(),
      Self::MultiFormatStart(_) => EmfPlusRecordType::MultiFormatStart.raw(),
      Self::MultiFormatSection(_) => EmfPlusRecordType::MultiFormatSection.raw(),
      Self::MultiFormatEnd(_) => EmfPlusRecordType::MultiFormatEnd.raw(),
      Self::Object(_) => EmfPlusRecordType::Object.raw(),
      Self::Clear(_) => EmfPlusRecordType::Clear.raw(),
      Self::FillRects(_) => EmfPlusRecordType::FillRects.raw(),
      Self::DrawRects(_) => EmfPlusRecordType::DrawRects.raw(),
      Self::FillPolygon(_) => EmfPlusRecordType::FillPolygon.raw(),
      Self::DrawLines(_) => EmfPlusRecordType::DrawLines.raw(),
      Self::FillEllipse(_) => EmfPlusRecordType::FillEllipse.raw(),
      Self::DrawEllipse(_) => EmfPlusRecordType::DrawEllipse.raw(),
      Self::FillPie(_) => EmfPlusRecordType::FillPie.raw(),
      Self::DrawPie(_) => EmfPlusRecordType::DrawPie.raw(),
      Self::DrawArc(_) => EmfPlusRecordType::DrawArc.raw(),
      Self::FillRegion(_) => EmfPlusRecordType::FillRegion.raw(),
      Self::FillPath(_) => EmfPlusRecordType::FillPath.raw(),
      Self::DrawPath(_) => EmfPlusRecordType::DrawPath.raw(),
      Self::FillClosedCurve(_) => EmfPlusRecordType::FillClosedCurve.raw(),
      Self::DrawClosedCurve(_) => EmfPlusRecordType::DrawClosedCurve.raw(),
      Self::DrawCurve(_) => EmfPlusRecordType::DrawCurve.raw(),
      Self::DrawBeziers(_) => EmfPlusRecordType::DrawBeziers.raw(),
      Self::DrawImage(_) => EmfPlusRecordType::DrawImage.raw(),
      Self::DrawImagePoints(_) => EmfPlusRecordType::DrawImagePoints.raw(),
      Self::DrawString(_) => EmfPlusRecordType::DrawString.raw(),
      Self::ResetClip => EmfPlusRecordType::ResetClip.raw(),
      Self::SetClipRect(_) => EmfPlusRecordType::SetClipRect.raw(),
      Self::SetClipPath(_) => EmfPlusRecordType::SetClipPath.raw(),
      Self::SetClipRegion(_) => EmfPlusRecordType::SetClipRegion.raw(),
      Self::OffsetClip(_) => EmfPlusRecordType::OffsetClip.raw(),
      Self::SetRenderingOrigin(_) => EmfPlusRecordType::SetRenderingOrigin.raw(),
      Self::SetAntiAliasMode(_) => EmfPlusRecordType::SetAntiAliasMode.raw(),
      Self::SetTextRenderingHint(_) => EmfPlusRecordType::SetTextRenderingHint.raw(),
      Self::SetTextContrast(_) => EmfPlusRecordType::SetTextContrast.raw(),
      Self::SetInterpolationMode(_) => EmfPlusRecordType::SetInterpolationMode.raw(),
      Self::SetPixelOffsetMode(_) => EmfPlusRecordType::SetPixelOffsetMode.raw(),
      Self::SetCompositingMode(_) => EmfPlusRecordType::SetCompositingMode.raw(),
      Self::SetCompositingQuality(_) => EmfPlusRecordType::SetCompositingQuality.raw(),
      Self::Save(_) => EmfPlusRecordType::Save.raw(),
      Self::Restore(_) => EmfPlusRecordType::Restore.raw(),
      Self::BeginContainer(_) => EmfPlusRecordType::BeginContainer.raw(),
      Self::BeginContainerNoParams(_) => EmfPlusRecordType::BeginContainerNoParams.raw(),
      Self::EndContainer(_) => EmfPlusRecordType::EndContainer.raw(),
      Self::SetWorldTransform(_) => EmfPlusRecordType::SetWorldTransform.raw(),
      Self::ResetWorldTransform => EmfPlusRecordType::ResetWorldTransform.raw(),
      Self::MultiplyWorldTransform(_) => EmfPlusRecordType::MultiplyWorldTransform.raw(),
      Self::TranslateWorldTransform(_) => EmfPlusRecordType::TranslateWorldTransform.raw(),
      Self::ScaleWorldTransform(_) => EmfPlusRecordType::ScaleWorldTransform.raw(),
      Self::RotateWorldTransform(_) => EmfPlusRecordType::RotateWorldTransform.raw(),
      Self::SetPageTransform(_) => EmfPlusRecordType::SetPageTransform.raw(),
      Self::DrawDriverString(_) => EmfPlusRecordType::DrawDriverString.raw(),
      Self::StrokeFillPath => EmfPlusRecordType::StrokeFillPath.raw(),
      Self::SerializableObject(_) => EmfPlusRecordType::SerializableObject.raw(),
      Self::SetTsGraphics(_) => EmfPlusRecordType::SetTsGraphics.raw(),
      Self::SetTsClip(_) => EmfPlusRecordType::SetTsClip.raw(),
      Self::Unknown(record) => record.record_type,
    }
  }

  pub fn record_kind(&self) -> Option<EmfPlusRecordType> {
    match self {
      Self::Header(_) => Some(EmfPlusRecordType::Header),
      Self::Eof => Some(EmfPlusRecordType::Eof),
      Self::Comment(_) => Some(EmfPlusRecordType::Comment),
      Self::GetDc => Some(EmfPlusRecordType::GetDc),
      Self::MultiFormatStart(_) => Some(EmfPlusRecordType::MultiFormatStart),
      Self::MultiFormatSection(_) => Some(EmfPlusRecordType::MultiFormatSection),
      Self::MultiFormatEnd(_) => Some(EmfPlusRecordType::MultiFormatEnd),
      Self::Object(_) => Some(EmfPlusRecordType::Object),
      Self::Clear(_) => Some(EmfPlusRecordType::Clear),
      Self::FillRects(_) => Some(EmfPlusRecordType::FillRects),
      Self::DrawRects(_) => Some(EmfPlusRecordType::DrawRects),
      Self::FillPolygon(_) => Some(EmfPlusRecordType::FillPolygon),
      Self::DrawLines(_) => Some(EmfPlusRecordType::DrawLines),
      Self::FillEllipse(_) => Some(EmfPlusRecordType::FillEllipse),
      Self::DrawEllipse(_) => Some(EmfPlusRecordType::DrawEllipse),
      Self::FillPie(_) => Some(EmfPlusRecordType::FillPie),
      Self::DrawPie(_) => Some(EmfPlusRecordType::DrawPie),
      Self::DrawArc(_) => Some(EmfPlusRecordType::DrawArc),
      Self::FillRegion(_) => Some(EmfPlusRecordType::FillRegion),
      Self::FillPath(_) => Some(EmfPlusRecordType::FillPath),
      Self::DrawPath(_) => Some(EmfPlusRecordType::DrawPath),
      Self::FillClosedCurve(_) => Some(EmfPlusRecordType::FillClosedCurve),
      Self::DrawClosedCurve(_) => Some(EmfPlusRecordType::DrawClosedCurve),
      Self::DrawCurve(_) => Some(EmfPlusRecordType::DrawCurve),
      Self::DrawBeziers(_) => Some(EmfPlusRecordType::DrawBeziers),
      Self::DrawImage(_) => Some(EmfPlusRecordType::DrawImage),
      Self::DrawImagePoints(_) => Some(EmfPlusRecordType::DrawImagePoints),
      Self::DrawString(_) => Some(EmfPlusRecordType::DrawString),
      Self::ResetClip => Some(EmfPlusRecordType::ResetClip),
      Self::SetClipRect(_) => Some(EmfPlusRecordType::SetClipRect),
      Self::SetClipPath(_) => Some(EmfPlusRecordType::SetClipPath),
      Self::SetClipRegion(_) => Some(EmfPlusRecordType::SetClipRegion),
      Self::OffsetClip(_) => Some(EmfPlusRecordType::OffsetClip),
      Self::SetRenderingOrigin(_) => Some(EmfPlusRecordType::SetRenderingOrigin),
      Self::SetAntiAliasMode(_) => Some(EmfPlusRecordType::SetAntiAliasMode),
      Self::SetTextRenderingHint(_) => Some(EmfPlusRecordType::SetTextRenderingHint),
      Self::SetTextContrast(_) => Some(EmfPlusRecordType::SetTextContrast),
      Self::SetInterpolationMode(_) => Some(EmfPlusRecordType::SetInterpolationMode),
      Self::SetPixelOffsetMode(_) => Some(EmfPlusRecordType::SetPixelOffsetMode),
      Self::SetCompositingMode(_) => Some(EmfPlusRecordType::SetCompositingMode),
      Self::SetCompositingQuality(_) => Some(EmfPlusRecordType::SetCompositingQuality),
      Self::Save(_) => Some(EmfPlusRecordType::Save),
      Self::Restore(_) => Some(EmfPlusRecordType::Restore),
      Self::BeginContainer(_) => Some(EmfPlusRecordType::BeginContainer),
      Self::BeginContainerNoParams(_) => Some(EmfPlusRecordType::BeginContainerNoParams),
      Self::EndContainer(_) => Some(EmfPlusRecordType::EndContainer),
      Self::SetWorldTransform(_) => Some(EmfPlusRecordType::SetWorldTransform),
      Self::ResetWorldTransform => Some(EmfPlusRecordType::ResetWorldTransform),
      Self::MultiplyWorldTransform(_) => Some(EmfPlusRecordType::MultiplyWorldTransform),
      Self::TranslateWorldTransform(_) => Some(EmfPlusRecordType::TranslateWorldTransform),
      Self::ScaleWorldTransform(_) => Some(EmfPlusRecordType::ScaleWorldTransform),
      Self::RotateWorldTransform(_) => Some(EmfPlusRecordType::RotateWorldTransform),
      Self::SetPageTransform(_) => Some(EmfPlusRecordType::SetPageTransform),
      Self::DrawDriverString(_) => Some(EmfPlusRecordType::DrawDriverString),
      Self::StrokeFillPath => Some(EmfPlusRecordType::StrokeFillPath),
      Self::SerializableObject(_) => Some(EmfPlusRecordType::SerializableObject),
      Self::SetTsGraphics(_) => Some(EmfPlusRecordType::SetTsGraphics),
      Self::SetTsClip(_) => Some(EmfPlusRecordType::SetTsClip),
      Self::Unknown(record) => record.record_kind(),
    }
  }

  pub fn sdk_size(&self) -> u64 {
    match self {
      Self::Header(value) => value.sdk_size(),
      Self::Eof | Self::GetDc => 0,
      Self::Comment(value) => value.len() as u64,
      Self::MultiFormatStart(value)
      | Self::MultiFormatSection(value)
      | Self::MultiFormatEnd(value) => value.data.len() as u64,
      Self::Object(value) => value.object_data.len() as u64,
      Self::Clear(value) => value.sdk_size(),
      Self::FillRects(value) => 8 + value.rects.iter().map(EmfPlusRect::sdk_size).sum::<u64>(),
      Self::DrawRects(value) => 4 + value.rects.iter().map(EmfPlusRect::sdk_size).sum::<u64>(),
      Self::FillPolygon(value) => record_point_payload_size(8, &value.points),
      Self::DrawLines(value) => record_point_payload_size(4, &value.points),
      Self::DrawBeziers(value) => record_point_payload_size(4, &value.points),
      Self::FillEllipse(value) => 4 + value.rect.sdk_size(),
      Self::DrawEllipse(value) => value.rect.sdk_size(),
      Self::FillPie(value) => 12 + value.rect.sdk_size(),
      Self::DrawPie(value) | Self::DrawArc(value) => 8 + value.rect.sdk_size(),
      Self::FillRegion(_) | Self::FillPath(_) => 4,
      Self::DrawPath(_) => 4,
      Self::FillClosedCurve(value) => record_point_payload_size(12, &value.points),
      Self::DrawClosedCurve(value) => record_point_payload_size(8, &value.points),
      Self::DrawCurve(value) => record_point_payload_size(16, &value.points),
      Self::DrawImage(value) => 24 + value.dest_rect.sdk_size(),
      Self::DrawImagePoints(value) => record_point_payload_size(28, &value.points),
      Self::DrawString(value) => {
        28 + sdk_string_current_size(&value.string) + value.padding.len() as u64
      }
      Self::ResetClip | Self::SetClipPath(_) | Self::SetClipRegion(_) => 0,
      Self::SetClipRect(value) => value.sdk_size(),
      Self::OffsetClip(value) => value.sdk_size(),
      Self::SetRenderingOrigin(value) => value.sdk_size(),
      Self::SetAntiAliasMode(_)
      | Self::SetTextRenderingHint(_)
      | Self::SetTextContrast(_)
      | Self::SetInterpolationMode(_)
      | Self::SetPixelOffsetMode(_)
      | Self::SetCompositingMode(_)
      | Self::SetCompositingQuality(_) => 0,
      Self::Save(value)
      | Self::Restore(value)
      | Self::BeginContainerNoParams(value)
      | Self::EndContainer(value) => value.sdk_size(),
      Self::BeginContainer(value) => value.sdk_size(),
      Self::SetWorldTransform(value) => value.sdk_size(),
      Self::MultiplyWorldTransform(value) => value.sdk_size(),
      Self::ResetWorldTransform => 0,
      Self::TranslateWorldTransform(value) => value.sdk_size(),
      Self::ScaleWorldTransform(value) => value.sdk_size(),
      Self::RotateWorldTransform(value) => value.sdk_size(),
      Self::SetPageTransform(value) => value.sdk_size(),
      Self::DrawDriverString(value) => {
        16 + value.glyphs.len() as u64 * 2
          + value.expected_glyph_position_count() as u64 * 8
          + value.transform_matrix.as_ref().map_or(0, SdkSize::sdk_size)
      }
      Self::StrokeFillPath => 0,
      Self::SerializableObject(value) => 20 + value.buffer.len() as u64,
      Self::SetTsGraphics(value) => {
        36 + value.palette.as_ref().map_or(0, |palette| {
          8 + palette.entries.len() as u64 * 4 + palette.trailing_data.len() as u64
        })
      }
      Self::SetTsClip(value) => value.sdk_size(),
      Self::Unknown(record) => record.data.len() as u64,
    }
  }

  fn record_flags(&self, flags: EmfPlusRecordFlags) -> Result<EmfPlusRecordFlags> {
    Ok(match self {
      Self::Object(value) => {
        validate_object_id_u8(value.object_id, "EmfPlusObject ObjectID")?;
        match value.object_type() {
          Some(EmfPlusObjectType::Invalid) | None => {
            return Err(Error::invalid(0, "EmfPlusObject ObjectType is invalid"));
          }
          Some(_) => {}
        }
        EmfPlusRecordFlags::from_bits_retain(
          u16::from(value.object_id)
            | (u16::from(value.object_type_raw) << 8)
            | if value.continues { 0x8000 } else { 0 },
        )
      }
      Self::FillRects(value) => {
        let next = set_brush_flags_checked(
          EmfPlusRecordFlags::empty(),
          value.brush,
          "EmfPlusFillRects BrushId",
        )?;
        set_rect_flags(next, &value.rects)
      }
      Self::DrawRects(value) => {
        let next = object_id_flags(value.pen_id, "EmfPlusDrawRects ObjectID")?;
        set_rect_flags(next, &value.rects)
      }
      Self::FillPolygon(value) => {
        let next = set_brush_flags_checked(
          EmfPlusRecordFlags::empty(),
          value.brush,
          "EmfPlusFillPolygon BrushId",
        )?;
        set_point_flags(next, &value.points)
      }
      Self::DrawLines(value) => {
        let mut next = object_id_flags(value.pen_id, "EmfPlusDrawLines ObjectID")?;
        next.set(EmfPlusRecordFlags::CLOSE_SHAPE, value.close_shape);
        set_point_flags(next, &value.points)
      }
      Self::DrawBeziers(value) => {
        let next = object_id_flags(value.pen_id, "EmfPlusDrawBeziers ObjectID")?;
        set_point_flags(next, &value.points)
      }
      Self::FillEllipse(value) => {
        let next = set_brush_flags_checked(
          EmfPlusRecordFlags::empty(),
          value.brush,
          "EmfPlusFillEllipse BrushId",
        )?;
        set_rect_flags(next, std::slice::from_ref(&value.rect))
      }
      Self::DrawEllipse(value) => {
        let next = object_id_flags(value.pen_id, "EmfPlusDrawEllipse ObjectID")?;
        set_rect_flags(next, std::slice::from_ref(&value.rect))
      }
      Self::FillPie(value) => {
        validate_start_angle(value.start_angle, "EmfPlusFillPie StartAngle")?;
        let next = set_brush_flags_checked(
          EmfPlusRecordFlags::empty(),
          value.brush,
          "EmfPlusFillPie BrushId",
        )?;
        set_rect_flags(next, std::slice::from_ref(&value.rect))
      }
      Self::DrawPie(value) => {
        validate_start_angle(value.start_angle, "EmfPlusDrawPie StartAngle")?;
        let next = object_id_flags(value.pen_id, "EmfPlusDrawPie ObjectID")?;
        set_rect_flags(next, std::slice::from_ref(&value.rect))
      }
      Self::DrawArc(value) => {
        validate_start_angle(value.start_angle, "EmfPlusDrawArc StartAngle")?;
        let next = object_id_flags(value.pen_id, "EmfPlusDrawArc ObjectID")?;
        set_rect_flags(next, std::slice::from_ref(&value.rect))
      }
      Self::FillRegion(value) => {
        let next = object_id_flags(value.object_id, "EmfPlusFillRegion ObjectID")?;
        set_brush_flags_checked(next, value.brush, "EmfPlusFillRegion BrushId")?
      }
      Self::FillPath(value) => {
        let next = object_id_flags(value.object_id, "EmfPlusFillPath ObjectID")?;
        set_brush_flags_checked(next, value.brush, "EmfPlusFillPath BrushId")?
      }
      Self::DrawPath(value) => object_id_flags(value.object_id, "EmfPlusDrawPath ObjectID")?,
      Self::FillClosedCurve(value) => {
        let mut next = set_brush_flags_checked(
          EmfPlusRecordFlags::empty(),
          value.brush,
          "EmfPlusFillClosedCurve BrushId",
        )?;
        next.set(EmfPlusRecordFlags::WINDING_FILL, value.winding_fill);
        set_point_flags(next, &value.points)
      }
      Self::DrawClosedCurve(value) => {
        let next = object_id_flags(value.pen_id, "EmfPlusDrawClosedCurve ObjectID")?;
        set_point_flags(next, &value.points)
      }
      Self::DrawCurve(value) => {
        validate_draw_curve_data(value)?;
        let next = object_id_flags(value.pen_id, "EmfPlusDrawCurve ObjectID")?;
        set_absolute_point_flags(next, &value.points)
      }
      Self::DrawImage(value) => {
        validate_object_id_u32(
          value.image_attributes_id,
          "EmfPlusDrawImage ImageAttributesID",
        )?;
        let next = object_id_flags(value.image_id, "EmfPlusDrawImage ObjectID")?;
        set_rect_flags(next, std::slice::from_ref(&value.dest_rect))
      }
      Self::DrawImagePoints(value) => {
        let mut next = object_id_flags(value.image_id, "EmfPlusDrawImagePoints ObjectID")?;
        next.set(EmfPlusRecordFlags::EFFECT, value.apply_effect);
        set_point_flags(next, &value.points)
      }
      Self::DrawString(value) => {
        let next = object_id_flags(value.font_id, "EmfPlusDrawString ObjectID")?;
        set_brush_flags_checked(next, value.brush, "EmfPlusDrawString BrushId")?
      }
      Self::DrawDriverString(value) => {
        validate_flag_bits(
          value.driver_string_options_flags,
          EmfPlusDriverStringOptionsFlags::all().bits(),
          "EmfPlusDrawDriverString DriverStringOptionsFlags",
        )?;
        let next = object_id_flags(value.font_id, "EmfPlusDrawDriverString ObjectID")?;
        set_brush_flags_checked(next, value.brush, "EmfPlusDrawDriverString BrushId")?
      }
      Self::SetClipRect(value) => EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits()?),
      Self::SetClipPath(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits("EmfPlusSetClipPath")?)
      }
      Self::SetClipRegion(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits("EmfPlusSetClipRegion")?)
      }
      Self::SetAntiAliasMode(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits()?)
      }
      Self::SetTextRenderingHint(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits(
          "EmfPlusSetTextRenderingHint TextRenderingHint",
          value.text_rendering_hint().is_some(),
        )?)
      }
      Self::SetInterpolationMode(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits(
          "EmfPlusSetInterpolationMode InterpolationMode",
          value.interpolation_mode().is_some(),
        )?)
      }
      Self::SetPixelOffsetMode(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits(
          "EmfPlusSetPixelOffsetMode PixelOffsetMode",
          value.pixel_offset_mode().is_some(),
        )?)
      }
      Self::SetCompositingMode(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits(
          "EmfPlusSetCompositingMode CompositingMode",
          value.compositing_mode().is_some(),
        )?)
      }
      Self::SetCompositingQuality(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.flags_bits())
      }
      Self::SetTextContrast(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.checked_flags_bits()?)
      }
      Self::MultiplyWorldTransform(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.flags_bits())
      }
      Self::TranslateWorldTransform(value) => {
        EmfPlusRecordFlags::from_bits_retain(value.flags_bits())
      }
      Self::ScaleWorldTransform(value) => EmfPlusRecordFlags::from_bits_retain(value.flags_bits()),
      Self::RotateWorldTransform(value) => EmfPlusRecordFlags::from_bits_retain(value.flags_bits()),
      Self::SetTsGraphics(value) => {
        let mut next = flags;
        next.set(
          EmfPlusRecordFlags::TS_GRAPHICS_PALETTE,
          value.palette.is_some(),
        );
        next
      }
      Self::SetTsClip(value) => {
        validate_set_ts_clip(value)?;
        EmfPlusRecordFlags::from_bits_retain(value.flags_bits())
      }
      _ => flags,
    })
  }
}

fn sdk_string_current_size(value: &SdkString) -> u64 {
  match value {
    SdkString::Raw { bytes, .. } => bytes.len() as u64,
    SdkString::Text { value, .. } => value.encode_utf16().count() as u64 * 2,
  }
}

fn ensure_empty_data(data: &[u8], name: &str) -> Result<()> {
  if data.is_empty() {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} record has unexpected payload"),
    ))
  }
}

fn read_exact_object<T: SdkRead>(data: &[u8], name: &str) -> Result<T> {
  let mut reader = Reader::new(std::io::Cursor::new(data));
  let value = T::read_from(&mut reader)?;
  ensure_reader_end(&mut reader, data.len() as u64, name)?;
  Ok(value)
}

fn ensure_reader_end<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  end: u64,
  name: &str,
) -> Result<()> {
  let position = reader.position()?;
  if position != end {
    return Err(Error::invalid(
      position,
      format!("{name} record has trailing data"),
    ));
  }
  Ok(())
}

fn validate_emf_plus_fixed_payload_shape(
  record_kind: Option<EmfPlusRecordType>,
  data_len: usize,
  padding_len: usize,
) -> Result<()> {
  let Some(expected_data_len) = emf_plus_fixed_payload_len(record_kind) else {
    return Ok(());
  };
  if data_len != expected_data_len {
    return Err(Error::invalid(
      0,
      "EMF+ fixed-size record DataSize does not match the record type",
    ));
  }
  if padding_len != 0 {
    return Err(Error::invalid(
      0,
      "EMF+ fixed-size record Size does not match the record type",
    ));
  }
  Ok(())
}

fn validate_emf_plus_record_padding_len(padding_len: usize) -> Result<()> {
  if padding_len > 3 {
    return Err(Error::invalid(0, "EMF+ record padding exceeds 3 bytes"));
  }
  Ok(())
}

fn validate_unknown_emf_plus_record(record_type: u16) -> Result<()> {
  if EmfPlusRecordType::from_raw(record_type).is_some() {
    return Err(Error::invalid(
      0,
      "EMF+ Unknown record requires an unknown RecordType",
    ));
  }
  Ok(())
}

fn emf_plus_fixed_payload_len(record_kind: Option<EmfPlusRecordType>) -> Option<usize> {
  match record_kind? {
    EmfPlusRecordType::Header => Some(16),
    EmfPlusRecordType::Eof | EmfPlusRecordType::GetDc => Some(0),
    EmfPlusRecordType::Clear => Some(4),
    EmfPlusRecordType::FillRegion | EmfPlusRecordType::FillPath | EmfPlusRecordType::DrawPath => {
      Some(4)
    }
    EmfPlusRecordType::ResetClip
    | EmfPlusRecordType::SetClipPath
    | EmfPlusRecordType::SetClipRegion => Some(0),
    EmfPlusRecordType::OffsetClip => Some(8),
    EmfPlusRecordType::SetClipRect => Some(16),
    EmfPlusRecordType::StrokeFillPath => Some(0),
    EmfPlusRecordType::SetRenderingOrigin => Some(8),
    EmfPlusRecordType::SetAntiAliasMode
    | EmfPlusRecordType::SetTextRenderingHint
    | EmfPlusRecordType::SetTextContrast
    | EmfPlusRecordType::SetInterpolationMode
    | EmfPlusRecordType::SetPixelOffsetMode
    | EmfPlusRecordType::SetCompositingMode
    | EmfPlusRecordType::SetCompositingQuality => Some(0),
    EmfPlusRecordType::Save
    | EmfPlusRecordType::Restore
    | EmfPlusRecordType::BeginContainerNoParams
    | EmfPlusRecordType::EndContainer => Some(4),
    EmfPlusRecordType::BeginContainer => Some(36),
    EmfPlusRecordType::SetWorldTransform | EmfPlusRecordType::MultiplyWorldTransform => Some(24),
    EmfPlusRecordType::ResetWorldTransform => Some(0),
    EmfPlusRecordType::TranslateWorldTransform | EmfPlusRecordType::ScaleWorldTransform => Some(8),
    EmfPlusRecordType::RotateWorldTransform | EmfPlusRecordType::SetPageTransform => Some(4),
    _ => None,
  }
}

fn read_emf_plus_brush_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBrushObject> {
  let version = EmfPlusGraphicsVersion::read_from(reader)?;
  let brush_type = reader.read_u32()?;
  let value = EmfPlusBrushObject {
    version,
    brush_type,
    brush_data: read_remaining_vec(reader, data_len, "EmfPlusBrush BrushData")?,
  };
  validate_brush_object(&value)?;
  Ok(value)
}

fn read_emf_plus_solid_brush_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusSolidBrushData> {
  let solid_color = EmfPlusArgb::read_from(reader)?;
  let value = EmfPlusSolidBrushData {
    solid_color,
    trailing_data: read_remaining_vec(reader, data_len, "EmfPlusSolidBrushData trailing data")?,
  };
  validate_solid_brush_data(&value)?;
  Ok(value)
}

fn read_emf_plus_hatch_brush_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusHatchBrushData> {
  let hatch_style = reader.read_u32()?;
  let fore_color = EmfPlusArgb::read_from(reader)?;
  let back_color = EmfPlusArgb::read_from(reader)?;
  let value = EmfPlusHatchBrushData {
    hatch_style,
    fore_color,
    back_color,
    trailing_data: read_remaining_vec(reader, data_len, "EmfPlusHatchBrushData trailing data")?,
  };
  validate_hatch_brush_data(&value)?;
  Ok(value)
}

fn read_emf_plus_linear_gradient_brush_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusLinearGradientBrushData> {
  let brush_data_flags = reader.read_u32()?;
  let wrap_mode = reader.read_i32()?;
  let rect = RectF::read_from(reader)?;
  let start_color = EmfPlusArgb::read_from(reader)?;
  let end_color = EmfPlusArgb::read_from(reader)?;
  let reserved1 = reader.read_u32()?;
  let reserved2 = reader.read_u32()?;
  let value = EmfPlusLinearGradientBrushData {
    brush_data_flags,
    wrap_mode,
    rect,
    start_color,
    end_color,
    reserved1,
    reserved2,
    optional_data: read_remaining_vec(
      reader,
      data_len,
      "EmfPlusLinearGradientBrushData OptionalData",
    )?,
  };
  validate_linear_gradient_brush_data(&value)?;
  Ok(value)
}

fn read_emf_plus_linear_gradient_optional_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  flags: EmfPlusBrushDataFlags,
) -> Result<EmfPlusLinearGradientBrushOptionalData> {
  let transform_matrix = if flags.contains(EmfPlusBrushDataFlags::TRANSFORM) {
    ensure_remaining(
      reader,
      data_len,
      24,
      "EmfPlusLinearGradient TransformMatrix",
    )?;
    Some(XForm::read_from(reader)?)
  } else {
    None
  };
  let blend_pattern = read_emf_plus_blend_pattern(reader, data_len, flags)?;
  let trailing_data = read_remaining_vec(
    reader,
    data_len,
    "EmfPlusLinearGradient optional trailing data",
  )?;
  Ok(EmfPlusLinearGradientBrushOptionalData {
    transform_matrix,
    blend_pattern,
    trailing_data,
  })
}

fn read_emf_plus_path_gradient_brush_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusPathGradientBrushData> {
  let brush_data_flags = reader.read_u32()?;
  let wrap_mode = reader.read_i32()?;
  let center_color = EmfPlusArgb::read_from(reader)?;
  let center_point = PointF::read_from(reader)?;
  let surrounding_color_count = reader.read_u32()? as usize;
  let surrounding_color_bytes = surrounding_color_count
    .checked_mul(4)
    .ok_or_else(|| Error::invalid(0, "EmfPlusPathGradient surrounding color size overflows"))?;
  ensure_remaining(
    reader,
    data_len,
    surrounding_color_bytes,
    "EmfPlusPathGradient SurroundingColor",
  )?;
  let mut surrounding_colors = Vec::with_capacity(surrounding_color_count);
  for _ in 0..surrounding_color_count {
    surrounding_colors.push(EmfPlusArgb::read_from(reader)?);
  }
  let value = EmfPlusPathGradientBrushData {
    brush_data_flags,
    wrap_mode,
    center_color,
    center_point,
    surrounding_colors,
    boundary_and_optional_data: read_remaining_vec(
      reader,
      data_len,
      "EmfPlusPathGradient BoundaryData and OptionalData",
    )?,
  };
  validate_path_gradient_brush_data(&value)?;
  Ok(value)
}

fn read_emf_plus_path_gradient_tail_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  flags: EmfPlusBrushDataFlags,
) -> Result<EmfPlusPathGradientBrushTailData> {
  if reader.position()? >= data_len {
    return Err(Error::invalid(
      reader.position()?,
      "EmfPlusPathGradient BoundaryData is missing",
    ));
  }
  let boundary_data = Some(if flags.contains(EmfPlusBrushDataFlags::PATH) {
    EmfPlusBoundaryData::Path(read_emf_plus_boundary_path_data(reader, data_len)?)
  } else {
    EmfPlusBoundaryData::Points(read_emf_plus_boundary_point_data(reader, data_len)?)
  });
  let transform_matrix = if flags.contains(EmfPlusBrushDataFlags::TRANSFORM) {
    ensure_remaining(reader, data_len, 24, "EmfPlusPathGradient TransformMatrix")?;
    Some(XForm::read_from(reader)?)
  } else {
    None
  };
  let blend_pattern = read_emf_plus_blend_pattern(reader, data_len, flags)?;
  let focus_scale_data = if flags.contains(EmfPlusBrushDataFlags::FOCUS_SCALES) {
    Some(read_emf_plus_focus_scale_data(reader, data_len)?)
  } else {
    None
  };
  let trailing_data =
    read_remaining_vec(reader, data_len, "EmfPlusPathGradient tail trailing data")?;
  Ok(EmfPlusPathGradientBrushTailData {
    boundary_data,
    optional_data: EmfPlusPathGradientBrushOptionalData {
      transform_matrix,
      blend_pattern,
      focus_scale_data,
    },
    trailing_data,
  })
}

fn read_emf_plus_blend_pattern<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  flags: EmfPlusBrushDataFlags,
) -> Result<Option<EmfPlusBlendPattern>> {
  if flags.contains(EmfPlusBrushDataFlags::PRESET_COLORS)
    && flags
      .intersects(EmfPlusBrushDataFlags::BLEND_FACTORS_H | EmfPlusBrushDataFlags::BLEND_FACTORS_V)
  {
    return Err(Error::invalid(
      0,
      "EMF+ BrushData flags must not request both preset colors and blend factors",
    ));
  }
  if flags.contains(EmfPlusBrushDataFlags::PRESET_COLORS) {
    return Ok(Some(EmfPlusBlendPattern::Colors(
      read_emf_plus_blend_colors(reader, data_len)?,
    )));
  }
  let has_h = flags.contains(EmfPlusBrushDataFlags::BLEND_FACTORS_H);
  let has_v = flags.contains(EmfPlusBrushDataFlags::BLEND_FACTORS_V);
  match (has_h, has_v) {
    (false, false) => Ok(None),
    (true, false) | (false, true) => Ok(Some(EmfPlusBlendPattern::Factors(
      read_emf_plus_blend_factors(reader, data_len)?,
    ))),
    (true, true) => {
      let vertical = read_emf_plus_blend_factors(reader, data_len)?;
      let horizontal = read_emf_plus_blend_factors(reader, data_len)?;
      Ok(Some(EmfPlusBlendPattern::FactorsHV {
        horizontal,
        vertical,
      }))
    }
  }
}

fn read_emf_plus_blend_colors<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBlendColors> {
  ensure_remaining(reader, data_len, 4, "EmfPlusBlendColors PositionCount")?;
  let count = reader.read_u32()? as usize;
  let positions = read_f32_array_body(reader, data_len, count, "EmfPlusBlendColors")?;
  validate_unit_interval_values(&positions, "EmfPlusBlendColors positions")?;
  let color_bytes = count
    .checked_mul(4)
    .ok_or_else(|| Error::invalid(0, "EmfPlusBlendColors color size overflows"))?;
  ensure_remaining(reader, data_len, color_bytes, "EmfPlusBlendColors colors")?;
  let mut colors = Vec::with_capacity(count);
  for _ in 0..count {
    colors.push(EmfPlusArgb::read_from(reader)?);
  }
  Ok(EmfPlusBlendColors {
    positions,
    colors,
    trailing_data: Vec::new(),
  })
}

fn read_emf_plus_blend_factors<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBlendFactors> {
  ensure_remaining(reader, data_len, 4, "EmfPlusBlendFactors PositionCount")?;
  let count = reader.read_u32()? as usize;
  let positions = read_f32_array_body(reader, data_len, count, "EmfPlusBlendFactors positions")?;
  let factors = read_f32_array_body(reader, data_len, count, "EmfPlusBlendFactors factors")?;
  validate_blend_factors(&positions, &factors)?;
  Ok(EmfPlusBlendFactors {
    positions,
    factors,
    trailing_data: Vec::new(),
  })
}

fn read_emf_plus_boundary_path_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBoundaryPathData> {
  Ok(EmfPlusBoundaryPathData {
    path_data: read_emf_plus_size_prefixed_path_data(reader, data_len, "EmfPlusBoundaryPathData")?,
    trailing_data: Vec::new(),
  })
}

fn read_emf_plus_boundary_point_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBoundaryPointData> {
  ensure_remaining(reader, data_len, 4, "EmfPlusBoundaryPointData count")?;
  let count = non_negative_count(reader.read_i32()?, "EmfPlusBoundaryPointData count")?;
  let point_bytes = count
    .checked_mul(8)
    .ok_or_else(|| Error::invalid(0, "EmfPlusBoundaryPointData size overflows"))?;
  ensure_remaining(
    reader,
    data_len,
    point_bytes,
    "EmfPlusBoundaryPointData points",
  )?;
  let mut points = Vec::with_capacity(count);
  for _ in 0..count {
    points.push(PointF::read_from(reader)?);
  }
  Ok(EmfPlusBoundaryPointData {
    points,
    trailing_data: Vec::new(),
  })
}

fn read_emf_plus_focus_scale_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusFocusScaleData> {
  ensure_remaining(reader, data_len, 12, "EmfPlusFocusScaleData")?;
  let value = EmfPlusFocusScaleData {
    focus_scale_count: reader.read_u32()?,
    focus_scale_x: reader.read_f32()?,
    focus_scale_y: reader.read_f32()?,
    trailing_data: Vec::new(),
  };
  validate_focus_scale_data(&value)?;
  Ok(value)
}

fn read_emf_plus_palette<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusPalette> {
  let mut palette = read_emf_plus_palette_prefix(reader, data_len, "EmfPlusPalette")?;
  palette.trailing_data = read_remaining_vec(reader, data_len, "EmfPlusPalette trailing data")?;
  validate_palette(&palette)?;
  Ok(palette)
}

fn read_emf_plus_palette_prefix<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<EmfPlusPalette> {
  ensure_remaining(reader, data_len, 8, name)?;
  let palette_style_flags = reader.read_u32()?;
  let count = reader.read_u32()? as usize;
  let entry_bytes = count
    .checked_mul(4)
    .ok_or_else(|| Error::invalid(0, "EmfPlusPalette entry size overflows"))?;
  ensure_remaining(reader, data_len, entry_bytes, "EmfPlusPalette entries")?;
  let mut entries = Vec::with_capacity(count);
  for _ in 0..count {
    entries.push(EmfPlusArgb::read_from(reader)?);
  }
  let value = EmfPlusPalette {
    palette_style_flags,
    entries,
    trailing_data: Vec::new(),
  };
  validate_palette(&value)?;
  Ok(value)
}

fn read_emf_plus_texture_brush_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusTextureBrushData> {
  let brush_data_flags = reader.read_u32()?;
  let wrap_mode = reader.read_i32()?;
  let value = EmfPlusTextureBrushData {
    brush_data_flags,
    wrap_mode,
    optional_data: read_remaining_vec(reader, data_len, "EmfPlusTextureBrushData OptionalData")?,
  };
  validate_texture_brush_data(&value)?;
  Ok(value)
}

fn read_emf_plus_texture_brush_optional_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  flags: EmfPlusBrushDataFlags,
) -> Result<EmfPlusTextureBrushOptionalData> {
  let transform_matrix = if flags.contains(EmfPlusBrushDataFlags::TRANSFORM) {
    ensure_remaining(
      reader,
      data_len,
      24,
      "EmfPlusTextureBrushOptionalData TransformMatrix",
    )?;
    Some(XForm::read_from(reader)?)
  } else {
    None
  };

  let image_object = if data_len.saturating_sub(reader.position()?) >= 8 {
    Some(read_emf_plus_image_object(reader, data_len)?)
  } else {
    None
  };
  let trailing_data = read_remaining_vec(
    reader,
    data_len,
    "EmfPlusTextureBrushOptionalData trailing data",
  )?;

  Ok(EmfPlusTextureBrushOptionalData {
    transform_matrix,
    image_object,
    trailing_data,
  })
}

fn read_emf_plus_custom_line_cap_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusCustomLineCapObject> {
  let version = EmfPlusGraphicsVersion::read_from(reader)?;
  let cap_type = reader.read_i32()?;
  let value = EmfPlusCustomLineCapObject {
    version,
    cap_type,
    custom_line_cap_data: read_remaining_vec(
      reader,
      data_len,
      "EmfPlusCustomLineCap CustomLineCapData",
    )?,
  };
  validate_custom_line_cap_object(&value)?;
  Ok(value)
}

fn read_sized_custom_line_cap(data: &[u8], name: &str) -> Result<EmfPlusCustomLineCapObject> {
  let mut reader = Reader::new(std::io::Cursor::new(data));
  let cap = read_emf_plus_custom_line_cap_object(&mut reader, data.len() as u64)?;
  cap
    .parse_cap_data()
    .map_err(|err| Error::invalid(0, format!("{name} CustomLineCapData is invalid: {err}")))?;
  Ok(cap)
}

fn read_emf_plus_custom_line_cap_arrow_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusCustomLineCapArrowData> {
  let width = reader.read_f32()?;
  let height = reader.read_f32()?;
  let middle_inset = reader.read_f32()?;
  let fill_state = reader.read_u32()?;
  let line_start_cap = reader.read_u32()?;
  let line_end_cap = reader.read_u32()?;
  let line_join = reader.read_u32()?;
  let line_miter_limit = reader.read_f32()?;
  let width_scale = reader.read_f32()?;
  let fill_hot_spot = PointF::read_from(reader)?;
  let line_hot_spot = PointF::read_from(reader)?;
  let value = EmfPlusCustomLineCapArrowData {
    width,
    height,
    middle_inset,
    fill_state,
    line_start_cap,
    line_end_cap,
    line_join,
    line_miter_limit,
    width_scale,
    fill_hot_spot,
    line_hot_spot,
    trailing_data: read_remaining_vec(
      reader,
      data_len,
      "EmfPlusCustomLineCapArrowData trailing data",
    )?,
  };
  validate_custom_line_cap_arrow_data(&value)?;
  Ok(value)
}

fn read_emf_plus_custom_line_cap_default_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusCustomLineCapDefaultData> {
  let custom_line_cap_data_flags = reader.read_u32()?;
  let base_cap = reader.read_u32()?;
  let base_inset = reader.read_f32()?;
  let stroke_start_cap = reader.read_u32()?;
  let stroke_end_cap = reader.read_u32()?;
  let stroke_join = reader.read_u32()?;
  let stroke_miter_limit = reader.read_f32()?;
  let width_scale = reader.read_f32()?;
  let fill_hot_spot = PointF::read_from(reader)?;
  let stroke_hot_spot = PointF::read_from(reader)?;
  let value = EmfPlusCustomLineCapDefaultData {
    custom_line_cap_data_flags,
    base_cap,
    base_inset,
    stroke_start_cap,
    stroke_end_cap,
    stroke_join,
    stroke_miter_limit,
    width_scale,
    fill_hot_spot,
    stroke_hot_spot,
    optional_data: read_remaining_vec(reader, data_len, "EmfPlusCustomLineCapData OptionalData")?,
  };
  validate_custom_line_cap_default_data(&value)?;
  Ok(value)
}

fn read_emf_plus_custom_line_cap_optional_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  flags: EmfPlusCustomLineCapDataFlags,
) -> Result<EmfPlusCustomLineCapOptionalData> {
  let fill_path = if flags.contains(EmfPlusCustomLineCapDataFlags::FILL_PATH) {
    Some(EmfPlusFillPathObject {
      path_data: read_emf_plus_size_prefixed_path_data(reader, data_len, "EmfPlusFillPath")?,
      trailing_data: Vec::new(),
    })
  } else {
    None
  };
  let line_path = if flags.contains(EmfPlusCustomLineCapDataFlags::LINE_PATH) {
    Some(EmfPlusLinePathObject {
      path_data: read_emf_plus_size_prefixed_path_data(reader, data_len, "EmfPlusLinePath")?,
      trailing_data: Vec::new(),
    })
  } else {
    None
  };
  Ok(EmfPlusCustomLineCapOptionalData {
    fill_path,
    line_path,
    trailing_data: read_remaining_vec(
      reader,
      data_len,
      "EmfPlusCustomLineCapOptionalData trailing data",
    )?,
  })
}

fn read_emf_plus_font_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusFontObject> {
  let version = EmfPlusGraphicsVersion::read_from(reader)?;
  let em_size = reader.read_f32()?;
  let size_unit = reader.read_u32()?;
  let font_style_flags = reader.read_i32()?;
  let reserved = reader.read_u32()?;
  let length = reader.read_u32()? as usize;
  let byte_len = length
    .checked_mul(2)
    .ok_or_else(|| Error::invalid(0, "EmfPlusFont FamilyName length overflows"))?;
  ensure_remaining(reader, data_len, byte_len, "EmfPlusFont FamilyName")?;
  let family_name = SdkString::raw(reader.read_vec(byte_len)?, SdkEncoding::Utf16Le);
  let value = EmfPlusFontObject {
    version,
    em_size,
    size_unit,
    font_style_flags,
    reserved,
    family_name,
    padding: read_remaining_vec(reader, data_len, "EmfPlusFont padding")?,
  };
  validate_font_object(&value)?;
  Ok(value)
}

fn read_emf_plus_image_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusImageObject> {
  let version = EmfPlusGraphicsVersion::read_from(reader)?;
  let image_type = reader.read_u32()?;
  let value = EmfPlusImageObject {
    version,
    image_type,
    image_data: read_remaining_vec(reader, data_len, "EmfPlusImage ImageData")?,
  };
  validate_image_object(&value)?;
  Ok(value)
}

fn read_emf_plus_bitmap_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBitmapObject> {
  ensure_remaining(reader, data_len, 20, "EmfPlusBitmap fixed fields")?;
  let width = reader.read_i32()?;
  let height = reader.read_i32()?;
  let stride = reader.read_i32()?;
  let pixel_format = reader.read_u32()?;
  let bitmap_data_type = reader.read_u32()?;
  let value = EmfPlusBitmapObject {
    width,
    height,
    stride,
    pixel_format,
    bitmap_data_type,
    bitmap_data: read_remaining_vec(reader, data_len, "EmfPlusBitmap BitmapData")?,
  };
  validate_emf_plus_bitmap_object(&value)?;
  Ok(value)
}

fn read_emf_plus_bitmap_payload(value: &EmfPlusBitmapObject) -> Result<EmfPlusBitmapPayload> {
  let mut reader = Reader::new(std::io::Cursor::new(value.bitmap_data.as_slice()));
  let data_len = value.bitmap_data.len() as u64;
  match value.bitmap_data_type_kind() {
    Some(EmfPlusBitmapDataType::Pixel) => {
      let palette = if value.is_indexed_pixel_format() {
        Some(read_emf_plus_palette_prefix(
          &mut reader,
          data_len,
          "EmfPlusBitmapData Colors",
        )?)
      } else {
        None
      };
      Ok(EmfPlusBitmapPayload::Pixel(EmfPlusBitmapDataObject {
        palette,
        pixel_data: read_remaining_vec(&mut reader, data_len, "EmfPlusBitmapData PixelData")?,
      }))
    }
    Some(EmfPlusBitmapDataType::Compressed) => Ok(EmfPlusBitmapPayload::Compressed(
      EmfPlusCompressedImageObject {
        compressed_image_data: value.bitmap_data.clone(),
      },
    )),
    None => Ok(EmfPlusBitmapPayload::Unknown {
      bitmap_data_type: value.bitmap_data_type,
      data: value.bitmap_data.clone(),
    }),
  }
}

fn read_emf_plus_metafile_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusMetafileObject> {
  let metafile_type = reader.read_u32()?;
  let metafile_data_size = reader.read_u32()? as usize;
  ensure_remaining(
    reader,
    data_len,
    metafile_data_size,
    "EmfPlusMetafile MetafileData",
  )?;
  let metafile_data = reader.read_vec(metafile_data_size)?;
  let value = EmfPlusMetafileObject {
    metafile_type,
    metafile_data,
    trailing_data: read_remaining_vec(reader, data_len, "EmfPlusMetafile trailing data")?,
  };
  validate_metafile_object(&value)?;
  Ok(value)
}

fn read_emf_plus_path_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusPathObject> {
  let version = EmfPlusGraphicsVersion::read_from(reader)?;
  let point_count = reader.read_u32()? as usize;
  let path_point_flags = reader.read_u32()?;
  let point_flags = EmfPlusRecordFlags::from_bits_retain((path_point_flags & 0xFFFF) as u16);
  let points = read_points(reader, point_count, point_flags, data_len)?;
  let point_types = read_path_point_types(reader, point_count, point_flags, data_len)?;
  let alignment_padding = read_remaining_vec(reader, data_len, "EmfPlusPath AlignmentPadding")?;
  let value = EmfPlusPathObject {
    version,
    path_point_flags,
    points,
    point_types,
    alignment_padding,
  };
  validate_path_object(&value)?;
  Ok(value)
}

fn read_path_point_types<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  point_count: usize,
  flags: EmfPlusRecordFlags,
  data_len: u64,
) -> Result<EmfPlusPathPointTypes> {
  if flags.contains(EmfPlusRecordFlags::RELATIVE_POSITION) {
    let mut values = Vec::new();
    let mut covered = 0usize;
    while covered < point_count {
      ensure_remaining(reader, data_len, 2, "EmfPlusPath RLE point types")?;
      let control = reader.read_u8()?;
      let point_type = EmfPlusPathPointTypeValue::new(reader.read_u8()?)?;
      let value = EmfPlusPathPointTypeRle {
        control,
        point_type,
      };
      let run_count = value.run_count() as usize;
      if run_count == 0 {
        return Err(Error::invalid(
          reader.position()?,
          "EmfPlusPath RLE point type has zero run count",
        ));
      }
      covered = covered
        .checked_add(run_count)
        .ok_or_else(|| Error::invalid(0, "EmfPlusPath RLE point type count overflows"))?;
      if covered > point_count {
        return Err(Error::invalid(
          reader.position()?,
          "EmfPlusPath RLE point type count exceeds PathPointCount",
        ));
      }
      values.push(value);
    }
    return Ok(EmfPlusPathPointTypes::Rle(values));
  }

  ensure_remaining(reader, data_len, point_count, "EmfPlusPath point types")?;
  let mut values = Vec::with_capacity(point_count);
  for _ in 0..point_count {
    values.push(EmfPlusPathPointTypeValue::new(reader.read_u8()?)?);
  }
  Ok(EmfPlusPathPointTypes::Values(values))
}

fn read_emf_plus_pen_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusPenObject> {
  let version = EmfPlusGraphicsVersion {
    value: reader.read_u32()?,
  };
  let pen_type = reader.read_u32()?;
  let value = EmfPlusPenObject {
    version,
    pen_type,
    pen_data_and_brush_object: read_remaining_vec(
      reader,
      data_len,
      "EmfPlusPen PenData and BrushObject",
    )?,
  };
  validate_pen_object(&value)?;
  Ok(value)
}

fn read_emf_plus_pen_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusPenData> {
  ensure_remaining(reader, data_len, 12, "EmfPlusPenData")?;
  let pen_data_flags = reader.read_u32()?;
  let pen_unit = reader.read_u32()?;
  let pen_width = reader.read_f32()?;
  let flags = EmfPlusPenDataFlags::from_bits_retain(pen_data_flags);
  let optional_data = read_emf_plus_pen_optional_data(reader, data_len, flags)?;
  let value = EmfPlusPenData {
    pen_data_flags,
    pen_unit,
    pen_width,
    optional_data,
    trailing_data: Vec::new(),
  };
  validate_pen_data(&value)?;
  value.optional_data.validate_for_flags(flags)?;
  Ok(value)
}

fn read_emf_plus_pen_optional_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  flags: EmfPlusPenDataFlags,
) -> Result<EmfPlusPenOptionalData> {
  let mut optional_data = EmfPlusPenOptionalData::default();
  if flags.contains(EmfPlusPenDataFlags::TRANSFORM) {
    ensure_remaining(
      reader,
      data_len,
      24,
      "EmfPlusPenOptionalData TransformMatrix",
    )?;
    optional_data.transform_matrix = Some(XForm::read_from(reader)?);
  }
  if flags.contains(EmfPlusPenDataFlags::START_CAP) {
    ensure_remaining(reader, data_len, 4, "EmfPlusPenOptionalData StartCap")?;
    optional_data.start_cap = Some(reader.read_i32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::END_CAP) {
    ensure_remaining(reader, data_len, 4, "EmfPlusPenOptionalData EndCap")?;
    optional_data.end_cap = Some(reader.read_i32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::JOIN) {
    ensure_remaining(reader, data_len, 4, "EmfPlusPenOptionalData Join")?;
    optional_data.join = Some(reader.read_i32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::MITER_LIMIT) {
    ensure_remaining(reader, data_len, 4, "EmfPlusPenOptionalData MiterLimit")?;
    optional_data.miter_limit = Some(reader.read_f32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::LINE_STYLE) {
    ensure_remaining(reader, data_len, 4, "EmfPlusPenOptionalData LineStyle")?;
    optional_data.line_style = Some(reader.read_i32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::DASHED_LINE_CAP) {
    ensure_remaining(
      reader,
      data_len,
      4,
      "EmfPlusPenOptionalData DashedLineCapType",
    )?;
    optional_data.dashed_line_cap_type = Some(reader.read_i32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::DASHED_LINE_OFFSET) {
    ensure_remaining(reader, data_len, 4, "EmfPlusPenOptionalData DashOffset")?;
    optional_data.dash_offset = Some(reader.read_f32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::DASHED_LINE) {
    optional_data.dashed_line_data = Some(EmfPlusDashedLineData {
      dashed_line_data: read_f32_array_with_u32_count(reader, data_len, "EmfPlusDashedLineData")?,
    });
  }
  if flags.contains(EmfPlusPenDataFlags::NON_CENTER) {
    ensure_remaining(reader, data_len, 4, "EmfPlusPenOptionalData PenAlignment")?;
    optional_data.pen_alignment = Some(reader.read_i32()?);
  }
  if flags.contains(EmfPlusPenDataFlags::COMPOUND_LINE) {
    let values = read_f32_array_with_u32_count(reader, data_len, "EmfPlusCompoundLineData")?;
    validate_increasing_unit_interval_values(&values, "EmfPlusCompoundLineData")?;
    optional_data.compound_line_data = Some(EmfPlusCompoundLineData {
      compound_line_data: values,
    });
  }
  if flags.contains(EmfPlusPenDataFlags::CUSTOM_START_CAP) {
    optional_data.custom_start_cap_data = Some(EmfPlusCustomStartCapData {
      custom_start_cap: read_size_prefixed_vec(reader, data_len, "EmfPlusCustomStartCapData")?,
    });
  }
  if flags.contains(EmfPlusPenDataFlags::CUSTOM_END_CAP) {
    optional_data.custom_end_cap_data = Some(EmfPlusCustomEndCapData {
      custom_end_cap: read_size_prefixed_vec(reader, data_len, "EmfPlusCustomEndCapData")?,
    });
  }
  Ok(optional_data)
}

fn read_emf_plus_region_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusRegionObject> {
  let version = EmfPlusGraphicsVersion::read_from(reader)?;
  let region_node_count = reader.read_u32()?;
  let value = EmfPlusRegionObject {
    version,
    region_node_count,
    region_nodes: read_remaining_vec(reader, data_len, "EmfPlusRegion RegionNode")?,
  };
  validate_region_object(&value)?;
  Ok(value)
}

fn read_emf_plus_region_node<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusRegionNode> {
  ensure_remaining(reader, data_len, 4, "EmfPlusRegionNode Type")?;
  let node_type = reader.read_u32()?;
  let data = match EmfPlusRegionNodeDataType::from_raw(node_type) {
    Some(EmfPlusRegionNodeDataType::Rect) => {
      ensure_remaining(reader, data_len, 16, "EmfPlusRegionNode Rect")?;
      EmfPlusRegionNodeData::Rect(RectF::read_from(reader)?)
    }
    Some(EmfPlusRegionNodeDataType::Path) => {
      EmfPlusRegionNodeData::Path(read_emf_plus_region_node_path(reader, data_len)?)
    }
    Some(EmfPlusRegionNodeDataType::Empty) => EmfPlusRegionNodeData::Empty,
    Some(EmfPlusRegionNodeDataType::Infinite) => EmfPlusRegionNodeData::Infinite,
    Some(
      EmfPlusRegionNodeDataType::And
      | EmfPlusRegionNodeDataType::Or
      | EmfPlusRegionNodeDataType::Xor
      | EmfPlusRegionNodeDataType::Exclude
      | EmfPlusRegionNodeDataType::Complement,
    ) => {
      let left = read_emf_plus_region_node(reader, data_len)?;
      let right = read_emf_plus_region_node(reader, data_len)?;
      EmfPlusRegionNodeData::ChildNodes(Box::new(EmfPlusRegionNodeChildNodes { left, right }))
    }
    None => EmfPlusRegionNodeData::Raw(read_remaining_vec(
      reader,
      data_len,
      "EmfPlusRegionNode data",
    )?),
  };
  Ok(EmfPlusRegionNode { node_type, data })
}

fn read_emf_plus_region_node_path<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusRegionNodePathData> {
  read_emf_plus_size_prefixed_path_data(reader, data_len, "EmfPlusRegionNodePath")
}

fn read_emf_plus_size_prefixed_path_data<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<EmfPlusRegionNodePathData> {
  let path_data = read_i32_size_prefixed_vec(reader, data_len, name)?;
  let mut path_reader = Reader::new(std::io::Cursor::new(path_data.as_slice()));
  read_emf_plus_path_object(&mut path_reader, path_data.len() as u64)
    .map(EmfPlusRegionNodePathData::Path)
}

fn read_emf_plus_string_format_object<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusStringFormatObject> {
  let version = EmfPlusGraphicsVersion::read_from(reader)?;
  let string_format_flags = reader.read_u32()?;
  let language = reader.read_u32()?;
  let string_alignment = reader.read_u32()?;
  let line_align = reader.read_u32()?;
  let digit_substitution = reader.read_u32()?;
  let digit_language = reader.read_u32()?;
  let first_tab_offset = reader.read_f32()?;
  let hotkey_prefix = reader.read_i32()?;
  let leading_margin = reader.read_f32()?;
  let trailing_margin = reader.read_f32()?;
  let tracking = reader.read_f32()?;
  let trimming = reader.read_u32()?;
  let tab_stop_count = non_negative_count(reader.read_i32()?, "EmfPlusStringFormat TabStopCount")?;
  let range_count = non_negative_count(reader.read_i32()?, "EmfPlusStringFormat RangeCount")?;

  let tab_bytes = tab_stop_count
    .checked_mul(4)
    .ok_or_else(|| Error::invalid(0, "EmfPlusStringFormat TabStops size overflows"))?;
  ensure_remaining(reader, data_len, tab_bytes, "EmfPlusStringFormat TabStops")?;
  let mut tab_stops = Vec::with_capacity(tab_stop_count);
  for _ in 0..tab_stop_count {
    tab_stops.push(reader.read_f32()?);
  }

  let range_bytes = range_count
    .checked_mul(8)
    .ok_or_else(|| Error::invalid(0, "EmfPlusStringFormat CharRange size overflows"))?;
  ensure_remaining(
    reader,
    data_len,
    range_bytes,
    "EmfPlusStringFormat CharRange",
  )?;
  let mut char_ranges = Vec::with_capacity(range_count);
  for _ in 0..range_count {
    char_ranges.push(EmfPlusCharacterRange::read_from(reader)?);
  }

  let value = EmfPlusStringFormatObject {
    version,
    string_format_flags,
    language,
    string_alignment,
    line_align,
    digit_substitution,
    digit_language,
    first_tab_offset,
    hotkey_prefix,
    leading_margin,
    trailing_margin,
    tracking,
    trimming,
    tab_stops,
    char_ranges,
    trailing_data: read_remaining_vec(reader, data_len, "EmfPlusStringFormat trailing data")?,
  };
  validate_string_format_object(&value)?;
  Ok(value)
}

fn read_emf_plus_image_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  object_guid: [u8; 16],
) -> Result<EmfPlusImageEffect> {
  match EmfPlusImageEffectKind::from_guid(object_guid) {
    Some(EmfPlusImageEffectKind::Blur) => Ok(EmfPlusImageEffect::Blur(read_emf_plus_blur_effect(
      reader, data_len,
    )?)),
    Some(EmfPlusImageEffectKind::BrightnessContrast) => Ok(EmfPlusImageEffect::BrightnessContrast(
      read_emf_plus_brightness_contrast_effect(reader, data_len)?,
    )),
    Some(EmfPlusImageEffectKind::ColorBalance) => Ok(EmfPlusImageEffect::ColorBalance(
      read_emf_plus_color_balance_effect(reader, data_len)?,
    )),
    Some(EmfPlusImageEffectKind::ColorCurve) => Ok(EmfPlusImageEffect::ColorCurve(
      read_emf_plus_color_curve_effect(reader, data_len)?,
    )),
    Some(EmfPlusImageEffectKind::ColorLookupTable) => Ok(EmfPlusImageEffect::ColorLookupTable(
      Box::new(read_emf_plus_color_lookup_table_effect(reader, data_len)?),
    )),
    Some(EmfPlusImageEffectKind::ColorMatrix) => Ok(EmfPlusImageEffect::ColorMatrix(
      read_emf_plus_color_matrix_effect(reader, data_len)?,
    )),
    Some(EmfPlusImageEffectKind::HueSaturationLightness) => {
      Ok(EmfPlusImageEffect::HueSaturationLightness(
        read_emf_plus_hue_saturation_lightness_effect(reader, data_len)?,
      ))
    }
    Some(EmfPlusImageEffectKind::Levels) => Ok(EmfPlusImageEffect::Levels(
      read_emf_plus_levels_effect(reader, data_len)?,
    )),
    Some(EmfPlusImageEffectKind::RedEyeCorrection) => Ok(EmfPlusImageEffect::RedEyeCorrection(
      read_emf_plus_red_eye_correction_effect(reader, data_len)?,
    )),
    Some(EmfPlusImageEffectKind::Sharpen) => Ok(EmfPlusImageEffect::Sharpen(
      read_emf_plus_sharpen_effect(reader, data_len)?,
    )),
    Some(EmfPlusImageEffectKind::Tint) => Ok(EmfPlusImageEffect::Tint(read_emf_plus_tint_effect(
      reader, data_len,
    )?)),
    None => Ok(EmfPlusImageEffect::Unknown {
      object_guid,
      buffer: read_remaining_vec(reader, data_len, "EmfPlusSerializableObject Buffer")?,
    }),
  }
}

fn read_emf_plus_blur_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBlurEffect> {
  let value = EmfPlusBlurEffect {
    blur_radius: reader.read_f32()?,
    expand_edge: reader.read_u32()?,
    trailing_data: read_remaining_vec(reader, data_len, "BlurEffect trailing data")?,
  };
  validate_blur_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_brightness_contrast_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusBrightnessContrastEffect> {
  let value = EmfPlusBrightnessContrastEffect {
    brightness_level: reader.read_i32()?,
    contrast_level: reader.read_i32()?,
    trailing_data: read_remaining_vec(reader, data_len, "BrightnessContrastEffect trailing data")?,
  };
  validate_brightness_contrast_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_color_balance_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusColorBalanceEffect> {
  let value = EmfPlusColorBalanceEffect {
    cyan_red: reader.read_i32()?,
    magenta_green: reader.read_i32()?,
    yellow_blue: reader.read_i32()?,
    trailing_data: read_remaining_vec(reader, data_len, "ColorBalanceEffect trailing data")?,
  };
  validate_color_balance_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_color_curve_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusColorCurveEffect> {
  let value = EmfPlusColorCurveEffect {
    curve_adjustment: reader.read_u32()?,
    curve_channel: reader.read_u32()?,
    adjustment_intensity: reader.read_i32()?,
    trailing_data: read_remaining_vec(reader, data_len, "ColorCurveEffect trailing data")?,
  };
  validate_color_curve_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_color_lookup_table_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusColorLookupTableEffect> {
  let value = EmfPlusColorLookupTableEffect {
    blue_lookup_table: read_u8_array_256(reader, data_len, "BlueLookupTable")?,
    green_lookup_table: read_u8_array_256(reader, data_len, "GreenLookupTable")?,
    red_lookup_table: read_u8_array_256(reader, data_len, "RedLookupTable")?,
    alpha_lookup_table: read_u8_array_256(reader, data_len, "AlphaLookupTable")?,
    trailing_data: read_remaining_vec(reader, data_len, "ColorLookupTableEffect trailing data")?,
  };
  validate_color_lookup_table_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_color_matrix_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusColorMatrixEffect> {
  let mut matrix = [[0.0; 5]; 5];
  for column in &mut matrix {
    for value in column {
      *value = reader.read_f32()?;
    }
  }
  Ok(EmfPlusColorMatrixEffect {
    matrix,
    trailing_data: read_remaining_vec(reader, data_len, "ColorMatrixEffect trailing data")?,
  })
  .and_then(|value| {
    validate_color_matrix_effect(&value)?;
    Ok(value)
  })
}

fn read_emf_plus_hue_saturation_lightness_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusHueSaturationLightnessEffect> {
  let value = EmfPlusHueSaturationLightnessEffect {
    hue_level: reader.read_i32()?,
    saturation_level: reader.read_i32()?,
    lightness_level: reader.read_i32()?,
    trailing_data: read_remaining_vec(
      reader,
      data_len,
      "HueSaturationLightnessEffect trailing data",
    )?,
  };
  validate_hue_saturation_lightness_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_levels_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusLevelsEffect> {
  let value = EmfPlusLevelsEffect {
    highlight: reader.read_i32()?,
    mid_tone: reader.read_i32()?,
    shadow: reader.read_i32()?,
    trailing_data: read_remaining_vec(reader, data_len, "LevelsEffect trailing data")?,
  };
  validate_levels_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_red_eye_correction_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusRedEyeCorrectionEffect> {
  let area_count = non_negative_count(reader.read_i32()?, "RedEyeCorrectionEffect areas")?;
  let area_bytes = area_count
    .checked_mul(16)
    .ok_or_else(|| Error::invalid(0, "RedEyeCorrectionEffect areas size overflows"))?;
  ensure_remaining(reader, data_len, area_bytes, "RedEyeCorrectionEffect areas")?;
  let mut areas = Vec::with_capacity(area_count);
  for _ in 0..area_count {
    areas.push(RectL::read_from(reader)?);
  }
  let value = EmfPlusRedEyeCorrectionEffect {
    areas,
    trailing_data: read_remaining_vec(reader, data_len, "RedEyeCorrectionEffect trailing data")?,
  };
  validate_red_eye_correction_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_sharpen_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusSharpenEffect> {
  let value = EmfPlusSharpenEffect {
    radius: reader.read_f32()?,
    amount: reader.read_f32()?,
    trailing_data: read_remaining_vec(reader, data_len, "SharpenEffect trailing data")?,
  };
  validate_sharpen_effect(&value)?;
  Ok(value)
}

fn read_emf_plus_tint_effect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
) -> Result<EmfPlusTintEffect> {
  let value = EmfPlusTintEffect {
    hue: reader.read_i32()?,
    amount: reader.read_i32()?,
    trailing_data: read_remaining_vec(reader, data_len, "TintEffect trailing data")?,
  };
  validate_tint_effect(&value)?;
  Ok(value)
}

fn read_u8_array_256<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<[u8; 256]> {
  ensure_remaining(reader, data_len, 256, name)?;
  reader.read_array::<256>()
}

fn validate_path_object(value: &EmfPlusPathObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusPath")?;
  let flags = EmfPlusRecordFlags::from_bits_retain((value.path_point_flags & 0xFFFF) as u16);
  if value.alignment_padding.len() > 3 {
    return Err(Error::invalid(
      0,
      format!(
        "EmfPlusPath AlignmentPadding has {} bytes; expected at most 3",
        value.alignment_padding.len()
      ),
    ));
  }
  let expected_padding = expected_path_alignment_padding(value)?;
  if value.alignment_padding.len() != expected_padding {
    return Err(Error::invalid(
      0,
      format!(
        "EmfPlusPath AlignmentPadding has {} bytes; expected {}",
        value.alignment_padding.len(),
        expected_padding
      ),
    ));
  }

  match (&value.points, &value.point_types) {
    (EmfPlusPointData::Relative(_), EmfPlusPathPointTypes::Rle(_))
      if flags.contains(EmfPlusRecordFlags::RELATIVE_POSITION) => {}
    (EmfPlusPointData::Compressed(_), EmfPlusPathPointTypes::Values(_))
      if !flags.contains(EmfPlusRecordFlags::RELATIVE_POSITION)
        && flags.contains(EmfPlusRecordFlags::COMPRESSED) => {}
    (EmfPlusPointData::Float(_), EmfPlusPathPointTypes::Values(_))
      if !flags.contains(EmfPlusRecordFlags::RELATIVE_POSITION)
        && !flags.contains(EmfPlusRecordFlags::COMPRESSED) => {}
    (EmfPlusPointData::Relative(_), _) => {
      return Err(Error::invalid(
        0,
        "EmfPlusPath relative points require R flag and RLE point types",
      ));
    }
    (EmfPlusPointData::Compressed(_), _) => {
      return Err(Error::invalid(
        0,
        "EmfPlusPath compressed points require C flag without R flag and value point types",
      ));
    }
    (EmfPlusPointData::Float(_), _) => {
      return Err(Error::invalid(
        0,
        "EmfPlusPath floating points require neither R nor C flag and value point types",
      ));
    }
  }

  validate_path_point_types(value.points.len(), &value.point_types)
}

fn validate_path_object_strict(value: &EmfPlusPathObject) -> Result<()> {
  validate_path_object(value)?;
  validate_flag_bits(
    value.path_point_flags,
    u32::from((EmfPlusRecordFlags::RELATIVE_POSITION | EmfPlusRecordFlags::COMPRESSED).bits()),
    "EmfPlusPath PathPointFlags",
  )
}

fn expected_path_alignment_padding(value: &EmfPlusPathObject) -> Result<usize> {
  let points_size = usize::try_from(value.points.sdk_size())
    .map_err(|_| Error::invalid(0, "EmfPlusPath point data size overflows usize"))?;
  let point_types_size = value.point_types.sdk_size()?;
  let unpadded_size = 12usize
    .checked_add(points_size)
    .and_then(|size| size.checked_add(point_types_size))
    .ok_or_else(|| Error::invalid(0, "EmfPlusPath size overflows"))?;
  Ok((4 - (unpadded_size % 4)) % 4)
}

fn validate_path_point_types(
  point_count: usize,
  point_types: &EmfPlusPathPointTypes,
) -> Result<()> {
  validate_path_point_type_sequence(point_types)?;
  let actual = point_types.point_count();
  if actual != point_count {
    return Err(Error::invalid(
      0,
      format!("EmfPlusPath point type count {actual} does not match PathPointCount {point_count}"),
    ));
  }
  Ok(())
}

fn validate_path_point_type_sequence(point_types: &EmfPlusPathPointTypes) -> Result<()> {
  match point_types {
    EmfPlusPathPointTypes::Values(values) => {
      for value in values {
        validate_path_point_type_byte(value.value)?;
      }
    }
    EmfPlusPathPointTypes::Rle(values) => {
      let mut covered = 0usize;
      for value in values {
        if value.control & 0x40 == 0 {
          return Err(Error::invalid(
            0,
            "EmfPlusPath RLE point type reserved control bit is not set",
          ));
        }
        let run_count = value.run_count() as usize;
        if run_count == 0 {
          return Err(Error::invalid(
            0,
            "EmfPlusPath RLE point type has zero run count",
          ));
        }
        validate_path_point_type_byte(value.point_type.value)?;
        covered = covered
          .checked_add(run_count)
          .ok_or_else(|| Error::invalid(0, "EmfPlusPath RLE point type count overflows"))?;
      }
    }
  }
  Ok(())
}

fn validate_path_point_type_byte(value: u8) -> Result<()> {
  if EmfPlusPathPointType::from_raw(value & 0x0F).is_none() {
    return Err(Error::invalid(
      0,
      format!(
        "EmfPlusPath point type {} is not a valid PathPointType",
        value & 0x0F
      ),
    ));
  }
  validate_flag_bits(
    u32::from(value >> 4),
    u32::from(EmfPlusPathPointTypeFlags::all().bits()),
    "EmfPlusPath point type flags",
  )
}

fn validate_region_object(value: &EmfPlusRegionObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusRegion")?;
  value.parse_region_nodes().map(|_| ())
}

fn validate_region_node_data_matches_type(value: &EmfPlusRegionNode) -> Result<()> {
  match (value.node_type_kind(), &value.data) {
    (
      Some(
        EmfPlusRegionNodeDataType::And
        | EmfPlusRegionNodeDataType::Or
        | EmfPlusRegionNodeDataType::Xor
        | EmfPlusRegionNodeDataType::Exclude
        | EmfPlusRegionNodeDataType::Complement,
      ),
      EmfPlusRegionNodeData::ChildNodes(_),
    )
    | (Some(EmfPlusRegionNodeDataType::Rect), EmfPlusRegionNodeData::Rect(_))
    | (Some(EmfPlusRegionNodeDataType::Empty), EmfPlusRegionNodeData::Empty)
    | (Some(EmfPlusRegionNodeDataType::Infinite), EmfPlusRegionNodeData::Infinite)
    | (None, EmfPlusRegionNodeData::Raw(_)) => Ok(()),
    (Some(EmfPlusRegionNodeDataType::Path), EmfPlusRegionNodeData::Path(value)) => {
      validate_region_node_path_data(value, "EmfPlusRegionNodePath")
    }
    _ => Err(Error::invalid(
      0,
      "EmfPlusRegionNode Type does not match RegionNodeData",
    )),
  }
}

fn validate_region_node_tree(
  root: &EmfPlusRegionNode,
  expected_count: usize,
  position: u64,
  data_len: u64,
  name: &str,
) -> Result<()> {
  let actual_count = root.node_count();
  if actual_count != expected_count {
    return Err(Error::invalid(
      position,
      format!(
        "{name} node tree has {actual_count} nodes; expected {expected_count} from RegionNodeCount"
      ),
    ));
  }
  if position != data_len {
    return Err(Error::invalid(
      position,
      format!("{name} RegionNode has trailing data"),
    ));
  }
  Ok(())
}

fn read_remaining_vec<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<Vec<u8>> {
  let position = reader.position()?;
  if position > data_len {
    return Err(Error::invalid(
      position,
      format!("{name} starts past end of object data"),
    ));
  }
  reader.read_vec((data_len - position) as usize)
}

fn read_size_prefixed_vec<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<Vec<u8>> {
  ensure_remaining(reader, data_len, 4, name)?;
  let size = reader.read_u32()? as usize;
  ensure_remaining(reader, data_len, size, name)?;
  reader.read_vec(size)
}

fn read_i32_size_prefixed_vec<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<Vec<u8>> {
  ensure_remaining(reader, data_len, 4, name)?;
  let size = reader.read_i32()?;
  if size < 0 {
    return Err(Error::invalid(
      0,
      format!("{name} size must not be negative"),
    ));
  }
  let size = size as usize;
  ensure_remaining(reader, data_len, size, name)?;
  reader.read_vec(size)
}

fn read_f32_array_with_u32_count<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<Vec<f32>> {
  ensure_remaining(reader, data_len, 4, name)?;
  let count = reader.read_u32()? as usize;
  read_f32_array_body(reader, data_len, count, name)
}

fn read_f32_array_body<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  count: usize,
  name: &str,
) -> Result<Vec<f32>> {
  let bytes = count
    .checked_mul(4)
    .ok_or_else(|| Error::invalid(0, format!("{name} byte size overflows")))?;
  ensure_remaining(reader, data_len, bytes, name)?;
  let mut values = Vec::with_capacity(count);
  for _ in 0..count {
    values.push(reader.read_f32()?);
  }
  Ok(values)
}

fn write_f32_array_with_u32_count<W: std::io::Write>(
  writer: &mut Writer<W>,
  values: &[f32],
  name: &str,
) -> Result<()> {
  writer.write_u32(len_to_u32(values.len(), name)?)?;
  for value in values {
    writer.write_f32(*value)?;
  }
  Ok(())
}

fn driver_string_glyph_position_count(
  glyph_count: usize,
  options: EmfPlusDriverStringOptionsFlags,
) -> usize {
  if glyph_count > 0 && options.contains(EmfPlusDriverStringOptionsFlags::REALIZED_ADVANCE) {
    1
  } else {
    glyph_count
  }
}

fn read_rects<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  count: usize,
  flags: EmfPlusRecordFlags,
  data_len: u64,
) -> Result<Vec<EmfPlusRect>> {
  let offset = reader.position()?;
  let rect_size = if flags.contains(EmfPlusRecordFlags::COMPRESSED) {
    8usize
  } else {
    16usize
  };
  let required = count
    .checked_mul(rect_size)
    .ok_or_else(|| Error::invalid(offset, "EMF+ rectangle payload size overflows usize"))?;
  if offset
    .checked_add(required as u64)
    .is_none_or(|end| end > data_len)
  {
    return Err(Error::invalid(
      offset,
      "EMF+ rectangle payload extends past record data",
    ));
  }

  let mut rects = Vec::with_capacity(count);
  for _ in 0..count {
    rects.push(if flags.contains(EmfPlusRecordFlags::COMPRESSED) {
      EmfPlusRect::Compressed(EmfPlusRectS::read_from(reader)?)
    } else {
      EmfPlusRect::Float(RectF::read_from(reader)?)
    });
  }
  ensure_reader_end(reader, data_len, "EMF+ rectangle payload")?;
  Ok(rects)
}

fn read_single_rect<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  flags: EmfPlusRecordFlags,
  data_len: u64,
) -> Result<EmfPlusRect> {
  let offset = reader.position()?;
  let rect_size = if flags.contains(EmfPlusRecordFlags::COMPRESSED) {
    8u64
  } else {
    16u64
  };
  if offset
    .checked_add(rect_size)
    .is_none_or(|end| end > data_len)
  {
    return Err(Error::invalid(
      offset,
      "EMF+ rectangle payload extends past record data",
    ));
  }
  let rect = if flags.contains(EmfPlusRecordFlags::COMPRESSED) {
    EmfPlusRect::Compressed(EmfPlusRectS::read_from(reader)?)
  } else {
    EmfPlusRect::Float(RectF::read_from(reader)?)
  };
  ensure_reader_end(reader, data_len, "EMF+ rectangle payload")?;
  Ok(rect)
}

fn read_points<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  count: usize,
  flags: EmfPlusRecordFlags,
  data_len: u64,
) -> Result<EmfPlusPointData> {
  if flags.contains(EmfPlusRecordFlags::RELATIVE_POSITION) {
    let minimum_required = count
      .checked_mul(2)
      .ok_or_else(|| Error::invalid(0, "EMF+ relative point payload size overflows usize"))?;
    ensure_remaining(
      reader,
      data_len,
      minimum_required,
      "EMF+ relative point payload",
    )?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
      points.push(EmfPlusPointR {
        x: read_emf_plus_integer(reader)?,
        y: read_emf_plus_integer(reader)?,
      });
    }
    ensure_position_within(reader, data_len, "EMF+ relative point payload")?;
    return Ok(EmfPlusPointData::Relative(points));
  }

  if flags.contains(EmfPlusRecordFlags::COMPRESSED) {
    let required = count
      .checked_mul(4)
      .ok_or_else(|| Error::invalid(0, "EMF+ compressed point payload size overflows usize"))?;
    ensure_remaining(reader, data_len, required, "EMF+ compressed point payload")?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
      points.push(PointS::read_from(reader)?);
    }
    return Ok(EmfPlusPointData::Compressed(points));
  }

  let required = count
    .checked_mul(8)
    .ok_or_else(|| Error::invalid(0, "EMF+ point payload size overflows usize"))?;
  ensure_remaining(reader, data_len, required, "EMF+ point payload")?;
  let mut points = Vec::with_capacity(count);
  for _ in 0..count {
    points.push(PointF::read_from(reader)?);
  }
  Ok(EmfPlusPointData::Float(points))
}

fn read_absolute_points<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  count: usize,
  flags: EmfPlusRecordFlags,
  data_len: u64,
) -> Result<EmfPlusPointData> {
  let flags = EmfPlusRecordFlags::from_bits_retain(
    flags.bits() & !EmfPlusRecordFlags::RELATIVE_POSITION.bits(),
  );
  read_points(reader, count, flags, data_len)
}

fn finish_record_point_array<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  points: &EmfPlusPointData,
  name: &str,
) -> Result<()> {
  if !matches!(points, EmfPlusPointData::Relative(_)) {
    return ensure_reader_end(reader, data_len, name);
  }

  let position = reader.position()?;
  let expected_padding = (4 - (position as usize % 4)) % 4;
  let remaining = data_len
    .checked_sub(position)
    .ok_or_else(|| Error::invalid(position, format!("{name} extends past record data")))?;
  if remaining as usize != expected_padding {
    return Err(Error::invalid(
      position,
      format!("{name} alignment padding length does not match DataSize"),
    ));
  }
  let padding = reader.read_vec(expected_padding)?;
  if padding.iter().any(|value| *value != 0) {
    return Err(Error::invalid(
      position,
      format!("{name} alignment padding must be zero"),
    ));
  }
  Ok(())
}

fn write_points<W: std::io::Write>(
  writer: &mut Writer<W>,
  points: &EmfPlusPointData,
) -> Result<()> {
  match points {
    EmfPlusPointData::Relative(values) => {
      for point in values {
        write_emf_plus_integer(writer, point.x)?;
        write_emf_plus_integer(writer, point.y)?;
      }
    }
    EmfPlusPointData::Compressed(values) => {
      for point in values {
        point.write_to(writer)?;
      }
    }
    EmfPlusPointData::Float(values) => {
      for point in values {
        point.write_to(writer)?;
      }
    }
  }
  Ok(())
}

fn write_record_points<W: std::io::Write>(
  writer: &mut Writer<W>,
  points: &EmfPlusPointData,
) -> Result<()> {
  write_points(writer, points)?;
  write_record_point_alignment_padding(writer, points)
}

fn write_record_point_alignment_padding<W: std::io::Write>(
  writer: &mut Writer<W>,
  points: &EmfPlusPointData,
) -> Result<()> {
  if !matches!(points, EmfPlusPointData::Relative(_)) {
    return Ok(());
  }
  let padding_len = (4 - (writer.position()? as usize % 4)) % 4;
  if padding_len > 0 {
    writer.write_all(&[0; 3][..padding_len])?;
  }
  Ok(())
}

fn record_point_payload_size(prefix_len: u64, points: &EmfPlusPointData) -> u64 {
  let unpadded = prefix_len + points.sdk_size();
  if matches!(points, EmfPlusPointData::Relative(_)) {
    unpadded + ((4 - (unpadded as usize % 4)) % 4) as u64
  } else {
    unpadded
  }
}

fn read_emf_plus_integer<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<i16> {
  let first = reader.read_u8()?;
  if first & 0x80 == 0 {
    return Ok(((first << 1) as i8 >> 1) as i16);
  }

  let second = reader.read_u8()?;
  let raw = (u16::from(first & 0x7F) << 8) | u16::from(second);
  Ok(((raw << 1) as i16) >> 1)
}

fn write_emf_plus_integer<W: std::io::Write>(writer: &mut Writer<W>, value: i16) -> Result<()> {
  if (-64..=63).contains(&value) {
    writer.write_u8((value as i8 as u8) & 0x7F)
  } else if (-16_384..=16_383).contains(&value) {
    let raw = (value as u16) & 0x7FFF;
    writer.write_u8(0x80 | ((raw >> 8) as u8 & 0x7F))?;
    writer.write_u8(raw as u8)
  } else {
    Err(Error::invalid(0, "EMF+ integer is outside Integer15 range"))
  }
}

fn emf_plus_integer_size(value: i16) -> u64 {
  if (-64..=63).contains(&value) { 1 } else { 2 }
}

fn read_brush_ref<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  flags: EmfPlusRecordFlags,
) -> Result<EmfPlusBrushRef> {
  if flags.contains(EmfPlusRecordFlags::SOLID_COLOR) {
    Ok(EmfPlusBrushRef::Color(EmfPlusArgb::read_from(reader)?))
  } else {
    Ok(EmfPlusBrushRef::ObjectId(reader.read_u32()?))
  }
}

fn write_brush_ref<W: std::io::Write>(
  writer: &mut Writer<W>,
  brush: EmfPlusBrushRef,
) -> Result<()> {
  match brush {
    EmfPlusBrushRef::ObjectId(value) => writer.write_u32(value),
    EmfPlusBrushRef::Color(value) => value.write_to(writer),
  }
}

fn write_rects<W: std::io::Write>(writer: &mut Writer<W>, rects: &[EmfPlusRect]) -> Result<()> {
  validate_homogeneous_rects(rects)?;
  for rect in rects {
    match rect {
      EmfPlusRect::Compressed(value) => value.write_to(writer)?,
      EmfPlusRect::Float(value) => value.write_to(writer)?,
    }
  }
  Ok(())
}

fn validate_homogeneous_rects(rects: &[EmfPlusRect]) -> Result<()> {
  let Some(first) = rects.first() else {
    return Ok(());
  };
  let first_compressed = matches!(first, EmfPlusRect::Compressed(_));
  if rects
    .iter()
    .any(|rect| matches!(rect, EmfPlusRect::Compressed(_)) != first_compressed)
  {
    return Err(Error::invalid(
      0,
      "EMF+ rectangle payload mixes compressed and floating-point rectangles",
    ));
  }
  Ok(())
}

fn set_rect_flags(flags: EmfPlusRecordFlags, rects: &[EmfPlusRect]) -> EmfPlusRecordFlags {
  let compressed = rects
    .first()
    .is_some_and(|rect| matches!(rect, EmfPlusRect::Compressed(_)));
  let mut next = flags;
  next.set(EmfPlusRecordFlags::COMPRESSED, compressed);
  next
}

fn object_id_flags(value: u8, name: &str) -> Result<EmfPlusRecordFlags> {
  validate_object_id_u8(value, name)?;
  Ok(EmfPlusRecordFlags::from_bits_retain(u16::from(value)))
}

fn set_brush_flags(flags: EmfPlusRecordFlags, brush: EmfPlusBrushRef) -> EmfPlusRecordFlags {
  let mut next = flags;
  next.set(
    EmfPlusRecordFlags::SOLID_COLOR,
    matches!(brush, EmfPlusBrushRef::Color(_)),
  );
  next
}

fn set_brush_flags_checked(
  flags: EmfPlusRecordFlags,
  brush: EmfPlusBrushRef,
  name: &str,
) -> Result<EmfPlusRecordFlags> {
  validate_brush_ref(brush, name)?;
  Ok(set_brush_flags(flags, brush))
}

fn set_point_flags(flags: EmfPlusRecordFlags, points: &EmfPlusPointData) -> EmfPlusRecordFlags {
  let mut next = flags;
  next.set(
    EmfPlusRecordFlags::RELATIVE_POSITION,
    matches!(points, EmfPlusPointData::Relative(_)),
  );
  next.set(
    EmfPlusRecordFlags::COMPRESSED,
    matches!(points, EmfPlusPointData::Compressed(_)),
  );
  next
}

fn set_absolute_point_flags(
  flags: EmfPlusRecordFlags,
  points: &EmfPlusPointData,
) -> EmfPlusRecordFlags {
  let mut next = EmfPlusRecordFlags::from_bits_retain(
    flags.bits() & !EmfPlusRecordFlags::RELATIVE_POSITION.bits(),
  );
  next.set(
    EmfPlusRecordFlags::COMPRESSED,
    matches!(points, EmfPlusPointData::Compressed(_)),
  );
  next
}

fn ensure_remaining<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  required: usize,
  name: &str,
) -> Result<()> {
  let position = reader.position()?;
  if position
    .checked_add(required as u64)
    .is_some_and(|end| end <= data_len)
  {
    Ok(())
  } else {
    Err(Error::invalid(
      position,
      format!("{name} extends past record data"),
    ))
  }
}

fn ensure_position_within<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data_len: u64,
  name: &str,
) -> Result<()> {
  let position = reader.position()?;
  if position <= data_len {
    Ok(())
  } else {
    Err(Error::invalid(
      position,
      format!("{name} extends past record data"),
    ))
  }
}

fn object_id_u8(value: u32, name: &str) -> Result<u8> {
  if value <= 63 {
    Ok(value as u8)
  } else {
    Err(Error::invalid(0, format!("{name} must be in 0..=63")))
  }
}

fn validate_object_id_u8(value: u8, name: &str) -> Result<()> {
  if value <= 63 {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} must be in 0..=63")))
  }
}

fn validate_object_id_u32(value: u32, name: &str) -> Result<()> {
  object_id_u8(value, name).map(|_| ())
}

fn validate_brush_ref(brush: EmfPlusBrushRef, name: &str) -> Result<()> {
  match brush {
    EmfPlusBrushRef::ObjectId(value) => {
      object_id_u8(value, name)?;
      Ok(())
    }
    EmfPlusBrushRef::Color(_) => Ok(()),
  }
}

fn validate_flag_bits(value: u32, allowed: u32, name: &str) -> Result<()> {
  if value & !allowed == 0 {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} contains reserved bits")))
  }
}

fn validate_graphics_version(value: &EmfPlusGraphicsVersion, name: &str) -> Result<()> {
  if !value.is_emf_plus_signature() {
    return Err(Error::invalid(
      0,
      format!("{name} MetafileSignature must be 0xDBC01"),
    ));
  }
  Ok(())
}

fn validate_emf_plus_graphics_version(value: &EmfPlusGraphicsVersion) -> Result<()> {
  validate_graphics_version(value, "EmfPlusGraphicsVersion")
}

fn require_count_at_least(count: usize, min: usize, name: &str) -> Result<()> {
  if count < min {
    return Err(Error::invalid(
      0,
      format!("{name} count must be at least {min}"),
    ));
  }
  Ok(())
}

fn validate_draw_beziers_point_count(count: usize) -> Result<()> {
  require_count_at_least(count, 4, "EmfPlusDrawBeziers point")?;
  if !(count - 1).is_multiple_of(3) {
    return Err(Error::invalid(
      0,
      "EmfPlusDrawBeziers point count must be 3n + 1",
    ));
  }
  Ok(())
}

fn validate_draw_curve_data(value: &EmfPlusDrawCurveData) -> Result<()> {
  validate_object_id_u8(value.pen_id, "EmfPlusDrawCurve ObjectID")?;
  require_count_at_least(value.points.len(), 2, "EmfPlusDrawCurve point")?;
  if matches!(value.points, EmfPlusPointData::Relative(_)) {
    return Err(Error::invalid(
      0,
      "EmfPlusDrawCurve PointData must be absolute",
    ));
  }

  let offset = usize::try_from(value.offset)
    .map_err(|_| Error::invalid(0, "EmfPlusDrawCurve Offset is too large"))?;
  let num_segments = usize::try_from(value.num_segments)
    .map_err(|_| Error::invalid(0, "EmfPlusDrawCurve NumSegments is too large"))?;
  let point_count = value.points.len();
  if offset >= point_count {
    return Err(Error::invalid(
      0,
      "EmfPlusDrawCurve Offset must reference PointData",
    ));
  }

  let max_segments = point_count - offset - 1;
  if num_segments > max_segments {
    return Err(Error::invalid(
      0,
      "EmfPlusDrawCurve NumSegments exceeds PointData range",
    ));
  }

  Ok(())
}

fn validate_draw_string_data(value: &EmfPlusDrawStringData) -> Result<()> {
  let string_bytes = value.string.encoded_bytes()?;
  if !string_bytes.len().is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      "EmfPlusDrawString UTF-16 byte length is odd",
    ));
  }
  if value.padding.len() > 3 {
    return Err(Error::invalid(
      0,
      "EmfPlusDrawString AlignmentPadding has more than 3 bytes",
    ));
  }
  let unpadded_size = 28usize
    .checked_add(string_bytes.len())
    .ok_or_else(|| Error::invalid(0, "EmfPlusDrawString DataSize overflows"))?;
  let expected_padding = (4 - (unpadded_size % 4)) % 4;
  if value.padding.len() != expected_padding {
    return Err(Error::invalid(
      0,
      format!(
        "EmfPlusDrawString AlignmentPadding has {} bytes; expected {}",
        value.padding.len(),
        expected_padding
      ),
    ));
  }
  Ok(())
}

fn validate_start_angle(value: f32, name: &str) -> Result<()> {
  if value < 0.0 || value.is_nan() {
    return Err(Error::invalid(0, format!("{name} must be non-negative")));
  }
  Ok(())
}

fn validate_unit_interval_values(values: &[f32], name: &str) -> Result<()> {
  for value in values {
    if !(0.0..=1.0).contains(value) {
      return Err(Error::invalid(0, format!("{name} value must be in 0..=1")));
    }
  }
  Ok(())
}

fn validate_increasing_unit_interval_values(values: &[f32], name: &str) -> Result<()> {
  validate_unit_interval_values(values, name)?;
  for pair in values.windows(2) {
    if pair[0] >= pair[1] {
      return Err(Error::invalid(
        0,
        format!("{name} values must be strictly increasing"),
      ));
    }
  }
  Ok(())
}

fn validate_blend_factors(positions: &[f32], factors: &[f32]) -> Result<()> {
  require_count_at_least(positions.len(), 2, "EmfPlusBlendFactors position")?;
  validate_unit_interval_values(positions, "EmfPlusBlendFactors positions")?;
  validate_unit_interval_values(factors, "EmfPlusBlendFactors factors")?;
  if positions.first() != Some(&0.0) || positions.last() != Some(&1.0) {
    return Err(Error::invalid(
      0,
      "EmfPlusBlendFactors positions must start at 0.0 and end at 1.0",
    ));
  }
  Ok(())
}

fn validate_blend_pattern(value: &EmfPlusBlendPattern) -> Result<()> {
  fn validate_factors_object(factors: &EmfPlusBlendFactors) -> Result<()> {
    if factors.positions.len() != factors.factors.len() {
      return Err(Error::invalid(
        0,
        "EmfPlusBlendFactors position and factor counts differ",
      ));
    }
    validate_blend_factors(&factors.positions, &factors.factors)?;
    validate_empty_trailing_data(&factors.trailing_data, "EmfPlusBlendFactors")
  }

  match value {
    EmfPlusBlendPattern::Colors(colors) => {
      if colors.positions.len() != colors.colors.len() {
        return Err(Error::invalid(
          0,
          "EmfPlusBlendColors position and color counts differ",
        ));
      }
      validate_unit_interval_values(&colors.positions, "EmfPlusBlendColors positions")?;
      validate_empty_trailing_data(&colors.trailing_data, "EmfPlusBlendColors")
    }
    EmfPlusBlendPattern::Factors(factors) => validate_factors_object(factors),
    EmfPlusBlendPattern::FactorsHV {
      horizontal,
      vertical,
    } => {
      validate_factors_object(horizontal)?;
      validate_factors_object(vertical)
    }
  }
}

fn validate_focus_scale_data(value: &EmfPlusFocusScaleData) -> Result<()> {
  if value.focus_scale_count != 2 {
    return Err(Error::invalid(
      0,
      "EmfPlusFocusScaleData FocusScaleCount must be 2",
    ));
  }
  if !(0.0..1.0).contains(&value.focus_scale_x) || !(0.0..1.0).contains(&value.focus_scale_y) {
    return Err(Error::invalid(
      0,
      "EmfPlusFocusScaleData values must be in 0..1",
    ));
  }
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusFocusScaleData")
}

fn validate_emf_plus_bitmap_object(value: &EmfPlusBitmapObject) -> Result<()> {
  let Some(bitmap_data_type) = value.bitmap_data_type_kind() else {
    return Err(Error::invalid(0, "EmfPlusBitmap BitmapDataType is invalid"));
  };
  if bitmap_data_type == EmfPlusBitmapDataType::Pixel {
    let pixel_format = value.pixel_format_value();
    if pixel_format.kind().is_none() {
      return Err(Error::invalid(0, "EmfPlusBitmap PixelFormat is invalid"));
    }
    if value.width <= 0 {
      return Err(Error::invalid(
        0,
        "EmfPlusBitmap Pixel width must be positive",
      ));
    }
    if value.height <= 0 {
      return Err(Error::invalid(
        0,
        "EmfPlusBitmap Pixel height must be positive",
      ));
    }
    if value.stride % 4 != 0 {
      return Err(Error::invalid(
        0,
        "EmfPlusBitmap Pixel stride must be a multiple of 4",
      ));
    }
    if (value.stride.unsigned_abs() as usize)
      < minimum_emf_plus_bitmap_stride_abs(value.width, pixel_format.bits_per_pixel())?
    {
      return Err(Error::invalid(
        0,
        "EmfPlusBitmap Pixel stride is too small for Width and PixelFormat",
      ));
    }
    let EmfPlusBitmapPayload::Pixel(payload) = read_emf_plus_bitmap_payload(value)? else {
      return Err(Error::invalid(0, "EmfPlusBitmap Pixel data is invalid"));
    };
    if payload.pixel_data.len() != expected_emf_plus_bitmap_pixel_data_len(value)? {
      return Err(Error::invalid(
        0,
        "EmfPlusBitmap PixelData length does not match Stride and Height",
      ));
    }
    if pixel_format.is_indexed() && payload.palette.is_none() {
      return Err(Error::invalid(
        0,
        "EmfPlusBitmap indexed PixelFormat requires Colors palette",
      ));
    }
  }
  Ok(())
}

fn expected_emf_plus_bitmap_pixel_data_len(value: &EmfPlusBitmapObject) -> Result<usize> {
  let stride = usize::try_from(value.stride.unsigned_abs())
    .map_err(|_| Error::invalid(0, "EmfPlusBitmap stride overflows usize"))?;
  let height = usize::try_from(value.height)
    .map_err(|_| Error::invalid(0, "EmfPlusBitmap height overflows usize"))?;
  stride
    .checked_mul(height)
    .ok_or_else(|| Error::invalid(0, "EmfPlusBitmap PixelData size overflows"))
}

fn minimum_emf_plus_bitmap_stride_abs(width: i32, bits_per_pixel: u8) -> Result<usize> {
  let width =
    usize::try_from(width).map_err(|_| Error::invalid(0, "EmfPlusBitmap width is invalid"))?;
  let bits = width
    .checked_mul(usize::from(bits_per_pixel))
    .ok_or_else(|| Error::invalid(0, "EmfPlusBitmap scan-line size overflows"))?;
  bits
    .checked_add(7)
    .map(|bits| bits / 8)
    .ok_or_else(|| Error::invalid(0, "EmfPlusBitmap scan-line size overflows"))
}

fn validate_emf_plus_image_attributes_object(value: &EmfPlusImageAttributesObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusImageAttributes")?;
  if value.wrap_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusImageAttributes WrapMode is invalid",
    ));
  }
  if value.object_clamp_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusImageAttributes ObjectClamp is invalid",
    ));
  }
  Ok(())
}

fn validate_string_format_object(value: &EmfPlusStringFormatObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusStringFormat")?;
  validate_flag_bits(
    value.string_format_flags,
    EmfPlusStringFormatFlags::all().bits(),
    "EmfPlusStringFormat StringFormatFlags",
  )?;
  if value.string_alignment_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusStringFormat StringAlignment is invalid",
    ));
  }
  if value.line_align_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusStringFormat LineAlign is invalid",
    ));
  }
  if value.digit_substitution_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusStringFormat DigitSubstitution is invalid",
    ));
  }
  if value.hotkey_prefix_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusStringFormat HotkeyPrefix is invalid",
    ));
  }
  if value.trimming_kind().is_none() {
    return Err(Error::invalid(0, "EmfPlusStringFormat Trimming is invalid"));
  }
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusStringFormat")
}

fn validate_unknown_object_data_type(object_type_raw: u8) -> Result<()> {
  if EmfPlusObjectType::from_raw(u16::from(object_type_raw)).is_some() {
    return Err(Error::invalid(
      0,
      "EmfPlusObjectData::Unknown requires an unknown ObjectType",
    ));
  }
  Ok(())
}

fn validate_unknown_brush_data_type(brush_type: u32) -> Result<()> {
  if EmfPlusBrushType::from_raw(brush_type).is_some() {
    return Err(Error::invalid(
      0,
      "EmfPlusBrushData::Unknown requires an unknown BrushType",
    ));
  }
  Ok(())
}

fn validate_unknown_custom_line_cap_data_type(cap_type: i32) -> Result<()> {
  if EmfPlusCustomLineCapDataType::from_raw(cap_type).is_some() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapData::Unknown requires an unknown CustomLineCapDataType",
    ));
  }
  Ok(())
}

fn validate_unknown_image_data_type(image_type: u32) -> Result<()> {
  if matches!(
    EmfPlusImageDataType::from_raw(image_type),
    Some(EmfPlusImageDataType::Bitmap | EmfPlusImageDataType::Metafile)
  ) {
    return Err(Error::invalid(
      0,
      "EmfPlusImageData::Unknown requires ImageDataTypeUnknown or an unknown ImageDataType",
    ));
  }
  Ok(())
}

fn validate_unknown_bitmap_data_type(bitmap_data_type: u32) -> Result<()> {
  if EmfPlusBitmapDataType::from_raw(bitmap_data_type).is_some() {
    return Err(Error::invalid(
      0,
      "EmfPlusBitmapPayload::Unknown requires an unknown BitmapDataType",
    ));
  }
  Ok(())
}

fn validate_unknown_image_effect_guid(object_guid: [u8; 16]) -> Result<()> {
  if EmfPlusImageEffectKind::from_guid(object_guid).is_some() {
    return Err(Error::invalid(
      0,
      "EmfPlusImageEffect::Unknown requires an unknown ImageEffects GUID",
    ));
  }
  Ok(())
}

fn validate_pen_data(value: &EmfPlusPenData) -> Result<()> {
  validate_flag_bits(
    value.pen_data_flags,
    EmfPlusPenDataFlags::all().bits(),
    "EmfPlusPenData PenDataFlags",
  )?;
  if value.pen_unit_kind().is_none() {
    return Err(Error::invalid(0, "EmfPlusPenData PenUnit is invalid"));
  }
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusPenData")
}

fn validate_pen_object(value: &EmfPlusPenObject) -> Result<()> {
  if value.pen_type != 0 {
    return Err(Error::invalid(0, "EmfPlusPen Type must be 0"));
  }
  value.parse_pen_payload().map(|_| ())
}

fn validate_pen_object_strict(value: &EmfPlusPenObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusPen")?;
  if value.pen_type != 0 {
    return Err(Error::invalid(0, "EmfPlusPen Type must be 0"));
  }
  let payload = value.parse_pen_payload()?;
  if payload
    .pen_data
    .optional_data
    .dashed_line_cap_type
    .is_some_and(|_| {
      payload
        .pen_data
        .optional_data
        .dashed_line_cap_type_kind()
        .is_none()
    })
  {
    return Err(Error::invalid(
      0,
      "EmfPlusPenData DashedLineCapType is invalid",
    ));
  }
  if let Some(brush) = &payload.brush_object {
    validate_brush_object_strict(brush)?;
  }
  Ok(())
}

fn validate_font_object(value: &EmfPlusFontObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusFont")?;
  validate_flag_bits(
    value.font_style_flags as u32,
    EmfPlusFontStyleFlags::all().bits(),
    "EmfPlusFont FontStyleFlags",
  )?;
  if value.size_unit_kind().is_none() {
    return Err(Error::invalid(0, "EmfPlusFont SizeUnit is invalid"));
  }
  let family_name = value.family_name.encoded_bytes()?;
  if !family_name.len().is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      "EmfPlusFont FamilyName byte length is odd",
    ));
  }
  if value.padding.len() > 3 {
    return Err(Error::invalid(
      0,
      "EmfPlusFont AlignmentPadding has more than 3 bytes",
    ));
  }
  let unpadded_size = 24usize
    .checked_add(family_name.len())
    .ok_or_else(|| Error::invalid(0, "EmfPlusFont DataSize overflows"))?;
  let expected_padding = (4 - (unpadded_size % 4)) % 4;
  if value.padding.len() != expected_padding {
    return Err(Error::invalid(
      0,
      format!(
        "EmfPlusFont AlignmentPadding has {} bytes; expected {}",
        value.padding.len(),
        expected_padding
      ),
    ));
  }
  Ok(())
}

fn validate_palette(value: &EmfPlusPalette) -> Result<()> {
  validate_flag_bits(
    value.palette_style_flags,
    EmfPlusPaletteStyleFlags::all().bits(),
    "EmfPlusPalette PaletteStyleFlags",
  )?;
  if value.flags().contains(EmfPlusPaletteStyleFlags::GRAYSCALE)
    && value
      .entries
      .iter()
      .any(|entry| entry.red != entry.green || entry.green != entry.blue)
  {
    return Err(Error::invalid(
      0,
      "EmfPlusPalette PaletteStyleGrayScale requires grayscale entries",
    ));
  }
  if value.flags().contains(EmfPlusPaletteStyleFlags::HAS_ALPHA)
    && !value.entries.iter().any(|entry| entry.alpha != 0xFF)
  {
    return Err(Error::invalid(
      0,
      "EmfPlusPalette PaletteStyleHasAlpha requires alpha transparency entries",
    ));
  }
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusPalette")
}

fn validate_image_object(value: &EmfPlusImageObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusImage")?;
  if value.image_data_type().is_none() {
    return Err(Error::invalid(0, "EmfPlusImage Type is invalid"));
  }
  value.parse_image_data().map(|_| ())
}

fn validate_image_object_strict(value: &EmfPlusImageObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusImage")?;
  if value.image_data_type().is_none() {
    return Err(Error::invalid(0, "EmfPlusImage Type is invalid"));
  }
  if let EmfPlusImageData::Metafile(metafile) = value.parse_image_data()? {
    metafile.validate_strict()?;
  }
  Ok(())
}

fn validate_metafile_object(value: &EmfPlusMetafileObject) -> Result<()> {
  if value.metafile_data_type_kind().is_none() {
    return Err(Error::invalid(0, "EmfPlusMetafile MetafileType is invalid"));
  }
  Ok(())
}

fn validate_metafile_object_strict(value: &EmfPlusMetafileObject) -> Result<()> {
  validate_metafile_object(value)?;
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusMetafile")
}

fn validate_brush_object(value: &EmfPlusBrushObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusBrush")?;
  if value.brush_kind().is_none() {
    return Err(Error::invalid(0, "EmfPlusBrush Type is invalid"));
  }
  Ok(())
}

fn validate_brush_object_strict(value: &EmfPlusBrushObject) -> Result<()> {
  validate_brush_object(value)?;
  value.parse_brush_data().map(|_| ())
}

fn validate_hatch_brush_data(value: &EmfPlusHatchBrushData) -> Result<()> {
  if value.hatch_style_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusHatchBrushData HatchStyle is invalid",
    ));
  }
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusHatchBrushData")
}

fn validate_solid_brush_data(value: &EmfPlusSolidBrushData) -> Result<()> {
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusSolidBrushData")
}

fn validate_texture_brush_data(value: &EmfPlusTextureBrushData) -> Result<()> {
  validate_flag_bits(
    value.brush_data_flags,
    (EmfPlusBrushDataFlags::TRANSFORM
      | EmfPlusBrushDataFlags::IS_GAMMA_CORRECTED
      | EmfPlusBrushDataFlags::DO_NOT_TRANSFORM)
      .bits(),
    "EmfPlusTextureBrushData BrushDataFlags",
  )?;
  if value.wrap_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusTextureBrushData WrapMode is invalid",
    ));
  }
  let optional_data = value.parse_optional_data()?;
  ensure_no_unparsed_optional_data(
    &optional_data.trailing_data,
    "EmfPlusTextureBrushData OptionalData",
  )?;
  Ok(())
}

fn validate_texture_brush_optional_data(value: &EmfPlusTextureBrushOptionalData) -> Result<()> {
  if let Some(image_object) = &value.image_object {
    validate_image_object(image_object)?;
  }
  ensure_no_unparsed_optional_data(&value.trailing_data, "EmfPlusTextureBrushOptionalData")
}

fn validate_linear_gradient_brush_data(value: &EmfPlusLinearGradientBrushData) -> Result<()> {
  validate_flag_bits(
    value.brush_data_flags,
    (EmfPlusBrushDataFlags::TRANSFORM
      | EmfPlusBrushDataFlags::PRESET_COLORS
      | EmfPlusBrushDataFlags::BLEND_FACTORS_H
      | EmfPlusBrushDataFlags::BLEND_FACTORS_V
      | EmfPlusBrushDataFlags::IS_GAMMA_CORRECTED)
      .bits(),
    "EmfPlusLinearGradientBrushData BrushDataFlags",
  )?;
  if value.wrap_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusLinearGradientBrushData WrapMode is invalid",
    ));
  }
  let optional_data = value.parse_optional_data()?;
  ensure_no_unparsed_optional_data(
    &optional_data.trailing_data,
    "EmfPlusLinearGradientBrushData OptionalData",
  )?;
  Ok(())
}

fn validate_linear_gradient_brush_optional_data(
  value: &EmfPlusLinearGradientBrushOptionalData,
) -> Result<()> {
  if let Some(blend_pattern) = &value.blend_pattern {
    validate_blend_pattern(blend_pattern)?;
  }
  ensure_no_unparsed_optional_data(
    &value.trailing_data,
    "EmfPlusLinearGradientBrushOptionalData",
  )
}

fn validate_path_gradient_brush_data(value: &EmfPlusPathGradientBrushData) -> Result<()> {
  validate_flag_bits(
    value.brush_data_flags,
    (EmfPlusBrushDataFlags::PATH
      | EmfPlusBrushDataFlags::TRANSFORM
      | EmfPlusBrushDataFlags::PRESET_COLORS
      | EmfPlusBrushDataFlags::BLEND_FACTORS_H
      | EmfPlusBrushDataFlags::FOCUS_SCALES
      | EmfPlusBrushDataFlags::IS_GAMMA_CORRECTED)
      .bits(),
    "EmfPlusPathGradientBrushData BrushDataFlags",
  )?;
  if value.wrap_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusPathGradientBrushData WrapMode is invalid",
    ));
  }
  let tail_data = value.parse_tail_data()?;
  ensure_no_unparsed_optional_data(
    &tail_data.trailing_data,
    "EmfPlusPathGradientBrushData OptionalData",
  )?;
  Ok(())
}

fn validate_path_gradient_brush_tail_data(value: &EmfPlusPathGradientBrushTailData) -> Result<()> {
  match &value.boundary_data {
    Some(EmfPlusBoundaryData::Path(path)) => {
      validate_boundary_path_data(path)?;
    }
    Some(EmfPlusBoundaryData::Points(points)) => {
      validate_empty_trailing_data(&points.trailing_data, "EmfPlusBoundaryPointData")?;
    }
    None => {
      return Err(Error::invalid(
        0,
        "EmfPlusPathGradientBrushTailData BoundaryData is missing",
      ));
    }
  }
  validate_path_gradient_brush_optional_data(&value.optional_data)?;
  ensure_no_unparsed_optional_data(&value.trailing_data, "EmfPlusPathGradientBrushTailData")
}

fn validate_path_gradient_brush_optional_data(
  value: &EmfPlusPathGradientBrushOptionalData,
) -> Result<()> {
  if let Some(blend_pattern) = &value.blend_pattern {
    validate_blend_pattern(blend_pattern)?;
  }
  if let Some(focus_scale_data) = &value.focus_scale_data {
    validate_focus_scale_data(focus_scale_data)?;
  }
  Ok(())
}

fn ensure_no_unparsed_optional_data(trailing_data: &[u8], name: &str) -> Result<()> {
  if trailing_data.is_empty() {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} has {} unparsed bytes", trailing_data.len()),
    ))
  }
}

fn validate_emf_plus_object_fragment(value: &EmfPlusObjectRecordData) -> Result<()> {
  validate_object_id_u8(value.object_id, "EmfPlusObject ObjectID")?;
  match value.object_type() {
    Some(EmfPlusObjectType::Invalid) | None => {
      return Err(Error::invalid(0, "EmfPlusObject ObjectType is invalid"));
    }
    Some(_) => {}
  }
  if value.continues != value.total_object_size.is_some() {
    return Err(Error::invalid(
      0,
      "EmfPlusObject continued flag requires TotalObjectSize",
    ));
  }
  if let Some(total_object_size) = value.total_object_size
    && u64::from(total_object_size) < value.object_data.len() as u64
  {
    return Err(Error::invalid(
      0,
      "EmfPlusObject TotalObjectSize is smaller than ObjectData",
    ));
  }
  Ok(())
}

fn validate_emf_plus_record_data(
  value: &EmfPlusRecordData<'_>,
  flags: EmfPlusRecordFlags,
) -> Result<()> {
  match value {
    EmfPlusRecordData::Header(value) => validate_header_data(value, flags)?,
    EmfPlusRecordData::Object(value) => {
      validate_emf_plus_object_fragment(value)?;
      if !value.continues {
        value.parse_object_data()?;
      }
    }
    EmfPlusRecordData::MultiFormatStart(_)
    | EmfPlusRecordData::MultiFormatSection(_)
    | EmfPlusRecordData::MultiFormatEnd(_) => {
      return Err(Error::invalid(0, "EMF+ MultiFormat records are reserved"));
    }
    EmfPlusRecordData::FillRects(value) => {
      validate_brush_ref(value.brush, "EmfPlusFillRects BrushId")?;
    }
    EmfPlusRecordData::DrawRects(value) => {
      validate_object_id_u8(value.pen_id, "EmfPlusDrawRects ObjectID")?;
    }
    EmfPlusRecordData::FillPolygon(value) => {
      validate_brush_ref(value.brush, "EmfPlusFillPolygon BrushId")?;
    }
    EmfPlusRecordData::DrawLines(value) => {
      validate_object_id_u8(value.pen_id, "EmfPlusDrawLines ObjectID")?;
    }
    EmfPlusRecordData::FillEllipse(value) => {
      validate_brush_ref(value.brush, "EmfPlusFillEllipse BrushId")?;
    }
    EmfPlusRecordData::DrawEllipse(value) => {
      validate_object_id_u8(value.pen_id, "EmfPlusDrawEllipse ObjectID")?;
    }
    EmfPlusRecordData::FillPie(value) => {
      validate_brush_ref(value.brush, "EmfPlusFillPie BrushId")?;
      validate_start_angle(value.start_angle, "EmfPlusFillPie StartAngle")?;
    }
    EmfPlusRecordData::DrawPie(value) => {
      validate_object_id_u8(value.pen_id, "EmfPlusDrawPie ObjectID")?;
      validate_start_angle(value.start_angle, "EmfPlusDrawPie StartAngle")?;
    }
    EmfPlusRecordData::DrawArc(value) => {
      validate_object_id_u8(value.pen_id, "EmfPlusDrawArc ObjectID")?;
      validate_start_angle(value.start_angle, "EmfPlusDrawArc StartAngle")?;
    }
    EmfPlusRecordData::FillRegion(value) => {
      validate_object_id_u8(value.object_id, "EmfPlusFillRegion ObjectID")?;
      validate_brush_ref(value.brush, "EmfPlusFillRegion BrushId")?;
    }
    EmfPlusRecordData::FillPath(value) => {
      validate_object_id_u8(value.object_id, "EmfPlusFillPath ObjectID")?;
      validate_brush_ref(value.brush, "EmfPlusFillPath BrushId")?;
    }
    EmfPlusRecordData::DrawPath(value) => {
      validate_object_id_u8(value.object_id, "EmfPlusDrawPath ObjectID")?;
      validate_object_id_u8(value.pen_id, "EmfPlusDrawPath PenId")?;
    }
    EmfPlusRecordData::FillClosedCurve(value) => {
      validate_brush_ref(value.brush, "EmfPlusFillClosedCurve BrushId")?;
    }
    EmfPlusRecordData::DrawClosedCurve(value) => {
      validate_object_id_u8(value.pen_id, "EmfPlusDrawClosedCurve ObjectID")?;
    }
    EmfPlusRecordData::DrawCurve(value) => {
      validate_draw_curve_data(value)?;
    }
    EmfPlusRecordData::DrawBeziers(value) => {
      validate_object_id_u8(value.pen_id, "EmfPlusDrawBeziers ObjectID")?;
      validate_draw_beziers_point_count(value.points.len())?;
    }
    EmfPlusRecordData::DrawImage(value) => {
      validate_object_id_u8(value.image_id, "EmfPlusDrawImage ObjectID")?;
      validate_object_id_u32(
        value.image_attributes_id,
        "EmfPlusDrawImage ImageAttributesID",
      )?;
    }
    EmfPlusRecordData::DrawImagePoints(value) => {
      validate_object_id_u8(value.image_id, "EmfPlusDrawImagePoints ObjectID")?;
    }
    EmfPlusRecordData::DrawString(value) => {
      validate_object_id_u8(value.font_id, "EmfPlusDrawString ObjectID")?;
      validate_brush_ref(value.brush, "EmfPlusDrawString BrushId")?;
      validate_draw_string_data(value)?;
    }
    EmfPlusRecordData::DrawDriverString(value) => {
      validate_object_id_u8(value.font_id, "EmfPlusDrawDriverString ObjectID")?;
      validate_brush_ref(value.brush, "EmfPlusDrawDriverString BrushId")?;
      validate_flag_bits(
        value.driver_string_options_flags,
        EmfPlusDriverStringOptionsFlags::all().bits(),
        "EmfPlusDrawDriverString DriverStringOptionsFlags",
      )?;
      if value.glyph_positions.len() != value.expected_glyph_position_count() {
        return Err(Error::invalid(
          0,
          "EmfPlusDrawDriverString glyph position count mismatch",
        ));
      }
    }
    EmfPlusRecordData::SetClipRect(value) if value.combine_mode_kind().is_none() => {
      return Err(Error::invalid(
        0,
        "EmfPlusSetClipRect CombineMode is invalid",
      ));
    }
    EmfPlusRecordData::SetClipPath(value) => {
      validate_object_id_u8(value.object_id, "EmfPlusSetClipPath ObjectID")?;
      if value.combine_mode_kind().is_none() {
        return Err(Error::invalid(
          0,
          "EmfPlusSetClipPath CombineMode is invalid",
        ));
      }
    }
    EmfPlusRecordData::SetClipRegion(value) => {
      validate_object_id_u8(value.object_id, "EmfPlusSetClipRegion ObjectID")?;
      if value.combine_mode_kind().is_none() {
        return Err(Error::invalid(
          0,
          "EmfPlusSetClipRegion CombineMode is invalid",
        ));
      }
    }
    EmfPlusRecordData::SetAntiAliasMode(value) if value.smoothing_mode_kind().is_none() => {
      return Err(Error::invalid(
        0,
        "EmfPlusSetAntiAliasMode SmoothingMode is invalid",
      ));
    }
    EmfPlusRecordData::SetTextRenderingHint(value) if value.text_rendering_hint().is_none() => {
      return Err(Error::invalid(
        0,
        "EmfPlusSetTextRenderingHint TextRenderingHint is invalid",
      ));
    }
    EmfPlusRecordData::SetTextContrast(value) => {
      validate_text_contrast(value.text_contrast, "EmfPlusSetTextContrast TextContrast")?;
    }
    EmfPlusRecordData::SetInterpolationMode(value) if value.interpolation_mode().is_none() => {
      return Err(Error::invalid(
        0,
        "EmfPlusSetInterpolationMode InterpolationMode is invalid",
      ));
    }
    EmfPlusRecordData::SetPixelOffsetMode(value) if value.pixel_offset_mode().is_none() => {
      return Err(Error::invalid(
        0,
        "EmfPlusSetPixelOffsetMode PixelOffsetMode is invalid",
      ));
    }
    EmfPlusRecordData::SetCompositingMode(value) if value.compositing_mode().is_none() => {
      return Err(Error::invalid(
        0,
        "EmfPlusSetCompositingMode CompositingMode is invalid",
      ));
    }
    EmfPlusRecordData::BeginContainer(_) => {
      validate_page_unit_flags(flags, "EmfPlusBeginContainer")?;
    }
    EmfPlusRecordData::SetPageTransform(_) => {
      validate_page_unit_flags(flags, "EmfPlusSetPageTransform")?;
    }
    EmfPlusRecordData::SerializableObject(value) => value.validate_known_effect_buffer()?,
    EmfPlusRecordData::SetTsGraphics(value) => validate_set_ts_graphics(value, flags)?,
    EmfPlusRecordData::SetTsClip(value) => validate_set_ts_clip(value)?,
    _ => {}
  }
  Ok(())
}

fn validate_page_unit_flags(flags: EmfPlusRecordFlags, name: &str) -> Result<()> {
  if flags.bits() & !0x00FF != 0 {
    return Err(Error::invalid(
      0,
      format!("{name} Flags contains nonzero reserved bits"),
    ));
  }
  if flags.page_unit().is_none() {
    return Err(Error::invalid(0, format!("{name} PageUnit is invalid")));
  }
  Ok(())
}

fn validate_header_data(value: &EmfPlusHeaderData, _flags: EmfPlusRecordFlags) -> Result<()> {
  validate_graphics_version(&value.graphics_version, "EmfPlusHeader")?;
  Ok(())
}

fn validate_emf_plus_header_data(value: &EmfPlusHeaderData) -> Result<()> {
  validate_header_data(value, EmfPlusRecordFlags::empty())
}

fn validate_draw_image_src_unit(value: i32, name: &str) -> Result<()> {
  if value == EmfPlusUnitType::Pixel.raw() as i32 {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} must be UnitTypePixel")))
  }
}

fn validate_text_contrast(value: u16, name: &str) -> Result<()> {
  if (1000..=2200).contains(&value) {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} must be in 1000..=2200")))
  }
}

fn validate_ts_text_contrast(value: u16, name: &str) -> Result<()> {
  if value <= 12 {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} must be in 0..=12")))
  }
}

fn validate_set_ts_graphics(
  value: &EmfPlusSetTsGraphicsData,
  flags: EmfPlusRecordFlags,
) -> Result<()> {
  if value.anti_alias_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics AntiAliasMode is invalid",
    ));
  }
  if value.text_rendering_hint_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics TextRenderHint is invalid",
    ));
  }
  if value.compositing_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics CompositingMode is invalid",
    ));
  }
  if value.compositing_quality_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics CompositingQuality is invalid",
    ));
  }
  validate_ts_text_contrast(value.text_contrast, "EmfPlusSetTSGraphics TextContrast")?;
  if value.filter_type_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics FilterType is invalid",
    ));
  }
  if value.pixel_offset_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics PixelOffset is invalid",
    ));
  }
  if flags.ts_graphics_palette_present() != value.palette.is_some() {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics Palette presence disagrees with flags",
    ));
  }
  if flags.ts_graphics_basic_vga() {
    let Some(palette) = &value.palette else {
      return Err(Error::invalid(
        0,
        "EmfPlusSetTSGraphics BasicVGA flag requires Palette data",
      ));
    };
    if palette
      .entries
      .iter()
      .any(|entry| !is_basic_vga_argb(entry))
    {
      return Err(Error::invalid(
        0,
        "EmfPlusSetTSGraphics BasicVGA palette contains non-VGA colors",
      ));
    }
  }
  if let Some(palette) = &value.palette
    && !palette.trailing_data.is_empty()
  {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSGraphics Palette must not contain trailing data",
    ));
  }
  Ok(())
}

fn validate_set_ts_clip(value: &EmfPlusSetTsClipData) -> Result<()> {
  if value.rect_count > EMFPLUS_TS_CLIP_MAX_RECTS {
    return Err(Error::invalid(
      0,
      "EmfPlusSetTSClip NumRects exceeds 15-bit field",
    ));
  }
  match &value.rects {
    EmfPlusSetTsClipRects::Rects(rects) => {
      if value.compressed {
        return Err(Error::invalid(
          0,
          "EmfPlusSetTSClip uncompressed rects require C flag clear",
        ));
      }
      if usize::from(value.rect_count) != rects.len() {
        return Err(Error::invalid(
          0,
          "EmfPlusSetTSClip RectCount does not match rects length",
        ));
      }
    }
    EmfPlusSetTsClipRects::Compressed(rects) => {
      if !value.compressed {
        return Err(Error::invalid(
          0,
          "EmfPlusSetTSClip compressed rects require C flag set",
        ));
      }
      if usize::from(value.rect_count) != rects.len() {
        return Err(Error::invalid(
          0,
          "EmfPlusSetTSClip RectCount does not match compressed rects length",
        ));
      }
      for rect in rects {
        rect.to_bytes()?;
      }
    }
  }
  Ok(())
}

fn is_basic_vga_argb(entry: &EmfPlusArgb) -> bool {
  matches!(
    (entry.red, entry.green, entry.blue),
    (0x00, 0x00, 0x00)
      | (0x00, 0x00, 0x80)
      | (0x00, 0x80, 0x00)
      | (0x00, 0x80, 0x80)
      | (0x80, 0x00, 0x00)
      | (0x80, 0x00, 0x80)
      | (0x80, 0x80, 0x00)
      | (0xC0, 0xC0, 0xC0)
      | (0x80, 0x80, 0x80)
      | (0x00, 0x00, 0xFF)
      | (0x00, 0xFF, 0x00)
      | (0x00, 0xFF, 0xFF)
      | (0xFF, 0x00, 0x00)
      | (0xFF, 0x00, 0xFF)
      | (0xFF, 0xFF, 0x00)
      | (0xFF, 0xFF, 0xFF)
  )
}

fn validate_blur_effect(value: &EmfPlusBlurEffect) -> Result<()> {
  validate_f32_range(value.blur_radius, 0.0..=255.0, "BlurEffect BlurRadius")?;
  validate_bool_u32(value.expand_edge, "BlurEffect ExpandEdge")?;
  validate_empty_trailing_data(&value.trailing_data, "BlurEffect")
}

fn validate_brightness_contrast_effect(value: &EmfPlusBrightnessContrastEffect) -> Result<()> {
  validate_i32_range(
    value.brightness_level,
    -255..=255,
    "BrightnessContrastEffect BrightnessLevel",
  )?;
  validate_i32_range(
    value.contrast_level,
    -100..=100,
    "BrightnessContrastEffect ContrastLevel",
  )?;
  validate_empty_trailing_data(&value.trailing_data, "BrightnessContrastEffect")
}

fn validate_color_balance_effect(value: &EmfPlusColorBalanceEffect) -> Result<()> {
  validate_i32_range(value.cyan_red, -100..=100, "ColorBalanceEffect CyanRed")?;
  validate_i32_range(
    value.magenta_green,
    -100..=100,
    "ColorBalanceEffect MagentaGreen",
  )?;
  validate_i32_range(
    value.yellow_blue,
    -100..=100,
    "ColorBalanceEffect YellowBlue",
  )?;
  validate_empty_trailing_data(&value.trailing_data, "ColorBalanceEffect")
}

fn validate_color_curve_effect(value: &EmfPlusColorCurveEffect) -> Result<()> {
  let adjustment = value
    .curve_adjustment_kind()
    .ok_or_else(|| Error::invalid(0, "ColorCurveEffect CurveAdjustment is invalid"))?;
  if value.curve_channel_kind().is_none() {
    return Err(Error::invalid(
      0,
      "ColorCurveEffect CurveChannel is invalid",
    ));
  }
  let valid_range = match adjustment {
    EmfPlusCurveAdjustment::Exposure | EmfPlusCurveAdjustment::Density => -255..=255,
    EmfPlusCurveAdjustment::Contrast
    | EmfPlusCurveAdjustment::Highlight
    | EmfPlusCurveAdjustment::Shadow
    | EmfPlusCurveAdjustment::Midtone => -100..=100,
    EmfPlusCurveAdjustment::WhiteSaturation | EmfPlusCurveAdjustment::BlackSaturation => 0..=255,
  };
  if !valid_range.contains(&value.adjustment_intensity) {
    return Err(Error::invalid(
      0,
      "ColorCurveEffect AdjustmentIntensity is outside the valid range",
    ));
  }
  validate_empty_trailing_data(&value.trailing_data, "ColorCurveEffect")
}

fn validate_color_lookup_table_effect(value: &EmfPlusColorLookupTableEffect) -> Result<()> {
  validate_empty_trailing_data(&value.trailing_data, "ColorLookupTableEffect")
}

fn validate_color_matrix_effect(value: &EmfPlusColorMatrixEffect) -> Result<()> {
  // The wire layout groups Matrix_N_0 through Matrix_N_4, so the outer
  // index is the second index in the names used by MS-EMFPLUS. The required
  // zeroes are Matrix_4_0 through Matrix_4_3, not the translation entries
  // Matrix_0_4 through Matrix_3_4.
  for column in 0..4 {
    if value.matrix[column][4] != 0.0 {
      return Err(Error::invalid(
        0,
        "ColorMatrixEffect Matrix_4_0 through Matrix_4_3 must be 0.0",
      ));
    }
  }
  validate_empty_trailing_data(&value.trailing_data, "ColorMatrixEffect")
}

fn validate_hue_saturation_lightness_effect(
  value: &EmfPlusHueSaturationLightnessEffect,
) -> Result<()> {
  validate_i32_range(
    value.hue_level,
    -180..=180,
    "HueSaturationLightnessEffect HueLevel",
  )?;
  validate_i32_range(
    value.saturation_level,
    -100..=100,
    "HueSaturationLightnessEffect SaturationLevel",
  )?;
  validate_i32_range(
    value.lightness_level,
    -100..=100,
    "HueSaturationLightnessEffect LightnessLevel",
  )?;
  validate_empty_trailing_data(&value.trailing_data, "HueSaturationLightnessEffect")
}

fn validate_levels_effect(value: &EmfPlusLevelsEffect) -> Result<()> {
  validate_i32_range(value.highlight, 0..=100, "LevelsEffect Highlight")?;
  validate_i32_range(value.mid_tone, -100..=100, "LevelsEffect MidTone")?;
  validate_i32_range(value.shadow, 0..=100, "LevelsEffect Shadow")?;
  validate_empty_trailing_data(&value.trailing_data, "LevelsEffect")
}

fn validate_red_eye_correction_effect(value: &EmfPlusRedEyeCorrectionEffect) -> Result<()> {
  validate_empty_trailing_data(&value.trailing_data, "RedEyeCorrectionEffect")
}

fn validate_sharpen_effect(value: &EmfPlusSharpenEffect) -> Result<()> {
  validate_f32_range(value.amount, 0.0..=100.0, "SharpenEffect Amount")?;
  validate_empty_trailing_data(&value.trailing_data, "SharpenEffect")
}

fn validate_tint_effect(value: &EmfPlusTintEffect) -> Result<()> {
  validate_i32_range(value.hue, -180..=180, "TintEffect Hue")?;
  validate_i32_range(value.amount, -100..=100, "TintEffect Amount")?;
  validate_empty_trailing_data(&value.trailing_data, "TintEffect")
}

fn validate_empty_trailing_data(data: &[u8], name: &str) -> Result<()> {
  if data.is_empty() {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} must not contain trailing data"),
    ))
  }
}

fn validate_custom_line_cap_object(value: &EmfPlusCustomLineCapObject) -> Result<()> {
  validate_graphics_version(&value.version, "EmfPlusCustomLineCap")?;
  if value.cap_data_type().is_none() {
    return Err(Error::invalid(0, "EmfPlusCustomLineCap Type is invalid"));
  }
  value.parse_cap_data().map(|_| ())
}

fn validate_custom_line_cap_arrow_data(value: &EmfPlusCustomLineCapArrowData) -> Result<()> {
  validate_bool_u32(value.fill_state, "EmfPlusCustomLineCapArrowData FillState")?;
  if value.line_start_cap_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapArrowData LineStartCap is invalid",
    ));
  }
  if value.line_end_cap_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapArrowData LineEndCap is invalid",
    ));
  }
  if value.line_join_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapArrowData LineJoin is invalid",
    ));
  }
  validate_zero_point_f(
    value.fill_hot_spot,
    "EmfPlusCustomLineCapArrowData FillHotSpot",
  )?;
  validate_zero_point_f(
    value.line_hot_spot,
    "EmfPlusCustomLineCapArrowData LineHotSpot",
  )?;
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusCustomLineCapArrowData")
}

fn validate_custom_line_cap_default_data(value: &EmfPlusCustomLineCapDefaultData) -> Result<()> {
  validate_flag_bits(
    value.custom_line_cap_data_flags,
    EmfPlusCustomLineCapDataFlags::all().bits(),
    "EmfPlusCustomLineCapData CustomLineCapDataFlags",
  )?;
  if value.base_cap_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapData BaseCap is invalid",
    ));
  }
  if value.stroke_start_cap_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapData StrokeStartCap is invalid",
    ));
  }
  if value.stroke_end_cap_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapData StrokeEndCap is invalid",
    ));
  }
  if value.stroke_join_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EmfPlusCustomLineCapData StrokeJoin is invalid",
    ));
  }
  validate_zero_point_f(value.fill_hot_spot, "EmfPlusCustomLineCapData FillHotSpot")?;
  validate_zero_point_f(
    value.stroke_hot_spot,
    "EmfPlusCustomLineCapData StrokeHotSpot",
  )?;
  let optional_data = value.parse_optional_data()?;
  validate_custom_line_cap_optional_data(&optional_data)?;
  Ok(())
}

fn validate_custom_line_cap_optional_data(value: &EmfPlusCustomLineCapOptionalData) -> Result<()> {
  if let Some(fill_path) = &value.fill_path {
    validate_fill_path_object(fill_path)?;
  }
  if let Some(line_path) = &value.line_path {
    validate_line_path_object(line_path)?;
  }
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusCustomLineCapOptionalData")
}

fn validate_fill_path_object(value: &EmfPlusFillPathObject) -> Result<()> {
  validate_region_node_path_data(&value.path_data, "EmfPlusFillPath")?;
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusFillPath")
}

fn validate_line_path_object(value: &EmfPlusLinePathObject) -> Result<()> {
  validate_region_node_path_data(&value.path_data, "EmfPlusLinePath")?;
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusLinePath")
}

fn validate_boundary_path_data(value: &EmfPlusBoundaryPathData) -> Result<()> {
  validate_region_node_path_data(&value.path_data, "EmfPlusBoundaryPathData")?;
  validate_empty_trailing_data(&value.trailing_data, "EmfPlusBoundaryPathData")
}

fn validate_region_node_path_data(value: &EmfPlusRegionNodePathData, name: &str) -> Result<()> {
  match value {
    EmfPlusRegionNodePathData::Path(path) => validate_path_object(path),
    EmfPlusRegionNodePathData::Raw(_) => Err(Error::invalid(
      0,
      format!("{name} must contain EmfPlusPath"),
    )),
  }
}

fn validate_zero_point_f(value: PointF, name: &str) -> Result<()> {
  if value.x == 0.0 && value.y == 0.0 {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} must be {{0.0, 0.0}}")))
  }
}

fn validate_bool_u32(value: u32, name: &str) -> Result<()> {
  if value <= 1 {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} must be 0 or 1")))
  }
}

fn validate_i32_range(value: i32, range: std::ops::RangeInclusive<i32>, name: &str) -> Result<()> {
  if range.contains(&value) {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} is outside {}..={}", range.start(), range.end()),
    ))
  }
}

fn validate_f32_range(value: f32, range: std::ops::RangeInclusive<f32>, name: &str) -> Result<()> {
  if range.contains(&value) {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} is outside {}..={}", range.start(), range.end()),
    ))
  }
}

fn len_to_u32(len: usize, name: &str) -> Result<u32> {
  if len > u32::MAX as usize {
    return Err(Error::invalid(0, format!("{name} exceeds u32::MAX")));
  }
  Ok(len as u32)
}

fn len_to_i32(len: usize, name: &str) -> Result<i32> {
  if len > i32::MAX as usize {
    return Err(Error::invalid(0, format!("{name} exceeds i32::MAX")));
  }
  Ok(len as i32)
}

fn non_negative_count(value: i32, name: &str) -> Result<usize> {
  if value < 0 {
    return Err(Error::invalid(0, format!("{name} must not be negative")));
  }
  Ok(value as usize)
}

fn wrap_mode_from_i32(value: i32) -> Option<EmfPlusWrapMode> {
  u32::try_from(value)
    .ok()
    .and_then(EmfPlusWrapMode::from_raw)
}

pub fn read_records(bytes: &[u8]) -> Result<Vec<EmfPlusRecord>> {
  Ok(EmfPlusStream::from_bytes_exact(bytes)?.records)
}

pub(crate) fn read_records_with_trailing(bytes: &[u8]) -> Result<(Vec<EmfPlusRecord>, Vec<u8>)> {
  let stream = EmfPlusStream::from_bytes(bytes)?;
  Ok((stream.records, stream.trailing_data))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::common::{SdkRead, SdkSize, SdkWrite};

  #[test]
  fn emf_plus_record_roundtrip_preserves_padding() {
    let bytes = [
      0x03, 0x40, // Type: Comment
      0x00, 0x00, // Flags
      0x10, 0x00, 0x00, 0x00, // Size
      0x02, 0x00, 0x00, 0x00, // DataSize
      0xAA, 0xBB, // Data
      0xCC, 0xDD, // Padding
    ];
    let records = read_records(&bytes).unwrap();
    assert_eq!(records[0].data, [0xAA, 0xBB]);
    assert_eq!(records[0].padding, [0xCC, 0xDD]);
    assert!(records[0].parse_data().is_err());

    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    records[0].write_to(&mut writer).unwrap();
    assert_eq!(writer.into_inner().into_inner(), bytes);
    assert_eq!(records[0].record_kind(), Some(EmfPlusRecordType::Comment));

    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::Comment(vec![0xAA, 0xBB]),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let excessive_padding = [
      0x03, 0x40, // Type: Comment
      0x00, 0x00, // Flags
      0x10, 0x00, 0x00, 0x00, // Size
      0x00, 0x00, 0x00, 0x00, // DataSize
      0x00, 0x00, 0x00, 0x00, // Padding
    ];
    assert!(read_records(&excessive_padding).is_err());

    let excessive_padding_record = EmfPlusRecord {
      record_type: EmfPlusRecordType::Comment.raw(),
      flags: 0,
      total_object_size: None,
      data: Vec::new(),
      padding: vec![0; 4],
    };
    assert!(
      excessive_padding_record
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
  }

  #[test]
  fn emf_plus_borrowed_stream_uses_input_storage_and_materializes_explicitly() {
    let bytes = [
      0x03, 0x40, // Type: Comment
      0x00, 0x00, // Flags
      0x10, 0x00, 0x00, 0x00, // Size
      0x02, 0x00, 0x00, 0x00, // DataSize
      0xAA, 0xBB, // Data
      0xCC, 0xDD, // Padding
    ];
    let stream = EmfPlusStreamRef::from_bytes(&bytes).unwrap();
    assert_eq!(stream.record_count(), 1);
    assert!(stream.trailing_data().is_empty());

    let record = stream.records().next().unwrap();
    assert_eq!(record.data.as_ptr(), bytes[12..].as_ptr());
    assert_eq!(record.padding.as_ptr(), bytes[14..].as_ptr());
    assert_eq!(record.sdk_size(), bytes.len() as u64);
    assert!(record.parse_data().is_err());
    assert_eq!(stream.into_owned().records, read_records(&bytes).unwrap());

    let eof_bytes = [
      0x02, 0x40, // Type: EndOfFile
      0x00, 0x00, // Flags
      0x0C, 0x00, 0x00, 0x00, // Size
      0x00, 0x00, 0x00, 0x00, // DataSize
    ];
    let eof = EmfPlusStreamRef::from_bytes(&eof_bytes)
      .unwrap()
      .records()
      .next()
      .unwrap();
    assert!(matches!(eof.parse_data().unwrap(), EmfPlusRecordData::Eof));
    assert_eq!(eof.rebuild_typed().unwrap().as_ref(), eof);
    let mut invalid_late_record = eof_bytes.repeat(2);
    invalid_late_record[16..20].copy_from_slice(&8u32.to_le_bytes());
    assert!(EmfPlusStreamRef::from_bytes(&invalid_late_record).is_err());

    let mut compatible = bytes.to_vec();
    compatible.extend_from_slice(&[0x11, 0x22]);
    let stream = EmfPlusStreamRef::from_bytes(&compatible).unwrap();
    assert_eq!(stream.trailing_data(), &[0x11, 0x22]);
    assert_eq!(stream.into_owned().to_bytes().unwrap(), compatible);
    assert!(EmfPlusStreamRef::from_bytes_exact(&compatible).is_err());
  }

  #[test]
  fn derived_emf_plus_header_data_roundtrips() {
    let header = EmfPlusHeaderData {
      graphics_version: EmfPlusGraphicsVersion::from_graphics_version(
        EmfPlusGraphicsVersionValue::Version1_1,
      ),
      emf_plus_flags: 1,
      logical_dpi_x: 96,
      logical_dpi_y: 96,
    };

    assert_eq!(
      header.graphics_version.metafile_signature(),
      EMFPLUS_METAFILE_SIGNATURE
    );
    assert!(header.graphics_version.is_emf_plus_signature());
    assert_eq!(
      header.graphics_version.graphics_version(),
      Some(EmfPlusGraphicsVersionValue::Version1_1)
    );
    assert_eq!(header.graphics_version.graphics_version_raw(), 0x0002);
    assert_eq!(
      EmfPlusGraphicsVersion::from_parts(EMFPLUS_METAFILE_SIGNATURE, 0x0002).unwrap(),
      header.graphics_version
    );
    assert!(EmfPlusGraphicsVersion::from_parts(0x0010_0000, 0x0002).is_err());
    assert!(EmfPlusGraphicsVersion::from_parts(EMFPLUS_METAFILE_SIGNATURE, 0x1000).is_err());
    let vendor_graphics_version =
      EmfPlusGraphicsVersion::from_parts(EMFPLUS_METAFILE_SIGNATURE, 0x0FFF).unwrap();
    assert_eq!(vendor_graphics_version.graphics_version_raw(), 0x0FFF);
    assert_eq!(vendor_graphics_version.graphics_version(), None);
    assert!(header.video_display());
    assert_eq!(header.emf_plus_reserved_flags(), 0);
    assert_eq!(header.sdk_size(), 16);

    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    header.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();

    let mut reader = Reader::new(std::io::Cursor::new(bytes));
    let parsed = EmfPlusHeaderData::read_from(&mut reader).unwrap();
    assert_eq!(parsed, header);

    let record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::Header(header.clone()),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    assert_eq!(
      record.parse_data().unwrap(),
      EmfPlusRecordData::Header(header.clone())
    );
    let dual_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::Header(header.clone()),
      EmfPlusRecordFlags::from_bits_retain(0x0001),
    )
    .unwrap();
    assert!(dual_record.flags().header_dual());
    assert_eq!(dual_record.flags().header_reserved_bits(), 0);

    let mut invalid_header = header.clone();
    invalid_header.graphics_version.value = 0x0000_0002;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::Header(invalid_header.clone()),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      invalid_header
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
    let invalid_bytes = [
      invalid_header.graphics_version.value.to_le_bytes(),
      invalid_header.emf_plus_flags.to_le_bytes(),
      invalid_header.logical_dpi_x.to_le_bytes(),
      invalid_header.logical_dpi_y.to_le_bytes(),
    ]
    .concat();
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::Header.raw(),
        flags: 0,
        total_object_size: None,
        data: invalid_bytes,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let mut vendor_graphics_version = header.clone();
    vendor_graphics_version.graphics_version.value = (EMFPLUS_METAFILE_SIGNATURE << 12) | 3;
    let vendor_graphics_version_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::Header(vendor_graphics_version.clone()),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    let mut vendor_graphics_version_bytes = Vec::new();
    vendor_graphics_version
      .write_to(&mut Writer::new(std::io::Cursor::new(
        &mut vendor_graphics_version_bytes,
      )))
      .unwrap();
    assert_eq!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::Header.raw(),
        flags: 0,
        total_object_size: None,
        data: vendor_graphics_version_bytes,
        padding: Vec::new(),
      }
      .parse_data()
      .unwrap(),
      vendor_graphics_version_record.parse_data().unwrap()
    );
    let reserved_flags_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::Header(header.clone()),
      EmfPlusRecordFlags::from_bits_retain(0x8001),
    )
    .unwrap();
    assert!(reserved_flags_record.flags().header_dual());
    assert_eq!(reserved_flags_record.flags().header_reserved_bits(), 0x8000);
    assert_eq!(
      reserved_flags_record.parse_data().unwrap(),
      EmfPlusRecordData::Header(header.clone())
    );
    let mut parsed_reserved_flags_record = record.clone();
    parsed_reserved_flags_record.flags = 0x8001;
    assert_eq!(
      parsed_reserved_flags_record.parse_data().unwrap(),
      EmfPlusRecordData::Header(header.clone())
    );
    let mut reserved_emf_plus_flags = header.clone();
    reserved_emf_plus_flags.emf_plus_flags = 0x8000_0001;
    let reserved_emf_plus_flags_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::Header(reserved_emf_plus_flags.clone()),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    assert_eq!(
      reserved_emf_plus_flags_record.parse_data().unwrap(),
      EmfPlusRecordData::Header(reserved_emf_plus_flags.clone())
    );
    let mut reserved_emf_plus_flags_bytes = Vec::new();
    reserved_emf_plus_flags
      .write_to(&mut Writer::new(std::io::Cursor::new(
        &mut reserved_emf_plus_flags_bytes,
      )))
      .unwrap();
    assert_eq!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::Header.raw(),
        flags: 0,
        total_object_size: None,
        data: reserved_emf_plus_flags_bytes,
        padding: Vec::new(),
      }
      .parse_data()
      .unwrap(),
      EmfPlusRecordData::Header(reserved_emf_plus_flags)
    );
    let mut invalid_record = record.clone();
    invalid_record.data.extend_from_slice(&[0, 0, 0, 0]);
    assert!(invalid_record.parse_data().is_err());
  }

  #[test]
  fn maps_emf_plus_object_type_flags() {
    let flags = EmfPlusRecordFlags::from_bits_retain(0x8305);
    assert!(flags.object_continues());
    assert_eq!(flags.object_type_raw(), 3);
    assert_eq!(flags.object_type(), Some(EmfPlusObjectType::Path));
    assert_eq!(flags.object_id(), 5);
    assert_eq!(
      EmfPlusRecordFlags::EFFECT.bits(),
      EmfPlusRecordFlags::POST_MULTIPLY.bits()
    );
  }

  #[test]
  fn emf_plus_known_truncated_records_are_not_unknown() {
    let known = EmfPlusRecord {
      record_type: EmfPlusRecordType::FillRects.raw(),
      flags: EmfPlusRecordFlags::SOLID_COLOR.bits(),
      total_object_size: None,
      data: vec![0, 0, 0, 0],
      padding: Vec::new(),
    };
    assert!(known.parse_data().is_err());

    let unknown = EmfPlusRecord {
      record_type: 0x7FFF,
      flags: 0,
      total_object_size: None,
      data: vec![1, 2, 3, 4],
      padding: Vec::new(),
    };
    let EmfPlusRecordData::Unknown(parsed) = unknown.parse_data().unwrap() else {
      panic!("expected unknown EMF+ record");
    };
    assert_eq!(parsed.record_type, 0x7FFF);
    assert_eq!(parsed.data, vec![1, 2, 3, 4]);
  }

  #[test]
  fn emf_plus_fill_rects_parse_and_write_roundtrips() {
    let record = EmfPlusRecord {
      record_type: EmfPlusRecordType::FillRects.raw(),
      flags: (EmfPlusRecordFlags::SOLID_COLOR | EmfPlusRecordFlags::COMPRESSED).bits(),
      total_object_size: None,
      data: vec![
        0x33, 0x22, 0x11, 0xFF, // ARGB in little-endian byte order
        0x01, 0x00, 0x00, 0x00, // Count
        0x01, 0x00, // x
        0x02, 0x00, // y
        0x03, 0x00, // width
        0x04, 0x00, // height
      ],
      padding: Vec::new(),
    };

    let data = record.parse_data().unwrap();
    assert_eq!(
      data,
      EmfPlusRecordData::FillRects(EmfPlusFillRectsData {
        brush: EmfPlusBrushRef::Color(EmfPlusArgb {
          blue: 0x33,
          green: 0x22,
          red: 0x11,
          alpha: 0xFF,
        }),
        rects: vec![EmfPlusRect::Compressed(EmfPlusRectS {
          x: 1,
          y: 2,
          width: 3,
          height: 4,
        })],
      })
    );

    let written = EmfPlusRecord::from_data(&data, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(written.record_type, record.record_type);
    assert_eq!(written.flags, record.flags);
    assert_eq!(written.data, record.data);

    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::FillRects.raw(),
        flags: EmfPlusRecordFlags::SOLID_COLOR.bits(),
        total_object_size: None,
        data: vec![
          0x33, 0x22, 0x11, 0xFF, // ARGB
          0x00, 0x00, 0x00, 0x00, // Count
        ],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::FillRects.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits(),
        total_object_size: None,
        data: vec![
          0x40, 0x00, 0x00, 0x00, // BrushId outside object table
          0x01, 0x00, 0x00, 0x00, // Count
          0x01, 0x00, // x
          0x02, 0x00, // y
          0x03, 0x00, // width
          0x04, 0x00, // height
        ],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::FillRects(EmfPlusFillRectsData {
          brush: EmfPlusBrushRef::ObjectId(2),
          rects: vec![
            EmfPlusRect::Compressed(EmfPlusRectS {
              x: 1,
              y: 2,
              width: 3,
              height: 4,
            }),
            EmfPlusRect::Float(RectF {
              x: 1.0,
              y: 2.0,
              width: 3.0,
              height: 4.0,
            }),
          ],
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
  }

  #[test]
  fn emf_plus_draw_rects_carries_pen_id_in_flags() {
    let data = EmfPlusRecordData::DrawRects(EmfPlusDrawRectsData {
      pen_id: 7,
      rects: vec![EmfPlusRect::Float(RectF {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
      })],
    });

    let record = EmfPlusRecord::from_data(&data, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::DrawRects.raw());
    assert_eq!(record.flags().object_id(), 7);
    assert!(!record.flags().contains(EmfPlusRecordFlags::COMPRESSED));
    assert_eq!(record.parse_data().unwrap(), data);

    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawRects.raw(),
        flags: 7,
        total_object_size: None,
        data: vec![0x00, 0x00, 0x00, 0x00],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawRects.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 64,
        total_object_size: None,
        data: vec![
          0x01, 0x00, 0x00, 0x00, // Count
          0x01, 0x00, // x
          0x02, 0x00, // y
          0x03, 0x00, // width
          0x04, 0x00, // height
        ],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawRects(EmfPlusDrawRectsData {
          pen_id: 7,
          rects: vec![
            EmfPlusRect::Compressed(EmfPlusRectS {
              x: 1,
              y: 2,
              width: 3,
              height: 4,
            }),
            EmfPlusRect::Float(RectF {
              x: 1.0,
              y: 2.0,
              width: 3.0,
              height: 4.0,
            }),
          ],
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
  }

  #[test]
  fn emf_plus_drawing_record_write_flags_are_derived_from_typed_data() {
    let fill_polygon = EmfPlusRecordData::FillPolygon(EmfPlusFillPolygonData {
      brush: EmfPlusBrushRef::Color(EmfPlusArgb {
        blue: 1,
        green: 2,
        red: 3,
        alpha: 255,
      }),
      points: EmfPlusPointData::Compressed(vec![
        PointS { x: 1, y: 2 },
        PointS { x: 3, y: 4 },
        PointS { x: 5, y: 6 },
      ]),
    });
    let fill_record =
      EmfPlusRecord::from_data(&fill_polygon, EmfPlusRecordFlags::POST_MULTIPLY).unwrap();
    let fill_flags = fill_record.flags();
    assert!(fill_flags.contains(EmfPlusRecordFlags::SOLID_COLOR));
    assert!(fill_flags.contains(EmfPlusRecordFlags::COMPRESSED));
    assert!(!fill_flags.contains(EmfPlusRecordFlags::POST_MULTIPLY));
    assert_eq!(fill_record.parse_data().unwrap(), fill_polygon);

    let draw_rects = EmfPlusRecordData::DrawRects(EmfPlusDrawRectsData {
      pen_id: 7,
      rects: vec![EmfPlusRect::Compressed(EmfPlusRectS {
        x: 1,
        y: 2,
        width: 3,
        height: 4,
      })],
    });
    let draw_record = EmfPlusRecord::from_data(
      &draw_rects,
      EmfPlusRecordFlags::SOLID_COLOR | EmfPlusRecordFlags::POST_MULTIPLY,
    )
    .unwrap();
    let draw_flags = draw_record.flags();
    assert_eq!(draw_flags.object_id(), 7);
    assert!(draw_flags.contains(EmfPlusRecordFlags::COMPRESSED));
    assert!(!draw_flags.contains(EmfPlusRecordFlags::SOLID_COLOR));
    assert!(!draw_flags.contains(EmfPlusRecordFlags::POST_MULTIPLY));
    assert_eq!(draw_record.parse_data().unwrap(), draw_rects);
  }

  #[test]
  fn emf_plus_transform_data_roundtrips() {
    let data = EmfPlusRecordData::TranslateWorldTransform(EmfPlusTransformOrderData {
      data: EmfPlusTranslateWorldTransformData {
        dx: 12.5,
        dy: -3.25,
      },
      post_multiply: true,
      reserved_flags: 0,
    });
    let flags = EmfPlusRecordFlags::empty();

    let record = EmfPlusRecord::from_data(&data, flags).unwrap();
    assert_eq!(
      record.record_type,
      EmfPlusRecordType::TranslateWorldTransform.raw()
    );
    assert!(record.flags().contains(EmfPlusRecordFlags::POST_MULTIPLY));
    assert_eq!(record.parse_data().unwrap(), data);
  }

  fn assert_emf_plus_data_roundtrip(data: EmfPlusRecordData<'_>, flags: EmfPlusRecordFlags) {
    let record = EmfPlusRecord::from_data(&data, flags).unwrap();
    assert_eq!(record.parse_data().unwrap(), data);
    assert_eq!(record.data.len() as u64, data.sdk_size());
  }

  #[test]
  fn emf_plus_control_comment_and_clear_records_roundtrip() {
    assert_emf_plus_data_roundtrip(EmfPlusRecordData::Eof, EmfPlusRecordFlags::empty());
    assert_emf_plus_data_roundtrip(EmfPlusRecordData::GetDc, EmfPlusRecordFlags::empty());
    for record_type in [EmfPlusRecordType::Eof, EmfPlusRecordType::GetDc] {
      let ignored_flags = EmfPlusRecord {
        record_type: record_type.raw(),
        flags: 0xFFFF,
        total_object_size: None,
        data: Vec::new(),
        padding: Vec::new(),
      };
      assert!(matches!(
        ignored_flags.parse_data().unwrap(),
        EmfPlusRecordData::Eof | EmfPlusRecordData::GetDc
      ));

      let unexpected_payload = EmfPlusRecord {
        data: vec![0, 0, 0, 0],
        ..ignored_flags
      };
      assert!(unexpected_payload.parse_data().is_err());
    }

    let comment_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::Comment(vec![1, 2, 3, 4]),
      EmfPlusRecordFlags::from_bits_retain(0xFFFF),
    )
    .unwrap();
    assert_eq!(comment_record.record_type, EmfPlusRecordType::Comment.raw());
    assert_eq!(comment_record.flags, 0xFFFF);
    assert_eq!(
      comment_record.parse_data().unwrap(),
      EmfPlusRecordData::Comment(vec![1, 2, 3, 4])
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::Comment(vec![1, 2, 3, 4]),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::Clear(EmfPlusClearData {
        color: EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
      }),
      EmfPlusRecordFlags::empty(),
    );
    let clear_with_ignored_flags = EmfPlusRecord {
      record_type: EmfPlusRecordType::Clear.raw(),
      flags: 0xFFFF,
      total_object_size: None,
      data: vec![1, 2, 3, 4],
      padding: Vec::new(),
    };
    assert_eq!(
      clear_with_ignored_flags.parse_data().unwrap(),
      EmfPlusRecordData::Clear(EmfPlusClearData {
        color: EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
      })
    );
    assert!(
      EmfPlusRecord {
        data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        ..clear_with_ignored_flags
      }
      .parse_data()
      .is_err()
    );
  }

  #[test]
  fn emf_plus_clip_and_property_records_roundtrip() {
    assert_emf_plus_data_roundtrip(EmfPlusRecordData::ResetClip, EmfPlusRecordFlags::empty());
    let reset_clip_with_reserved_flags = EmfPlusRecord {
      record_type: EmfPlusRecordType::ResetClip.raw(),
      flags: 0xFFFF,
      total_object_size: None,
      data: Vec::new(),
      padding: Vec::new(),
    };
    assert_eq!(
      reset_clip_with_reserved_flags.parse_data().unwrap(),
      EmfPlusRecordData::ResetClip
    );
    let reset_clip_with_payload = EmfPlusRecord {
      data: vec![0, 0, 0, 0],
      ..reset_clip_with_reserved_flags
    };
    assert!(reset_clip_with_payload.parse_data().is_err());
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetClipRect(EmfPlusSetClipRectData {
        combine_mode: EmfPlusCombineMode::Union.raw() as u8,
        reserved_flags: 0x0002,
        clip_rect: RectF {
          x: 1.0,
          y: 2.0,
          width: 3.0,
          height: 4.0,
        },
      }),
      EmfPlusRecordFlags::empty(),
    );
    let clip_rect = EmfPlusSetClipRectData {
      combine_mode: EmfPlusCombineMode::Union.raw() as u8,
      reserved_flags: 0x0002,
      clip_rect: RectF {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
      },
    };
    assert_eq!(clip_rect.flags_bits(), 0x0202);
    assert_eq!(
      clip_rect.combine_mode_kind(),
      Some(EmfPlusCombineMode::Union)
    );
    let mut clip_rect_bytes = Vec::new();
    clip_rect
      .write_to(&mut Writer::new(&mut clip_rect_bytes))
      .unwrap();
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetClipRect.raw(),
        flags: 0x0F00,
        total_object_size: None,
        data: clip_rect_bytes,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetClipPath(EmfPlusClipObjectData {
        combine_mode: EmfPlusCombineMode::Union.raw() as u8,
        object_id: 7,
        reserved_flags: 0x8000,
      }),
      EmfPlusRecordFlags::empty(),
    );
    let clip_path_data = EmfPlusClipObjectData {
      combine_mode: EmfPlusCombineMode::Union.raw() as u8,
      object_id: 7,
      reserved_flags: 0x8000,
    };
    let clip_path_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::SetClipPath(clip_path_data),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    assert_eq!(clip_path_record.record_type, 0x4033);
    let clip_path_flags = clip_path_record.flags();
    assert_eq!(clip_path_flags.bits(), 0x8207);
    assert_eq!(clip_path_flags.object_id(), 7);
    assert_eq!(
      clip_path_flags.combine_mode(),
      Some(EmfPlusCombineMode::Union)
    );
    assert_eq!(
      clip_path_record.parse_data().unwrap(),
      EmfPlusRecordData::SetClipPath(clip_path_data)
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetClipRect(EmfPlusSetClipRectData {
          combine_mode: 0xFF,
          reserved_flags: 0,
          clip_rect: RectF {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
          },
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetClipPath.raw(),
        flags: 0x0F01,
        total_object_size: None,
        data: Vec::new(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetClipPath.raw(),
        flags: 0x0040,
        total_object_size: None,
        data: Vec::new(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetClipRegion(EmfPlusClipObjectData {
          combine_mode: EmfPlusCombineMode::Intersect.raw() as u8,
          object_id: 64,
          reserved_flags: 0,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetClipRegion(EmfPlusClipObjectData {
        combine_mode: EmfPlusCombineMode::Intersect.raw() as u8,
        object_id: 4,
        reserved_flags: 0x4000,
      }),
      EmfPlusRecordFlags::empty(),
    );
    let clip_region_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::SetClipRegion(EmfPlusClipObjectData {
        combine_mode: EmfPlusCombineMode::Intersect.raw() as u8,
        object_id: 4,
        reserved_flags: 0x4000,
      }),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    assert_eq!(clip_region_record.record_type, 0x4034);
    let mut clip_region_with_payload = clip_region_record.clone();
    clip_region_with_payload
      .data
      .extend_from_slice(&[0, 0, 0, 0]);
    assert!(clip_region_with_payload.parse_data().is_err());
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::OffsetClip(EmfPlusTranslateWorldTransformData { dx: 5.0, dy: 6.0 }),
      EmfPlusRecordFlags::empty(),
    );
    assert_eq!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::OffsetClip.raw(),
        flags: 0xFFFF,
        total_object_size: None,
        data: 5.0f32
          .to_le_bytes()
          .into_iter()
          .chain(6.0f32.to_le_bytes())
          .collect(),
        padding: Vec::new(),
      }
      .parse_data()
      .unwrap(),
      EmfPlusRecordData::OffsetClip(EmfPlusTranslateWorldTransformData { dx: 5.0, dy: 6.0 })
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetAntiAliasMode(EmfPlusSetAntiAliasModeData {
        smoothing_mode: EmfPlusSmoothingMode::AntiAlias8x4.raw() as u8,
        anti_alias: true,
        reserved_flags: 0x0100,
      }),
      EmfPlusRecordFlags::empty(),
    );
    let anti_alias_data = EmfPlusSetAntiAliasModeData {
      smoothing_mode: EmfPlusSmoothingMode::AntiAlias8x4.raw() as u8,
      anti_alias: true,
      reserved_flags: 0x0100,
    };
    assert_eq!(anti_alias_data.flags_bits(), 0x0109);
    assert_eq!(
      anti_alias_data.smoothing_mode_kind(),
      Some(EmfPlusSmoothingMode::AntiAlias8x4)
    );
    let anti_alias_flags = EmfPlusRecordFlags::from_bits_retain(anti_alias_data.flags_bits());
    assert!(anti_alias_flags.anti_alias_enabled());
    assert_eq!(
      anti_alias_flags.smoothing_mode(),
      Some(EmfPlusSmoothingMode::AntiAlias8x4)
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetCompositingMode(EmfPlusU8PropertyData {
        value: EmfPlusCompositingMode::SourceCopy.raw() as u8,
        reserved_flags: 0x0100,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_eq!(
      EmfPlusRecordFlags::from_bits_retain(EmfPlusCompositingMode::SourceCopy.raw() as u16)
        .compositing_mode(),
      Some(EmfPlusCompositingMode::SourceCopy)
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetCompositingQuality(EmfPlusU8PropertyData {
        value: EmfPlusCompositingQuality::GammaCorrected.raw() as u8,
        reserved_flags: 0x0200,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_eq!(
      EmfPlusRecordFlags::from_bits_retain(EmfPlusCompositingQuality::GammaCorrected.raw() as u16)
        .compositing_quality(),
      Some(EmfPlusCompositingQuality::GammaCorrected)
    );
    let invalid_quality_record = EmfPlusRecord {
      record_type: EmfPlusRecordType::SetCompositingQuality.raw(),
      flags: 0x00FF,
      total_object_size: None,
      data: Vec::new(),
      padding: Vec::new(),
    };
    let invalid_quality = invalid_quality_record.parse_data().unwrap();
    assert_eq!(
      EmfPlusRecord::from_data(&invalid_quality, invalid_quality_record.flags()).unwrap(),
      invalid_quality_record
    );
    assert!(invalid_quality.validate_strict().is_err());
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetInterpolationMode(EmfPlusU8PropertyData {
        value: EmfPlusInterpolationMode::HighQualityBicubic.raw() as u8,
        reserved_flags: 0x0300,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_eq!(
      EmfPlusRecordFlags::from_bits_retain(
        EmfPlusInterpolationMode::HighQualityBicubic.raw() as u16
      )
      .interpolation_mode(),
      Some(EmfPlusInterpolationMode::HighQualityBicubic)
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetPixelOffsetMode(EmfPlusU8PropertyData {
        value: EmfPlusPixelOffsetMode::Half.raw() as u8,
        reserved_flags: 0x0400,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_eq!(
      EmfPlusRecordFlags::from_bits_retain(EmfPlusPixelOffsetMode::Half.raw() as u16)
        .pixel_offset_mode(),
      Some(EmfPlusPixelOffsetMode::Half)
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetTextContrast(EmfPlusSetTextContrastData {
        text_contrast: 1500,
        reserved_flags: 0x8000,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_eq!(
      EmfPlusRecordFlags::from_bits_retain(0x05DC).text_contrast(),
      1500
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetTextRenderingHint(EmfPlusU8PropertyData {
        value: EmfPlusTextRenderingHint::AntiAlias.raw() as u8,
        reserved_flags: 0x0500,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_eq!(
      EmfPlusRecordFlags::from_bits_retain(EmfPlusTextRenderingHint::AntiAlias.raw() as u16)
        .text_rendering_hint(),
      Some(EmfPlusTextRenderingHint::AntiAlias)
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetRenderingOrigin(PointL { x: 10, y: -20 }),
      EmfPlusRecordFlags::empty(),
    );
  }

  #[test]
  fn emf_plus_state_and_transform_records_roundtrip() {
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::Save(EmfPlusStackIndexData { stack_index: 7 }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::Restore(EmfPlusStackIndexData { stack_index: 7 }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::BeginContainerNoParams(EmfPlusStackIndexData { stack_index: 9 }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::BeginContainer(EmfPlusBeginContainerData {
        dest_rect: RectF {
          x: 0.0,
          y: 1.0,
          width: 2.0,
          height: 3.0,
        },
        src_rect: RectF {
          x: 4.0,
          y: 5.0,
          width: 6.0,
          height: 7.0,
        },
        stack_index: 11,
      }),
      EmfPlusRecordFlags::from_bits_retain(0x0002),
    );
    let begin_container = EmfPlusRecordData::BeginContainer(EmfPlusBeginContainerData {
      dest_rect: RectF {
        x: 0.0,
        y: 1.0,
        width: 2.0,
        height: 3.0,
      },
      src_rect: RectF {
        x: 4.0,
        y: 5.0,
        width: 6.0,
        height: 7.0,
      },
      stack_index: 11,
    });
    assert!(
      EmfPlusRecord::from_data(
        &begin_container,
        EmfPlusRecordFlags::from_bits_retain(0x0102),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::BeginContainer.raw(),
        flags: 0x0102,
        total_object_size: None,
        data: {
          let mut bytes = Vec::new();
          if let EmfPlusRecordData::BeginContainer(value) = begin_container {
            value.write_to(&mut Writer::new(&mut bytes)).unwrap();
          }
          bytes
        },
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::EndContainer(EmfPlusStackIndexData { stack_index: 11 }),
      EmfPlusRecordFlags::empty(),
    );
    let transform = XForm {
      m11: 1.0,
      m12: 0.0,
      m21: 0.0,
      m22: 1.0,
      dx: 2.0,
      dy: 3.0,
    };
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetWorldTransform(transform),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::MultiplyWorldTransform(EmfPlusTransformOrderData {
        data: transform,
        post_multiply: true,
        reserved_flags: 0,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::ResetWorldTransform,
      EmfPlusRecordFlags::empty(),
    );
    for data in [
      EmfPlusRecordData::SetWorldTransform(transform),
      EmfPlusRecordData::ResetWorldTransform,
      EmfPlusRecordData::MultiplyWorldTransform(EmfPlusTransformOrderData {
        data: transform,
        post_multiply: false,
        reserved_flags: 0,
      }),
      EmfPlusRecordData::TranslateWorldTransform(EmfPlusTransformOrderData {
        data: EmfPlusTranslateWorldTransformData { dx: 1.0, dy: 2.0 },
        post_multiply: false,
        reserved_flags: 0,
      }),
      EmfPlusRecordData::ScaleWorldTransform(EmfPlusTransformOrderData {
        data: EmfPlusScaleWorldTransformData { sx: 1.0, sy: 2.0 },
        post_multiply: false,
        reserved_flags: 0,
      }),
    ] {
      let mut record = EmfPlusRecord::from_data(&data, EmfPlusRecordFlags::empty()).unwrap();
      record.data.extend_from_slice(&[0, 0, 0, 0]);
      assert!(record.parse_data().is_err());
    }
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::RotateWorldTransform(EmfPlusTransformOrderData {
        data: EmfPlusRotateWorldTransformData { angle: 45.0 },
        post_multiply: true,
        reserved_flags: 0,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::SetPageTransform(EmfPlusSetPageTransformData { page_scale: 1.5 }),
      EmfPlusRecordFlags::from_bits_retain(0x0002),
    );
  }

  #[test]
  fn emf_plus_property_record_enum_and_range_validation() {
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetAntiAliasMode(EmfPlusSetAntiAliasModeData {
          smoothing_mode: 0x7F,
          anti_alias: false,
          reserved_flags: 0,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetAntiAliasMode.raw(),
        flags: 0x00FE,
        total_object_size: None,
        data: Vec::new(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );

    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetInterpolationMode(EmfPlusU8PropertyData {
          value: 0xFF,
          reserved_flags: 0,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetInterpolationMode.raw(),
        flags: 0x00FF,
        total_object_size: None,
        data: Vec::new(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );

    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTextContrast(EmfPlusSetTextContrastData {
          text_contrast: 999,
          reserved_flags: 0,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetTextContrast.raw(),
        flags: 999,
        total_object_size: None,
        data: Vec::new(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );

    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetPageTransform(EmfPlusSetPageTransformData { page_scale: 1.0 }),
        EmfPlusRecordFlags::from_bits_retain(0x00FF),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetPageTransform.raw(),
        flags: 0x00FF,
        total_object_size: None,
        data: 1.0f32.to_le_bytes().to_vec(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetPageTransform(EmfPlusSetPageTransformData { page_scale: 1.0 }),
        EmfPlusRecordFlags::from_bits_retain(0x0102),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetPageTransform.raw(),
        flags: 0x0102,
        total_object_size: None,
        data: 1.0f32.to_le_bytes().to_vec(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
  }

  #[test]
  fn emf_plus_fixed_drawing_records_roundtrip() {
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::FillEllipse(EmfPlusFillRectShapeData {
        brush: EmfPlusBrushRef::Color(EmfPlusArgb {
          blue: 10,
          green: 20,
          red: 30,
          alpha: 255,
        }),
        rect: EmfPlusRect::Compressed(EmfPlusRectS {
          x: 1,
          y: 2,
          width: 3,
          height: 4,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawEllipse(EmfPlusDrawRectShapeData {
        pen_id: 6,
        rect: EmfPlusRect::Float(RectF {
          x: 1.0,
          y: 2.0,
          width: 3.0,
          height: 4.0,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::FillPie(EmfPlusFillPieData {
        brush: EmfPlusBrushRef::ObjectId(3),
        start_angle: 45.0,
        sweep_angle: 720.0,
        rect: EmfPlusRect::Compressed(EmfPlusRectS {
          x: 5,
          y: 6,
          width: 7,
          height: 8,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawPie(EmfPlusDrawArcData {
        pen_id: 4,
        start_angle: 10.0,
        sweep_angle: -720.0,
        rect: EmfPlusRect::Float(RectF {
          x: 9.0,
          y: 10.0,
          width: 11.0,
          height: 12.0,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawArc(EmfPlusDrawArcData {
        pen_id: 4,
        start_angle: 10.0,
        sweep_angle: -20.0,
        rect: EmfPlusRect::Compressed(EmfPlusRectS {
          x: 9,
          y: 10,
          width: 11,
          height: 12,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawArc(EmfPlusDrawArcData {
          pen_id: 4,
          start_angle: -1.0,
          sweep_angle: 90.0,
          rect: EmfPlusRect::Compressed(EmfPlusRectS {
            x: 9,
            y: 10,
            width: 11,
            height: 12,
          }),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::FillPie(EmfPlusFillPieData {
          brush: EmfPlusBrushRef::Color(EmfPlusArgb {
            blue: 1,
            green: 2,
            red: 3,
            alpha: 4,
          }),
          start_angle: -1.0,
          sweep_angle: 90.0,
          rect: EmfPlusRect::Compressed(EmfPlusRectS {
            x: 5,
            y: 6,
            width: 7,
            height: 8,
          }),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    let mut invalid_arc_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::DrawArc(EmfPlusDrawArcData {
        pen_id: 4,
        start_angle: 10.0,
        sweep_angle: 90.0,
        rect: EmfPlusRect::Compressed(EmfPlusRectS {
          x: 9,
          y: 10,
          width: 11,
          height: 12,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    invalid_arc_record.data[0..4].copy_from_slice(&(-1.0f32).to_le_bytes());
    assert!(invalid_arc_record.parse_data().is_err());
    let mut invalid_fill_pie_record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::FillPie(EmfPlusFillPieData {
        brush: EmfPlusBrushRef::ObjectId(3),
        start_angle: 10.0,
        sweep_angle: 90.0,
        rect: EmfPlusRect::Compressed(EmfPlusRectS {
          x: 5,
          y: 6,
          width: 7,
          height: 8,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    invalid_fill_pie_record.data[4..8].copy_from_slice(&(-1.0f32).to_le_bytes());
    assert!(invalid_fill_pie_record.parse_data().is_err());
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawEllipse.raw(),
        flags: 4,
        total_object_size: None,
        data: vec![0; 8],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let mut draw_ellipse_with_trailing_data = EmfPlusRecord::from_data(
      &EmfPlusRecordData::DrawEllipse(EmfPlusDrawRectShapeData {
        pen_id: 4,
        rect: EmfPlusRect::Compressed(EmfPlusRectS {
          x: 1,
          y: 2,
          width: 3,
          height: 4,
        }),
      }),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    draw_ellipse_with_trailing_data
      .data
      .extend_from_slice(&[0, 0, 0, 0]);
    assert!(draw_ellipse_with_trailing_data.parse_data().is_err());
    let mut truncated_draw_image = Vec::new();
    truncated_draw_image.extend_from_slice(&0u32.to_le_bytes());
    truncated_draw_image.extend_from_slice(&(EmfPlusUnitType::Pixel.raw() as i32).to_le_bytes());
    for value in [1.0f32, 2.0, 3.0, 4.0] {
      truncated_draw_image.extend_from_slice(&value.to_le_bytes());
    }
    truncated_draw_image.extend_from_slice(&1i16.to_le_bytes());
    truncated_draw_image.extend_from_slice(&2i16.to_le_bytes());
    assert_eq!(truncated_draw_image.len(), 28);
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawImage.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 4,
        total_object_size: None,
        data: truncated_draw_image,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::FillPath(EmfPlusBrushObjectData {
        object_id: 3,
        brush: EmfPlusBrushRef::ObjectId(2),
      }),
      EmfPlusRecordFlags::from_bits_retain(0x0300),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::FillRegion(EmfPlusBrushObjectData {
        object_id: 5,
        brush: EmfPlusBrushRef::Color(EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        }),
      }),
      EmfPlusRecordFlags::from_bits_retain(0x0500),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawPath(EmfPlusDrawObjectData {
        object_id: 5,
        pen_id: 6,
      }),
      EmfPlusRecordFlags::from_bits_retain(0x0500),
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawRects(EmfPlusDrawRectsData {
          pen_id: 64,
          rects: vec![EmfPlusRect::Compressed(EmfPlusRectS {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
          })],
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::FillPath(EmfPlusBrushObjectData {
          object_id: 64,
          brush: EmfPlusBrushRef::ObjectId(2),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::FillRegion(EmfPlusBrushObjectData {
          object_id: 64,
          brush: EmfPlusBrushRef::ObjectId(2),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawPath(EmfPlusDrawObjectData {
          object_id: 3,
          pen_id: 64,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::FillRegion.raw(),
        flags: 0x0040,
        total_object_size: None,
        data: 2_u32.to_le_bytes().to_vec(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawPath.raw(),
        flags: 0x0003,
        total_object_size: None,
        data: 64_u32.to_le_bytes().to_vec(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::FillPolygon(EmfPlusFillPolygonData {
          brush: EmfPlusBrushRef::ObjectId(64),
          points: EmfPlusPointData::Compressed(vec![
            PointS { x: 1, y: 2 },
            PointS { x: 3, y: 4 },
            PointS { x: 5, y: 6 },
          ]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::FillPath.raw(),
        flags: 0x0040,
        total_object_size: None,
        data: 2_u32.to_le_bytes().to_vec(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let stroke_fill = EmfPlusRecordData::StrokeFillPath;
    let record = EmfPlusRecord::from_data(&stroke_fill, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::StrokeFillPath.raw());
    assert_eq!(record.parse_data().unwrap(), stroke_fill);
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::StrokeFillPath.raw(),
        flags: 0xFFFF,
        total_object_size: None,
        data: Vec::new(),
        padding: vec![0, 0, 0, 0],
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::StrokeFillPath.raw(),
        flags: 0xFFFF,
        total_object_size: None,
        data: vec![0, 0, 0, 0],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
  }

  #[test]
  fn emf_plus_point_array_drawing_records_roundtrip() {
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::FillPolygon(EmfPlusFillPolygonData {
        brush: EmfPlusBrushRef::Color(EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        }),
        points: EmfPlusPointData::Compressed(vec![
          PointS { x: 1, y: 2 },
          PointS { x: 3, y: 4 },
          PointS { x: 5, y: 6 },
        ]),
      }),
      EmfPlusRecordFlags::empty(),
    );
    let draw_lines = EmfPlusRecordData::DrawLines(EmfPlusDrawLinesData {
      pen_id: 8,
      close_shape: true,
      points: EmfPlusPointData::Float(vec![PointF { x: 1.0, y: 2.0 }, PointF { x: 3.0, y: 4.0 }]),
    });
    let record = EmfPlusRecord::from_data(&draw_lines, EmfPlusRecordFlags::empty()).unwrap();
    assert!(record.flags().contains(EmfPlusRecordFlags::CLOSE_SHAPE));
    assert_eq!(record.parse_data().unwrap(), draw_lines);
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawBeziers(EmfPlusDrawPointsData {
        pen_id: 9,
        points: EmfPlusPointData::Relative(vec![
          EmfPlusPointR { x: -1, y: 2 },
          EmfPlusPointR { x: 130, y: -130 },
          EmfPlusPointR { x: 3, y: 4 },
          EmfPlusPointR { x: 5, y: 6 },
        ]),
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawCurve(EmfPlusDrawCurveData {
        pen_id: 10,
        tension: 0.5,
        offset: 0,
        num_segments: 1,
        points: EmfPlusPointData::Compressed(vec![
          PointS { x: 10, y: 20 },
          PointS { x: 30, y: 40 },
        ]),
      }),
      EmfPlusRecordFlags::empty(),
    );
    let mut draw_curve_with_reserved_p = Vec::new();
    draw_curve_with_reserved_p.extend_from_slice(&0.5f32.to_le_bytes());
    draw_curve_with_reserved_p.extend_from_slice(&0u32.to_le_bytes());
    draw_curve_with_reserved_p.extend_from_slice(&1u32.to_le_bytes());
    draw_curve_with_reserved_p.extend_from_slice(&2u32.to_le_bytes());
    draw_curve_with_reserved_p.extend_from_slice(&10i16.to_le_bytes());
    draw_curve_with_reserved_p.extend_from_slice(&20i16.to_le_bytes());
    draw_curve_with_reserved_p.extend_from_slice(&30i16.to_le_bytes());
    draw_curve_with_reserved_p.extend_from_slice(&40i16.to_le_bytes());
    let record = EmfPlusRecord {
      record_type: EmfPlusRecordType::DrawCurve.raw(),
      flags: (EmfPlusRecordFlags::RELATIVE_POSITION | EmfPlusRecordFlags::COMPRESSED).bits() | 10,
      total_object_size: None,
      data: draw_curve_with_reserved_p,
      padding: Vec::new(),
    };
    assert_eq!(
      record.parse_data().unwrap(),
      EmfPlusRecordData::DrawCurve(EmfPlusDrawCurveData {
        pen_id: 10,
        tension: 0.5,
        offset: 0,
        num_segments: 1,
        points: EmfPlusPointData::Compressed(vec![
          PointS { x: 10, y: 20 },
          PointS { x: 30, y: 40 },
        ]),
      })
    );
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawClosedCurve(EmfPlusClosedCurveData {
        pen_id: 11,
        tension: 0.75,
        points: EmfPlusPointData::Float(vec![
          PointF { x: 1.0, y: 2.0 },
          PointF { x: 3.0, y: 4.0 },
          PointF { x: 5.0, y: 6.0 },
        ]),
      }),
      EmfPlusRecordFlags::empty(),
    );
    let fill_closed_curve = EmfPlusRecordData::FillClosedCurve(EmfPlusFillClosedCurveData {
      brush: EmfPlusBrushRef::ObjectId(12),
      winding_fill: true,
      tension: 1.0,
      points: EmfPlusPointData::Relative(vec![
        EmfPlusPointR { x: 1, y: 2 },
        EmfPlusPointR { x: 3, y: 4 },
        EmfPlusPointR { x: 5, y: 6 },
      ]),
    });
    let record = EmfPlusRecord::from_data(&fill_closed_curve, EmfPlusRecordFlags::empty()).unwrap();
    assert!(record.flags().contains(EmfPlusRecordFlags::WINDING_FILL));
    assert_eq!(record.parse_data().unwrap(), fill_closed_curve);

    let relative_draw_lines = EmfPlusRecordData::DrawLines(EmfPlusDrawLinesData {
      pen_id: 8,
      close_shape: false,
      points: EmfPlusPointData::Relative(vec![
        EmfPlusPointR { x: 1, y: 2 },
        EmfPlusPointR { x: 3, y: 4 },
        EmfPlusPointR { x: 5, y: 6 },
      ]),
    });
    let record =
      EmfPlusRecord::from_data(&relative_draw_lines, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.data.len(), 12);
    assert_eq!(&record.data[10..12], &[0, 0]);
    record
      .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
      .unwrap();
    assert_eq!(record.parse_data().unwrap(), relative_draw_lines);
    let mut nonzero_relative_padding = record.clone();
    nonzero_relative_padding.data[11] = 0xAA;
    assert!(nonzero_relative_padding.parse_data().is_err());

    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::FillPolygon.raw(),
        flags: EmfPlusRecordFlags::SOLID_COLOR.bits(),
        total_object_size: None,
        data: vec![
          0x04, 0x03, 0x02, 0x01, // ARGB
          0x02, 0x00, 0x00, 0x00, // Count
        ],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawLines.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 3,
        total_object_size: None,
        data: vec![0x01, 0x00, 0x00, 0x00],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::FillClosedCurve.raw(),
        flags: 0,
        total_object_size: None,
        data: [
          2_u32.to_le_bytes().as_slice(),   // BrushId
          0.5_f32.to_le_bytes().as_slice(), // Tension
          2_u32.to_le_bytes().as_slice(),   // Count
        ]
        .concat(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawClosedCurve.raw(),
        flags: 5,
        total_object_size: None,
        data: [
          0.5_f32.to_le_bytes().as_slice(), // Tension
          2_u32.to_le_bytes().as_slice(),   // Count
        ]
        .concat(),
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawRects(EmfPlusDrawRectsData {
          pen_id: 1,
          rects: Vec::new(),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawLines(EmfPlusDrawLinesData {
          pen_id: 64,
          close_shape: false,
          points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 },]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::FillPolygon(EmfPlusFillPolygonData {
          brush: EmfPlusBrushRef::ObjectId(2),
          points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 },]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawLines(EmfPlusDrawLinesData {
          pen_id: 3,
          close_shape: false,
          points: EmfPlusPointData::Float(vec![PointF { x: 1.0, y: 2.0 }]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawBeziers(EmfPlusDrawPointsData {
          pen_id: 4,
          points: EmfPlusPointData::Relative(vec![
            EmfPlusPointR { x: 1, y: 2 },
            EmfPlusPointR { x: 3, y: 4 },
            EmfPlusPointR { x: 5, y: 6 },
          ]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    let invalid_draw_beziers_count = EmfPlusRecordData::DrawBeziers(EmfPlusDrawPointsData {
      pen_id: 4,
      points: EmfPlusPointData::Compressed(vec![
        PointS { x: 1, y: 2 },
        PointS { x: 3, y: 4 },
        PointS { x: 5, y: 6 },
        PointS { x: 7, y: 8 },
        PointS { x: 9, y: 10 },
      ]),
    });
    assert!(
      EmfPlusRecord::from_data(&invalid_draw_beziers_count, EmfPlusRecordFlags::empty()).is_err()
    );

    let mut invalid_draw_beziers_data = Vec::new();
    invalid_draw_beziers_data.extend_from_slice(&5_u32.to_le_bytes());
    for point in [
      PointS { x: 1, y: 2 },
      PointS { x: 3, y: 4 },
      PointS { x: 5, y: 6 },
      PointS { x: 7, y: 8 },
      PointS { x: 9, y: 10 },
    ] {
      invalid_draw_beziers_data.extend_from_slice(&point.x.to_le_bytes());
      invalid_draw_beziers_data.extend_from_slice(&point.y.to_le_bytes());
    }
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawBeziers.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 4,
        total_object_size: None,
        data: invalid_draw_beziers_data,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawBeziers.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 64,
        total_object_size: None,
        data: {
          let mut data = Vec::new();
          data.extend_from_slice(&4_u32.to_le_bytes());
          for point in [
            PointS { x: 1, y: 2 },
            PointS { x: 3, y: 4 },
            PointS { x: 5, y: 6 },
            PointS { x: 7, y: 8 },
          ] {
            data.extend_from_slice(&point.x.to_le_bytes());
            data.extend_from_slice(&point.y.to_le_bytes());
          }
          data
        },
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawClosedCurve.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 64,
        total_object_size: None,
        data: {
          let mut data = Vec::new();
          data.extend_from_slice(&0.5_f32.to_le_bytes());
          data.extend_from_slice(&3_u32.to_le_bytes());
          for point in [
            PointS { x: 1, y: 2 },
            PointS { x: 3, y: 4 },
            PointS { x: 5, y: 6 },
          ] {
            data.extend_from_slice(&point.x.to_le_bytes());
            data.extend_from_slice(&point.y.to_le_bytes());
          }
          data
        },
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawCurve.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 64,
        total_object_size: None,
        data: {
          let mut data = Vec::new();
          data.extend_from_slice(&0.5_f32.to_le_bytes());
          data.extend_from_slice(&0_u32.to_le_bytes());
          data.extend_from_slice(&1_u32.to_le_bytes());
          data.extend_from_slice(&2_u32.to_le_bytes());
          for point in [PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 }] {
            data.extend_from_slice(&point.x.to_le_bytes());
            data.extend_from_slice(&point.y.to_le_bytes());
          }
          data
        },
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let mut oversized_relative_points_data = Vec::new();
    oversized_relative_points_data.extend_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawLines.raw(),
        flags: EmfPlusRecordFlags::RELATIVE_POSITION.bits() | 4,
        total_object_size: None,
        data: oversized_relative_points_data,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawCurve(EmfPlusDrawCurveData {
          pen_id: 5,
          tension: 0.5,
          offset: 2,
          num_segments: 0,
          points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 },]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawCurve.raw(),
        flags: EmfPlusRecordFlags::COMPRESSED.bits() | 5,
        total_object_size: None,
        data: {
          let mut data = Vec::new();
          data.extend_from_slice(&0.5_f32.to_le_bytes());
          data.extend_from_slice(&2_u32.to_le_bytes());
          data.extend_from_slice(&0_u32.to_le_bytes());
          data.extend_from_slice(&2_u32.to_le_bytes());
          for point in [PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 }] {
            data.extend_from_slice(&point.x.to_le_bytes());
            data.extend_from_slice(&point.y.to_le_bytes());
          }
          data
        },
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawCurve(EmfPlusDrawCurveData {
          pen_id: 5,
          tension: 0.5,
          offset: 0,
          num_segments: 2,
          points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 },]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawCurve(EmfPlusDrawCurveData {
          pen_id: 5,
          tension: 0.5,
          offset: 0,
          num_segments: 1,
          points: EmfPlusPointData::Relative(vec![
            EmfPlusPointR { x: 1, y: 2 },
            EmfPlusPointR { x: 3, y: 4 },
          ]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
  }

  #[test]
  fn emf_plus_image_string_and_driver_string_records_roundtrip() {
    let draw_image = EmfPlusRecordData::DrawImage(EmfPlusDrawImageData {
      image_id: 9,
      image_attributes_id: 2,
      src_unit: 2,
      src_rect: RectF {
        x: 0.0,
        y: 1.0,
        width: 10.0,
        height: 20.0,
      },
      dest_rect: EmfPlusRect::Compressed(EmfPlusRectS {
        x: 1,
        y: 2,
        width: 30,
        height: 40,
      }),
    });
    let record = EmfPlusRecord::from_data(&draw_image, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::DrawImage.raw());
    assert!(record.flags().contains(EmfPlusRecordFlags::COMPRESSED));
    assert_eq!(record.flags().object_id(), 9);
    let parsed = record.parse_data().unwrap();
    let EmfPlusRecordData::DrawImage(value) = &parsed else {
      panic!("expected EmfPlusDrawImage");
    };
    assert_eq!(value.src_unit_kind(), Some(EmfPlusUnitType::Pixel));
    assert_eq!(parsed, draw_image);
    let compatible_src_unit = EmfPlusRecordData::DrawImage(EmfPlusDrawImageData {
      image_id: 9,
      image_attributes_id: 2,
      src_unit: EmfPlusUnitType::Point.raw() as i32,
      src_rect: RectF {
        x: 0.0,
        y: 1.0,
        width: 10.0,
        height: 20.0,
      },
      dest_rect: EmfPlusRect::Compressed(EmfPlusRectS {
        x: 1,
        y: 2,
        width: 30,
        height: 40,
      }),
    });
    assert!(compatible_src_unit.validate_strict().is_err());
    assert!(EmfPlusRecord::from_data(&compatible_src_unit, EmfPlusRecordFlags::empty()).is_ok());
    let mut invalid_record = record.clone();
    invalid_record.data[4..8].copy_from_slice(&(EmfPlusUnitType::Point.raw() as i32).to_le_bytes());
    let parsed = invalid_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    let flags = EmfPlusRecordFlags::from_bits_retain(invalid_record.flags);
    assert_eq!(
      EmfPlusRecord::from_data(&parsed, flags).unwrap(),
      invalid_record
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawImage(EmfPlusDrawImageData {
          image_id: 9,
          image_attributes_id: 64,
          src_unit: EmfPlusUnitType::Pixel.raw() as i32,
          src_rect: RectF {
            x: 0.0,
            y: 1.0,
            width: 10.0,
            height: 20.0,
          },
          dest_rect: EmfPlusRect::Compressed(EmfPlusRectS {
            x: 1,
            y: 2,
            width: 30,
            height: 40,
          }),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[0..4].copy_from_slice(&64_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());

    let draw_image_points = EmfPlusRecordData::DrawImagePoints(EmfPlusDrawImagePointsData {
      image_id: 7,
      apply_effect: true,
      image_attributes_id: 3,
      src_unit: 2,
      src_rect: RectF {
        x: 0.0,
        y: 0.0,
        width: 5.0,
        height: 6.0,
      },
      points: EmfPlusPointData::Relative(vec![
        EmfPlusPointR { x: 1, y: 2 },
        EmfPlusPointR { x: 3, y: 4 },
        EmfPlusPointR { x: 5, y: 6 },
      ]),
    });
    let record = EmfPlusRecord::from_data(&draw_image_points, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::DrawImagePoints.raw());
    assert!(
      record
        .flags()
        .contains(EmfPlusRecordFlags::RELATIVE_POSITION)
    );
    assert!(record.flags().contains(EmfPlusRecordFlags::EFFECT));
    assert_eq!(record.flags().object_id(), 7);
    let parsed = record.parse_data().unwrap();
    let EmfPlusRecordData::DrawImagePoints(value) = &parsed else {
      panic!("expected EmfPlusDrawImagePoints");
    };
    assert_eq!(value.src_unit_kind(), Some(EmfPlusUnitType::Pixel));
    assert_eq!(parsed, draw_image_points);
    let compatible_src_unit = EmfPlusRecordData::DrawImagePoints(EmfPlusDrawImagePointsData {
      image_id: 7,
      apply_effect: false,
      image_attributes_id: 3,
      src_unit: EmfPlusUnitType::Millimeter.raw() as i32,
      src_rect: RectF {
        x: 0.0,
        y: 0.0,
        width: 5.0,
        height: 6.0,
      },
      points: EmfPlusPointData::Relative(vec![
        EmfPlusPointR { x: 1, y: 2 },
        EmfPlusPointR { x: 3, y: 4 },
        EmfPlusPointR { x: 5, y: 6 },
      ]),
    });
    assert!(compatible_src_unit.validate_strict().is_err());
    assert!(EmfPlusRecord::from_data(&compatible_src_unit, EmfPlusRecordFlags::empty()).is_ok());
    let mut invalid_record = record.clone();
    invalid_record.data[4..8]
      .copy_from_slice(&(EmfPlusUnitType::Millimeter.raw() as i32).to_le_bytes());
    let parsed = invalid_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    let flags = EmfPlusRecordFlags::from_bits_retain(invalid_record.flags);
    assert_eq!(
      EmfPlusRecord::from_data(&parsed, flags).unwrap(),
      invalid_record
    );
    let mut invalid_record = record.clone();
    invalid_record.data[0..4].copy_from_slice(&64_u32.to_le_bytes());
    let parsed = invalid_record.parse_data().unwrap();
    assert_eq!(
      EmfPlusRecord::from_data(&parsed, invalid_record.flags()).unwrap(),
      invalid_record
    );
    assert!(parsed.validate_strict().is_err());
    let mut invalid_record = record.clone();
    invalid_record.flags = EmfPlusRecordFlags::RELATIVE_POSITION.bits() | 64;
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_count_record = record.clone();
    invalid_count_record.data[24..28].copy_from_slice(&2_u32.to_le_bytes());
    invalid_count_record.data.truncate(32);
    assert!(invalid_count_record.parse_data().is_err());

    let invalid_draw_image_points =
      EmfPlusRecordData::DrawImagePoints(EmfPlusDrawImagePointsData {
        image_id: 7,
        apply_effect: false,
        image_attributes_id: 3,
        src_unit: EmfPlusUnitType::Pixel.raw() as i32,
        src_rect: RectF {
          x: 0.0,
          y: 0.0,
          width: 5.0,
          height: 6.0,
        },
        points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 }]),
      });
    assert!(
      EmfPlusRecord::from_data(&invalid_draw_image_points, EmfPlusRecordFlags::empty()).is_err()
    );

    let draw_string = EmfPlusRecordData::DrawString(EmfPlusDrawStringData {
      font_id: 4,
      brush: EmfPlusBrushRef::Color(EmfPlusArgb {
        alpha: 0xFF,
        red: 10,
        green: 20,
        blue: 30,
      }),
      format_id: 8,
      layout_rect: RectF {
        x: 1.0,
        y: 2.0,
        width: 100.0,
        height: 20.0,
      },
      string: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
      padding: Vec::new(),
    });
    let record = EmfPlusRecord::from_data(&draw_string, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::DrawString.raw());
    assert!(record.flags().contains(EmfPlusRecordFlags::SOLID_COLOR));
    assert_eq!(record.flags().object_id(), 4);
    assert_eq!(record.parse_data().unwrap(), draw_string);
    let odd_length_draw_string = EmfPlusDrawStringData {
      font_id: 4,
      brush: EmfPlusBrushRef::Color(EmfPlusArgb {
        alpha: 0xFF,
        red: 10,
        green: 20,
        blue: 30,
      }),
      format_id: 8,
      layout_rect: RectF {
        x: 1.0,
        y: 2.0,
        width: 100.0,
        height: 20.0,
      },
      string: SdkString::raw(vec![b'H', 0], SdkEncoding::Utf16Le),
      padding: vec![0, 0],
    };
    let record = EmfPlusRecord::from_data(
      &EmfPlusRecordData::DrawString(odd_length_draw_string.clone()),
      EmfPlusRecordFlags::empty(),
    )
    .unwrap();
    assert_eq!(
      record.parse_data().unwrap(),
      EmfPlusRecordData::DrawString(odd_length_draw_string.clone())
    );
    let mut invalid_draw_string_padding = odd_length_draw_string.clone();
    invalid_draw_string_padding.padding = vec![0];
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawString(invalid_draw_string_padding),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    let compatible_format_id = EmfPlusRecordData::DrawString(EmfPlusDrawStringData {
      font_id: 4,
      brush: EmfPlusBrushRef::Color(EmfPlusArgb {
        alpha: 0xFF,
        red: 10,
        green: 20,
        blue: 30,
      }),
      format_id: 64,
      layout_rect: RectF {
        x: 1.0,
        y: 2.0,
        width: 100.0,
        height: 20.0,
      },
      string: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
      padding: Vec::new(),
    });
    assert!(EmfPlusRecord::from_data(&compatible_format_id, EmfPlusRecordFlags::empty()).is_ok());
    assert!(compatible_format_id.validate_strict().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[4..8].copy_from_slice(&64_u32.to_le_bytes());
    let parsed = invalid_record.parse_data().unwrap();
    assert_eq!(
      EmfPlusRecord::from_data(&parsed, invalid_record.flags()).unwrap(),
      invalid_record
    );
    assert!(parsed.validate_strict().is_err());
    let mut invalid_record = record.clone();
    invalid_record.flags = 64;
    assert!(invalid_record.parse_data().is_err());
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawString(EmfPlusDrawStringData {
          font_id: 4,
          brush: EmfPlusBrushRef::Color(EmfPlusArgb {
            alpha: 0xFF,
            red: 10,
            green: 20,
            blue: 30,
          }),
          format_id: 8,
          layout_rect: RectF {
            x: 1.0,
            y: 2.0,
            width: 100.0,
            height: 20.0,
          },
          string: SdkString::raw(vec![b'H', 0], SdkEncoding::Utf16Le),
          padding: vec![0, 0, 0, 0],
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data.extend_from_slice(&[0, 0, 0, 0]);
    assert!(invalid_record.parse_data().is_err());
    let mut oversized_draw_string = record.clone();
    oversized_draw_string.data[8..12].copy_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(oversized_draw_string.parse_data().is_err());

    let driver_string = EmfPlusRecordData::DrawDriverString(EmfPlusDrawDriverStringData {
      font_id: 6,
      brush: EmfPlusBrushRef::ObjectId(3),
      driver_string_options_flags: (EmfPlusDriverStringOptionsFlags::CMAP_LOOKUP
        | EmfPlusDriverStringOptionsFlags::REALIZED_ADVANCE)
        .bits(),
      glyphs: vec![65, 66],
      glyph_positions: vec![PointF { x: 1.0, y: 2.0 }],
      transform_matrix: Some(XForm {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 5.0,
        dy: 6.0,
      }),
    });
    let record = EmfPlusRecord::from_data(&driver_string, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(
      record.record_type,
      EmfPlusRecordType::DrawDriverString.raw()
    );
    assert_eq!(record.flags().object_id(), 6);
    let parsed = record.parse_data().unwrap();
    let EmfPlusRecordData::DrawDriverString(value) = &parsed else {
      panic!("expected EmfPlusDrawDriverString");
    };
    assert!(
      value
        .driver_string_options()
        .contains(EmfPlusDriverStringOptionsFlags::CMAP_LOOKUP)
    );
    assert!(
      value
        .driver_string_options()
        .contains(EmfPlusDriverStringOptionsFlags::REALIZED_ADVANCE)
    );
    assert_eq!(parsed, driver_string);
    let reserved_flags_record =
      EmfPlusRecord::from_data(&driver_string, EmfPlusRecordFlags::POST_MULTIPLY).unwrap();
    assert_eq!(reserved_flags_record.flags, record.flags);
    let mut reserved_flags_record = record.clone();
    reserved_flags_record.flags |= EmfPlusRecordFlags::POST_MULTIPLY.bits();
    assert_eq!(reserved_flags_record.parse_data().unwrap(), driver_string);
    assert_emf_plus_data_roundtrip(
      EmfPlusRecordData::DrawDriverString(EmfPlusDrawDriverStringData {
        font_id: 6,
        brush: EmfPlusBrushRef::ObjectId(3),
        driver_string_options_flags: EmfPlusDriverStringOptionsFlags::CMAP_LOOKUP.bits(),
        glyphs: vec![65, 66],
        glyph_positions: vec![PointF { x: 1.0, y: 2.0 }, PointF { x: 3.0, y: 4.0 }],
        transform_matrix: None,
      }),
      EmfPlusRecordFlags::empty(),
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawDriverString(EmfPlusDrawDriverStringData {
          font_id: 6,
          brush: EmfPlusBrushRef::ObjectId(3),
          driver_string_options_flags: 0,
          glyphs: vec![65, 66],
          glyph_positions: vec![PointF { x: 1.0, y: 2.0 }],
          transform_matrix: None,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawDriverString(EmfPlusDrawDriverStringData {
          font_id: 6,
          brush: EmfPlusBrushRef::ObjectId(3),
          driver_string_options_flags: EmfPlusDriverStringOptionsFlags::REALIZED_ADVANCE.bits(),
          glyphs: vec![65, 66],
          glyph_positions: vec![PointF { x: 1.0, y: 2.0 }, PointF { x: 3.0, y: 4.0 }],
          transform_matrix: None,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::DrawDriverString(EmfPlusDrawDriverStringData {
          font_id: 6,
          brush: EmfPlusBrushRef::ObjectId(3),
          driver_string_options_flags: 0x8000_0000,
          glyphs: vec![65, 66],
          glyph_positions: vec![PointF { x: 1.0, y: 2.0 }, PointF { x: 3.0, y: 4.0 }],
          transform_matrix: None,
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[4..8].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[8..12].copy_from_slice(&2_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_record = record.clone();
    invalid_record.flags = 64;
    assert!(invalid_record.parse_data().is_err());
    let mut truncated_matrix_record = record.clone();
    truncated_matrix_record
      .data
      .truncate(truncated_matrix_record.data.len() - 4);
    assert!(truncated_matrix_record.parse_data().is_err());
    let mut oversized_driver_string_data = Vec::new();
    oversized_driver_string_data.extend_from_slice(&3_u32.to_le_bytes());
    oversized_driver_string_data.extend_from_slice(&0_u32.to_le_bytes());
    oversized_driver_string_data.extend_from_slice(&0_u32.to_le_bytes());
    oversized_driver_string_data.extend_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::DrawDriverString.raw(),
        flags: 6,
        total_object_size: None,
        data: oversized_driver_string_data,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
  }

  #[test]
  fn emf_plus_object_serializable_ts_and_reserved_records_roundtrip() {
    for value in [
      EmfPlusRecordData::MultiFormatStart(EmfPlusRawRecordData {
        data: vec![1, 2, 3, 4],
      }),
      EmfPlusRecordData::MultiFormatSection(EmfPlusRawRecordData { data: vec![5, 6] }),
      EmfPlusRecordData::MultiFormatEnd(EmfPlusRawRecordData { data: Vec::new() }),
    ] {
      assert!(EmfPlusRecord::from_data(&value, EmfPlusRecordFlags::empty()).is_err());
    }
    for record_type in [
      EmfPlusRecordType::MultiFormatStart,
      EmfPlusRecordType::MultiFormatSection,
      EmfPlusRecordType::MultiFormatEnd,
    ] {
      assert!(
        EmfPlusRecord {
          record_type: record_type.raw(),
          flags: EmfPlusRecordFlags::empty().bits(),
          total_object_size: None,
          data: vec![1, 2],
          padding: Vec::new(),
        }
        .parse_data()
        .is_err()
      );
    }

    let raw_values = [
      EmfPlusRecordData::Object(EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::Pen.raw() as u8,
        continues: true,
        total_object_size: Some(4),
        object_data: vec![0xAA, 0xBB, 0xCC, 0xDD],
      }),
      EmfPlusRecordData::SetTsClip(EmfPlusSetTsClipData {
        compressed: true,
        rect_count: 1,
        rects: EmfPlusSetTsClipRects::Compressed(vec![EmfPlusSetTsClipCompressedRect {
          left_delta: 1,
          top_delta: -2,
          right_delta: 3,
          bottom_delta: 4,
        }]),
      }),
    ];
    for value in raw_values {
      let record = EmfPlusRecord::from_data(&value, EmfPlusRecordFlags::empty()).unwrap();
      assert_eq!(record.parse_data().unwrap(), value);
    }

    let ts_clip = EmfPlusRecordData::SetTsClip(EmfPlusSetTsClipData {
      compressed: false,
      rect_count: 1,
      rects: EmfPlusSetTsClipRects::Rects(vec![EmfPlusRectS {
        x: 1,
        y: 2,
        width: 30,
        height: 40,
      }]),
    });
    let record = EmfPlusRecord::from_data(&ts_clip, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.flags, 1);
    assert_eq!(record.data.len(), 8);
    assert_eq!(record.parse_data().unwrap(), ts_clip);
    let mut truncated_ts_clip = record.clone();
    truncated_ts_clip.data.truncate(4);
    assert!(truncated_ts_clip.parse_data().is_err());
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsClip(EmfPlusSetTsClipData {
          compressed: false,
          rect_count: 2,
          rects: EmfPlusSetTsClipRects::Rects(vec![EmfPlusRectS {
            x: 1,
            y: 2,
            width: 30,
            height: 40,
          }]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsClip(EmfPlusSetTsClipData {
          compressed: false,
          rect_count: 1,
          rects: EmfPlusSetTsClipRects::Compressed(vec![EmfPlusSetTsClipCompressedRect {
            left_delta: 1,
            top_delta: 2,
            right_delta: 3,
            bottom_delta: 4,
          },]),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SetTsClip.raw(),
        flags: 0x8001,
        total_object_size: None,
        data: vec![1, 2, 3, 4],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let empty_ts_clip = EmfPlusRecordData::SetTsClip(EmfPlusSetTsClipData {
      compressed: false,
      rect_count: 0,
      rects: EmfPlusSetTsClipRects::Rects(Vec::new()),
    });
    let record = EmfPlusRecord::from_data(&empty_ts_clip, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.flags, 0);
    assert!(record.data.is_empty());
    assert_eq!(record.parse_data().unwrap(), empty_ts_clip);
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsClip(EmfPlusSetTsClipData {
          compressed: false,
          rect_count: 0x8000,
          rects: EmfPlusSetTsClipRects::Rects(Vec::new()),
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let continued_object = EmfPlusRecordData::Object(EmfPlusObjectRecordData {
      object_id: 5,
      object_type_raw: EmfPlusObjectType::Path.raw() as u8,
      continues: true,
      total_object_size: Some(16),
      object_data: vec![1, 2, 3, 4],
    });
    let record = EmfPlusRecord::from_data(&continued_object, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::Object.raw());
    assert_eq!(record.total_object_size, Some(16));
    assert!(record.flags().object_continues());
    assert_eq!(record.flags().object_type(), Some(EmfPlusObjectType::Path));
    assert_eq!(record.flags().object_id(), 5);
    assert_eq!(record.parse_data().unwrap(), continued_object);
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    record.write_to(&mut writer).unwrap();
    let continued_bytes = writer.into_inner().into_inner();
    assert_eq!(
      continued_bytes,
      vec![
        0x08, 0x40, // Type
        0x05, 0x83, // Flags: C + ObjectTypePath + ObjectID 5
        0x14, 0x00, 0x00, 0x00, // Size
        0x08, 0x00, 0x00, 0x00, // DataSize
        0x10, 0x00, 0x00, 0x00, // TotalObjectSize
        1, 2, 3, 4, // ObjectData
      ]
    );
    let record_ref = EmfPlusStreamRef::from_bytes(&continued_bytes)
      .unwrap()
      .records()
      .next()
      .unwrap();
    assert_eq!(record_ref.total_object_size, Some(16));
    assert_eq!(record_ref.data.as_ptr(), continued_bytes[16..].as_ptr());
    assert_eq!(record_ref.parse_data().unwrap(), continued_object);

    let brush_data = EmfPlusBrushData::Solid(EmfPlusSolidBrushData {
      solid_color: EmfPlusArgb {
        blue: 1,
        green: 2,
        red: 3,
        alpha: 0xFF,
      },
      trailing_data: Vec::new(),
    });
    let complete_object_data = EmfPlusObjectData::Brush(EmfPlusBrushObject {
      version: test_graphics_version(),
      brush_type: EmfPlusBrushType::SolidColor.raw(),
      brush_data: brush_data.to_bytes().unwrap(),
    });
    let complete_bytes = complete_object_data.to_bytes().unwrap();
    let total_size = complete_bytes.len() as u32;
    let continued_records =
      EmfPlusRecord::from_continued_object(6, &complete_object_data, 4).unwrap();
    assert_eq!(continued_records.len(), 3);
    assert!(continued_records[0].flags().object_continues());
    assert!(continued_records[1].flags().object_continues());
    assert!(!continued_records[2].flags().object_continues());
    assert_eq!(continued_records[0].total_object_size, Some(total_size));
    assert_eq!(continued_records[1].total_object_size, Some(total_size));
    assert_eq!(continued_records[2].total_object_size, None);
    for record in &continued_records {
      let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
      record.write_to(&mut writer).unwrap();
    }
    let fragments = continued_records
      .iter()
      .map(EmfPlusRecord::object_fragment)
      .collect::<Result<Vec<_>>>()
      .unwrap();
    assert_eq!(
      continued_records[0].clone().into_object_fragment().unwrap(),
      fragments[0]
    );
    assert_eq!(
      continued_records[0].sdk_size() as usize,
      16 + continued_records[0].data.len() + continued_records[0].padding.len()
    );
    let mut assembler = EmfPlusObjectAssembler::default();
    assert!(assembler.push(fragments[0].clone()).unwrap().is_none());
    assert!(assembler.push(fragments[1].clone()).unwrap().is_none());
    let assembled = assembler.push(fragments[2].clone()).unwrap().unwrap();
    assert_eq!(assembled.object_data, complete_bytes);
    assert_eq!(assembled.parse_object_data().unwrap(), complete_object_data);
    assembler.finish().unwrap();

    let mut incomplete_assembler = EmfPlusObjectAssembler::default();
    incomplete_assembler.push(fragments[0].clone()).unwrap();
    assert!(incomplete_assembler.finish().is_err());
    let mut changed_total_size = fragments[1].clone();
    changed_total_size.total_object_size = Some(total_size + 4);
    assert!(incomplete_assembler.push(changed_total_size).is_err());

    let invalid_object_type = EmfPlusRecordData::Object(EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Invalid.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: vec![1, 2, 3, 4],
    });
    assert!(EmfPlusRecord::from_data(&invalid_object_type, EmfPlusRecordFlags::empty()).is_err());
    let invalid_known_object_payload = EmfPlusRecordData::Object(EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Brush.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: vec![1, 2, 3, 4],
    });
    assert!(
      EmfPlusRecord::from_data(&invalid_known_object_payload, EmfPlusRecordFlags::empty()).is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::Object.raw(),
        flags: (EmfPlusObjectType::Brush.raw() << 8) | 1,
        total_object_size: None,
        data: vec![1, 2, 3, 4],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let invalid_total_object_size = EmfPlusRecordData::Object(EmfPlusObjectRecordData {
      object_id: 5,
      object_type_raw: EmfPlusObjectType::Path.raw() as u8,
      continues: true,
      total_object_size: Some(3),
      object_data: vec![1, 2, 3, 4],
    });
    assert!(
      EmfPlusRecord::from_data(&invalid_total_object_size, EmfPlusRecordFlags::empty()).is_err()
    );
    let mut invalid_total_object_size_record = record.clone();
    invalid_total_object_size_record.total_object_size = Some(3);
    assert!(invalid_total_object_size_record.parse_data().is_err());
    assert!(
      invalid_total_object_size_record
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
    let invalid_total_object_size_bytes = vec![
      0x08, 0x40, // Type
      0x05, 0x83, // Flags: C + ObjectTypePath + ObjectID 5
      0x14, 0x00, 0x00, 0x00, // Size
      0x08, 0x00, 0x00, 0x00, // DataSize
      0x03, 0x00, 0x00, 0x00, // TotalObjectSize
      1, 2, 3, 4, // ObjectData
    ];
    assert!(
      EmfPlusRecord::read_from(
        &mut Reader::new(std::io::Cursor::new(
          invalid_total_object_size_bytes.as_slice()
        )),
        invalid_total_object_size_bytes.len() as u64,
      )
      .is_err()
    );
    let mut invalid_object_record = record.clone();
    invalid_object_record.flags = 1;
    invalid_object_record.total_object_size = None;
    assert!(invalid_object_record.parse_data().is_err());
    let mut continued_without_total_size = record.clone();
    continued_without_total_size.total_object_size = None;
    assert!(continued_without_total_size.parse_data().is_err());
    let mut noncontinued_with_total_size = record.clone();
    noncontinued_with_total_size.flags = EmfPlusObjectType::Path.raw() << 8 | 5;
    assert!(noncontinued_with_total_size.parse_data().is_err());
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::Object(EmfPlusObjectRecordData {
          object_id: 64,
          object_type_raw: EmfPlusObjectType::Path.raw() as u8,
          continues: true,
          total_object_size: Some(4),
          object_data: vec![1, 2, 3, 4],
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::Object.raw(),
        flags: 0x8340,
        total_object_size: Some(4),
        data: vec![1, 2, 3, 4],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::Object(EmfPlusObjectRecordData {
          object_id: 1,
          object_type_raw: 10,
          continues: true,
          total_object_size: Some(4),
          object_data: vec![1, 2, 3, 4],
        }),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::Object.raw(),
        flags: 0x8A01,
        total_object_size: Some(4),
        data: vec![1, 2, 3, 4],
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );

    let mut unaligned_size = vec![
      0x03, 0x40, // Type: Comment
      0x00, 0x00, // Flags
      0x0D, 0x00, 0x00, 0x00, // Size
      0x00, 0x00, 0x00, 0x00, // DataSize
      0x00, // Padding
    ];
    assert!(
      EmfPlusRecord::read_from(
        &mut Reader::new(std::io::Cursor::new(&mut unaligned_size)),
        13,
      )
      .is_err()
    );

    let unaligned_write_data = EmfPlusRecord {
      record_type: EmfPlusRecordType::Comment.raw(),
      flags: EmfPlusRecordFlags::empty().bits(),
      total_object_size: None,
      data: vec![0xAA, 0xBB, 0xCC, 0xDD],
      padding: vec![0],
    };
    assert!(
      unaligned_write_data
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
    let fixed_record_with_padding = EmfPlusRecord {
      record_type: EmfPlusRecordType::SetRenderingOrigin.raw(),
      flags: EmfPlusRecordFlags::empty().bits(),
      total_object_size: None,
      data: vec![0; 8],
      padding: vec![0; 4],
    };
    assert!(fixed_record_with_padding.parse_data().is_err());
    assert!(
      fixed_record_with_padding
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
    let fixed_record_with_short_data = EmfPlusRecord {
      record_type: EmfPlusRecordType::SetRenderingOrigin.raw(),
      flags: EmfPlusRecordFlags::empty().bits(),
      total_object_size: None,
      data: vec![0; 4],
      padding: Vec::new(),
    };
    assert!(fixed_record_with_short_data.parse_data().is_err());
    let mut fixed_record_writer = Writer::new(std::io::Cursor::new(Vec::new()));
    fixed_record_with_short_data
      .write_to(&mut fixed_record_writer)
      .unwrap();
    let fixed_record_bytes = fixed_record_writer.into_inner().into_inner();
    assert_eq!(
      EmfPlusRecord::read_from(
        &mut Reader::new(std::io::Cursor::new(fixed_record_bytes.as_slice())),
        fixed_record_bytes.len() as u64,
      )
      .unwrap(),
      fixed_record_with_short_data
    );
    let header_record_with_short_data = EmfPlusRecord {
      record_type: EmfPlusRecordType::Header.raw(),
      flags: EmfPlusRecordFlags::empty().bits(),
      total_object_size: None,
      data: vec![0; 12],
      padding: Vec::new(),
    };
    assert!(header_record_with_short_data.parse_data().is_err());
    header_record_with_short_data
      .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
      .unwrap();
    let set_clip_rect_record_with_padding = EmfPlusRecord {
      record_type: EmfPlusRecordType::SetClipRect.raw(),
      flags: EmfPlusRecordFlags::empty().bits(),
      total_object_size: None,
      data: vec![0; 16],
      padding: vec![0],
    };
    assert!(set_clip_rect_record_with_padding.parse_data().is_err());
    assert!(
      set_clip_rect_record_with_padding
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );

    let unknown_serializable = EmfPlusSerializableObjectData {
      object_guid: [0x11; 16],
      buffer: vec![1, 2, 3, 4],
    };
    assert_eq!(unknown_serializable.effect_kind(), None);
    assert_eq!(
      unknown_serializable.parse_effect().unwrap(),
      EmfPlusImageEffect::Unknown {
        object_guid: [0x11; 16],
        buffer: vec![1, 2, 3, 4],
      }
    );
    let unknown_serializable_record =
      EmfPlusRecordData::SerializableObject(unknown_serializable.clone());
    assert!(
      EmfPlusRecord::from_data(&unknown_serializable_record, EmfPlusRecordFlags::empty()).is_err()
    );
    let mut unknown_serializable_record_data = Vec::new();
    unknown_serializable_record_data.extend_from_slice(&[0x11; 16]);
    unknown_serializable_record_data.extend_from_slice(&4_u32.to_le_bytes());
    unknown_serializable_record_data.extend_from_slice(&[1, 2, 3, 4]);
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SerializableObject.raw(),
        flags: 0,
        total_object_size: None,
        data: unknown_serializable_record_data,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let mut oversized_buffer_record_data = Vec::new();
    oversized_buffer_record_data.extend_from_slice(&[0x11; 16]);
    oversized_buffer_record_data.extend_from_slice(&8_u32.to_le_bytes());
    oversized_buffer_record_data.extend_from_slice(&[1, 2, 3, 4]);
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SerializableObject.raw(),
        flags: 0,
        total_object_size: None,
        data: oversized_buffer_record_data,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    assert!(
      EmfPlusSerializableObjectData {
        object_guid: EmfPlusImageEffectKind::Blur.guid(),
        buffer: vec![1, 2, 3, 4],
      }
      .parse_effect()
      .is_err()
    );
    let invalid_known_effect =
      EmfPlusRecordData::SerializableObject(EmfPlusSerializableObjectData {
        object_guid: EmfPlusImageEffectKind::Blur.guid(),
        buffer: vec![1, 2, 3, 4],
      });
    assert!(EmfPlusRecord::from_data(&invalid_known_effect, EmfPlusRecordFlags::empty()).is_err());
    let mut invalid_known_effect_record_data = Vec::new();
    invalid_known_effect_record_data.extend_from_slice(&EmfPlusImageEffectKind::Blur.guid());
    invalid_known_effect_record_data.extend_from_slice(&4_u32.to_le_bytes());
    invalid_known_effect_record_data.extend_from_slice(&[1, 2, 3, 4]);
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SerializableObject.raw(),
        flags: 0,
        total_object_size: None,
        data: invalid_known_effect_record_data,
        padding: Vec::new(),
      }
      .parse_data()
      .is_err()
    );
    let mut unaligned_buffer_record_data = Vec::new();
    unaligned_buffer_record_data.extend_from_slice(&[0x11; 16]);
    unaligned_buffer_record_data.extend_from_slice(&1_u32.to_le_bytes());
    unaligned_buffer_record_data.push(0xAA);
    assert!(
      EmfPlusRecord {
        record_type: EmfPlusRecordType::SerializableObject.raw(),
        flags: 0,
        total_object_size: None,
        data: unaligned_buffer_record_data,
        padding: vec![0, 0, 0],
      }
      .parse_data()
      .is_err()
    );

    let blur_effect = EmfPlusImageEffect::Blur(EmfPlusBlurEffect {
      blur_radius: 2.5,
      expand_edge: 1,
      trailing_data: Vec::new(),
    });
    let blur_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::Blur.guid(),
      buffer: blur_effect.to_bytes().unwrap(),
    };
    let serializable = EmfPlusRecordData::SerializableObject(blur_serializable.clone());
    let record = EmfPlusRecord::from_data(&serializable, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(
      record.record_type,
      EmfPlusRecordType::SerializableObject.raw()
    );
    assert_eq!(record.parse_data().unwrap(), serializable);
    let serializable_with_ignored_flags =
      EmfPlusRecord::from_data(&serializable, EmfPlusRecordFlags::POST_MULTIPLY).unwrap();
    assert_eq!(
      serializable_with_ignored_flags.parse_data().unwrap(),
      serializable
    );
    let mut invalid_serializable = record.clone();
    invalid_serializable.data.push(0xEE);
    assert!(invalid_serializable.parse_data().is_err());
    assert_eq!(
      blur_serializable.effect_kind(),
      Some(EmfPlusImageEffectKind::Blur)
    );
    let parsed_blur_effect = blur_serializable.parse_effect().unwrap();
    assert_eq!(parsed_blur_effect, blur_effect);
    assert_eq!(
      parsed_blur_effect.to_bytes().unwrap(),
      blur_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::Blur(EmfPlusBlurEffect {
        blur_radius: 256.0,
        expand_edge: 1,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusSerializableObjectData {
        object_guid: EmfPlusImageEffectKind::Blur.guid(),
        buffer: {
          let mut data = Vec::new();
          data.extend_from_slice(&2.5_f32.to_le_bytes());
          data.extend_from_slice(&2_u32.to_le_bytes());
          data
        },
      }
      .parse_effect()
      .is_err()
    );
    assert!(
      EmfPlusImageEffect::Blur(EmfPlusBlurEffect {
        blur_radius: 2.5,
        expand_edge: 1,
        trailing_data: vec![0, 0, 0, 0],
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusSerializableObjectData {
        object_guid: EmfPlusImageEffectKind::Blur.guid(),
        buffer: {
          let mut data = blur_serializable.buffer.clone();
          data.extend_from_slice(&[0, 0, 0, 0]);
          data
        },
      }
      .parse_effect()
      .is_err()
    );

    let brightness_effect =
      EmfPlusImageEffect::BrightnessContrast(EmfPlusBrightnessContrastEffect {
        brightness_level: 25,
        contrast_level: -25,
        trailing_data: Vec::new(),
      });
    let brightness_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::BrightnessContrast.guid(),
      buffer: brightness_effect.to_bytes().unwrap(),
    };
    let parsed_brightness_effect = brightness_serializable.parse_effect().unwrap();
    assert_eq!(parsed_brightness_effect, brightness_effect);
    assert_eq!(
      parsed_brightness_effect.to_bytes().unwrap(),
      brightness_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::BrightnessContrast(EmfPlusBrightnessContrastEffect {
        brightness_level: 0,
        contrast_level: 101,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let balance_effect = EmfPlusImageEffect::ColorBalance(EmfPlusColorBalanceEffect {
      cyan_red: -10,
      magenta_green: 0,
      yellow_blue: 10,
      trailing_data: Vec::new(),
    });
    let balance_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::ColorBalance.guid(),
      buffer: balance_effect.to_bytes().unwrap(),
    };
    let parsed_balance_effect = balance_serializable.parse_effect().unwrap();
    assert_eq!(parsed_balance_effect, balance_effect);
    assert_eq!(
      parsed_balance_effect.to_bytes().unwrap(),
      balance_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::ColorBalance(EmfPlusColorBalanceEffect {
        cyan_red: -101,
        magenta_green: 0,
        yellow_blue: 0,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let color_curve_effect = EmfPlusImageEffect::ColorCurve(EmfPlusColorCurveEffect {
      curve_adjustment: EmfPlusCurveAdjustment::Contrast.raw(),
      curve_channel: EmfPlusCurveChannel::Blue.raw(),
      adjustment_intensity: -25,
      trailing_data: Vec::new(),
    });
    let color_curve_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::ColorCurve.guid(),
      buffer: color_curve_effect.to_bytes().unwrap(),
    };
    let parsed_color_curve_effect = color_curve_serializable.parse_effect().unwrap();
    assert_eq!(parsed_color_curve_effect, color_curve_effect);
    let EmfPlusImageEffect::ColorCurve(parsed_color_curve) = parsed_color_curve_effect else {
      panic!("expected color curve effect");
    };
    assert_eq!(
      parsed_color_curve.curve_adjustment_kind(),
      Some(EmfPlusCurveAdjustment::Contrast)
    );
    assert_eq!(
      parsed_color_curve.curve_channel_kind(),
      Some(EmfPlusCurveChannel::Blue)
    );
    assert!(
      EmfPlusImageEffect::ColorCurve(EmfPlusColorCurveEffect {
        curve_adjustment: EmfPlusCurveAdjustment::Contrast.raw(),
        curve_channel: EmfPlusCurveChannel::Blue.raw(),
        adjustment_intensity: 101,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusSerializableObjectData {
        object_guid: EmfPlusImageEffectKind::ColorCurve.guid(),
        buffer: {
          let mut data = Vec::new();
          data.extend_from_slice(&EmfPlusCurveAdjustment::Contrast.raw().to_le_bytes());
          data.extend_from_slice(&EmfPlusCurveChannel::Blue.raw().to_le_bytes());
          data.extend_from_slice(&101_i32.to_le_bytes());
          data
        },
      }
      .parse_effect()
      .is_err()
    );

    let mut color_matrix = [[0.0; 5]; 5];
    color_matrix[0][0] = 1.0;
    color_matrix[1][1] = 1.0;
    color_matrix[2][2] = 1.0;
    color_matrix[3][3] = 1.0;
    color_matrix[4][4] = 1.0;
    color_matrix[4][0] = 0.25;
    let color_matrix_effect = EmfPlusImageEffect::ColorMatrix(EmfPlusColorMatrixEffect {
      matrix: color_matrix,
      trailing_data: Vec::new(),
    });
    let color_matrix_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::ColorMatrix.guid(),
      buffer: color_matrix_effect.to_bytes().unwrap(),
    };
    let parsed_color_matrix_effect = color_matrix_serializable.parse_effect().unwrap();
    assert_eq!(parsed_color_matrix_effect, color_matrix_effect);
    assert_eq!(
      parsed_color_matrix_effect.to_bytes().unwrap(),
      color_matrix_serializable.buffer
    );
    let mut invalid_color_matrix = color_matrix;
    invalid_color_matrix[2][4] = 0.5;
    assert!(
      EmfPlusImageEffect::ColorMatrix(EmfPlusColorMatrixEffect {
        matrix: invalid_color_matrix,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let hsl_effect =
      EmfPlusImageEffect::HueSaturationLightness(EmfPlusHueSaturationLightnessEffect {
        hue_level: -180,
        saturation_level: 50,
        lightness_level: -50,
        trailing_data: Vec::new(),
      });
    let hsl_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::HueSaturationLightness.guid(),
      buffer: hsl_effect.to_bytes().unwrap(),
    };
    let parsed_hsl_effect = hsl_serializable.parse_effect().unwrap();
    assert_eq!(parsed_hsl_effect, hsl_effect);
    assert_eq!(
      parsed_hsl_effect.to_bytes().unwrap(),
      hsl_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::HueSaturationLightness(EmfPlusHueSaturationLightnessEffect {
        hue_level: 181,
        saturation_level: 0,
        lightness_level: 0,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let levels_effect = EmfPlusImageEffect::Levels(EmfPlusLevelsEffect {
      highlight: 25,
      mid_tone: -25,
      shadow: 75,
      trailing_data: Vec::new(),
    });
    let levels_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::Levels.guid(),
      buffer: levels_effect.to_bytes().unwrap(),
    };
    let parsed_levels_effect = levels_serializable.parse_effect().unwrap();
    assert_eq!(parsed_levels_effect, levels_effect);
    assert_eq!(
      parsed_levels_effect.to_bytes().unwrap(),
      levels_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::Levels(EmfPlusLevelsEffect {
        highlight: -1,
        mid_tone: 0,
        shadow: 0,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let red_eye_effect = EmfPlusImageEffect::RedEyeCorrection(EmfPlusRedEyeCorrectionEffect {
      areas: vec![RectL {
        left: 1,
        top: 2,
        right: 3,
        bottom: 4,
      }],
      trailing_data: Vec::new(),
    });
    let red_eye_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::RedEyeCorrection.guid(),
      buffer: red_eye_effect.to_bytes().unwrap(),
    };
    let parsed_red_eye_effect = red_eye_serializable.parse_effect().unwrap();
    assert_eq!(parsed_red_eye_effect, red_eye_effect);
    assert_eq!(
      parsed_red_eye_effect.to_bytes().unwrap(),
      red_eye_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::RedEyeCorrection(EmfPlusRedEyeCorrectionEffect {
        areas: Vec::new(),
        trailing_data: vec![0, 0, 0, 0],
      })
      .to_bytes()
      .is_err()
    );
    let negative_red_eye_area_count = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::RedEyeCorrection.guid(),
      buffer: (-1_i32).to_le_bytes().to_vec(),
    };
    assert!(negative_red_eye_area_count.parse_effect().is_err());
    let mut truncated_red_eye_area = 1_i32.to_le_bytes().to_vec();
    truncated_red_eye_area.extend_from_slice(&[0; 15]);
    let truncated_red_eye_area = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::RedEyeCorrection.guid(),
      buffer: truncated_red_eye_area,
    };
    assert!(truncated_red_eye_area.parse_effect().is_err());

    let lookup_effect =
      EmfPlusImageEffect::ColorLookupTable(Box::new(EmfPlusColorLookupTableEffect {
        blue_lookup_table: [1; 256],
        green_lookup_table: [2; 256],
        red_lookup_table: [3; 256],
        alpha_lookup_table: [4; 256],
        trailing_data: Vec::new(),
      }));
    let lookup_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::ColorLookupTable.guid(),
      buffer: lookup_effect.to_bytes().unwrap(),
    };
    let parsed_lookup_effect = lookup_serializable.parse_effect().unwrap();
    assert_eq!(parsed_lookup_effect, lookup_effect);
    assert_eq!(
      parsed_lookup_effect.to_bytes().unwrap(),
      lookup_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::ColorLookupTable(Box::new(EmfPlusColorLookupTableEffect {
        blue_lookup_table: [1; 256],
        green_lookup_table: [2; 256],
        red_lookup_table: [3; 256],
        alpha_lookup_table: [4; 256],
        trailing_data: vec![0, 0, 0, 0],
      }))
      .to_bytes()
      .is_err()
    );

    let sharpen_effect = EmfPlusImageEffect::Sharpen(EmfPlusSharpenEffect {
      radius: 2.0,
      amount: 50.0,
      trailing_data: Vec::new(),
    });
    let sharpen_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::Sharpen.guid(),
      buffer: sharpen_effect.to_bytes().unwrap(),
    };
    let parsed_sharpen_effect = sharpen_serializable.parse_effect().unwrap();
    assert_eq!(parsed_sharpen_effect, sharpen_effect);
    assert_eq!(
      parsed_sharpen_effect.to_bytes().unwrap(),
      sharpen_serializable.buffer
    );
    assert!(
      EmfPlusImageEffect::Sharpen(EmfPlusSharpenEffect {
        radius: 2.0,
        amount: 101.0,
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let tint_effect = EmfPlusImageEffect::Tint(EmfPlusTintEffect {
      hue: 180,
      amount: -50,
      trailing_data: Vec::new(),
    });
    let tint_serializable = EmfPlusSerializableObjectData {
      object_guid: EmfPlusImageEffectKind::Tint.guid(),
      buffer: tint_effect.to_bytes().unwrap(),
    };
    let parsed_tint_effect = tint_serializable.parse_effect().unwrap();
    assert_eq!(parsed_tint_effect, tint_effect);
    assert_eq!(
      parsed_tint_effect.to_bytes().unwrap(),
      tint_serializable.buffer
    );
    assert!(
      EmfPlusSerializableObjectData {
        object_guid: EmfPlusImageEffectKind::Tint.guid(),
        buffer: {
          let mut data = Vec::new();
          data.extend_from_slice(&181_i32.to_le_bytes());
          data.extend_from_slice(&0_i32.to_le_bytes());
          data
        },
      }
      .parse_effect()
      .is_err()
    );

    let ts_graphics = EmfPlusRecordData::SetTsGraphics(EmfPlusSetTsGraphicsData {
      anti_alias_mode: 1,
      text_render_hint: EmfPlusTextRenderingHint::SingleBitPerPixel.raw() as u8,
      compositing_mode: EmfPlusCompositingMode::SourceCopy.raw() as u8,
      compositing_quality: EmfPlusCompositingQuality::GammaCorrected.raw() as u8,
      render_origin_x: -5,
      render_origin_y: 6,
      text_contrast: 7,
      filter_type: EmfPlusFilterType::PyramidalQuad.raw() as u8,
      pixel_offset: EmfPlusPixelOffsetMode::Half.raw() as u8,
      world_to_device: XForm {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 10.0,
        dy: 11.0,
      },
      palette: Some(EmfPlusPalette {
        palette_style_flags: EmfPlusPaletteStyleFlags::HAS_ALPHA.bits(),
        entries: vec![EmfPlusArgb {
          blue: 0xAA,
          green: 0xBB,
          red: 0xCC,
          alpha: 0xDD,
        }],
        trailing_data: Vec::new(),
      }),
    });
    let record = EmfPlusRecord::from_data(&ts_graphics, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::SetTsGraphics.raw());
    assert!(record.flags().ts_graphics_palette_present());
    assert!(!record.flags().ts_graphics_basic_vga());
    assert_eq!(record.parse_data().unwrap(), ts_graphics);
    let EmfPlusRecordData::SetTsGraphics(parsed_ts_graphics) = record.parse_data().unwrap() else {
      panic!("expected TS graphics");
    };
    assert!(parsed_ts_graphics.anti_alias_enabled());
    assert_eq!(
      parsed_ts_graphics.anti_alias_mode_kind(),
      Some(EmfPlusSmoothingMode::HighSpeed)
    );
    assert_eq!(
      parsed_ts_graphics.anti_alias_smoothing_mode(),
      Some(EmfPlusSmoothingMode::HighSpeed)
    );
    assert_eq!(
      parsed_ts_graphics.text_rendering_hint_kind(),
      Some(EmfPlusTextRenderingHint::SingleBitPerPixel)
    );
    assert_eq!(
      parsed_ts_graphics.compositing_mode_kind(),
      Some(EmfPlusCompositingMode::SourceCopy)
    );
    assert_eq!(
      parsed_ts_graphics.compositing_quality_kind(),
      Some(EmfPlusCompositingQuality::GammaCorrected)
    );
    assert_eq!(
      parsed_ts_graphics.filter_type_kind(),
      Some(EmfPlusFilterType::PyramidalQuad)
    );
    assert_eq!(
      parsed_ts_graphics.pixel_offset_mode_kind(),
      Some(EmfPlusPixelOffsetMode::Half)
    );
    let EmfPlusRecordData::SetTsGraphics(ts_graphics_data) = &ts_graphics else {
      unreachable!();
    };
    let mut invalid_ts_graphics = ts_graphics_data.clone();
    invalid_ts_graphics.text_contrast = 13;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(invalid_ts_graphics),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let mut invalid_ts_graphics = ts_graphics_data.clone();
    invalid_ts_graphics.anti_alias_mode = 0xFF;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(invalid_ts_graphics),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let mut invalid_ts_graphics = ts_graphics_data.clone();
    invalid_ts_graphics.text_render_hint = 0xFF;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(invalid_ts_graphics),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let mut invalid_ts_graphics = ts_graphics_data.clone();
    invalid_ts_graphics.compositing_mode = 0xFF;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(invalid_ts_graphics),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let mut invalid_ts_graphics = ts_graphics_data.clone();
    invalid_ts_graphics.compositing_quality = 0xFF;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(invalid_ts_graphics),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let mut invalid_ts_graphics = ts_graphics_data.clone();
    invalid_ts_graphics.filter_type = 0xFF;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(invalid_ts_graphics),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let mut invalid_ts_graphics = ts_graphics_data.clone();
    invalid_ts_graphics.pixel_offset = 0xFF;
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(invalid_ts_graphics),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let mut invalid_record = record.clone();
    invalid_record.data[3] = 0xFF;
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[1] = 0xFF;
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[2] = 0xFF;
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[10] = 0xFF;
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[11] = 0xFF;
    assert!(invalid_record.parse_data().is_err());

    let mut missing_palette_flag = record.clone();
    missing_palette_flag.flags &= !EmfPlusRecordFlags::TS_GRAPHICS_PALETTE.bits();
    assert!(missing_palette_flag.parse_data().is_err());

    let mut missing_palette_data = record.clone();
    missing_palette_data.data.truncate(36);
    assert!(missing_palette_data.parse_data().is_err());

    let mut trailing_palette = ts_graphics_data.clone();
    trailing_palette.palette.as_mut().unwrap().trailing_data = vec![0];
    assert!(
      EmfPlusRecord::from_data(
        &EmfPlusRecordData::SetTsGraphics(trailing_palette),
        EmfPlusRecordFlags::empty(),
      )
      .is_err()
    );

    let ts_graphics_without_palette = EmfPlusRecordData::SetTsGraphics(EmfPlusSetTsGraphicsData {
      palette: None,
      ..ts_graphics_data.clone()
    });
    let record_without_palette =
      EmfPlusRecord::from_data(&ts_graphics_without_palette, EmfPlusRecordFlags::empty()).unwrap();
    assert!(!record_without_palette.flags().ts_graphics_palette_present());
    assert_eq!(
      record_without_palette.parse_data().unwrap(),
      ts_graphics_without_palette
    );
    assert!(
      EmfPlusRecord::from_data(
        &ts_graphics_without_palette,
        EmfPlusRecordFlags::TS_GRAPHICS_BASIC_VGA,
      )
      .is_err()
    );

    let ts_graphics_basic_vga = EmfPlusRecordData::SetTsGraphics(EmfPlusSetTsGraphicsData {
      palette: Some(EmfPlusPalette {
        palette_style_flags: 0,
        entries: vec![
          EmfPlusArgb {
            blue: 0x00,
            green: 0x00,
            red: 0x00,
            alpha: 0xFF,
          },
          EmfPlusArgb {
            blue: 0xFF,
            green: 0xFF,
            red: 0xFF,
            alpha: 0xFF,
          },
        ],
        trailing_data: Vec::new(),
      }),
      ..ts_graphics_data.clone()
    });
    let record = EmfPlusRecord::from_data(
      &ts_graphics_basic_vga,
      EmfPlusRecordFlags::TS_GRAPHICS_BASIC_VGA,
    )
    .unwrap();
    assert!(record.flags().ts_graphics_palette_present());
    assert!(record.flags().ts_graphics_basic_vga());
    assert_eq!(record.parse_data().unwrap(), ts_graphics_basic_vga);
    assert!(
      EmfPlusRecord::from_data(&ts_graphics, EmfPlusRecordFlags::TS_GRAPHICS_BASIC_VGA,).is_err()
    );

    let stroke_fill = EmfPlusRecordData::StrokeFillPath;
    let record = EmfPlusRecord::from_data(&stroke_fill, EmfPlusRecordFlags::empty()).unwrap();
    assert_eq!(record.record_type, EmfPlusRecordType::StrokeFillPath.raw());
    assert_eq!(record.parse_data().unwrap(), stroke_fill);
  }

  fn test_graphics_version() -> EmfPlusGraphicsVersion {
    EmfPlusGraphicsVersion {
      value: (EMFPLUS_METAFILE_SIGNATURE << 12) | 0x0002,
    }
  }

  fn invalid_graphics_version() -> EmfPlusGraphicsVersion {
    EmfPlusGraphicsVersion { value: 0x0000_0002 }
  }

  fn test_pen_payload_bytes() -> Vec<u8> {
    EmfPlusPenPayload {
      pen_data: EmfPlusPenData {
        pen_data_flags: 0,
        pen_unit: EmfPlusUnitType::Pixel.raw(),
        pen_width: 1.0,
        optional_data: EmfPlusPenOptionalData::default(),
        trailing_data: Vec::new(),
      },
      brush_object: Some(EmfPlusBrushObject {
        version: test_graphics_version(),
        brush_type: EmfPlusBrushType::SolidColor.raw(),
        brush_data: EmfPlusBrushData::Solid(EmfPlusSolidBrushData {
          solid_color: EmfPlusArgb {
            blue: 0,
            green: 0,
            red: 0,
            alpha: 0xFF,
          },
          trailing_data: Vec::new(),
        })
        .to_bytes()
        .unwrap(),
      }),
    }
    .to_bytes()
    .unwrap()
  }

  fn path_point_type(value: u8) -> EmfPlusPathPointTypeValue {
    EmfPlusPathPointTypeValue::new(value).unwrap()
  }

  fn path_point_types(values: &[u8]) -> Vec<EmfPlusPathPointTypeValue> {
    values.iter().copied().map(path_point_type).collect()
  }

  fn assert_object_data_roundtrip(object_type: EmfPlusObjectType, data: EmfPlusObjectData) {
    let bytes = data.to_bytes().unwrap();
    let record_data = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: object_type.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: bytes.clone(),
    };
    let parsed = record_data.parse_object_data().unwrap();
    assert_eq!(parsed, data);
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  fn assert_brush_data_roundtrip(brush_type: EmfPlusBrushType, data: EmfPlusBrushData) {
    let bytes = data.to_bytes().unwrap();
    let brush = EmfPlusBrushObject {
      version: test_graphics_version(),
      brush_type: brush_type.raw(),
      brush_data: bytes.clone(),
    };
    let parsed = brush.parse_brush_data().unwrap();
    assert_eq!(parsed, data);
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn emf_plus_known_truncated_object_payloads_are_not_unknown() {
    let object = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Brush.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: vec![1, 2, 3, 4],
    };
    assert!(object.parse_object_data().is_err());

    let mut continued = object.clone();
    continued.continues = true;
    assert!(matches!(
      continued.parse_object_data().unwrap(),
      EmfPlusObjectData::Unknown { .. }
    ));

    let mut unknown_object = object.clone();
    unknown_object.object_type_raw = 0x7F;
    assert!(matches!(
      unknown_object.parse_object_data().unwrap(),
      EmfPlusObjectData::Unknown { .. }
    ));

    let brush = EmfPlusBrushObject {
      version: test_graphics_version(),
      brush_type: EmfPlusBrushType::SolidColor.raw(),
      brush_data: vec![1, 2],
    };
    assert!(brush.parse_brush_data().is_err());
    let brush_data = EmfPlusObjectData::Brush(brush.clone());
    assert_eq!(brush_data.to_bytes().unwrap().len(), 10);
    assert!(brush_data.validate_strict().is_err());
    let unknown_brush = EmfPlusBrushObject {
      brush_type: 0xFFFF,
      ..brush.clone()
    };
    assert!(matches!(
      unknown_brush.parse_brush_data().unwrap(),
      EmfPlusBrushData::Unknown { .. }
    ));

    let cap = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::Default.raw(),
      custom_line_cap_data: vec![1, 2, 3, 4],
    };
    assert!(cap.parse_cap_data().is_err());
    let unknown_cap = EmfPlusCustomLineCapObject {
      cap_type: 0xFFFF,
      ..cap.clone()
    };
    assert!(matches!(
      unknown_cap.parse_cap_data().unwrap(),
      EmfPlusCustomLineCapData::Unknown { .. }
    ));

    let image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: vec![1, 2, 3, 4],
    };
    assert!(image.parse_image_data().is_err());
    assert!(EmfPlusObjectData::Image(image.clone()).to_bytes().is_err());
    let unknown_image = EmfPlusImageObject {
      image_type: EmfPlusImageDataType::Unknown.raw(),
      ..image
    };
    assert!(matches!(
      unknown_image.parse_image_data().unwrap(),
      EmfPlusImageData::Unknown { .. }
    ));
  }

  #[test]
  fn emf_plus_graphics_object_fixed_headers_roundtrip() {
    assert_object_data_roundtrip(
      EmfPlusObjectType::Brush,
      EmfPlusObjectData::Brush(EmfPlusBrushObject {
        version: test_graphics_version(),
        brush_type: 0,
        brush_data: vec![0xAA, 0xBB, 0xCC, 0xDD],
      }),
    );
    assert!(
      EmfPlusObjectData::Brush(EmfPlusBrushObject {
        version: test_graphics_version(),
        brush_type: 0xFFFF_FFFF,
        brush_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::Brush.raw() as u8,
        continues: false,
        total_object_size: None,
        object_data: {
          let mut data = Vec::new();
          data.extend_from_slice(&test_graphics_version().value.to_le_bytes());
          data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
          data
        },
      }
      .parse_object_data()
      .is_err()
    );
    assert!(
      EmfPlusObjectData::Brush(EmfPlusBrushObject {
        version: invalid_graphics_version(),
        brush_type: EmfPlusBrushType::SolidColor.raw(),
        brush_data: vec![0, 0, 0, 0],
      })
      .to_bytes()
      .is_err()
    );
    assert_object_data_roundtrip(
      EmfPlusObjectType::CustomLineCap,
      EmfPlusObjectData::CustomLineCap(EmfPlusCustomLineCapObject {
        version: test_graphics_version(),
        cap_type: EmfPlusCustomLineCapDataType::Default.raw(),
        custom_line_cap_data: EmfPlusCustomLineCapData::Default(EmfPlusCustomLineCapDefaultData {
          custom_line_cap_data_flags: 0,
          base_cap: EmfPlusLineCapType::Flat.raw() as u32,
          base_inset: 0.0,
          stroke_start_cap: EmfPlusLineCapType::Flat.raw() as u32,
          stroke_end_cap: EmfPlusLineCapType::Flat.raw() as u32,
          stroke_join: EmfPlusLineJoinType::Miter.raw() as u32,
          stroke_miter_limit: 1.0,
          width_scale: 1.0,
          fill_hot_spot: PointF { x: 0.0, y: 0.0 },
          stroke_hot_spot: PointF { x: 0.0, y: 0.0 },
          optional_data: Vec::new(),
        })
        .to_bytes()
        .unwrap(),
      }),
    );
    assert!(
      EmfPlusObjectData::CustomLineCap(EmfPlusCustomLineCapObject {
        version: invalid_graphics_version(),
        cap_type: EmfPlusCustomLineCapDataType::Default.raw(),
        custom_line_cap_data: vec![1, 2, 3, 4],
      })
      .to_bytes()
      .is_err()
    );
    let font_object = EmfPlusFontObject {
      version: test_graphics_version(),
      em_size: 12.5,
      size_unit: 3,
      font_style_flags: 1,
      reserved: 0x1234,
      family_name: SdkString::raw(
        vec![b'A', 0, b'r', 0, b'i', 0, b'a', 0, b'l', 0],
        SdkEncoding::Utf16Le,
      ),
      padding: vec![0, 0],
    };
    assert_object_data_roundtrip(
      EmfPlusObjectType::Font,
      EmfPlusObjectData::Font(font_object.clone()),
    );
    let invalid_font_padding = EmfPlusFontObject {
      padding: vec![0],
      ..font_object.clone()
    };
    assert!(
      EmfPlusObjectData::Font(invalid_font_padding)
        .to_bytes()
        .is_err()
    );
    let invalid_font_padding_record = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Font.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: {
        let mut data = EmfPlusObjectData::Font(font_object.clone())
          .to_bytes()
          .unwrap();
        data.pop();
        data
      },
    };
    assert!(invalid_font_padding_record.parse_object_data().is_err());
    assert!(
      EmfPlusObjectData::Font(EmfPlusFontObject {
        version: test_graphics_version(),
        em_size: 12.5,
        size_unit: 0xFFFF_FFFF,
        font_style_flags: 1,
        reserved: 0,
        family_name: SdkString::raw(vec![b'A', 0], SdkEncoding::Utf16Le),
        padding: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    let invalid_font_record = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Font.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&test_graphics_version().value.to_le_bytes());
        data.extend_from_slice(&12.5_f32.to_le_bytes());
        data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        data.extend_from_slice(&1_i32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&[b'A', 0]);
        data
      },
    };
    assert!(invalid_font_record.parse_object_data().is_err());
    assert!(
      EmfPlusObjectData::Font(EmfPlusFontObject {
        version: invalid_graphics_version(),
        em_size: 12.5,
        size_unit: EmfPlusUnitType::Pixel.raw(),
        font_style_flags: 0,
        reserved: 0,
        family_name: SdkString::raw(vec![b'A', 0], SdkEncoding::Utf16Le),
        padding: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    assert_object_data_roundtrip(
      EmfPlusObjectType::Image,
      EmfPlusObjectData::Image(EmfPlusImageObject {
        version: test_graphics_version(),
        image_type: EmfPlusImageDataType::Bitmap.raw(),
        image_data: EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
          width: 1,
          height: 1,
          stride: 4,
          pixel_format: EmfPlusPixelFormat::Format32bppArgb.raw(),
          bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
          bitmap_data: vec![0, 0, 0, 0],
        })
        .to_bytes()
        .unwrap(),
      }),
    );
    assert!(
      EmfPlusObjectData::Image(EmfPlusImageObject {
        version: invalid_graphics_version(),
        image_type: EmfPlusImageDataType::Bitmap.raw(),
        image_data: vec![0; 20],
      })
      .to_bytes()
      .is_err()
    );
    assert_object_data_roundtrip(
      EmfPlusObjectType::ImageAttributes,
      EmfPlusObjectData::ImageAttributes(EmfPlusImageAttributesObject {
        version: test_graphics_version(),
        reserved1: 0,
        wrap_mode: 4,
        clamp_color: EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
        object_clamp: 1,
        reserved2: 0,
      }),
    );
    assert!(
      EmfPlusObjectData::ImageAttributes(EmfPlusImageAttributesObject {
        version: test_graphics_version(),
        reserved1: 0,
        wrap_mode: 0xFFFF_FFFF,
        clamp_color: EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
        object_clamp: EmfPlusObjectClamp::BitmapClamp.raw(),
        reserved2: 0,
      })
      .to_bytes()
      .is_err()
    );
    let invalid_attributes_record = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::ImageAttributes.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&test_graphics_version().value.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        data.extend_from_slice(&[1, 2, 3, 4]);
        data.extend_from_slice(&EmfPlusObjectClamp::BitmapClamp.raw().to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data
      },
    };
    assert!(invalid_attributes_record.parse_object_data().is_err());
    assert!(
      EmfPlusObjectData::ImageAttributes(EmfPlusImageAttributesObject {
        version: invalid_graphics_version(),
        reserved1: 0,
        wrap_mode: EmfPlusWrapMode::Clamp.raw(),
        clamp_color: EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
        object_clamp: EmfPlusObjectClamp::BitmapClamp.raw(),
        reserved2: 0,
      })
      .to_bytes()
      .is_err()
    );
    assert_object_data_roundtrip(
      EmfPlusObjectType::Pen,
      EmfPlusObjectData::Pen(EmfPlusPenObject {
        version: test_graphics_version(),
        pen_type: 0,
        pen_data_and_brush_object: test_pen_payload_bytes(),
      }),
    );
    let compatible_pen_version = EmfPlusObjectData::Pen(EmfPlusPenObject {
      version: invalid_graphics_version(),
      pen_type: 0,
      pen_data_and_brush_object: test_pen_payload_bytes(),
    });
    assert!(compatible_pen_version.to_bytes().is_ok());
    assert!(compatible_pen_version.validate_strict().is_err());
    assert!(
      EmfPlusObjectData::Pen(EmfPlusPenObject {
        version: test_graphics_version(),
        pen_type: 1,
        pen_data_and_brush_object: test_pen_payload_bytes(),
      })
      .to_bytes()
      .is_err()
    );
    let invalid_pen_type_record = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Pen.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&test_graphics_version().value.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&test_pen_payload_bytes());
        data
      },
    };
    assert!(invalid_pen_type_record.parse_object_data().is_err());
    assert_object_data_roundtrip(
      EmfPlusObjectType::Region,
      EmfPlusObjectData::Region(EmfPlusRegionObject {
        version: test_graphics_version(),
        region_node_count: 0,
        region_nodes: vec![0x02, 0x00, 0x00, 0x10],
      }),
    );
    assert!(
      EmfPlusObjectData::Region(EmfPlusRegionObject {
        version: invalid_graphics_version(),
        region_node_count: 0,
        region_nodes: vec![0x02, 0x00, 0x00, 0x10],
      })
      .to_bytes()
      .is_err()
    );
    assert_object_data_roundtrip(
      EmfPlusObjectType::StringFormat,
      EmfPlusObjectData::StringFormat(EmfPlusStringFormatObject {
        version: test_graphics_version(),
        string_format_flags: 0x0000_1001,
        language: 0x0409,
        string_alignment: 1,
        line_align: 2,
        digit_substitution: 1,
        digit_language: 0x0409,
        first_tab_offset: 4.0,
        hotkey_prefix: 1,
        leading_margin: 0.1,
        trailing_margin: 0.2,
        tracking: 1.03,
        trimming: 3,
        tab_stops: vec![8.0, 16.0],
        char_ranges: vec![EmfPlusCharacterRange {
          first: 1,
          length: 5,
        }],
        trailing_data: Vec::new(),
      }),
    );
    assert!(
      EmfPlusObjectData::StringFormat(EmfPlusStringFormatObject {
        version: invalid_graphics_version(),
        string_format_flags: 0,
        language: 0x0409,
        string_alignment: EmfPlusStringAlignment::Near.raw(),
        line_align: EmfPlusStringAlignment::Near.raw(),
        digit_substitution: EmfPlusStringDigitSubstitution::None.raw(),
        digit_language: 0x0409,
        first_tab_offset: 0.0,
        hotkey_prefix: EmfPlusHotkeyPrefix::None.raw(),
        leading_margin: 0.0,
        trailing_margin: 0.0,
        tracking: 1.0,
        trimming: EmfPlusStringTrimming::None.raw(),
        tab_stops: Vec::new(),
        char_ranges: Vec::new(),
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
  }

  #[test]
  fn emf_plus_brush_data_variants_roundtrip() {
    assert_brush_data_roundtrip(
      EmfPlusBrushType::SolidColor,
      EmfPlusBrushData::Solid(EmfPlusSolidBrushData {
        solid_color: EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
        trailing_data: Vec::new(),
      }),
    );
    assert!(
      EmfPlusBrushData::Solid(EmfPlusSolidBrushData {
        solid_color: EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
        trailing_data: vec![0xAA],
      })
      .to_bytes()
      .is_err()
    );

    let hatch = EmfPlusBrushData::Hatch(EmfPlusHatchBrushData {
      hatch_style: EmfPlusHatchStyle::DashedHorizontal.raw(),
      fore_color: EmfPlusArgb {
        blue: 10,
        green: 20,
        red: 30,
        alpha: 255,
      },
      back_color: EmfPlusArgb {
        blue: 40,
        green: 50,
        red: 60,
        alpha: 255,
      },
      trailing_data: Vec::new(),
    });
    assert_brush_data_roundtrip(EmfPlusBrushType::HatchFill, hatch.clone());
    let EmfPlusBrushData::Hatch(parsed_hatch) = hatch else {
      panic!("expected hatch brush data");
    };
    assert_eq!(
      parsed_hatch.hatch_style_kind(),
      Some(EmfPlusHatchStyle::DashedHorizontal)
    );
    let mut hatch_with_trailing_data = parsed_hatch.clone();
    hatch_with_trailing_data.trailing_data.push(0xAA);
    assert!(
      EmfPlusBrushData::Hatch(hatch_with_trailing_data)
        .to_bytes()
        .is_err()
    );
    assert!(
      EmfPlusBrushData::Hatch(EmfPlusHatchBrushData {
        hatch_style: 0xFFFF_FFFF,
        fore_color: EmfPlusArgb {
          blue: 0,
          green: 0,
          red: 0,
          alpha: 255,
        },
        back_color: EmfPlusArgb {
          blue: 255,
          green: 255,
          red: 255,
          alpha: 255,
        },
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let linear = EmfPlusBrushData::LinearGradient(EmfPlusLinearGradientBrushData {
      brush_data_flags: (EmfPlusBrushDataFlags::TRANSFORM
        | EmfPlusBrushDataFlags::IS_GAMMA_CORRECTED)
        .bits(),
      wrap_mode: EmfPlusWrapMode::TileFlipX.raw() as i32,
      rect: RectF {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
      },
      start_color: EmfPlusArgb {
        blue: 1,
        green: 1,
        red: 1,
        alpha: 255,
      },
      end_color: EmfPlusArgb {
        blue: 2,
        green: 2,
        red: 2,
        alpha: 255,
      },
      reserved1: 0x11,
      reserved2: 0x22,
      optional_data: EmfPlusLinearGradientBrushOptionalData {
        transform_matrix: Some(XForm {
          m11: 1.0,
          m12: 0.0,
          m21: 0.0,
          m22: 1.0,
          dx: 0.0,
          dy: 0.0,
        }),
        blend_pattern: None,
        trailing_data: Vec::new(),
      }
      .to_bytes()
      .unwrap(),
    });
    assert_brush_data_roundtrip(EmfPlusBrushType::LinearGradient, linear.clone());
    let EmfPlusBrushData::LinearGradient(parsed_linear) = linear else {
      panic!("expected linear gradient brush data");
    };
    assert!(
      parsed_linear
        .flags()
        .contains(EmfPlusBrushDataFlags::TRANSFORM)
    );
    assert_eq!(
      parsed_linear.wrap_mode_kind(),
      Some(EmfPlusWrapMode::TileFlipX)
    );
    assert!(parsed_linear.parse_optional_data().is_ok());
    let mut invalid_linear_optional = parsed_linear.parse_optional_data().unwrap();
    invalid_linear_optional.trailing_data.push(0xAA);
    assert!(invalid_linear_optional.to_bytes().is_err());
    assert!(
      EmfPlusBrushData::LinearGradient(EmfPlusLinearGradientBrushData {
        brush_data_flags: EmfPlusBrushDataFlags::TRANSFORM.bits(),
        wrap_mode: EmfPlusWrapMode::Tile.raw() as i32,
        rect: RectF {
          x: 0.0,
          y: 0.0,
          width: 1.0,
          height: 1.0,
        },
        start_color: EmfPlusArgb {
          blue: 0,
          green: 0,
          red: 0,
          alpha: 255,
        },
        end_color: EmfPlusArgb {
          blue: 255,
          green: 255,
          red: 255,
          alpha: 255,
        },
        reserved1: 0,
        reserved2: 0,
        optional_data: vec![0; 23],
      })
      .to_bytes()
      .is_err()
    );
    let mut invalid_linear_flags = parsed_linear.clone();
    invalid_linear_flags.brush_data_flags = 0x8000_0000;
    assert!(
      EmfPlusBrushData::LinearGradient(invalid_linear_flags.clone())
        .to_bytes()
        .is_err()
    );
    let mut invalid_linear_flags = parsed_linear.clone();
    invalid_linear_flags.brush_data_flags =
      (EmfPlusBrushDataFlags::TRANSFORM | EmfPlusBrushDataFlags::DO_NOT_TRANSFORM).bits();
    assert!(
      EmfPlusBrushData::LinearGradient(invalid_linear_flags)
        .to_bytes()
        .is_err()
    );
    let mut invalid_linear_flags = parsed_linear.clone();
    invalid_linear_flags.brush_data_flags =
      (EmfPlusBrushDataFlags::PRESET_COLORS | EmfPlusBrushDataFlags::BLEND_FACTORS_H).bits();
    invalid_linear_flags.optional_data = Vec::new();
    assert!(
      EmfPlusBrushData::LinearGradient(invalid_linear_flags)
        .to_bytes()
        .is_err()
    );
    let invalid_linear_brush = EmfPlusBrushObject {
      version: test_graphics_version(),
      brush_type: EmfPlusBrushType::LinearGradient.raw(),
      brush_data: {
        let mut data = EmfPlusBrushData::LinearGradient(parsed_linear.clone())
          .to_bytes()
          .unwrap();
        data[0..4].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
        data
      },
    };
    assert!(invalid_linear_brush.parse_brush_data().is_err());

    let path_gradient = EmfPlusBrushData::PathGradient(EmfPlusPathGradientBrushData {
      brush_data_flags: EmfPlusBrushDataFlags::empty().bits(),
      wrap_mode: EmfPlusWrapMode::Clamp.raw() as i32,
      center_color: EmfPlusArgb {
        blue: 3,
        green: 4,
        red: 5,
        alpha: 255,
      },
      center_point: PointF { x: 5.0, y: 6.0 },
      surrounding_colors: vec![
        EmfPlusArgb {
          blue: 7,
          green: 8,
          red: 9,
          alpha: 255,
        },
        EmfPlusArgb {
          blue: 10,
          green: 11,
          red: 12,
          alpha: 255,
        },
      ],
      boundary_and_optional_data: EmfPlusPathGradientBrushTailData {
        boundary_data: Some(EmfPlusBoundaryData::Points(EmfPlusBoundaryPointData {
          points: vec![PointF { x: 1.0, y: 2.0 }, PointF { x: 3.0, y: 4.0 }],
          trailing_data: Vec::new(),
        })),
        optional_data: EmfPlusPathGradientBrushOptionalData {
          transform_matrix: None,
          blend_pattern: None,
          focus_scale_data: None,
        },
        trailing_data: Vec::new(),
      }
      .to_bytes()
      .unwrap(),
    });
    assert_brush_data_roundtrip(EmfPlusBrushType::PathGradient, path_gradient.clone());
    let EmfPlusBrushData::PathGradient(parsed_path_gradient) = path_gradient else {
      panic!("expected path gradient brush data");
    };
    assert!(
      !parsed_path_gradient
        .flags()
        .contains(EmfPlusBrushDataFlags::PATH)
    );
    assert_eq!(
      parsed_path_gradient.wrap_mode_kind(),
      Some(EmfPlusWrapMode::Clamp)
    );
    assert!(parsed_path_gradient.parse_tail_data().is_ok());
    let invalid_path_tail = EmfPlusPathGradientBrushTailData {
      boundary_data: None,
      optional_data: EmfPlusPathGradientBrushOptionalData {
        transform_matrix: None,
        blend_pattern: None,
        focus_scale_data: None,
      },
      trailing_data: Vec::new(),
    };
    assert!(invalid_path_tail.to_bytes().is_err());
    let mut invalid_path_gradient_flags = parsed_path_gradient.clone();
    invalid_path_gradient_flags.brush_data_flags = EmfPlusBrushDataFlags::BLEND_FACTORS_V.bits();
    assert!(
      EmfPlusBrushData::PathGradient(invalid_path_gradient_flags)
        .to_bytes()
        .is_err()
    );
    let mut invalid_path_gradient_flags = parsed_path_gradient.clone();
    invalid_path_gradient_flags.brush_data_flags =
      (EmfPlusBrushDataFlags::PRESET_COLORS | EmfPlusBrushDataFlags::BLEND_FACTORS_H).bits();
    assert!(
      EmfPlusBrushData::PathGradient(invalid_path_gradient_flags)
        .to_bytes()
        .is_err()
    );
    assert!(
      EmfPlusBrushData::PathGradient(EmfPlusPathGradientBrushData {
        brush_data_flags: EmfPlusBrushDataFlags::empty().bits(),
        wrap_mode: EmfPlusWrapMode::Clamp.raw() as i32,
        center_color: EmfPlusArgb {
          blue: 0,
          green: 0,
          red: 0,
          alpha: 255,
        },
        center_point: PointF { x: 0.0, y: 0.0 },
        surrounding_colors: Vec::new(),
        boundary_and_optional_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let texture = EmfPlusBrushData::Texture(EmfPlusTextureBrushData {
      brush_data_flags: EmfPlusBrushDataFlags::DO_NOT_TRANSFORM.bits(),
      wrap_mode: EmfPlusWrapMode::TileFlipXY.raw() as i32,
      optional_data: Vec::new(),
    });
    assert_brush_data_roundtrip(EmfPlusBrushType::TextureFill, texture.clone());
    let EmfPlusBrushData::Texture(parsed_texture) = texture else {
      panic!("expected texture brush data");
    };
    assert!(
      parsed_texture
        .flags()
        .contains(EmfPlusBrushDataFlags::DO_NOT_TRANSFORM)
    );
    assert_eq!(
      parsed_texture.wrap_mode_kind(),
      Some(EmfPlusWrapMode::TileFlipXY)
    );
    let mut invalid_texture_flags = parsed_texture.clone();
    invalid_texture_flags.brush_data_flags = EmfPlusBrushDataFlags::PRESET_COLORS.bits();
    assert!(
      EmfPlusBrushData::Texture(invalid_texture_flags)
        .to_bytes()
        .is_err()
    );
    let invalid_texture_trailing_data = EmfPlusBrushData::Texture(EmfPlusTextureBrushData {
      brush_data_flags: EmfPlusBrushDataFlags::DO_NOT_TRANSFORM.bits(),
      wrap_mode: EmfPlusWrapMode::TileFlipXY.raw() as i32,
      optional_data: vec![0x33, 0x44],
    });
    assert!(invalid_texture_trailing_data.to_bytes().is_err());
    let texture_transform = XForm {
      m11: 1.0,
      m12: 0.0,
      m21: 0.0,
      m22: 1.0,
      dx: 2.0,
      dy: 3.0,
    };
    let texture_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Unknown.raw(),
      image_data: vec![0x55, 0x66],
    };
    let texture_optional = EmfPlusTextureBrushOptionalData {
      transform_matrix: Some(texture_transform),
      image_object: Some(texture_image.clone()),
      trailing_data: Vec::new(),
    };
    let texture_with_optional = EmfPlusBrushData::Texture(EmfPlusTextureBrushData {
      brush_data_flags: EmfPlusBrushDataFlags::TRANSFORM.bits(),
      wrap_mode: EmfPlusWrapMode::Tile.raw() as i32,
      optional_data: texture_optional.to_bytes().unwrap(),
    });
    assert_brush_data_roundtrip(EmfPlusBrushType::TextureFill, texture_with_optional.clone());
    let EmfPlusBrushData::Texture(parsed_texture) = texture_with_optional else {
      panic!("expected texture brush data");
    };
    let parsed_optional = parsed_texture.parse_optional_data().unwrap();
    assert_eq!(parsed_optional.transform_matrix, Some(texture_transform));
    assert_eq!(parsed_optional.image_object, Some(texture_image));
    assert!(parsed_optional.trailing_data.is_empty());
    let mut invalid_texture_optional = parsed_optional.clone();
    invalid_texture_optional.trailing_data.push(0xAA);
    assert!(invalid_texture_optional.to_bytes().is_err());
    assert!(
      EmfPlusBrushData::Texture(EmfPlusTextureBrushData {
        brush_data_flags: EmfPlusBrushDataFlags::TRANSFORM.bits(),
        wrap_mode: EmfPlusWrapMode::Tile.raw() as i32,
        optional_data: vec![0; 23],
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusBrushObject {
        version: test_graphics_version(),
        brush_type: EmfPlusBrushType::TextureFill.raw(),
        brush_data: {
          let mut data = Vec::new();
          data.extend_from_slice(&EmfPlusBrushDataFlags::TRANSFORM.bits().to_le_bytes());
          data.extend_from_slice(&(EmfPlusWrapMode::Tile.raw() as i32).to_le_bytes());
          data.extend_from_slice(&[0; 23]);
          data
        },
      }
      .parse_brush_data()
      .is_err()
    );
    assert!(
      EmfPlusBrushObject {
        version: test_graphics_version(),
        brush_type: EmfPlusBrushType::TextureFill.raw(),
        brush_data: {
          let mut data = Vec::new();
          data.extend_from_slice(&0_u32.to_le_bytes());
          data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
          data
        },
      }
      .parse_brush_data()
      .is_err()
    );
  }

  #[test]
  fn emf_plus_path_object_point_types_roundtrip() {
    assert_object_data_roundtrip(
      EmfPlusObjectType::Path,
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: EmfPlusRecordFlags::COMPRESSED.bits() as u32,
        points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 }]),
        point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00, 0x81])),
        alignment_padding: vec![0, 0],
      }),
    );
    assert!(
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: EmfPlusRecordFlags::COMPRESSED.bits() as u32,
        points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 },]),
        point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00, 0x81])),
        alignment_padding: vec![0],
      })
      .to_bytes()
      .is_err()
    );
    assert_object_data_roundtrip(
      EmfPlusObjectType::Path,
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: EmfPlusRecordFlags::RELATIVE_POSITION.bits() as u32,
        points: EmfPlusPointData::Relative(vec![
          EmfPlusPointR { x: 1, y: 2 },
          EmfPlusPointR { x: 3, y: 4 },
        ]),
        point_types: EmfPlusPathPointTypes::Rle(vec![
          EmfPlusPathPointTypeRle::new(false, 2, path_point_type(0x01)).unwrap(),
        ]),
        alignment_padding: vec![0, 0],
      }),
    );
    let small_relative = EmfPlusPointR { x: 63, y: -64 };
    assert_eq!(
      small_relative.x_integer7(),
      Some(EmfPlusInteger7 { value: 63 })
    );
    assert_eq!(
      small_relative.y_integer7(),
      Some(EmfPlusInteger7 { value: -64 })
    );
    let large_relative = EmfPlusPointR { x: 130, y: -130 };
    assert_eq!(large_relative.x_integer7(), None);
    assert_eq!(large_relative.x_integer15().value, 130);
    assert_eq!(large_relative.y_integer15().value, -130);

    assert!(
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: 0,
        points: EmfPlusPointData::Float(vec![PointF { x: 1.0, y: 2.0 }]),
        point_types: EmfPlusPathPointTypes::Values(vec![EmfPlusPathPointTypeValue { value: 0x02 }]),
        alignment_padding: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: 0,
        points: EmfPlusPointData::Float(vec![PointF { x: 1.0, y: 2.0 }]),
        point_types: EmfPlusPathPointTypes::Values(vec![EmfPlusPathPointTypeValue { value: 0x40 }]),
        alignment_padding: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    let path_with_reserved_flags = EmfPlusObjectData::Path(EmfPlusPathObject {
      version: test_graphics_version(),
      path_point_flags: 0x0000_0001,
      points: EmfPlusPointData::Float(vec![PointF { x: 1.0, y: 2.0 }]),
      point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00])),
      alignment_padding: vec![0, 0, 0],
    });
    let compatible_bytes = path_with_reserved_flags.to_bytes().unwrap();
    let record = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Path.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: compatible_bytes.clone(),
    };
    let reparsed = record.parse_object_data().unwrap();
    assert_eq!(reparsed.to_bytes().unwrap(), compatible_bytes);
    assert!(reparsed.validate_strict().is_err());
    assert!(
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: 0,
        points: EmfPlusPointData::Compressed(vec![PointS { x: 1, y: 2 }]),
        point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00])),
        alignment_padding: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: 0,
        points: EmfPlusPointData::Float(vec![PointF { x: 1.0, y: 2.0 }]),
        point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00])),
        alignment_padding: vec![0, 0, 0, 0],
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusObjectData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: EmfPlusRecordFlags::RELATIVE_POSITION.bits() as u32,
        points: EmfPlusPointData::Relative(vec![EmfPlusPointR { x: 1, y: 2 }]),
        point_types: EmfPlusPathPointTypes::Rle(vec![EmfPlusPathPointTypeRle {
          control: 0x01,
          point_type: path_point_type(0x00),
        }]),
        alignment_padding: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );

    let invalid_path_record = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Path.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&test_graphics_version().value.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&1.0_f32.to_le_bytes());
        data.extend_from_slice(&2.0_f32.to_le_bytes());
        data.push(0x02);
        data.extend_from_slice(&[0, 0, 0]);
        data
      },
    };
    assert!(invalid_path_record.parse_object_data().is_err());

    let invalid_path_padding_record = EmfPlusObjectRecordData {
      object_id: 1,
      object_type_raw: EmfPlusObjectType::Path.raw() as u8,
      continues: false,
      total_object_size: None,
      object_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&test_graphics_version().value.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&1.0_f32.to_le_bytes());
        data.extend_from_slice(&2.0_f32.to_le_bytes());
        data.push(0x00);
        data.extend_from_slice(&[0, 0]);
        data
      },
    };
    assert!(invalid_path_padding_record.parse_object_data().is_err());
  }

  #[test]
  fn emf_plus_graphics_object_enum_accessors_map_spec_values() {
    let brush = EmfPlusBrushObject {
      version: test_graphics_version(),
      brush_type: EmfPlusBrushType::LinearGradient.raw(),
      brush_data: Vec::new(),
    };
    assert_eq!(brush.brush_kind(), Some(EmfPlusBrushType::LinearGradient));

    let cap = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::AdjustableArrow.raw(),
      custom_line_cap_data: Vec::new(),
    };
    assert_eq!(
      cap.cap_data_type(),
      Some(EmfPlusCustomLineCapDataType::AdjustableArrow)
    );

    let font = EmfPlusFontObject {
      version: test_graphics_version(),
      em_size: 9.0,
      size_unit: EmfPlusUnitType::Point.raw(),
      font_style_flags: (EmfPlusFontStyleFlags::BOLD | EmfPlusFontStyleFlags::ITALIC).bits() as i32,
      reserved: 0,
      family_name: SdkString::raw(Vec::new(), SdkEncoding::Utf16Le),
      padding: Vec::new(),
    };
    assert_eq!(font.size_unit_kind(), Some(EmfPlusUnitType::Point));
    assert!(font.font_style().contains(EmfPlusFontStyleFlags::BOLD));
    assert!(font.font_style().contains(EmfPlusFontStyleFlags::ITALIC));
    let mut invalid_font = font.clone();
    invalid_font.font_style_flags = 0x10;
    assert!(EmfPlusObjectData::Font(invalid_font).to_bytes().is_err());
    let mut invalid_font_bytes = EmfPlusObjectData::Font(font.clone()).to_bytes().unwrap();
    invalid_font_bytes[12..16].copy_from_slice(&0x10_u32.to_le_bytes());
    assert!(
      EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::Font.raw() as u8,
        continues: false,
        total_object_size: None,
        object_data: invalid_font_bytes,
      }
      .parse_object_data()
      .is_err()
    );

    let image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Metafile.raw(),
      image_data: Vec::new(),
    };
    assert_eq!(
      image.image_data_type(),
      Some(EmfPlusImageDataType::Metafile)
    );

    let attributes = EmfPlusImageAttributesObject {
      version: test_graphics_version(),
      reserved1: 0,
      wrap_mode: EmfPlusWrapMode::Clamp.raw(),
      clamp_color: EmfPlusArgb {
        blue: 0,
        green: 0,
        red: 0,
        alpha: 0,
      },
      object_clamp: EmfPlusObjectClamp::BitmapClamp.raw(),
      reserved2: 0,
    };
    assert_eq!(attributes.wrap_mode_kind(), Some(EmfPlusWrapMode::Clamp));
    assert_eq!(attributes.sdk_size(), 24);
    assert_eq!(
      attributes.object_clamp_kind(),
      Some(EmfPlusObjectClamp::BitmapClamp)
    );

    let path_type = EmfPlusPathPointTypeRle::new(true, 2, path_point_type(0x83)).unwrap();
    assert!(path_type.bezier());
    assert!(path_type.marker_bit_set());
    assert_eq!(path_type.control, 0xC2);
    assert_eq!(path_type.run_count(), 2);
    assert_eq!(
      path_type.path_point_type(),
      Some(EmfPlusPathPointType::Bezier)
    );
    assert!(
      path_type
        .path_point_flags()
        .contains(EmfPlusPathPointTypeFlags::CLOSE_SUBPATH)
    );
    assert!(EmfPlusPathPointTypeRle::new(false, 0, path_point_type(0x00)).is_err());
    assert!(EmfPlusPathPointTypeRle::new(false, 64, path_point_type(0x00)).is_err());

    let string_format = EmfPlusStringFormatObject {
      version: test_graphics_version(),
      string_format_flags: (EmfPlusStringFormatFlags::DIRECTION_RIGHT_TO_LEFT
        | EmfPlusStringFormatFlags::NO_WRAP)
        .bits(),
      language: 0x0409,
      string_alignment: EmfPlusStringAlignment::Center.raw(),
      line_align: EmfPlusStringAlignment::Far.raw(),
      digit_substitution: EmfPlusStringDigitSubstitution::National.raw(),
      digit_language: 0x0409,
      first_tab_offset: 0.0,
      hotkey_prefix: EmfPlusHotkeyPrefix::Show.raw(),
      leading_margin: 0.0,
      trailing_margin: 0.0,
      tracking: 1.0,
      trimming: EmfPlusStringTrimming::EllipsisWord.raw(),
      tab_stops: vec![4.0],
      char_ranges: vec![EmfPlusCharacterRange {
        first: 1,
        length: 3,
      }],
      trailing_data: Vec::new(),
    };
    assert!(
      string_format
        .flags()
        .contains(EmfPlusStringFormatFlags::DIRECTION_RIGHT_TO_LEFT)
    );
    assert!(
      string_format
        .flags()
        .contains(EmfPlusStringFormatFlags::NO_WRAP)
    );
    assert_eq!(
      string_format.string_alignment_kind(),
      Some(EmfPlusStringAlignment::Center)
    );
    assert_eq!(
      string_format.line_align_kind(),
      Some(EmfPlusStringAlignment::Far)
    );
    assert_eq!(
      string_format.digit_substitution_kind(),
      Some(EmfPlusStringDigitSubstitution::National)
    );
    assert_eq!(
      string_format.hotkey_prefix_kind(),
      Some(EmfPlusHotkeyPrefix::Show)
    );
    assert_eq!(
      string_format.trimming_kind(),
      Some(EmfPlusStringTrimming::EllipsisWord)
    );
    assert_eq!(
      string_format.language_identifier().primary_language_id(),
      0x0009
    );
    assert_eq!(string_format.language_identifier().sub_language_id(), 0x01);
    assert_eq!(string_format.language_identifier().language_id(), 0x0409);
    assert_eq!(string_format.language_identifier().high_word(), 0);
    assert!(string_format.language_identifier().is_word_sized());
    assert!(
      !string_format
        .language_identifier()
        .is_vendor_primary_language_id()
    );
    assert!(
      !string_format
        .language_identifier()
        .is_vendor_sub_language_id()
    );
    let language_identifier = EmfPlusLanguageIdentifier::from_parts(0x0009, 0x01).unwrap();
    assert_eq!(language_identifier.raw, 0x0409);
    assert_eq!(language_identifier.primary_language_id(), 0x0009);
    assert_eq!(language_identifier.sub_language_id(), 0x01);
    let vendor_language_identifier = EmfPlusLanguageIdentifier::from_parts(0x0200, 0x20).unwrap();
    assert_eq!(vendor_language_identifier.raw, 0x8200);
    assert!(vendor_language_identifier.is_word_sized());
    assert!(vendor_language_identifier.is_vendor_primary_language_id());
    assert!(vendor_language_identifier.is_vendor_sub_language_id());
    assert!(EmfPlusLanguageIdentifier::from_parts(0x0400, 0x01).is_err());
    assert!(EmfPlusLanguageIdentifier::from_parts(0x0009, 0x40).is_err());
    assert_eq!(
      string_format
        .digit_language_identifier()
        .primary_language_id(),
      0x0009
    );
    assert_eq!(string_format.tab_stops(), &[4.0]);
    assert_eq!(
      string_format.char_ranges(),
      &[EmfPlusCharacterRange {
        first: 1,
        length: 3,
      }]
    );
    assert_eq!(string_format.string_format_data().tab_stops, [4.0]);
    assert_eq!(
      string_format.string_format_data().char_ranges,
      [EmfPlusCharacterRange {
        first: 1,
        length: 3,
      }]
    );

    let mut invalid_string_format = string_format.clone();
    invalid_string_format.string_format_flags = 0x0000_0080;
    assert!(
      EmfPlusObjectData::StringFormat(invalid_string_format)
        .to_bytes()
        .is_err()
    );
    let mut invalid_string_format_bytes = EmfPlusObjectData::StringFormat(string_format.clone())
      .to_bytes()
      .unwrap();
    invalid_string_format_bytes[4..8].copy_from_slice(&0x0000_0080_u32.to_le_bytes());
    assert!(
      EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::StringFormat.raw() as u8,
        continues: false,
        total_object_size: None,
        object_data: invalid_string_format_bytes,
      }
      .parse_object_data()
      .is_err()
    );

    let mut invalid_string_format = string_format.clone();
    invalid_string_format.language = 0x0001_0409;
    assert_eq!(invalid_string_format.language_identifier().high_word(), 1);
    assert!(!invalid_string_format.language_identifier().is_word_sized());
    assert!(
      EmfPlusObjectData::StringFormat(invalid_string_format)
        .to_bytes()
        .is_ok()
    );

    let mut invalid_string_format = string_format.clone();
    invalid_string_format.digit_language = 0x0001_0409;
    assert!(
      EmfPlusObjectData::StringFormat(invalid_string_format)
        .to_bytes()
        .is_ok()
    );

    let mut invalid_string_format_bytes = EmfPlusObjectData::StringFormat(string_format.clone())
      .to_bytes()
      .unwrap();
    invalid_string_format_bytes[8..12].copy_from_slice(&0x0001_0409_u32.to_le_bytes());
    assert!(matches!(
        EmfPlusObjectRecordData {
            object_id: 1,
            object_type_raw: EmfPlusObjectType::StringFormat.raw() as u8,
            continues: false,
            total_object_size: None,
            object_data: invalid_string_format_bytes,
        }
        .parse_object_data(),
        Ok(EmfPlusObjectData::StringFormat(value)) if value.language == 0x0001_0409
    ));

    let mut invalid_string_format_bytes = EmfPlusObjectData::StringFormat(string_format.clone())
      .to_bytes()
      .unwrap();
    invalid_string_format_bytes[24..28].copy_from_slice(&0x0001_0409_u32.to_le_bytes());
    assert!(matches!(
        EmfPlusObjectRecordData {
            object_id: 1,
            object_type_raw: EmfPlusObjectType::StringFormat.raw() as u8,
            continues: false,
            total_object_size: None,
            object_data: invalid_string_format_bytes,
        }
        .parse_object_data(),
        Ok(EmfPlusObjectData::StringFormat(value)) if value.digit_language == 0x0001_0409
    ));

    let mut invalid_string_format = string_format.clone();
    invalid_string_format.trailing_data = vec![0xAA];
    assert!(
      EmfPlusObjectData::StringFormat(invalid_string_format)
        .to_bytes()
        .is_err()
    );
    let mut invalid_string_format_bytes = EmfPlusObjectData::StringFormat(string_format.clone())
      .to_bytes()
      .unwrap();
    invalid_string_format_bytes.push(0xAA);
    assert!(
      EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::StringFormat.raw() as u8,
        continues: false,
        total_object_size: None,
        object_data: invalid_string_format_bytes,
      }
      .parse_object_data()
      .is_err()
    );

    let mut invalid_string_format = string_format.clone();
    invalid_string_format.line_align = 0xFFFF_FFFF;
    assert!(
      EmfPlusObjectData::StringFormat(invalid_string_format)
        .to_bytes()
        .is_err()
    );

    let mut signed_string_format = string_format.clone();
    signed_string_format.char_ranges[0].first = -1;
    signed_string_format.char_ranges[0].length = -1;
    assert!(
      EmfPlusObjectData::StringFormat(signed_string_format)
        .to_bytes()
        .is_ok()
    );

    let mut invalid_string_format_bytes = EmfPlusObjectData::StringFormat(string_format.clone())
      .to_bytes()
      .unwrap();
    invalid_string_format_bytes[16..20].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(
      EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::StringFormat.raw() as u8,
        continues: false,
        total_object_size: None,
        object_data: invalid_string_format_bytes,
      }
      .parse_object_data()
      .is_err()
    );

    let mut invalid_string_format_bytes = EmfPlusObjectData::StringFormat(string_format)
      .to_bytes()
      .unwrap();
    invalid_string_format_bytes[56..60].copy_from_slice(&(-1_i32).to_le_bytes());
    assert!(
      EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::StringFormat.raw() as u8,
        continues: false,
        total_object_size: None,
        object_data: invalid_string_format_bytes,
      }
      .parse_object_data()
      .is_err()
    );
  }

  #[test]
  fn emf_plus_image_data_bitmap_and_metafile_roundtrip() {
    let bitmap = EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
      width: 2,
      height: 2,
      stride: 8,
      pixel_format: EmfPlusPixelFormat::Format32bppArgb.raw(),
      bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
      bitmap_data: vec![0xAA; 16],
    });
    let image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: bitmap.to_bytes().unwrap(),
    };
    let parsed = image.parse_image_data().unwrap();
    assert_eq!(parsed, bitmap);
    assert_eq!(parsed.to_bytes().unwrap(), image.image_data);
    let EmfPlusImageData::Bitmap(parsed_bitmap) = parsed else {
      panic!("expected bitmap image data");
    };
    assert_eq!(
      parsed_bitmap.pixel_format_kind(),
      Some(EmfPlusPixelFormat::Format32bppArgb)
    );
    assert_eq!(
      parsed_bitmap.bitmap_data_type_kind(),
      Some(EmfPlusBitmapDataType::Pixel)
    );
    assert_eq!(parsed_bitmap.pixel_format_index(), 0x0A);
    assert_eq!(parsed_bitmap.bits_per_pixel(), 32);
    assert!(parsed_bitmap.is_gdi_pixel_format());
    assert!(parsed_bitmap.has_alpha_pixel_format());
    assert!(parsed_bitmap.is_canonical_pixel_format());
    assert!(!parsed_bitmap.is_pre_multiplied_alpha_pixel_format());
    let pixel_format_value = parsed_bitmap.pixel_format_value();
    assert_eq!(
      pixel_format_value.kind(),
      Some(EmfPlusPixelFormat::Format32bppArgb)
    );
    assert_eq!(pixel_format_value.index(), 0x0A);
    assert_eq!(pixel_format_value.bits_per_pixel(), 32);
    assert!(pixel_format_value.is_gdi());
    assert!(pixel_format_value.has_alpha());
    assert!(pixel_format_value.is_canonical());
    assert_eq!(pixel_format_value.reserved_bits(), 0);
    assert_eq!(
      EmfPlusPixelFormat::Format32bppArgb.pixel_format_index(),
      0x0A
    );
    assert_eq!(EmfPlusPixelFormat::Format32bppArgb.bits_per_pixel(), 32);
    assert!(EmfPlusPixelFormat::Format32bppArgb.is_gdi());
    assert!(EmfPlusPixelFormat::Format32bppArgb.has_alpha());
    assert!(EmfPlusPixelFormat::Format32bppArgb.is_canonical());
    assert!(!EmfPlusPixelFormat::Format32bppArgb.is_pre_multiplied_alpha());
    assert_eq!(
      parsed_bitmap.parse_bitmap_data().unwrap(),
      EmfPlusBitmapPayload::Pixel(EmfPlusBitmapDataObject {
        palette: None,
        pixel_data: vec![0xAA; 16],
      })
    );

    let invalid_pixel_bitmap = EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
      width: 2,
      height: 2,
      stride: 6,
      pixel_format: EmfPlusPixelFormat::Format32bppArgb.raw(),
      bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
      bitmap_data: vec![0xAA; 16],
    });
    assert!(invalid_pixel_bitmap.to_bytes().is_err());

    let invalid_pixel_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&6_i32.to_le_bytes());
        data.extend_from_slice(&EmfPlusPixelFormat::Format32bppArgb.raw().to_le_bytes());
        data.extend_from_slice(&EmfPlusBitmapDataType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&[0xAA; 16]);
        data
      },
    };
    assert!(invalid_pixel_image.parse_image_data().is_err());
    let too_small_stride_bitmap = EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
      width: 2,
      height: 2,
      stride: 4,
      pixel_format: EmfPlusPixelFormat::Format32bppArgb.raw(),
      bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
      bitmap_data: vec![0xAA; 8],
    });
    assert!(too_small_stride_bitmap.to_bytes().is_err());
    let too_small_stride_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&4_i32.to_le_bytes());
        data.extend_from_slice(&EmfPlusPixelFormat::Format32bppArgb.raw().to_le_bytes());
        data.extend_from_slice(&EmfPlusBitmapDataType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&[0xAA; 8]);
        data
      },
    };
    assert!(too_small_stride_image.parse_image_data().is_err());
    assert!(
      EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
        width: 2,
        height: 2,
        stride: 8,
        pixel_format: EmfPlusPixelFormat::Format32bppArgb.raw(),
        bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
        bitmap_data: vec![0xAA; 15],
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
        width: 0,
        height: 2,
        stride: 8,
        pixel_format: EmfPlusPixelFormat::Format32bppArgb.raw(),
        bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
        bitmap_data: vec![0xAA; 16],
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
        width: 2,
        height: 2,
        stride: 8,
        pixel_format: 0xFFFF_FFFF,
        bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
        bitmap_data: vec![0xAA; 16],
      })
      .to_bytes()
      .is_err()
    );
    let invalid_pixel_format_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&8_i32.to_le_bytes());
        data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        data.extend_from_slice(&EmfPlusBitmapDataType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&[0xAA; 16]);
        data
      },
    };
    assert!(invalid_pixel_format_image.parse_image_data().is_err());
    let reserved_pixel_format = EmfPlusPixelFormat::Format32bppArgb.raw() | 0x0040_0000;
    let reserved_pixel_format_bitmap = EmfPlusBitmapObject {
      width: 2,
      height: 2,
      stride: 8,
      pixel_format: reserved_pixel_format,
      bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
      bitmap_data: vec![0xAA; 16],
    };
    assert!(
      EmfPlusImageData::Bitmap(reserved_pixel_format_bitmap.clone())
        .to_bytes()
        .is_ok()
    );
    assert_eq!(
      reserved_pixel_format_bitmap.pixel_format_kind(),
      Some(EmfPlusPixelFormat::Format32bppArgb)
    );
    assert_eq!(
      reserved_pixel_format_bitmap
        .pixel_format_value()
        .reserved_bits(),
      0x0040_0000
    );
    let reserved_pixel_format_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&8_i32.to_le_bytes());
        data.extend_from_slice(&reserved_pixel_format.to_le_bytes());
        data.extend_from_slice(&EmfPlusBitmapDataType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&[0xAA; 16]);
        data
      },
    };
    let EmfPlusImageData::Bitmap(parsed_reserved_pixel_format_image) =
      reserved_pixel_format_image.parse_image_data().unwrap()
    else {
      panic!("expected bitmap image data");
    };
    assert_eq!(
      parsed_reserved_pixel_format_image.pixel_format,
      reserved_pixel_format
    );
    assert_eq!(
      parsed_reserved_pixel_format_image.pixel_format_kind(),
      Some(EmfPlusPixelFormat::Format32bppArgb)
    );
    assert!(
      EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
        width: 2,
        height: 2,
        stride: 8,
        pixel_format: EmfPlusPixelFormat::Format32bppArgb.raw(),
        bitmap_data_type: 0xFFFF_FFFF,
        bitmap_data: vec![0xAA; 16],
      })
      .to_bytes()
      .is_err()
    );
    let invalid_bitmap_type_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&2_i32.to_le_bytes());
        data.extend_from_slice(&8_i32.to_le_bytes());
        data.extend_from_slice(&EmfPlusPixelFormat::Format32bppArgb.raw().to_le_bytes());
        data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        data.extend_from_slice(&[0xAA; 16]);
        data
      },
    };
    assert!(invalid_bitmap_type_image.parse_image_data().is_err());
    assert!(
      EmfPlusImageObject {
        version: test_graphics_version(),
        image_type: 0xFFFF_FFFF,
        image_data: Vec::new(),
      }
      .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
      .is_err()
    );

    let compressed_bitmap = EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
      width: 2,
      height: 2,
      stride: 6,
      pixel_format: EmfPlusPixelFormat::Undefined.raw(),
      bitmap_data_type: EmfPlusBitmapDataType::Compressed.raw(),
      bitmap_data: vec![0xAA; 16],
    });
    assert!(compressed_bitmap.to_bytes().is_ok());

    let indexed_payload = EmfPlusBitmapDataObject {
      palette: Some(EmfPlusPalette {
        palette_style_flags: EmfPlusPaletteStyleFlags::HAS_ALPHA.bits(),
        entries: vec![
          EmfPlusArgb {
            blue: 0,
            green: 0,
            red: 0,
            alpha: 0xFF,
          },
          EmfPlusArgb {
            blue: 0x33,
            green: 0x22,
            red: 0x11,
            alpha: 0x80,
          },
        ],
        trailing_data: Vec::new(),
      }),
      pixel_data: vec![0, 1, 0, 1, 0, 1, 0, 1],
    };
    let indexed_bitmap = EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
      width: 2,
      height: 2,
      stride: 4,
      pixel_format: EmfPlusPixelFormat::Format8bppIndexed.raw(),
      bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
      bitmap_data: indexed_payload.to_bytes().unwrap(),
    });
    let indexed_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Bitmap.raw(),
      image_data: indexed_bitmap.to_bytes().unwrap(),
    };
    let EmfPlusImageData::Bitmap(parsed_indexed_bitmap) = indexed_image.parse_image_data().unwrap()
    else {
      panic!("expected indexed bitmap image data");
    };
    assert_eq!(
      parsed_indexed_bitmap.parse_bitmap_data().unwrap(),
      EmfPlusBitmapPayload::Pixel(indexed_payload)
    );
    assert!(
      EmfPlusImageData::Bitmap(EmfPlusBitmapObject {
        width: 2,
        height: 2,
        stride: 4,
        pixel_format: EmfPlusPixelFormat::Format8bppIndexed.raw(),
        bitmap_data_type: EmfPlusBitmapDataType::Pixel.raw(),
        bitmap_data: vec![0, 1, 0, 1, 0, 1, 0, 1],
      })
      .to_bytes()
      .is_err()
    );
    assert!(
      EmfPlusBitmapDataObject {
        palette: Some(EmfPlusPalette {
          palette_style_flags: 0,
          entries: Vec::new(),
          trailing_data: vec![0xFF],
        }),
        pixel_data: Vec::new(),
      }
      .to_bytes()
      .is_err()
    );

    let metafile = EmfPlusImageData::Metafile(EmfPlusMetafileObject {
      metafile_type: EmfPlusMetafileDataType::EmfPlusDual.raw(),
      metafile_data: vec![1, 2, 3, 4],
      trailing_data: Vec::new(),
    });
    let image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Metafile.raw(),
      image_data: metafile.to_bytes().unwrap(),
    };
    let parsed = image.parse_image_data().unwrap();
    assert_eq!(parsed, metafile);
    assert_eq!(parsed.to_bytes().unwrap(), image.image_data);
    let EmfPlusImageData::Metafile(parsed_metafile) = parsed else {
      panic!("expected metafile image data");
    };
    assert_eq!(
      parsed_metafile.metafile_data_type_kind(),
      Some(EmfPlusMetafileDataType::EmfPlusDual)
    );
    assert!(
      EmfPlusImageData::Metafile(EmfPlusMetafileObject {
        metafile_type: 0xFFFF_FFFF,
        metafile_data: Vec::new(),
        trailing_data: Vec::new(),
      })
      .to_bytes()
      .is_err()
    );
    let metafile_with_trailing_data = EmfPlusMetafileObject {
      metafile_type: EmfPlusMetafileDataType::EmfPlusDual.raw(),
      metafile_data: Vec::new(),
      trailing_data: vec![0xEE],
    };
    assert_eq!(
      EmfPlusImageData::Metafile(metafile_with_trailing_data.clone())
        .to_bytes()
        .unwrap()
        .len(),
      9
    );
    assert!(metafile_with_trailing_data.validate_strict().is_err());
    let invalid_metafile_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Metafile.raw(),
      image_data: {
        let mut data = Vec::new();
        data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data
      },
    };
    assert!(invalid_metafile_image.parse_image_data().is_err());
    let invalid_metafile_image = EmfPlusImageObject {
      version: test_graphics_version(),
      image_type: EmfPlusImageDataType::Metafile.raw(),
      image_data: {
        let mut data = metafile.to_bytes().unwrap();
        data.push(0xEE);
        data
      },
    };
    assert!(invalid_metafile_image.parse_image_data().is_ok());
    assert!(validate_image_object_strict(&invalid_metafile_image).is_err());
  }

  #[test]
  fn emf_plus_typed_object_payload_helpers_roundtrip() {
    let brush_data = EmfPlusBrushData::Solid(EmfPlusSolidBrushData {
      solid_color: EmfPlusArgb {
        blue: 1,
        green: 2,
        red: 3,
        alpha: 0xFF,
      },
      trailing_data: Vec::new(),
    });
    let brush_object =
      EmfPlusBrushObject::from_typed_data(test_graphics_version(), &brush_data).unwrap();
    assert_eq!(
      brush_object.brush_kind(),
      Some(EmfPlusBrushType::SolidColor)
    );
    assert_eq!(brush_object.parse_brush_data().unwrap(), brush_data);

    let mut object_record =
      EmfPlusObjectRecordData::from_typed_data(3, &EmfPlusObjectData::Brush(brush_object.clone()))
        .unwrap();
    assert_eq!(object_record.object_type(), Some(EmfPlusObjectType::Brush));
    assert_eq!(
      object_record.parse_object_data().unwrap(),
      EmfPlusObjectData::Brush(brush_object.clone())
    );

    let cap_data = EmfPlusCustomLineCapData::Default(EmfPlusCustomLineCapDefaultData {
      custom_line_cap_data_flags: 0,
      base_cap: EmfPlusLineCapType::Flat.raw() as u32,
      base_inset: 0.0,
      stroke_start_cap: EmfPlusLineCapType::Flat.raw() as u32,
      stroke_end_cap: EmfPlusLineCapType::Round.raw() as u32,
      stroke_join: EmfPlusLineJoinType::Miter.raw() as u32,
      stroke_miter_limit: 10.0,
      width_scale: 1.0,
      fill_hot_spot: PointF { x: 0.0, y: 0.0 },
      stroke_hot_spot: PointF { x: 0.0, y: 0.0 },
      optional_data: Vec::new(),
    });
    let cap_object =
      EmfPlusCustomLineCapObject::from_typed_data(test_graphics_version(), &cap_data).unwrap();
    assert_eq!(
      cap_object.cap_data_type(),
      Some(EmfPlusCustomLineCapDataType::Default)
    );
    assert_eq!(cap_object.parse_cap_data().unwrap(), cap_data);
    object_record
      .set_typed_data(&EmfPlusObjectData::CustomLineCap(cap_object.clone()))
      .unwrap();
    assert_eq!(
      object_record.parse_object_data().unwrap(),
      EmfPlusObjectData::CustomLineCap(cap_object)
    );

    let bitmap_payload = EmfPlusBitmapPayload::Compressed(EmfPlusCompressedImageObject {
      compressed_image_data: vec![0xAA, 0xBB, 0xCC],
    });
    let bitmap_object = EmfPlusBitmapObject::from_typed_payload(
      2,
      2,
      6,
      EmfPlusPixelFormat::Undefined.raw(),
      &bitmap_payload,
    )
    .unwrap();
    assert_eq!(
      bitmap_object.bitmap_data_type_kind(),
      Some(EmfPlusBitmapDataType::Compressed)
    );
    assert_eq!(bitmap_object.parse_bitmap_data().unwrap(), bitmap_payload);
    let image_data = EmfPlusImageData::Bitmap(bitmap_object);
    let image_object =
      EmfPlusImageObject::from_typed_data(test_graphics_version(), &image_data).unwrap();
    assert_eq!(
      image_object.image_data_type(),
      Some(EmfPlusImageDataType::Bitmap)
    );
    assert_eq!(image_object.parse_image_data().unwrap(), image_data);
    object_record
      .set_typed_data(&EmfPlusObjectData::Image(image_object.clone()))
      .unwrap();
    assert_eq!(
      object_record.parse_object_data().unwrap(),
      EmfPlusObjectData::Image(image_object)
    );

    let pen_payload = EmfPlusPenPayload {
      pen_data: EmfPlusPenData {
        pen_data_flags: 0,
        pen_unit: EmfPlusUnitType::Pixel.raw(),
        pen_width: 1.5,
        optional_data: EmfPlusPenOptionalData::default(),
        trailing_data: Vec::new(),
      },
      brush_object: Some(brush_object),
    };
    let pen_object =
      EmfPlusPenObject::from_typed_payload(test_graphics_version(), &pen_payload).unwrap();
    assert_eq!(pen_object.parse_pen_payload().unwrap(), pen_payload);
    object_record
      .set_typed_data(&EmfPlusObjectData::Pen(pen_object.clone()))
      .unwrap();
    assert_eq!(
      object_record.parse_object_data().unwrap(),
      EmfPlusObjectData::Pen(pen_object)
    );
  }

  #[test]
  fn emf_plus_pen_payload_region_and_custom_cap_data_roundtrip() {
    let pen_payload = EmfPlusPenPayload {
      pen_data: EmfPlusPenData {
        pen_data_flags: (EmfPlusPenDataFlags::START_CAP
          | EmfPlusPenDataFlags::JOIN
          | EmfPlusPenDataFlags::DASHED_LINE
          | EmfPlusPenDataFlags::NON_CENTER)
          .bits(),
        pen_unit: EmfPlusUnitType::Pixel.raw(),
        pen_width: 2.5,
        optional_data: EmfPlusPenOptionalData {
          start_cap: Some(EmfPlusLineCapType::Round.raw()),
          join: Some(EmfPlusLineJoinType::MiterClipped.raw()),
          dashed_line_data: Some(EmfPlusDashedLineData {
            dashed_line_data: vec![1.0, 2.0, 3.0],
          }),
          pen_alignment: Some(EmfPlusPenAlignment::Inset.raw()),
          ..Default::default()
        },
        trailing_data: Vec::new(),
      },
      brush_object: Some(EmfPlusBrushObject {
        version: test_graphics_version(),
        brush_type: EmfPlusBrushType::SolidColor.raw(),
        brush_data: EmfPlusBrushData::Solid(EmfPlusSolidBrushData {
          solid_color: EmfPlusArgb {
            blue: 1,
            green: 2,
            red: 3,
            alpha: 4,
          },
          trailing_data: Vec::new(),
        })
        .to_bytes()
        .unwrap(),
      }),
    };
    let pen = EmfPlusPenObject {
      version: test_graphics_version(),
      pen_type: 0,
      pen_data_and_brush_object: pen_payload.to_bytes().unwrap(),
    };
    let parsed = pen.parse_pen_payload().unwrap();
    assert_eq!(parsed, pen_payload);
    assert_eq!(parsed.to_bytes().unwrap(), pen.pen_data_and_brush_object);
    assert_eq!(
      parsed.pen_data.pen_unit_kind(),
      Some(EmfPlusUnitType::Pixel)
    );
    assert_eq!(
      parsed.pen_data.optional_data.start_cap_kind(),
      Some(EmfPlusLineCapType::Round)
    );
    assert_eq!(
      parsed.pen_data.optional_data.join_kind(),
      Some(EmfPlusLineJoinType::MiterClipped)
    );
    assert_eq!(
      parsed.pen_data.optional_data.pen_alignment_kind(),
      Some(EmfPlusPenAlignment::Inset)
    );

    let missing_optional_pen_data = EmfPlusPenData {
      pen_data_flags: EmfPlusPenDataFlags::START_CAP.bits(),
      pen_unit: EmfPlusUnitType::Pixel.raw(),
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData::default(),
      trailing_data: Vec::new(),
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    assert!(missing_optional_pen_data.write_to(&mut writer).is_err());

    let invalid_pen_unit_data = EmfPlusPenData {
      pen_data_flags: 0,
      pen_unit: 0xFFFF_FFFF,
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData::default(),
      trailing_data: Vec::new(),
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    assert!(invalid_pen_unit_data.write_to(&mut writer).is_err());

    let invalid_pen_flags_data = EmfPlusPenData {
      pen_data_flags: 0x8000_0000,
      pen_unit: EmfPlusUnitType::Pixel.raw(),
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData::default(),
      trailing_data: Vec::new(),
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    assert!(invalid_pen_flags_data.write_to(&mut writer).is_err());
    let stray_optional_pen_data = EmfPlusPenData {
      pen_data_flags: 0,
      pen_unit: EmfPlusUnitType::Pixel.raw(),
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData {
        start_cap: Some(EmfPlusLineCapType::Round.raw()),
        ..Default::default()
      },
      trailing_data: Vec::new(),
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    assert!(stray_optional_pen_data.write_to(&mut writer).is_err());

    let trailing_pen_data = EmfPlusPenData {
      pen_data_flags: 0,
      pen_unit: EmfPlusUnitType::Pixel.raw(),
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData::default(),
      trailing_data: vec![0xAA],
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    assert!(trailing_pen_data.write_to(&mut writer).is_err());

    let invalid_pen_flags = EmfPlusPenObject {
      version: test_graphics_version(),
      pen_type: 0,
      pen_data_and_brush_object: {
        let mut data = Vec::new();
        data.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
        data.extend_from_slice(&EmfPlusUnitType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&1.0_f32.to_le_bytes());
        data
      },
    };
    assert!(invalid_pen_flags.parse_pen_payload().is_err());

    let truncated_brush_pen = EmfPlusPenObject {
      version: test_graphics_version(),
      pen_type: 0,
      pen_data_and_brush_object: {
        let mut data = Vec::new();
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&EmfPlusUnitType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&1.0_f32.to_le_bytes());
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        data
      },
    };
    assert!(truncated_brush_pen.parse_pen_payload().is_err());

    let invalid_line_style_pen = EmfPlusPenObject {
      version: test_graphics_version(),
      pen_type: 0,
      pen_data_and_brush_object: {
        let mut data = Vec::new();
        data.extend_from_slice(&EmfPlusPenDataFlags::LINE_STYLE.bits().to_le_bytes());
        data.extend_from_slice(&EmfPlusUnitType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&1.0_f32.to_le_bytes());
        data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        data
      },
    };
    assert!(invalid_line_style_pen.parse_pen_payload().is_err());

    let invalid_nested_brush_pen = EmfPlusPenObject {
      version: test_graphics_version(),
      pen_type: 0,
      pen_data_and_brush_object: {
        let mut data = Vec::new();
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&EmfPlusUnitType::Pixel.raw().to_le_bytes());
        data.extend_from_slice(&1.0_f32.to_le_bytes());
        data.extend_from_slice(&invalid_graphics_version().value.to_le_bytes());
        data.extend_from_slice(&EmfPlusBrushType::SolidColor.raw().to_le_bytes());
        data.extend_from_slice(&[1, 2, 3, 4]);
        data
      },
    };
    assert!(invalid_nested_brush_pen.parse_pen_payload().is_ok());
    let invalid_nested_brush_pen = EmfPlusObjectData::Pen(invalid_nested_brush_pen);
    assert!(invalid_nested_brush_pen.to_bytes().is_ok());
    assert!(invalid_nested_brush_pen.validate_strict().is_err());

    let invalid_compound_pen_data = EmfPlusPenData {
      pen_data_flags: EmfPlusPenDataFlags::COMPOUND_LINE.bits(),
      pen_unit: EmfPlusUnitType::Pixel.raw(),
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData {
        compound_line_data: Some(EmfPlusCompoundLineData {
          compound_line_data: vec![0.0, 0.75, 0.5],
        }),
        ..Default::default()
      },
      trailing_data: Vec::new(),
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    assert!(invalid_compound_pen_data.write_to(&mut writer).is_err());

    let region_node = EmfPlusRegionNode {
      node_type: EmfPlusRegionNodeDataType::Rect.raw(),
      data: EmfPlusRegionNodeData::Rect(RectF {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
      }),
    };
    let mut node_bytes = Vec::new();
    let mut writer = Writer::new(&mut node_bytes);
    region_node.write_to(&mut writer).unwrap();
    let region = EmfPlusRegionObject {
      version: test_graphics_version(),
      region_node_count: 0,
      region_nodes: node_bytes.clone(),
    };
    let parsed_nodes = region.parse_region_nodes().unwrap();
    assert_eq!(parsed_nodes, vec![region_node.clone()]);
    assert_eq!(parsed_nodes[0].node_count(), 1);
    let mut rewritten_node = Vec::new();
    let mut writer = Writer::new(&mut rewritten_node);
    parsed_nodes[0].write_to(&mut writer).unwrap();
    assert_eq!(rewritten_node, node_bytes);
    let invalid_region_node = EmfPlusRegionNode {
      node_type: EmfPlusRegionNodeDataType::Empty.raw(),
      data: EmfPlusRegionNodeData::Rect(RectF {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
      }),
    };
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(invalid_region_node.write_to(&mut writer).is_err());
    let invalid_region_node = EmfPlusRegionNode {
      node_type: EmfPlusRegionNodeDataType::And.raw(),
      data: EmfPlusRegionNodeData::Empty,
    };
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(invalid_region_node.write_to(&mut writer).is_err());
    let unknown_region_node = EmfPlusRegionNode {
      node_type: 0xFFFF_FFFF,
      data: EmfPlusRegionNodeData::Raw(vec![1, 2, 3, 4]),
    };
    let mut unknown_region_node_bytes = Vec::new();
    let mut writer = Writer::new(&mut unknown_region_node_bytes);
    unknown_region_node.write_to(&mut writer).unwrap();
    assert_eq!(
      unknown_region_node_bytes,
      [0xFFFF_FFFF_u32.to_le_bytes().as_slice(), &[1, 2, 3, 4]].concat()
    );

    let path_node = EmfPlusRegionNode {
      node_type: EmfPlusRegionNodeDataType::Path.raw(),
      data: EmfPlusRegionNodeData::Path(EmfPlusRegionNodePathData::Path(EmfPlusPathObject {
        version: test_graphics_version(),
        path_point_flags: 0,
        points: EmfPlusPointData::Float(vec![PointF { x: 1.0, y: 2.0 }]),
        point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00])),
        alignment_padding: vec![0, 0, 0],
      })),
    };
    let mut path_node_bytes = Vec::new();
    let mut writer = Writer::new(&mut path_node_bytes);
    path_node.write_to(&mut writer).unwrap();
    let path_region = EmfPlusRegionObject {
      version: test_graphics_version(),
      region_node_count: 0,
      region_nodes: path_node_bytes.clone(),
    };
    let parsed_path_nodes = path_region.parse_region_nodes().unwrap();
    assert_eq!(parsed_path_nodes, vec![path_node]);
    let EmfPlusRegionNodeData::Path(parsed_path_data) = &parsed_path_nodes[0].data else {
      panic!("expected region node path");
    };
    assert!(parsed_path_data.path().is_some());
    let mut rewritten_path_node = Vec::new();
    let mut writer = Writer::new(&mut rewritten_path_node);
    parsed_path_nodes[0].write_to(&mut writer).unwrap();
    assert_eq!(rewritten_path_node, path_node_bytes);

    let raw_path_node = EmfPlusRegionNode {
      node_type: EmfPlusRegionNodeDataType::Path.raw(),
      data: EmfPlusRegionNodeData::Path(EmfPlusRegionNodePathData::Raw(vec![1, 2, 3])),
    };
    let raw_path_node_bytes = [
      EmfPlusRegionNodeDataType::Path
        .raw()
        .to_le_bytes()
        .as_slice(),
      3_i32.to_le_bytes().as_slice(),
      &[1, 2, 3],
    ]
    .concat();
    let raw_path_region = EmfPlusRegionObject {
      version: test_graphics_version(),
      region_node_count: 0,
      region_nodes: raw_path_node_bytes,
    };
    assert!(raw_path_region.parse_region_nodes().is_err());
    assert!(
      raw_path_node
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );

    let child_region_node = EmfPlusRegionNode {
      node_type: EmfPlusRegionNodeDataType::And.raw(),
      data: EmfPlusRegionNodeData::ChildNodes(Box::new(EmfPlusRegionNodeChildNodes {
        left: region_node.clone(),
        right: EmfPlusRegionNode {
          node_type: EmfPlusRegionNodeDataType::Empty.raw(),
          data: EmfPlusRegionNodeData::Empty,
        },
      })),
    };
    let mut child_node_bytes = Vec::new();
    let mut writer = Writer::new(&mut child_node_bytes);
    child_region_node.write_to(&mut writer).unwrap();
    let child_region = EmfPlusRegionObject {
      version: test_graphics_version(),
      region_node_count: 2,
      region_nodes: child_node_bytes.clone(),
    };
    let parsed_child_nodes = child_region.parse_region_nodes().unwrap();
    assert_eq!(parsed_child_nodes, vec![child_region_node]);
    assert_eq!(parsed_child_nodes[0].node_count(), 3);
    let mut rewritten_child_node = Vec::new();
    let mut writer = Writer::new(&mut rewritten_child_node);
    parsed_child_nodes[0].write_to(&mut writer).unwrap();
    assert_eq!(rewritten_child_node, child_node_bytes);

    let invalid_count_region = EmfPlusRegionObject {
      version: test_graphics_version(),
      region_node_count: 1,
      region_nodes: node_bytes.clone(),
    };
    assert!(invalid_count_region.parse_region_nodes().is_err());
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(invalid_count_region.write_to(&mut writer).is_err());
    let impossible_count_region = EmfPlusRegionObject {
      version: test_graphics_version(),
      region_node_count: 1_000,
      region_nodes: node_bytes.clone(),
    };
    assert!(impossible_count_region.parse_region_nodes().is_err());
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(impossible_count_region.write_to(&mut writer).is_err());
    assert!(
      EmfPlusObjectRecordData {
        object_id: 1,
        object_type_raw: EmfPlusObjectType::Region.raw() as u8,
        continues: false,
        total_object_size: None,
        object_data: {
          let mut data = Vec::new();
          data.extend_from_slice(&test_graphics_version().value.to_le_bytes());
          data.extend_from_slice(&1_u32.to_le_bytes());
          data.extend_from_slice(&node_bytes);
          data
        },
      }
      .parse_object_data()
      .is_err()
    );

    let trailing_region = EmfPlusRegionObject {
      version: test_graphics_version(),
      region_node_count: 0,
      region_nodes: {
        let mut data = node_bytes.clone();
        data.push(0);
        data
      },
    };
    assert!(trailing_region.parse_region_nodes().is_err());

    let line_cap_optional_data = EmfPlusCustomLineCapOptionalData {
      fill_path: Some(EmfPlusFillPathObject {
        path_data: EmfPlusRegionNodePathData::Path(EmfPlusPathObject {
          version: test_graphics_version(),
          path_point_flags: 0,
          points: EmfPlusPointData::Float(vec![PointF { x: 3.0, y: 4.0 }]),
          point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00])),
          alignment_padding: vec![0, 0, 0],
        }),
        trailing_data: Vec::new(),
      }),
      line_path: Some(EmfPlusLinePathObject {
        path_data: EmfPlusRegionNodePathData::Path(EmfPlusPathObject {
          version: test_graphics_version(),
          path_point_flags: 0,
          points: EmfPlusPointData::Float(vec![PointF { x: 5.0, y: 6.0 }]),
          point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00])),
          alignment_padding: vec![0, 0, 0],
        }),
        trailing_data: Vec::new(),
      }),
      trailing_data: Vec::new(),
    };
    let raw_line_path = EmfPlusLinePathObject {
      path_data: EmfPlusRegionNodePathData::Raw(vec![5, 6, 7, 8]),
      trailing_data: Vec::new(),
    };
    assert!(
      raw_line_path
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
    let mut invalid_optional_data = line_cap_optional_data.clone();
    invalid_optional_data.trailing_data = vec![0xAA];
    assert!(invalid_optional_data.to_bytes().is_err());
    let mut invalid_fill_path = line_cap_optional_data.fill_path.clone().unwrap();
    invalid_fill_path.trailing_data = vec![0xAA];
    assert!(
      invalid_fill_path
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
    let mut invalid_line_path = line_cap_optional_data.line_path.clone().unwrap();
    invalid_line_path.trailing_data = vec![0xAA];
    assert!(
      invalid_line_path
        .write_to(&mut Writer::new(std::io::Cursor::new(Vec::new())))
        .is_err()
    );
    let default_cap = EmfPlusCustomLineCapData::Default(EmfPlusCustomLineCapDefaultData {
      custom_line_cap_data_flags: (EmfPlusCustomLineCapDataFlags::FILL_PATH
        | EmfPlusCustomLineCapDataFlags::LINE_PATH)
        .bits(),
      base_cap: EmfPlusLineCapType::Flat.raw() as u32,
      base_inset: 0.5,
      stroke_start_cap: EmfPlusLineCapType::Round.raw() as u32,
      stroke_end_cap: EmfPlusLineCapType::Square.raw() as u32,
      stroke_join: EmfPlusLineJoinType::Bevel.raw() as u32,
      stroke_miter_limit: 4.0,
      width_scale: 1.0,
      fill_hot_spot: PointF { x: 0.0, y: 0.0 },
      stroke_hot_spot: PointF { x: 0.0, y: 0.0 },
      optional_data: line_cap_optional_data.to_bytes().unwrap(),
    });
    let cap = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::Default.raw(),
      custom_line_cap_data: default_cap.to_bytes().unwrap(),
    };
    let parsed_cap = cap.parse_cap_data().unwrap();
    assert_eq!(parsed_cap, default_cap);
    assert_eq!(parsed_cap.to_bytes().unwrap(), cap.custom_line_cap_data);
    let custom_start_cap = EmfPlusCustomStartCapData::from_typed_cap(&cap).unwrap();
    assert_eq!(custom_start_cap.custom_start_cap, cap.to_bytes().unwrap());
    assert_eq!(custom_start_cap.parse_custom_start_cap().unwrap(), cap);
    let custom_end_cap = EmfPlusCustomEndCapData::from_typed_cap(&cap).unwrap();
    assert_eq!(custom_end_cap.custom_end_cap, cap.to_bytes().unwrap());
    assert_eq!(custom_end_cap.parse_custom_end_cap().unwrap(), cap);
    let mut mutable_custom_start_cap = EmfPlusCustomStartCapData {
      custom_start_cap: Vec::new(),
    };
    mutable_custom_start_cap.set_typed_cap(&cap).unwrap();
    assert_eq!(mutable_custom_start_cap, custom_start_cap);
    let mut mutable_custom_end_cap = EmfPlusCustomEndCapData {
      custom_end_cap: Vec::new(),
    };
    mutable_custom_end_cap.set_typed_cap(&cap).unwrap();
    assert_eq!(mutable_custom_end_cap, custom_end_cap);
    let mut truncated_custom_start_cap = custom_start_cap.clone();
    truncated_custom_start_cap.custom_start_cap.truncate(8);
    assert!(truncated_custom_start_cap.parse_custom_start_cap().is_err());
    let mut truncated_custom_end_cap = custom_end_cap.clone();
    truncated_custom_end_cap.custom_end_cap.truncate(8);
    assert!(truncated_custom_end_cap.parse_custom_end_cap().is_err());
    let invalid_start_cap_pen_data = EmfPlusPenData {
      pen_data_flags: EmfPlusPenDataFlags::CUSTOM_START_CAP.bits(),
      pen_unit: EmfPlusUnitType::Pixel.raw(),
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData {
        custom_start_cap_data: Some(truncated_custom_start_cap),
        ..Default::default()
      },
      trailing_data: Vec::new(),
    };
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(invalid_start_cap_pen_data.write_to(&mut writer).is_err());
    let invalid_end_cap_pen_data = EmfPlusPenData {
      pen_data_flags: EmfPlusPenDataFlags::CUSTOM_END_CAP.bits(),
      pen_unit: EmfPlusUnitType::Pixel.raw(),
      pen_width: 1.0,
      optional_data: EmfPlusPenOptionalData {
        custom_end_cap_data: Some(truncated_custom_end_cap),
        ..Default::default()
      },
      trailing_data: Vec::new(),
    };
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(invalid_end_cap_pen_data.write_to(&mut writer).is_err());
    let EmfPlusCustomLineCapData::Default(parsed_default_cap) = &parsed_cap else {
      panic!("expected default custom line cap");
    };
    let parsed_optional_data = parsed_default_cap.parse_optional_data().unwrap();
    assert_eq!(parsed_optional_data, line_cap_optional_data);
    assert_eq!(
      parsed_optional_data.to_bytes().unwrap(),
      parsed_default_cap.optional_data
    );
    let invalid_optional_cap = EmfPlusCustomLineCapData::Default(EmfPlusCustomLineCapDefaultData {
      optional_data: {
        let mut data = parsed_default_cap.optional_data.clone();
        data.push(0xAA);
        data
      },
      ..parsed_default_cap.clone()
    });
    assert!(invalid_optional_cap.to_bytes().is_err());
    let mut invalid_optional_cap_data = default_cap.to_bytes().unwrap();
    invalid_optional_cap_data.push(0xAA);
    let invalid_optional_cap_object = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::Default.raw(),
      custom_line_cap_data: invalid_optional_cap_data,
    };
    assert!(invalid_optional_cap_object.parse_cap_data().is_err());
    assert_eq!(
      parsed_default_cap.base_cap_kind(),
      Some(EmfPlusLineCapType::Flat)
    );
    assert_eq!(
      parsed_default_cap.stroke_start_cap_kind(),
      Some(EmfPlusLineCapType::Round)
    );
    assert_eq!(
      parsed_default_cap.stroke_end_cap_kind(),
      Some(EmfPlusLineCapType::Square)
    );
    assert_eq!(
      parsed_default_cap.stroke_join_kind(),
      Some(EmfPlusLineJoinType::Bevel)
    );
    let mut invalid_cap_flags = parsed_default_cap.clone();
    invalid_cap_flags.custom_line_cap_data_flags = 0x8000_0000;
    assert!(
      EmfPlusCustomLineCapData::Default(invalid_cap_flags.clone())
        .to_bytes()
        .is_err()
    );
    let invalid_cap_flags_object = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::Default.raw(),
      custom_line_cap_data: {
        let mut data = EmfPlusCustomLineCapData::Default(parsed_default_cap.clone())
          .to_bytes()
          .unwrap();
        data[0..4].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
        data
      },
    };
    assert!(invalid_cap_flags_object.parse_cap_data().is_err());
    assert!(invalid_cap_flags_object.to_bytes().is_err());

    let missing_optional_cap = EmfPlusCustomLineCapData::Default(EmfPlusCustomLineCapDefaultData {
      custom_line_cap_data_flags: EmfPlusCustomLineCapDataFlags::FILL_PATH.bits(),
      base_cap: EmfPlusLineCapType::Flat.raw() as u32,
      base_inset: 0.0,
      stroke_start_cap: EmfPlusLineCapType::Flat.raw() as u32,
      stroke_end_cap: EmfPlusLineCapType::Flat.raw() as u32,
      stroke_join: EmfPlusLineJoinType::Miter.raw() as u32,
      stroke_miter_limit: 1.0,
      width_scale: 1.0,
      fill_hot_spot: PointF { x: 0.0, y: 0.0 },
      stroke_hot_spot: PointF { x: 0.0, y: 0.0 },
      optional_data: Vec::new(),
    });
    assert!(missing_optional_cap.to_bytes().is_err());
    let missing_optional_cap_object = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::Default.raw(),
      custom_line_cap_data: [
        EmfPlusCustomLineCapDataFlags::FILL_PATH
          .bits()
          .to_le_bytes()
          .as_slice(),
        (EmfPlusLineCapType::Flat.raw() as u32)
          .to_le_bytes()
          .as_slice(),
        0.0_f32.to_le_bytes().as_slice(),
        (EmfPlusLineCapType::Flat.raw() as u32)
          .to_le_bytes()
          .as_slice(),
        (EmfPlusLineCapType::Flat.raw() as u32)
          .to_le_bytes()
          .as_slice(),
        (EmfPlusLineJoinType::Miter.raw() as u32)
          .to_le_bytes()
          .as_slice(),
        1.0_f32.to_le_bytes().as_slice(),
        1.0_f32.to_le_bytes().as_slice(),
        0.0_f32.to_le_bytes().as_slice(),
        0.0_f32.to_le_bytes().as_slice(),
        0.0_f32.to_le_bytes().as_slice(),
        0.0_f32.to_le_bytes().as_slice(),
      ]
      .concat(),
    };
    assert!(missing_optional_cap_object.parse_cap_data().is_err());
    assert!(missing_optional_cap_object.to_bytes().is_err());

    let invalid_default_cap = EmfPlusCustomLineCapData::Default(EmfPlusCustomLineCapDefaultData {
      custom_line_cap_data_flags: 0,
      base_cap: 0xFFFF_FFFF,
      base_inset: 0.0,
      stroke_start_cap: EmfPlusLineCapType::Flat.raw() as u32,
      stroke_end_cap: EmfPlusLineCapType::Flat.raw() as u32,
      stroke_join: EmfPlusLineJoinType::Miter.raw() as u32,
      stroke_miter_limit: 1.0,
      width_scale: 1.0,
      fill_hot_spot: PointF { x: 0.0, y: 0.0 },
      stroke_hot_spot: PointF { x: 0.0, y: 0.0 },
      optional_data: Vec::new(),
    });
    assert!(invalid_default_cap.to_bytes().is_err());

    let arrow_cap = EmfPlusCustomLineCapData::Arrow(EmfPlusCustomLineCapArrowData {
      width: 3.0,
      height: 4.0,
      middle_inset: 1.0,
      fill_state: 1,
      line_start_cap: EmfPlusLineCapType::Flat.raw() as u32,
      line_end_cap: EmfPlusLineCapType::Triangle.raw() as u32,
      line_join: EmfPlusLineJoinType::Round.raw() as u32,
      line_miter_limit: 2.0,
      width_scale: 1.25,
      fill_hot_spot: PointF { x: 0.0, y: 0.0 },
      line_hot_spot: PointF { x: 0.0, y: 0.0 },
      trailing_data: Vec::new(),
    });
    let cap = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::AdjustableArrow.raw(),
      custom_line_cap_data: arrow_cap.to_bytes().unwrap(),
    };
    let parsed_cap = cap.parse_cap_data().unwrap();
    assert_eq!(parsed_cap, arrow_cap);
    assert_eq!(parsed_cap.to_bytes().unwrap(), cap.custom_line_cap_data);
    let EmfPlusCustomLineCapData::Arrow(parsed_arrow_cap) = &parsed_cap else {
      panic!("expected arrow custom line cap");
    };
    assert_eq!(
      parsed_arrow_cap.line_start_cap_kind(),
      Some(EmfPlusLineCapType::Flat)
    );
    assert_eq!(
      parsed_arrow_cap.line_end_cap_kind(),
      Some(EmfPlusLineCapType::Triangle)
    );
    assert_eq!(
      parsed_arrow_cap.line_join_kind(),
      Some(EmfPlusLineJoinType::Round)
    );

    let invalid_cap = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: 0x7FFF_FFFF,
      custom_line_cap_data: Vec::new(),
    };
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    assert!(invalid_cap.write_to(&mut writer).is_err());

    let mut invalid_arrow_bytes = arrow_cap.to_bytes().unwrap();
    invalid_arrow_bytes[12..16].copy_from_slice(&2_u32.to_le_bytes());
    let invalid_arrow_cap = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::AdjustableArrow.raw(),
      custom_line_cap_data: invalid_arrow_bytes,
    };
    assert!(invalid_arrow_cap.parse_cap_data().is_err());
    assert!(invalid_arrow_cap.to_bytes().is_err());
    let invalid_arrow_cap = EmfPlusCustomLineCapData::Arrow(EmfPlusCustomLineCapArrowData {
      width: 3.0,
      height: 4.0,
      middle_inset: 1.0,
      fill_state: 1,
      line_start_cap: EmfPlusLineCapType::Flat.raw() as u32,
      line_end_cap: EmfPlusLineCapType::Triangle.raw() as u32,
      line_join: EmfPlusLineJoinType::Round.raw() as u32,
      line_miter_limit: 2.0,
      width_scale: 1.25,
      fill_hot_spot: PointF { x: 0.0, y: 0.0 },
      line_hot_spot: PointF { x: 0.0, y: 0.0 },
      trailing_data: vec![0xAA],
    });
    assert!(invalid_arrow_cap.to_bytes().is_err());
    let mut invalid_arrow_bytes = arrow_cap.to_bytes().unwrap();
    invalid_arrow_bytes.push(0xAA);
    let invalid_arrow_cap_object = EmfPlusCustomLineCapObject {
      version: test_graphics_version(),
      cap_type: EmfPlusCustomLineCapDataType::AdjustableArrow.raw(),
      custom_line_cap_data: invalid_arrow_bytes,
    };
    assert!(invalid_arrow_cap_object.parse_cap_data().is_err());
  }

  #[test]
  fn emf_plus_palette_blend_boundary_and_focus_data_roundtrip() {
    let palette = EmfPlusPalette {
      palette_style_flags: (EmfPlusPaletteStyleFlags::HAS_ALPHA
        | EmfPlusPaletteStyleFlags::HALFTONE)
        .bits(),
      entries: vec![
        EmfPlusArgb {
          blue: 1,
          green: 2,
          red: 3,
          alpha: 4,
        },
        EmfPlusArgb {
          blue: 5,
          green: 6,
          red: 7,
          alpha: 8,
        },
      ],
      trailing_data: Vec::new(),
    };
    let parsed_palette = EmfPlusPalette::read_from_bytes(&palette.to_bytes().unwrap()).unwrap();
    assert_eq!(parsed_palette, palette);
    assert!(
      parsed_palette
        .flags()
        .contains(EmfPlusPaletteStyleFlags::HAS_ALPHA)
    );
    let mut invalid_palette = palette.clone();
    invalid_palette.palette_style_flags = 0x8000_0000;
    assert!(invalid_palette.to_bytes().is_err());
    let mut invalid_palette = palette.clone();
    invalid_palette.trailing_data = vec![0xAA];
    assert!(invalid_palette.to_bytes().is_err());
    let mut invalid_palette_bytes = palette.to_bytes().unwrap();
    invalid_palette_bytes.push(0xAA);
    assert!(EmfPlusPalette::read_from_bytes(&invalid_palette_bytes).is_err());
    let mut invalid_palette_bytes = palette.to_bytes().unwrap();
    invalid_palette_bytes[0..4].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
    assert!(EmfPlusPalette::read_from_bytes(&invalid_palette_bytes).is_err());
    let grayscale_palette = EmfPlusPalette {
      palette_style_flags: EmfPlusPaletteStyleFlags::GRAYSCALE.bits(),
      entries: vec![EmfPlusArgb {
        blue: 9,
        green: 9,
        red: 9,
        alpha: 0xFF,
      }],
      trailing_data: Vec::new(),
    };
    assert!(grayscale_palette.to_bytes().is_ok());
    let mut invalid_grayscale_palette = grayscale_palette.clone();
    invalid_grayscale_palette.entries[0].red = 10;
    assert!(invalid_grayscale_palette.to_bytes().is_err());
    let mut invalid_grayscale_bytes = grayscale_palette.to_bytes().unwrap();
    invalid_grayscale_bytes[8] = 8;
    assert!(EmfPlusPalette::read_from_bytes(&invalid_grayscale_bytes).is_err());
    let mut invalid_alpha_palette = palette.clone();
    for entry in &mut invalid_alpha_palette.entries {
      entry.alpha = 0xFF;
    }
    assert!(invalid_alpha_palette.to_bytes().is_err());
    let mut invalid_alpha_bytes = palette.to_bytes().unwrap();
    invalid_alpha_bytes[11] = 0xFF;
    invalid_alpha_bytes[15] = 0xFF;
    assert!(EmfPlusPalette::read_from_bytes(&invalid_alpha_bytes).is_err());

    let linear_optional = EmfPlusLinearGradientBrushOptionalData {
      transform_matrix: Some(XForm {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 2.0,
        dy: 3.0,
      }),
      blend_pattern: Some(EmfPlusBlendPattern::Colors(EmfPlusBlendColors {
        positions: vec![0.0, 1.0],
        colors: vec![
          EmfPlusArgb {
            blue: 10,
            green: 20,
            red: 30,
            alpha: 255,
          },
          EmfPlusArgb {
            blue: 40,
            green: 50,
            red: 60,
            alpha: 255,
          },
        ],
        trailing_data: Vec::new(),
      })),
      trailing_data: Vec::new(),
    };
    let linear = EmfPlusLinearGradientBrushData {
      brush_data_flags: (EmfPlusBrushDataFlags::TRANSFORM | EmfPlusBrushDataFlags::PRESET_COLORS)
        .bits(),
      wrap_mode: EmfPlusWrapMode::Tile.raw() as i32,
      rect: RectF {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 20.0,
      },
      start_color: EmfPlusArgb {
        blue: 0,
        green: 0,
        red: 0,
        alpha: 255,
      },
      end_color: EmfPlusArgb {
        blue: 255,
        green: 255,
        red: 255,
        alpha: 255,
      },
      reserved1: 0,
      reserved2: 0,
      optional_data: linear_optional.to_bytes().unwrap(),
    };
    let parsed_linear_optional = linear.parse_optional_data().unwrap();
    assert_eq!(parsed_linear_optional, linear_optional);
    assert_eq!(
      parsed_linear_optional.to_bytes().unwrap(),
      linear.optional_data
    );
    let invalid_blend_colors_trailing_data = EmfPlusBlendColors {
      positions: vec![0.0, 1.0],
      colors: vec![
        EmfPlusArgb {
          blue: 10,
          green: 20,
          red: 30,
          alpha: 255,
        },
        EmfPlusArgb {
          blue: 40,
          green: 50,
          red: 60,
          alpha: 255,
        },
      ],
      trailing_data: vec![0xAA],
    };
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(
      invalid_blend_colors_trailing_data
        .write_to(&mut writer)
        .is_err()
    );
    let vertical_factors = EmfPlusBlendFactors {
      positions: vec![0.0, 1.0],
      factors: vec![0.1, 0.9],
      trailing_data: Vec::new(),
    };
    let horizontal_factors = EmfPlusBlendFactors {
      positions: vec![0.0, 1.0],
      factors: vec![0.2, 0.8],
      trailing_data: Vec::new(),
    };
    let linear_hv_optional = EmfPlusLinearGradientBrushOptionalData {
      transform_matrix: None,
      blend_pattern: Some(EmfPlusBlendPattern::FactorsHV {
        horizontal: horizontal_factors.clone(),
        vertical: vertical_factors.clone(),
      }),
      trailing_data: Vec::new(),
    };
    let mut expected_hv_bytes = Vec::new();
    {
      let mut writer = Writer::new(&mut expected_hv_bytes);
      vertical_factors.write_to(&mut writer).unwrap();
      horizontal_factors.write_to(&mut writer).unwrap();
    }
    assert_eq!(linear_hv_optional.to_bytes().unwrap(), expected_hv_bytes);
    let invalid_blend_factors_trailing_data = EmfPlusBlendFactors {
      positions: vec![0.0, 1.0],
      factors: vec![0.25, 0.75],
      trailing_data: vec![0xAA],
    };
    let mut writer = Writer::new(std::io::Cursor::new(Vec::new()));
    assert!(
      invalid_blend_factors_trailing_data
        .write_to(&mut writer)
        .is_err()
    );
    let linear_hv = EmfPlusLinearGradientBrushData {
      brush_data_flags: (EmfPlusBrushDataFlags::BLEND_FACTORS_H
        | EmfPlusBrushDataFlags::BLEND_FACTORS_V)
        .bits(),
      wrap_mode: EmfPlusWrapMode::Tile.raw() as i32,
      rect: RectF {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 20.0,
      },
      start_color: EmfPlusArgb {
        blue: 0,
        green: 0,
        red: 0,
        alpha: 255,
      },
      end_color: EmfPlusArgb {
        blue: 255,
        green: 255,
        red: 255,
        alpha: 255,
      },
      reserved1: 0,
      reserved2: 0,
      optional_data: expected_hv_bytes,
    };
    assert_eq!(linear_hv.parse_optional_data().unwrap(), linear_hv_optional);

    let path_tail = EmfPlusPathGradientBrushTailData {
      boundary_data: Some(EmfPlusBoundaryData::Points(EmfPlusBoundaryPointData {
        points: vec![PointF { x: 1.0, y: 2.0 }, PointF { x: 3.0, y: 4.0 }],
        trailing_data: Vec::new(),
      })),
      optional_data: EmfPlusPathGradientBrushOptionalData {
        transform_matrix: None,
        blend_pattern: Some(EmfPlusBlendPattern::Factors(EmfPlusBlendFactors {
          positions: vec![0.0, 1.0],
          factors: vec![0.25, 0.75],
          trailing_data: Vec::new(),
        })),
        focus_scale_data: Some(EmfPlusFocusScaleData {
          focus_scale_count: 2,
          focus_scale_x: 0.2,
          focus_scale_y: 0.3,
          trailing_data: Vec::new(),
        }),
      },
      trailing_data: Vec::new(),
    };
    let path_gradient = EmfPlusPathGradientBrushData {
      brush_data_flags: (EmfPlusBrushDataFlags::BLEND_FACTORS_H
        | EmfPlusBrushDataFlags::FOCUS_SCALES)
        .bits(),
      wrap_mode: EmfPlusWrapMode::Clamp.raw() as i32,
      center_color: EmfPlusArgb {
        blue: 1,
        green: 1,
        red: 1,
        alpha: 255,
      },
      center_point: PointF { x: 5.0, y: 6.0 },
      surrounding_colors: Vec::new(),
      boundary_and_optional_data: path_tail.to_bytes().unwrap(),
    };
    let parsed_path_tail = path_gradient.parse_tail_data().unwrap();
    assert_eq!(parsed_path_tail, path_tail);
    assert_eq!(
      parsed_path_tail.to_bytes().unwrap(),
      path_gradient.boundary_and_optional_data
    );
    let mut invalid_path_tail_trailing_data = path_tail.clone();
    invalid_path_tail_trailing_data.trailing_data.push(0xAA);
    assert!(invalid_path_tail_trailing_data.to_bytes().is_err());
    let mut invalid_boundary_point_tail = path_tail.clone();
    let Some(EmfPlusBoundaryData::Points(points)) = &mut invalid_boundary_point_tail.boundary_data
    else {
      panic!("expected boundary points");
    };
    points.trailing_data.push(0xAA);
    assert!(invalid_boundary_point_tail.to_bytes().is_err());

    let mut invalid_focus_tail_data = path_tail.clone();
    invalid_focus_tail_data
      .optional_data
      .focus_scale_data
      .as_mut()
      .unwrap()
      .trailing_data
      .push(0xAA);
    assert!(invalid_focus_tail_data.to_bytes().is_err());

    let mut linear_with_trailing_optional_data = linear.clone();
    linear_with_trailing_optional_data.optional_data.push(0xCC);
    assert!(
      EmfPlusBrushData::LinearGradient(linear_with_trailing_optional_data)
        .to_bytes()
        .is_err()
    );

    let mut path_gradient_with_trailing_optional_data = path_gradient.clone();
    path_gradient_with_trailing_optional_data
      .boundary_and_optional_data
      .push(0xDD);
    assert!(
      EmfPlusBrushData::PathGradient(path_gradient_with_trailing_optional_data)
        .to_bytes()
        .is_err()
    );

    let invalid_linear_optional = EmfPlusLinearGradientBrushOptionalData {
      transform_matrix: None,
      blend_pattern: Some(EmfPlusBlendPattern::Colors(EmfPlusBlendColors {
        positions: vec![-0.1],
        colors: vec![EmfPlusArgb {
          blue: 0,
          green: 0,
          red: 0,
          alpha: 255,
        }],
        trailing_data: Vec::new(),
      })),
      trailing_data: Vec::new(),
    };
    assert!(invalid_linear_optional.to_bytes().is_err());

    let invalid_blend_tail = EmfPlusPathGradientBrushTailData {
      boundary_data: None,
      optional_data: EmfPlusPathGradientBrushOptionalData {
        transform_matrix: None,
        blend_pattern: Some(EmfPlusBlendPattern::Factors(EmfPlusBlendFactors {
          positions: vec![0.25, 0.75],
          factors: vec![0.5, 0.5],
          trailing_data: Vec::new(),
        })),
        focus_scale_data: None,
      },
      trailing_data: Vec::new(),
    };
    assert!(invalid_blend_tail.to_bytes().is_err());

    let invalid_focus_tail = EmfPlusPathGradientBrushTailData {
      boundary_data: None,
      optional_data: EmfPlusPathGradientBrushOptionalData {
        transform_matrix: None,
        blend_pattern: None,
        focus_scale_data: Some(EmfPlusFocusScaleData {
          focus_scale_count: 1,
          focus_scale_x: 0.2,
          focus_scale_y: 1.0,
          trailing_data: Vec::new(),
        }),
      },
      trailing_data: Vec::new(),
    };
    assert!(invalid_focus_tail.to_bytes().is_err());

    let boundary_path_tail = EmfPlusPathGradientBrushTailData {
      boundary_data: Some(EmfPlusBoundaryData::Path(EmfPlusBoundaryPathData {
        path_data: EmfPlusRegionNodePathData::Path(EmfPlusPathObject {
          version: test_graphics_version(),
          path_point_flags: 0,
          points: EmfPlusPointData::Float(vec![PointF { x: 7.0, y: 8.0 }]),
          point_types: EmfPlusPathPointTypes::Values(path_point_types(&[0x00])),
          alignment_padding: vec![0, 0, 0],
        }),
        trailing_data: Vec::new(),
      })),
      optional_data: EmfPlusPathGradientBrushOptionalData {
        transform_matrix: None,
        blend_pattern: None,
        focus_scale_data: None,
      },
      trailing_data: Vec::new(),
    };
    let path_boundary_gradient = EmfPlusPathGradientBrushData {
      brush_data_flags: EmfPlusBrushDataFlags::PATH.bits(),
      wrap_mode: EmfPlusWrapMode::Clamp.raw() as i32,
      center_color: EmfPlusArgb {
        blue: 1,
        green: 1,
        red: 1,
        alpha: 255,
      },
      center_point: PointF { x: 5.0, y: 6.0 },
      surrounding_colors: Vec::new(),
      boundary_and_optional_data: boundary_path_tail.to_bytes().unwrap(),
    };
    assert_eq!(
      path_boundary_gradient.parse_tail_data().unwrap(),
      boundary_path_tail
    );
    let mut invalid_boundary_path_tail = boundary_path_tail.clone();
    let Some(EmfPlusBoundaryData::Path(path)) = &mut invalid_boundary_path_tail.boundary_data
    else {
      panic!("expected boundary path");
    };
    path.trailing_data.push(0xAA);
    assert!(invalid_boundary_path_tail.to_bytes().is_err());
  }
}
