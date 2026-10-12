use std::io::Cursor;

use emfsdk_derive::{SdkEnum, SdkObject};

use crate::bitmap::{BitmapBitCount, DeviceIndependentBitmap, DibBitmapInfo, DibColorUsage};
use crate::common::{Error, Reader, Result, SdkEnumValue, SdkRead, SdkSize, SdkWrite, Writer};
use crate::string::{SdkEncoding, SdkString};
use crate::types::{ColorRef, PointL, PointS, RectL, SizeL, TriVertex, XForm};
use crate::wmf::{
  WmfBinaryRasterOperation, WmfBrushStyle, WmfCharacterSet, WmfClipPrecisionFlags, WmfFamilyFont,
  WmfFontQuality, WmfMetafile, WmfMetafileEscape, WmfMetafileVersion, WmfOutPrecision,
  WmfPitchAndFamily, WmfPitchFont, WmfTernaryRasterOperation, WmfTernaryRasterOperationCode,
  WmfTextAlignmentModeFlags, WmfVerticalTextAlignmentModeFlags, validate_wmf_text_alignment_value,
};

pub const EMR_HEADER: u32 = 0x0000_0001;
pub const EMR_EOF: u32 = 0x0000_000E;
pub const EMF_HEADER_MIN_SIZE: u32 = 88;
const EMF_HEADER_FIXED_DATA_SIZE: usize = EMF_HEADER_MIN_SIZE as usize - 8;
pub const EMF_SIGNATURE: u32 = 0x464D_4520;
pub const EMR_COMMENT: u32 = 0x0000_0046;
pub const EMR_COMMENT_EMFPLUS: u32 = 0x2B46_4D45;
pub const EMR_COMMENT_EMFSPOOL: u32 = 0x0000_0000;
pub const EMR_COMMENT_EMFSPOOL_FONT_DEFINITION: u32 = 0x544F_4E46;
pub const EMR_COMMENT_PUBLIC: u32 = 0x4349_4447;
pub const ENHMETA_STOCK_OBJECT: u32 = 0x8000_0000;
pub const LOGFONT_FACE_NAME_CHARS: usize = 32;
pub const LOGFONT_EX_FULL_NAME_CHARS: usize = 64;
pub const LOGFONT_EX_STYLE_CHARS: usize = 32;
pub const LOGFONT_EX_SCRIPT_CHARS: usize = 32;
pub const LOGFONT_PANOSE_SIZE: usize = 320;
pub const LOGFONT_EX_SIZE: usize = 348;
pub const DESIGN_VECTOR_SIGNATURE: u32 = 0x0800_7664;

pub type EmrDibColors = DibColorUsage;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ExtTextOutOptions: u32 {
        const OPAQUE = 0x0000_0002;
        const CLIPPED = 0x0000_0004;
        const GLYPH_INDEX = 0x0000_0010;
        const RTL_READING = 0x0000_0080;
        const NO_RECT = 0x0000_0100;
        const SMALL_CHARS = 0x0000_0200;
        const NUMERICS_LOCAL = 0x0000_0400;
        const NUMERICS_LATIN = 0x0000_0800;
        const IGNORE_LANGUAGE = 0x0000_1000;
        const PDY = 0x0000_2000;
        const REVERSE_INDEX_MAP = 0x0001_0000;
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmrColorAdjustmentFlags: u16 {
        const NEGATIVE = 0x0001;
        const LOG_FILTER = 0x0002;
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmrCreateColorSpaceWFlags: u32 {
        const COLOR_PROFILE_DATA = 0x0000_0001;
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmrLayoutModeFlags: u32 {
        const RTL = 0x0000_0001;
        const BITMAP_ORIENTATION_PRESERVED = 0x0000_0008;
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmrPenStyleFlags: u32 {
        const DASH = 0x0000_0001;
        const DOT = 0x0000_0002;
        const DASH_DOT = 0x0000_0003;
        const DASH_DOT_DOT = 0x0000_0004;
        const NULL = 0x0000_0005;
        const INSIDE_FRAME = 0x0000_0006;
        const USER_STYLE = 0x0000_0007;
        const ALTERNATE = 0x0000_0008;
        const END_CAP_SQUARE = 0x0000_0100;
        const END_CAP_FLAT = 0x0000_0200;
        const JOIN_BEVEL = 0x0000_1000;
        const JOIN_MITER = 0x0000_2000;
        const GEOMETRIC = 0x0001_0000;
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct EmrPixelFormatFlags: u32 {
        const DOUBLEBUFFER = 0x0000_0001;
        const STEREO = 0x0000_0002;
        const DRAW_TO_WINDOW = 0x0000_0004;
        const DRAW_TO_BITMAP = 0x0000_0008;
        const SUPPORT_GDI = 0x0000_0010;
        const SUPPORT_OPENGL = 0x0000_0020;
        const GENERIC_FORMAT = 0x0000_0040;
        const NEED_PALETTE = 0x0000_0080;
        const NEED_SYSTEM_PALETTE = 0x0000_0100;
        const SWAP_EXCHANGE = 0x0000_0200;
        const SWAP_COPY = 0x0000_0400;
        const SWAP_LAYER_BUFFERS = 0x0000_0800;
        const GENERIC_ACCELERATED = 0x0000_1000;
        const SUPPORT_DIRECTDRAW = 0x0000_2000;
        const DIRECT3D_ACCELERATED = 0x0000_4000;
        const SUPPORT_COMPOSITION = 0x0000_8000;
        const DEPTH_DONTCARE = 0x1000_0000;
        const DOUBLEBUFFER_DONTCARE = 0x2000_0000;
        const STEREO_DONTCARE = 0x4000_0000;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrPenLineStyle {
  Solid = 0x0000_0000,
  Dash = 0x0000_0001,
  Dot = 0x0000_0002,
  DashDot = 0x0000_0003,
  DashDotDot = 0x0000_0004,
  Null = 0x0000_0005,
  InsideFrame = 0x0000_0006,
  UserStyle = 0x0000_0007,
  Alternate = 0x0000_0008,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrPenEndCap {
  Round = 0x0000_0000,
  Square = 0x0000_0100,
  Flat = 0x0000_0200,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrPenJoin {
  Round = 0x0000_0000,
  Bevel = 0x0000_1000,
  Miter = 0x0000_2000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrPenType {
  Cosmetic = 0x0000_0000,
  Geometric = 0x0001_0000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPixelFormatType {
  Rgba = 0x00,
  ColorIndex = 0x01,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseFamilyType {
  Any = 0x00,
  NoFit = 0x01,
  TextDisplay = 0x02,
  Script = 0x03,
  Decorative = 0x04,
  Pictorial = 0x05,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseSerifType {
  Any = 0x00,
  NoFit = 0x01,
  Cove = 0x02,
  ObtuseCove = 0x03,
  SquareCove = 0x04,
  ObtuseSquareCove = 0x05,
  Square = 0x06,
  Thin = 0x07,
  Bone = 0x08,
  Exaggerated = 0x09,
  Triangle = 0x0A,
  NormalSans = 0x0B,
  ObtuseSans = 0x0C,
  PerpSans = 0x0D,
  Flared = 0x0E,
  Rounded = 0x0F,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseWeight {
  Any = 0x00,
  NoFit = 0x01,
  VeryLight = 0x02,
  Light = 0x03,
  Thin = 0x04,
  Book = 0x05,
  Medium = 0x06,
  Demi = 0x07,
  Bold = 0x08,
  Heavy = 0x09,
  Black = 0x0A,
  Nord = 0x0B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseProportion {
  Any = 0x00,
  NoFit = 0x01,
  OldStyle = 0x02,
  Modern = 0x03,
  EvenWidth = 0x04,
  Expanded = 0x05,
  Condensed = 0x06,
  VeryExpanded = 0x07,
  VeryCondensed = 0x08,
  Monospaced = 0x09,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseContrast {
  Any = 0x00,
  NoFit = 0x01,
  None = 0x02,
  VeryLow = 0x03,
  Low = 0x04,
  MediumLow = 0x05,
  Medium = 0x06,
  MediumHigh = 0x07,
  High = 0x08,
  VeryHigh = 0x09,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseStrokeVariation {
  Any = 0x00,
  NoFit = 0x01,
  GradualDiagonal = 0x02,
  GradualTransitional = 0x03,
  GradualVertical = 0x04,
  GradualHorizontal = 0x05,
  RapidVertical = 0x06,
  RapidHorizontal = 0x07,
  InstantVertical = 0x08,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseArmStyle {
  Any = 0x00,
  NoFit = 0x01,
  StraightHorizontal = 0x02,
  StraightWedge = 0x03,
  StraightVertical = 0x04,
  StraightSingleSerif = 0x05,
  StraightDoubleSerif = 0x06,
  BentHorizontal = 0x07,
  BentWedge = 0x08,
  BentVertical = 0x09,
  BentSingleSerif = 0x0A,
  BentDoubleSerif = 0x0B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseLetterform {
  Any = 0x00,
  NoFit = 0x01,
  NormalContact = 0x02,
  NormalWeighted = 0x03,
  NormalBoxed = 0x04,
  NormalFlattened = 0x05,
  NormalRounded = 0x06,
  NormalOffCenter = 0x07,
  NormalSquare = 0x08,
  ObliqueContact = 0x09,
  ObliqueWeighted = 0x0A,
  ObliqueBoxed = 0x0B,
  ObliqueFlattened = 0x0C,
  ObliqueRounded = 0x0D,
  ObliqueOffCenter = 0x0E,
  ObliqueSquare = 0x0F,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseMidLine {
  Any = 0x00,
  NoFit = 0x01,
  StandardTrimmed = 0x02,
  StandardPointed = 0x03,
  StandardSerifed = 0x04,
  HighTrimmed = 0x05,
  HighPointed = 0x06,
  HighSerifed = 0x07,
  ConstantTrimmed = 0x08,
  ConstantPointed = 0x09,
  ConstantSerifed = 0x0A,
  LowTrimmed = 0x0B,
  LowPointed = 0x0C,
  LowSerifed = 0x0D,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPanoseXHeight {
  Any = 0x00,
  NoFit = 0x01,
  ConstantSmall = 0x02,
  ConstantStandard = 0x03,
  ConstantLarge = 0x04,
  DuckingSmall = 0x05,
  DuckingStandard = 0x06,
  DuckingLarge = 0x07,
}

pub type EmrArmStyle = EmrPanoseArmStyle;
pub type EmrContrast = EmrPanoseContrast;
pub type EmrFamilyType = EmrPanoseFamilyType;
pub type EmrLetterform = EmrPanoseLetterform;
pub type EmrMidLine = EmrPanoseMidLine;
pub type EmrProportion = EmrPanoseProportion;
pub type EmrSerifType = EmrPanoseSerifType;
pub type EmrStrokeVariation = EmrPanoseStrokeVariation;
pub type EmrWeight = EmrPanoseWeight;
pub type EmrXHeight = EmrPanoseXHeight;

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrArcDirection {
  CounterClockwise = 0x0000_0001,
  Clockwise = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrBackgroundMode {
  Transparent = 0x0000_0001,
  Opaque = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrColorMatchToTarget {
  NotEmbedded = 0x0000_0000,
  Embedded = 0x0000_0001,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrColorSpaceMode {
  Enable = 0x0000_0001,
  Disable = 0x0000_0002,
  DeleteTransform = 0x0000_0003,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrFloodFillMode {
  Border = 0x0000_0000,
  Surface = 0x0000_0001,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrFormatSignature {
  EnhMeta = 0x464D_4520,
  Eps = 0x4653_5045,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[repr(u32)]
#[sdk(repr = "u32")]
pub enum EmrPublicCommentIdentifier {
  WindowsMetafile = 0x8000_0001,
  BeginGroup = 0x0000_0002,
  EndGroup = 0x0000_0003,
  MultiFormats = 0x4000_0004,
  UnicodeString = 0x0000_0040,
  UnicodeEnd = 0x0000_0080,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrLogColorSpaceSignature {
  Psoc = 0x5053_4F43,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmrLogicalColorSpace {
  CalibratedRgb = 0x0000_0000,
  SRgb = 0x7352_4742,
  WindowsColorSpace = 0x5769_6E20,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum EmrGamutMappingIntent {
  Business = 0x0000_0001,
  Graphics = 0x0000_0002,
  Images = 0x0000_0004,
  AbsoluteColorimetric = 0x0000_0008,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrGradientFillMode {
  RectangleHorizontal = 0x0000_0000,
  RectangleVertical = 0x0000_0001,
  Triangle = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrGraphicsMode {
  Compatible = 0x0000_0001,
  Advanced = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrHatchStyle {
  Horizontal = 0x0000_0000,
  Vertical = 0x0000_0001,
  ForwardDiagonal = 0x0000_0002,
  BackwardDiagonal = 0x0000_0003,
  Cross = 0x0000_0004,
  DiagonalCross = 0x0000_0005,
  SolidColor = 0x0000_0006,
  DitheredColor = 0x0000_0007,
  SolidTextColor = 0x0000_0008,
  DitheredTextColor = 0x0000_0009,
  SolidBackgroundColor = 0x0000_000A,
  DitheredBackgroundColor = 0x0000_000B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrIcmMode {
  Off = 0x0000_0001,
  On = 0x0000_0002,
  Query = 0x0000_0003,
  DoneOutsideDc = 0x0000_0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum EmrIlluminant {
  DeviceDefault = 0x0000,
  Tungsten = 0x0001,
  B = 0x0002,
  Daylight = 0x0003,
  D50 = 0x0004,
  D55 = 0x0005,
  D65 = 0x0006,
  D75 = 0x0007,
  Fluorescent = 0x0008,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrMapMode {
  Text = 0x0000_0001,
  LoMetric = 0x0000_0002,
  HiMetric = 0x0000_0003,
  LoEnglish = 0x0000_0004,
  HiEnglish = 0x0000_0005,
  Twips = 0x0000_0006,
  Isotropic = 0x0000_0007,
  Anisotropic = 0x0000_0008,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrMetafileVersion {
  Enhanced = 0x0001_0000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrModifyWorldTransformMode {
  Identity = 0x0000_0001,
  LeftMultiply = 0x0000_0002,
  RightMultiply = 0x0000_0003,
  Set = 0x0000_0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrPolygonFillMode {
  Alternate = 0x0000_0001,
  Winding = 0x0000_0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrPointType {
  CloseFigure = 0x01,
  LineTo = 0x02,
  BezierTo = 0x04,
  MoveTo = 0x06,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmrPointTypeValue {
  pub value: u8,
}

impl EmrPointTypeValue {
  pub fn new(value: u8) -> Result<Self> {
    validate_emr_point_type_value(value)?;
    Ok(Self { value })
  }

  pub fn close_figure(self) -> bool {
    self.value & EmrPointType::CloseFigure.raw() != 0
  }

  pub fn point_type_raw(self) -> u8 {
    if self.close_figure()
      && matches!(
          self.value & !EmrPointType::CloseFigure.raw(),
          value if value == EmrPointType::LineTo.raw()
              || value == EmrPointType::BezierTo.raw()
      )
    {
      self.value & !EmrPointType::CloseFigure.raw()
    } else {
      self.value
    }
  }

  pub fn point_type(self) -> Option<EmrPointType> {
    EmrPointType::from_raw(self.point_type_raw())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrRegionMode {
  And = 0x0000_0001,
  Or = 0x0000_0002,
  Xor = 0x0000_0003,
  Diff = 0x0000_0004,
  Copy = 0x0000_0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmrStretchMode {
  AndScans = 0x0000_0001,
  OrScans = 0x0000_0002,
  DeleteScans = 0x0000_0003,
  Halftone = 0x0000_0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrBlendOperation {
  SourceOver = 0x00,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum EmrAlphaFormat {
  ConstantAlpha = 0x00,
  SourceAlpha = 0x01,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[repr(u32)]
#[sdk(repr = "u32")]
pub enum EmrStockObject {
  WhiteBrush = 0x8000_0000,
  LtGrayBrush = 0x8000_0001,
  GrayBrush = 0x8000_0002,
  DkGrayBrush = 0x8000_0003,
  BlackBrush = 0x8000_0004,
  NullBrush = 0x8000_0005,
  WhitePen = 0x8000_0006,
  BlackPen = 0x8000_0007,
  NullPen = 0x8000_0008,
  OemFixedFont = 0x8000_000A,
  AnsiFixedFont = 0x8000_000B,
  AnsiVarFont = 0x8000_000C,
  SystemFont = 0x8000_000D,
  DeviceDefaultFont = 0x8000_000E,
  DefaultPalette = 0x8000_000F,
  SystemFixedFont = 0x8000_0010,
  DefaultGuiFont = 0x8000_0011,
  DcBrush = 0x8000_0012,
  DcPen = 0x8000_0013,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum EmfRecordType {
  Header = 0x0000_0001,
  PolyBezier = 0x0000_0002,
  Polygon = 0x0000_0003,
  Polyline = 0x0000_0004,
  PolyBezierTo = 0x0000_0005,
  PolylineTo = 0x0000_0006,
  PolyPolyline = 0x0000_0007,
  PolyPolygon = 0x0000_0008,
  SetWindowExtEx = 0x0000_0009,
  SetWindowOrgEx = 0x0000_000A,
  SetViewportExtEx = 0x0000_000B,
  SetViewportOrgEx = 0x0000_000C,
  SetBrushOrgEx = 0x0000_000D,
  Eof = 0x0000_000E,
  SetPixelV = 0x0000_000F,
  SetMapperFlags = 0x0000_0010,
  SetMapMode = 0x0000_0011,
  SetBkMode = 0x0000_0012,
  SetPolyfillMode = 0x0000_0013,
  SetRop2 = 0x0000_0014,
  SetStretchBltMode = 0x0000_0015,
  SetTextAlign = 0x0000_0016,
  SetColorAdjustment = 0x0000_0017,
  SetTextColor = 0x0000_0018,
  SetBkColor = 0x0000_0019,
  OffsetClipRgn = 0x0000_001A,
  MoveToEx = 0x0000_001B,
  SetMetaRgn = 0x0000_001C,
  ExcludeClipRect = 0x0000_001D,
  IntersectClipRect = 0x0000_001E,
  ScaleViewportExtEx = 0x0000_001F,
  ScaleWindowExtEx = 0x0000_0020,
  SaveDc = 0x0000_0021,
  RestoreDc = 0x0000_0022,
  SetWorldTransform = 0x0000_0023,
  ModifyWorldTransform = 0x0000_0024,
  SelectObject = 0x0000_0025,
  CreatePen = 0x0000_0026,
  CreateBrushIndirect = 0x0000_0027,
  DeleteObject = 0x0000_0028,
  AngleArc = 0x0000_0029,
  Ellipse = 0x0000_002A,
  Rectangle = 0x0000_002B,
  RoundRect = 0x0000_002C,
  Arc = 0x0000_002D,
  Chord = 0x0000_002E,
  Pie = 0x0000_002F,
  SelectPalette = 0x0000_0030,
  CreatePalette = 0x0000_0031,
  SetPaletteEntries = 0x0000_0032,
  ResizePalette = 0x0000_0033,
  RealizePalette = 0x0000_0034,
  ExtFloodFill = 0x0000_0035,
  LineTo = 0x0000_0036,
  ArcTo = 0x0000_0037,
  PolyDraw = 0x0000_0038,
  SetArcDirection = 0x0000_0039,
  SetMiterLimit = 0x0000_003A,
  BeginPath = 0x0000_003B,
  EndPath = 0x0000_003C,
  CloseFigure = 0x0000_003D,
  FillPath = 0x0000_003E,
  StrokeAndFillPath = 0x0000_003F,
  StrokePath = 0x0000_0040,
  FlattenPath = 0x0000_0041,
  WidenPath = 0x0000_0042,
  SelectClipPath = 0x0000_0043,
  AbortPath = 0x0000_0044,
  Comment = 0x0000_0046,
  FillRgn = 0x0000_0047,
  FrameRgn = 0x0000_0048,
  InvertRgn = 0x0000_0049,
  PaintRgn = 0x0000_004A,
  ExtSelectClipRgn = 0x0000_004B,
  BitBlt = 0x0000_004C,
  StretchBlt = 0x0000_004D,
  MaskBlt = 0x0000_004E,
  PlgBlt = 0x0000_004F,
  SetDiBitsToDevice = 0x0000_0050,
  StretchDiBits = 0x0000_0051,
  ExtCreateFontIndirectW = 0x0000_0052,
  ExtTextOutA = 0x0000_0053,
  ExtTextOutW = 0x0000_0054,
  PolyBezier16 = 0x0000_0055,
  Polygon16 = 0x0000_0056,
  Polyline16 = 0x0000_0057,
  PolyBezierTo16 = 0x0000_0058,
  PolylineTo16 = 0x0000_0059,
  PolyPolyline16 = 0x0000_005A,
  PolyPolygon16 = 0x0000_005B,
  PolyDraw16 = 0x0000_005C,
  CreateMonoBrush = 0x0000_005D,
  CreateDibPatternBrushPt = 0x0000_005E,
  ExtCreatePen = 0x0000_005F,
  PolyTextOutA = 0x0000_0060,
  PolyTextOutW = 0x0000_0061,
  SetIcmMode = 0x0000_0062,
  CreateColorSpace = 0x0000_0063,
  SetColorSpace = 0x0000_0064,
  DeleteColorSpace = 0x0000_0065,
  GlsRecord = 0x0000_0066,
  GlsBoundedRecord = 0x0000_0067,
  PixelFormat = 0x0000_0068,
  DrawEscape = 0x0000_0069,
  ExtEscape = 0x0000_006A,
  SmallTextOut = 0x0000_006C,
  ForceUfiMapping = 0x0000_006D,
  NamedEscape = 0x0000_006E,
  ColorCorrectPalette = 0x0000_006F,
  SetIcmProfileA = 0x0000_0070,
  SetIcmProfileW = 0x0000_0071,
  AlphaBlend = 0x0000_0072,
  SetLayout = 0x0000_0073,
  TransparentBlt = 0x0000_0074,
  GradientFill = 0x0000_0076,
  SetLinkedUfis = 0x0000_0077,
  SetTextJustification = 0x0000_0078,
  ColorMatchToTargetW = 0x0000_0079,
  CreateColorSpaceW = 0x0000_007A,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfRecordRef<'a> {
  pub record_type: u32,
  pub data: &'a [u8],
}

impl<'a> EmfRecordRef<'a> {
  pub fn record_kind(&self) -> Option<EmfRecordType> {
    EmfRecordType::from_raw(self.record_type)
  }

  pub fn into_owned(self) -> EmfRecord {
    EmfRecord::new(self.record_type, self.data.to_vec())
  }

  pub fn parse_data(self) -> Result<EmfRecordData<'a>> {
    EmfRecordData::from_record_ref(self)
  }

  pub fn rebuild_typed(self) -> Result<EmfRecord> {
    self.parse_data()?.to_record()
  }

  pub fn emf_plus_payload(self) -> Option<&'a [u8]> {
    emf_plus_payload(self.record_type, self.data)
  }
}

impl SdkSize for EmfRecordRef<'_> {
  fn sdk_size(&self) -> u64 {
    8 + self.data.len() as u64
  }
}

#[derive(Clone, Debug)]
pub struct EmfRecords<'a> {
  bytes: &'a [u8],
  offset: usize,
  remaining: usize,
}

impl<'a> Iterator for EmfRecords<'a> {
  type Item = EmfRecordRef<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.remaining == 0 {
      return None;
    }
    let record_type = u32::from_le_bytes(
      self.bytes[self.offset..self.offset + 4]
        .try_into()
        .expect("validated EMF record header"),
    );
    let size = u32::from_le_bytes(
      self.bytes[self.offset + 4..self.offset + 8]
        .try_into()
        .expect("validated EMF record header"),
    ) as usize;
    let data_start = self.offset + 8;
    let end = self.offset + size;
    self.offset = end;
    self.remaining -= 1;
    Some(EmfRecordRef {
      record_type,
      data: &self.bytes[data_start..end],
    })
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.remaining, Some(self.remaining))
  }
}

impl ExactSizeIterator for EmfRecords<'_> {}
impl std::iter::FusedIterator for EmfRecords<'_> {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmfMetafileRef<'a> {
  records_bytes: &'a [u8],
  trailing_data: &'a [u8],
  record_count: usize,
}

impl<'a> EmfMetafileRef<'a> {
  pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
    let (records_end, record_count) = scan_emf_records(bytes)?;
    Ok(Self {
      records_bytes: &bytes[..records_end],
      trailing_data: &bytes[records_end..],
      record_count,
    })
  }

  pub fn records(&self) -> EmfRecords<'a> {
    EmfRecords {
      bytes: self.records_bytes,
      offset: 0,
      remaining: self.record_count,
    }
  }

  pub fn header(&self) -> EmfRecordRef<'a> {
    self
      .records()
      .next()
      .expect("validated EMF metafile contains an EMR_HEADER record")
  }

  pub const fn record_count(&self) -> usize {
    self.record_count
  }

  pub const fn trailing_data(&self) -> &'a [u8] {
    self.trailing_data
  }

  pub fn into_owned(self) -> EmfMetafile {
    EmfMetafile {
      records: self.records().map(EmfRecordRef::into_owned).collect(),
      trailing_data: self.trailing_data.to_vec(),
    }
  }
}

fn scan_emf_records(bytes: &[u8]) -> Result<(usize, usize)> {
  let mut offset = 0usize;
  let mut record_count = 0usize;
  let mut first_record_type = None;
  loop {
    let header = bytes
      .get(offset..offset.saturating_add(8))
      .ok_or_else(|| Error::invalid(offset as u64, "EMF record header is truncated"))?;
    let record_type = u32::from_le_bytes(header[..4].try_into().expect("slice length checked"));
    let size = u32::from_le_bytes(header[4..].try_into().expect("slice length checked")) as usize;
    if size < 8 {
      return Err(Error::invalid(
        offset as u64,
        "EMF record size is smaller than its header",
      ));
    }
    if !size.is_multiple_of(4) {
      return Err(Error::invalid(
        offset as u64,
        "EMF record size is not 32-bit aligned",
      ));
    }
    let end = offset
      .checked_add(size)
      .ok_or_else(|| Error::invalid(offset as u64, "EMF record size overflows"))?;
    if end > bytes.len() {
      return Err(Error::invalid(
        offset as u64,
        "EMF record extends past end of file",
      ));
    }
    first_record_type.get_or_insert(record_type);
    record_count += 1;
    offset = end;
    if record_type == EMR_EOF {
      break;
    }
  }
  if first_record_type != Some(EMR_HEADER) {
    return Err(Error::invalid(0, "EMF metafile must start with EMR_HEADER"));
  }
  Ok((offset, record_count))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfMetafile {
  pub records: Vec<EmfRecord>,
  pub trailing_data: Vec<u8>,
}

impl EmfMetafile {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Ok(EmfMetafileRef::from_bytes(bytes)?.into_owned())
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let capacity = (self.computed_bytes()? as usize)
      .checked_add(self.trailing_data.len())
      .ok_or_else(|| Error::invalid(0, "EMF serialized size overflows usize"))?;
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

  pub fn header(&self) -> Option<&EmfRecord> {
    self
      .records
      .first()
      .filter(|record| record.record_type == EMR_HEADER)
  }

  pub fn computed_bytes(&self) -> Result<u32> {
    let mut total = 0u64;
    for record in &self.records {
      total = total
        .checked_add(u64::from(emf_record_size(record)?))
        .ok_or_else(|| Error::invalid(0, "EMF Bytes overflows"))?;
    }
    if total > u64::from(u32::MAX) {
      return Err(Error::invalid(0, "EMF Bytes exceeds u32::MAX"));
    }
    Ok(total as u32)
  }

  pub fn computed_record_count(&self) -> Result<u32> {
    if self.records.len() > u32::MAX as usize {
      return Err(Error::invalid(0, "EMF Records exceeds u32::MAX"));
    }
    Ok(self.records.len() as u32)
  }

  pub fn validate_header_metrics(&self) -> Result<()> {
    let header = self
      .header()
      .ok_or_else(|| Error::invalid(0, "EMF metafile must start with EMR_HEADER"))?
      .as_header()?
      .ok_or_else(|| Error::invalid(0, "EMF metafile must start with EMR_HEADER"))?;
    let bytes = self.computed_bytes()?;
    if header.bytes != bytes {
      return Err(Error::invalid(0, "EMR_HEADER Bytes does not match records"));
    }
    let records = self.computed_record_count()?;
    if header.records != records {
      return Err(Error::invalid(
        0,
        "EMR_HEADER Records does not match record count",
      ));
    }
    validate_emf_header_handles(header.handles, &self.records)?;
    validate_emf_comment_groups(&self.records)?;
    validate_emf_path_brackets(&self.records)?;
    Ok(())
  }
}

impl SdkWrite for EmfMetafile {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.write_to_writer(writer)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfRecord {
  pub record_type: u32,
  pub data: Vec<u8>,
}

impl EmfRecord {
  pub fn new(record_type: u32, data: Vec<u8>) -> Self {
    Self { record_type, data }
  }

  pub fn as_header(&self) -> Result<Option<EmfHeader>> {
    if self.record_type != EMR_HEADER {
      return Ok(None);
    }
    Ok(Some(EmfHeader::from_record_data(&self.data)?))
  }

  pub fn record_kind(&self) -> Option<EmfRecordType> {
    EmfRecordType::from_raw(self.record_type)
  }

  pub fn as_ref(&self) -> EmfRecordRef<'_> {
    EmfRecordRef {
      record_type: self.record_type,
      data: &self.data,
    }
  }

  pub fn parse_data(&self) -> Result<EmfRecordData<'_>> {
    EmfRecordData::from_record(self)
  }

  pub fn rebuild_typed(&self) -> Result<Self> {
    self.as_ref().rebuild_typed()
  }

  pub fn emf_plus_payload(&self) -> Option<&[u8]> {
    emf_plus_payload(self.record_type, &self.data)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    let size = self
      .data
      .len()
      .checked_add(8)
      .ok_or_else(|| Error::invalid(writer.position().unwrap_or(0), "EMF record is too large"))?;
    if size > u32::MAX as usize {
      return Err(Error::invalid(
        writer.position()?,
        "EMF record size exceeds u32::MAX",
      ));
    }
    if size % 4 != 0 {
      return Err(Error::invalid(
        writer.position()?,
        "EMF record data must include any required 32-bit alignment padding",
      ));
    }
    writer.write_u32(self.record_type)?;
    writer.write_u32(size as u32)?;
    writer.write_all(&self.data)
  }
}

impl SdkWrite for EmfRecord {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    EmfRecord::write_to(self, writer)
  }
}

impl SdkSize for EmfRecord {
  fn sdk_size(&self) -> u64 {
    8 + self.data.len() as u64
  }
}

fn emf_plus_payload(record_type: u32, data: &[u8]) -> Option<&[u8]> {
  if record_type != EMR_COMMENT || data.len() < 8 {
    return None;
  }
  let data_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
  let identifier = u32::from_le_bytes(data[4..8].try_into().ok()?);
  if identifier != EMR_COMMENT_EMFPLUS || data_size < 4 {
    return None;
  }
  let payload_len = data_size - 4;
  let payload_end = 8usize.checked_add(payload_len)?;
  data.get(8..payload_end)
}

fn emf_record_size(record: &EmfRecord) -> Result<u32> {
  let size = record
    .data
    .len()
    .checked_add(8)
    .ok_or_else(|| Error::invalid(0, "EMF record size overflows"))?;
  if !size.is_multiple_of(4) {
    return Err(Error::invalid(0, "EMF record size is not 32-bit aligned"));
  }
  if size > u32::MAX as usize {
    return Err(Error::invalid(0, "EMF record size exceeds u32::MAX"));
  }
  Ok(size as u32)
}

fn validate_emf_header_handles(handles: u16, records: &[EmfRecord]) -> Result<()> {
  let max_created_object_index = max_emf_created_object_index(records)?;
  if max_created_object_index > u32::from(handles) {
    return Err(Error::invalid(
      0,
      "EMR_HEADER Handles is smaller than created object indexes",
    ));
  }
  let max_referenced_object_index = max_emf_referenced_object_index(records)?;
  if max_referenced_object_index > u32::from(handles) {
    return Err(Error::invalid(
      0,
      "EMR_HEADER Handles is smaller than referenced object indexes",
    ));
  }
  Ok(())
}

fn max_emf_created_object_index(records: &[EmfRecord]) -> Result<u32> {
  let mut max_index = 0u32;
  for record in records {
    if is_emf_object_creation_record(record.record_kind()) {
      let bytes = record
        .data
        .get(0..4)
        .ok_or_else(|| Error::invalid(0, "EMF object creation record is missing object index"))?;
      let index = u32::from_le_bytes(bytes.try_into().expect("slice length checked"));
      max_index = max_index.max(index);
    }
  }
  Ok(max_index)
}

fn is_emf_object_creation_record(record_type: Option<EmfRecordType>) -> bool {
  matches!(
    record_type,
    Some(
      EmfRecordType::CreatePen
        | EmfRecordType::CreateBrushIndirect
        | EmfRecordType::CreatePalette
        | EmfRecordType::ExtCreateFontIndirectW
        | EmfRecordType::CreateMonoBrush
        | EmfRecordType::CreateDibPatternBrushPt
        | EmfRecordType::ExtCreatePen
        | EmfRecordType::CreateColorSpace
        | EmfRecordType::CreateColorSpaceW
    )
  )
}

fn max_emf_referenced_object_index(records: &[EmfRecord]) -> Result<u32> {
  let mut max_index = 0u32;
  for record in records {
    let Some(offset) = emf_object_reference_offset(record.record_kind()) else {
      continue;
    };
    let index = read_emf_record_u32_at(record, offset, "EMF object reference index")?;
    if index == 0 || EmrStockObject::from_raw(index).is_some() {
      continue;
    }
    max_index = max_index.max(index);
  }
  Ok(max_index)
}

fn emf_object_reference_offset(record_type: Option<EmfRecordType>) -> Option<usize> {
  match record_type? {
    EmfRecordType::SelectObject
    | EmfRecordType::SelectPalette
    | EmfRecordType::ResizePalette
    | EmfRecordType::DeleteObject
    | EmfRecordType::SetPaletteEntries
    | EmfRecordType::ColorCorrectPalette
    | EmfRecordType::SetColorSpace
    | EmfRecordType::DeleteColorSpace => Some(0),
    _ => None,
  }
}

fn read_emf_record_u32_at(record: &EmfRecord, offset: usize, name: &str) -> Result<u32> {
  let end = offset
    .checked_add(4)
    .ok_or_else(|| Error::invalid(0, format!("{name} offset overflows")))?;
  let bytes = record
    .data
    .get(offset..end)
    .ok_or_else(|| Error::invalid(0, format!("{name} is missing")))?;
  Ok(u32::from_le_bytes(
    bytes.try_into().expect("slice length checked"),
  ))
}

fn validate_emf_comment_groups(records: &[EmfRecord]) -> Result<()> {
  let mut depth = 0usize;
  for record in records {
    if record.record_type != EMR_COMMENT {
      continue;
    }
    match record.parse_data()? {
      EmfRecordData::Comment(EmrComment::Public {
        comment: EmrPublicComment::BeginGroup(_),
        ..
      }) => {
        depth = depth
          .checked_add(1)
          .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_BEGINGROUP nesting overflows"))?;
      }
      EmfRecordData::Comment(EmrComment::Public {
        comment: EmrPublicComment::EndGroup,
        ..
      }) => {
        depth = depth
          .checked_sub(1)
          .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_ENDGROUP without matching BEGINGROUP"))?;
      }
      _ => {}
    }
  }
  if depth == 0 {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      "EMR_COMMENT_BEGINGROUP without matching ENDGROUP",
    ))
  }
}

fn validate_emf_path_brackets(records: &[EmfRecord]) -> Result<()> {
  let mut open = false;
  for record in records {
    match record.record_kind() {
      Some(EmfRecordType::BeginPath) => {
        if open {
          return Err(Error::invalid(
            0,
            "EMR_BEGINPATH encountered while path bracket construction is open",
          ));
        }
        open = true;
      }
      Some(EmfRecordType::EndPath | EmfRecordType::AbortPath) => {
        if !open {
          return Err(Error::invalid(
            0,
            "EMR_ENDPATH or EMR_ABORTPATH encountered without EMR_BEGINPATH",
          ));
        }
        open = false;
      }
      _ => {}
    }
  }
  if open {
    Err(Error::invalid(
      0,
      "EMR_BEGINPATH is not closed by EMR_ENDPATH or EMR_ABORTPATH",
    ))
  } else {
    Ok(())
  }
}

fn validate_unknown_emf_record(record_type: u32) -> Result<()> {
  if EmfRecordType::from_raw(record_type).is_some() {
    return Err(Error::invalid(
      0,
      "EMF Unknown record requires an unknown RecordType",
    ));
  }
  Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmfRecordData<'a> {
  Header(EmfHeader),
  Eof(EmrEof),
  PolyBezier(EmrPolyPointsL),
  SetWindowExtEx(EmrSetWindowExtEx),
  SetWindowOrgEx(EmrSetWindowOrgEx),
  SetViewportExtEx(EmrSetViewportExtEx),
  SetViewportOrgEx(EmrSetViewportOrgEx),
  SetBrushOrgEx(EmrSetBrushOrgEx),
  SetPixelV(EmrSetPixelV),
  SetMapperFlags(EmrSetMapperFlags),
  SetMapMode(EmrSetMapMode),
  SetBkMode(EmrSetBkMode),
  SetPolyFillMode(EmrSetPolyFillMode),
  SetRop2(EmrSetRop2),
  SetStretchBltMode(EmrSetStretchBltMode),
  SetTextAlign(EmrSetTextAlign),
  SetColorAdjustment(EmrSetColorAdjustment),
  SetTextColor(EmrSetTextColor),
  SetBkColor(EmrSetBkColor),
  OffsetClipRgn(EmrOffsetClipRgn),
  SetMetaRgn,
  ExcludeClipRect(EmrExcludeClipRect),
  IntersectClipRect(EmrIntersectClipRect),
  ScaleViewportExtEx(EmrScaleViewportExtEx),
  ScaleWindowExtEx(EmrScaleWindowExtEx),
  SaveDc,
  RestoreDc(EmrRestoreDc),
  SetWorldTransform(EmrSetWorldTransform),
  ModifyWorldTransform(EmrModifyWorldTransform),
  SelectObject(EmrSelectObject),
  SelectPalette(EmrSelectPalette),
  ResizePalette(EmrResizePalette),
  DeleteObject(EmrDeleteObject),
  MoveToEx(EmrMoveToEx),
  LineTo(EmrLineTo),
  AngleArc(EmrAngleArc),
  RoundRect(EmrRoundRect),
  Arc(EmrArc),
  ArcTo(EmrArc),
  Chord(EmrArc),
  Pie(EmrArc),
  ExtFloodFill(EmrExtFloodFill),
  SetArcDirection(EmrSetArcDirection),
  SetMiterLimit(EmrSetMiterLimit),
  BeginPath,
  EndPath,
  CloseFigure,
  FillPath(EmrFillPath),
  StrokeAndFillPath(EmrStrokeAndFillPath),
  StrokePath(EmrStrokePath),
  FlattenPath,
  WidenPath,
  SelectClipPath(EmrSelectClipPath),
  AbortPath,
  FillRgn(EmrFillRgn),
  FrameRgn(EmrFrameRgn),
  InvertRgn(EmrRgnDataRecord),
  PaintRgn(EmrRgnDataRecord),
  ExtSelectClipRgn(EmrExtSelectClipRgn),
  CreatePen(EmrCreatePen),
  CreateBrushIndirect(EmrCreateBrushIndirect),
  CreatePalette(EmrCreatePalette),
  SetPaletteEntries(EmrSetPaletteEntries),
  ExtCreatePen(EmrExtCreatePen),
  ExtCreateFontIndirectW(EmrExtCreateFontIndirectW),
  CreateColorSpace(EmrCreateColorSpace),
  CreateColorSpaceW(EmrCreateColorSpaceW),
  CreateMonoBrush(EmrCreateMonoBrush),
  CreateDibPatternBrushPt(EmrCreateDibPatternBrushPt),
  Polygon(EmrPolyPointsL),
  Polyline(EmrPolyPointsL),
  PolyBezierTo(EmrPolyPointsL),
  PolylineTo(EmrPolyPointsL),
  PolyDraw(EmrPolyDrawL),
  Polygon16(EmrPolyPointsS),
  Polyline16(EmrPolyPointsS),
  PolyBezier16(EmrPolyPointsS),
  PolyBezierTo16(EmrPolyPointsS),
  PolylineTo16(EmrPolyPointsS),
  PolyDraw16(EmrPolyDrawS),
  PolyPolyline(EmrPolyPolygonL),
  PolyPolygon(EmrPolyPolygonL),
  PolyPolyline16(EmrPolyPolygonS),
  PolyPolygon16(EmrPolyPolygonS),
  Rectangle(EmrRectangle),
  Ellipse(EmrEllipse),
  RealizePalette,
  ExtTextOutA(EmrExtTextOut),
  ExtTextOutW(EmrExtTextOut),
  PolyTextOutA(EmrPolyTextOut),
  PolyTextOutW(EmrPolyTextOut),
  SmallTextOut(EmrSmallTextOut),
  SetDiBitsToDevice(EmrSetDiBitsToDevice),
  StretchDiBits(EmrStretchDiBits),
  BitBlt(EmrBitBlt),
  StretchBlt(EmrStretchBlt),
  MaskBlt(EmrMaskBlt),
  PlgBlt(EmrPlgBlt),
  AlphaBlend(EmrAlphaBlend),
  TransparentBlt(EmrTransparentBlt),
  GradientFill(EmrGradientFill),
  GlsRecord(EmrOpenGlRecord),
  GlsBoundedRecord(EmrGlsBoundedRecord),
  PixelFormat(EmrPixelFormat),
  DrawEscape(EmrEscape),
  ExtEscape(EmrEscape),
  NamedEscape(EmrNamedEscape),
  ColorCorrectPalette(EmrColorCorrectPalette),
  ForceUfiMapping(EmrForceUfiMapping),
  SetIcmProfileA(EmrColorProfile),
  SetIcmProfileW(EmrColorProfile),
  SetLinkedUfis(EmrSetLinkedUfis),
  ColorMatchToTargetW(EmrColorMatchToTargetW),
  SetIcmMode(EmrSetIcmMode),
  SetColorSpace(EmrSetColorSpace),
  DeleteColorSpace(EmrDeleteColorSpace),
  SetLayout(EmrSetLayout),
  SetTextJustification(EmrSetTextJustification),
  Comment(EmrComment),
  Unknown(EmfRecordRef<'a>),
}

impl<'a> EmfRecordData<'a> {
  pub fn from_record(record: &'a EmfRecord) -> Result<Self> {
    Self::from_record_ref(record.as_ref())
  }

  pub fn from_record_ref(record: EmfRecordRef<'a>) -> Result<Self> {
    let data = record.data;
    Ok(match record.record_kind() {
      Some(EmfRecordType::Header) => Self::Header(EmfHeader::from_record_data(data)?),
      Some(EmfRecordType::Eof) => {
        let value = EmrEof::read_data(data)?;
        Self::Eof(value)
      }
      Some(EmfRecordType::PolyBezier) => {
        let value = EmrPolyPointsL::read_data(data)?;
        validate_emr_poly_bezier_points(value.points.len(), "EMR_POLYBEZIER")?;
        Self::PolyBezier(value)
      }
      Some(EmfRecordType::SetWindowExtEx) => Self::SetWindowExtEx(read_object(data)?),
      Some(EmfRecordType::SetWindowOrgEx) => Self::SetWindowOrgEx(read_object(data)?),
      Some(EmfRecordType::SetViewportExtEx) => Self::SetViewportExtEx(read_object(data)?),
      Some(EmfRecordType::SetViewportOrgEx) => Self::SetViewportOrgEx(read_object(data)?),
      Some(EmfRecordType::SetBrushOrgEx) => Self::SetBrushOrgEx(read_object(data)?),
      Some(EmfRecordType::SetPixelV) => Self::SetPixelV(read_object(data)?),
      Some(EmfRecordType::SetMapperFlags) => {
        let value = read_object(data)?;
        validate_emr_set_mapper_flags(&value)?;
        Self::SetMapperFlags(value)
      }
      Some(EmfRecordType::SetMapMode) => {
        let value = read_object(data)?;
        validate_emr_set_map_mode(&value)?;
        Self::SetMapMode(value)
      }
      Some(EmfRecordType::SetBkMode) => {
        let value = read_object(data)?;
        validate_emr_set_bk_mode(&value)?;
        Self::SetBkMode(value)
      }
      Some(EmfRecordType::SetPolyfillMode) => {
        let value = read_object(data)?;
        validate_emr_set_poly_fill_mode(&value)?;
        Self::SetPolyFillMode(value)
      }
      Some(EmfRecordType::SetRop2) => {
        let value = read_object(data)?;
        validate_emr_set_rop2(&value)?;
        Self::SetRop2(value)
      }
      Some(EmfRecordType::SetStretchBltMode) => {
        let value = read_object(data)?;
        validate_emr_set_stretch_blt_mode(&value)?;
        Self::SetStretchBltMode(value)
      }
      Some(EmfRecordType::SetTextAlign) => {
        let value = read_object(data)?;
        validate_emr_set_text_align(&value)?;
        Self::SetTextAlign(value)
      }
      Some(EmfRecordType::SetColorAdjustment) => {
        let value = read_object(data)?;
        validate_emr_set_color_adjustment(&value)?;
        Self::SetColorAdjustment(value)
      }
      Some(EmfRecordType::SetTextColor) => Self::SetTextColor(read_object(data)?),
      Some(EmfRecordType::SetBkColor) => Self::SetBkColor(read_object(data)?),
      Some(EmfRecordType::OffsetClipRgn) => Self::OffsetClipRgn(read_object(data)?),
      Some(EmfRecordType::SetMetaRgn) => {
        ensure_no_data(data, "EMR_SETMETARGN")?;
        Self::SetMetaRgn
      }
      Some(EmfRecordType::ExcludeClipRect) => Self::ExcludeClipRect(read_object(data)?),
      Some(EmfRecordType::IntersectClipRect) => Self::IntersectClipRect(read_object(data)?),
      Some(EmfRecordType::ScaleViewportExtEx) => {
        let value: EmrScaleViewportExtEx = read_object(data)?;
        validate_emr_scale_ext(
          value.x_num,
          value.x_denom,
          value.y_num,
          value.y_denom,
          "EMR_SCALEVIEWPORTEXTEX",
        )?;
        Self::ScaleViewportExtEx(value)
      }
      Some(EmfRecordType::ScaleWindowExtEx) => {
        let value: EmrScaleWindowExtEx = read_object(data)?;
        validate_emr_scale_ext(
          value.x_num,
          value.x_denom,
          value.y_num,
          value.y_denom,
          "EMR_SCALEWINDOWEXTEX",
        )?;
        Self::ScaleWindowExtEx(value)
      }
      Some(EmfRecordType::SaveDc) => {
        ensure_no_data(data, "EMR_SAVEDC")?;
        Self::SaveDc
      }
      Some(EmfRecordType::RestoreDc) => {
        let value = read_object(data)?;
        validate_emr_restore_dc(&value)?;
        Self::RestoreDc(value)
      }
      Some(EmfRecordType::SetWorldTransform) => Self::SetWorldTransform(read_object(data)?),
      Some(EmfRecordType::ModifyWorldTransform) => {
        let value = read_object(data)?;
        validate_emr_modify_world_transform(&value)?;
        Self::ModifyWorldTransform(value)
      }
      Some(EmfRecordType::SelectObject) => {
        let value = read_object(data)?;
        validate_emr_select_object(&value)?;
        Self::SelectObject(value)
      }
      Some(EmfRecordType::SelectPalette) => {
        let value = read_object(data)?;
        validate_emr_select_palette(&value)?;
        Self::SelectPalette(value)
      }
      Some(EmfRecordType::ResizePalette) => {
        let value = read_object(data)?;
        validate_emr_resize_palette(&value)?;
        Self::ResizePalette(value)
      }
      Some(EmfRecordType::DeleteObject) => {
        let value = read_object(data)?;
        validate_emr_delete_object(&value)?;
        Self::DeleteObject(value)
      }
      Some(EmfRecordType::MoveToEx) => Self::MoveToEx(read_object(data)?),
      Some(EmfRecordType::LineTo) => Self::LineTo(read_object(data)?),
      Some(EmfRecordType::AngleArc) => Self::AngleArc(read_object(data)?),
      Some(EmfRecordType::RoundRect) => Self::RoundRect(read_object(data)?),
      Some(EmfRecordType::Arc) => Self::Arc(read_object(data)?),
      Some(EmfRecordType::ArcTo) => Self::ArcTo(read_object(data)?),
      Some(EmfRecordType::Chord) => Self::Chord(read_object(data)?),
      Some(EmfRecordType::Pie) => Self::Pie(read_object(data)?),
      Some(EmfRecordType::ExtFloodFill) => {
        let value = read_object(data)?;
        validate_emr_ext_flood_fill(&value)?;
        Self::ExtFloodFill(value)
      }
      Some(EmfRecordType::SetArcDirection) => {
        let value = read_object(data)?;
        validate_emr_set_arc_direction(&value)?;
        Self::SetArcDirection(value)
      }
      Some(EmfRecordType::SetMiterLimit) => Self::SetMiterLimit(read_object(data)?),
      Some(EmfRecordType::BeginPath) => {
        ensure_no_data(data, "EMR_BEGINPATH")?;
        Self::BeginPath
      }
      Some(EmfRecordType::EndPath) => {
        ensure_no_data(data, "EMR_ENDPATH")?;
        Self::EndPath
      }
      Some(EmfRecordType::CloseFigure) => {
        ensure_no_data(data, "EMR_CLOSEFIGURE")?;
        Self::CloseFigure
      }
      Some(EmfRecordType::FillPath) => Self::FillPath(read_object(data)?),
      Some(EmfRecordType::StrokeAndFillPath) => Self::StrokeAndFillPath(read_object(data)?),
      Some(EmfRecordType::StrokePath) => Self::StrokePath(read_object(data)?),
      Some(EmfRecordType::FlattenPath) => {
        ensure_no_data(data, "EMR_FLATTENPATH")?;
        Self::FlattenPath
      }
      Some(EmfRecordType::WidenPath) => {
        ensure_no_data(data, "EMR_WIDENPATH")?;
        Self::WidenPath
      }
      Some(EmfRecordType::SelectClipPath) => {
        let value = read_object(data)?;
        validate_emr_select_clip_path(&value)?;
        Self::SelectClipPath(value)
      }
      Some(EmfRecordType::AbortPath) => {
        ensure_no_data(data, "EMR_ABORTPATH")?;
        Self::AbortPath
      }
      Some(EmfRecordType::FillRgn) => Self::FillRgn(EmrFillRgn::read_data(data)?),
      Some(EmfRecordType::FrameRgn) => Self::FrameRgn(EmrFrameRgn::read_data(data)?),
      Some(EmfRecordType::InvertRgn) => {
        Self::InvertRgn(EmrRgnDataRecord::read_data(data, "EMR_INVERTRGN")?)
      }
      Some(EmfRecordType::PaintRgn) => {
        Self::PaintRgn(EmrRgnDataRecord::read_data(data, "EMR_PAINTRGN")?)
      }
      Some(EmfRecordType::ExtSelectClipRgn) => {
        Self::ExtSelectClipRgn(EmrExtSelectClipRgn::read_data(data)?)
      }
      Some(EmfRecordType::CreatePen) => {
        let value = read_object(data)?;
        validate_emr_create_pen(&value)?;
        Self::CreatePen(value)
      }
      Some(EmfRecordType::CreateBrushIndirect) => {
        let value = read_object(data)?;
        validate_emr_create_brush_indirect(&value)?;
        Self::CreateBrushIndirect(value)
      }
      Some(EmfRecordType::CreatePalette) => Self::CreatePalette(EmrCreatePalette::read_data(data)?),
      Some(EmfRecordType::SetPaletteEntries) => {
        Self::SetPaletteEntries(EmrSetPaletteEntries::read_data(data)?)
      }
      Some(EmfRecordType::ExtCreatePen) => {
        let value = EmrExtCreatePen::read_data(data)?;
        validate_emr_ext_create_pen(&value)?;
        Self::ExtCreatePen(value)
      }
      Some(EmfRecordType::ExtCreateFontIndirectW) => {
        Self::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW::read_data(data)?)
      }
      Some(EmfRecordType::CreateColorSpace) => {
        Self::CreateColorSpace(EmrCreateColorSpace::read_data(data)?)
      }
      Some(EmfRecordType::CreateColorSpaceW) => {
        Self::CreateColorSpaceW(EmrCreateColorSpaceW::read_data(data)?)
      }
      Some(EmfRecordType::CreateMonoBrush) => {
        Self::CreateMonoBrush(EmrCreateMonoBrush::read_data(data)?)
      }
      Some(EmfRecordType::CreateDibPatternBrushPt) => {
        Self::CreateDibPatternBrushPt(EmrCreateDibPatternBrushPt::read_data(data)?)
      }
      Some(EmfRecordType::Polygon) => Self::Polygon(EmrPolyPointsL::read_data(data)?),
      Some(EmfRecordType::Polyline) => Self::Polyline(EmrPolyPointsL::read_data(data)?),
      Some(EmfRecordType::PolyBezierTo) => {
        let value = EmrPolyPointsL::read_data(data)?;
        validate_emr_poly_bezier_to_points(value.points.len(), "EMR_POLYBEZIERTO")?;
        Self::PolyBezierTo(value)
      }
      Some(EmfRecordType::PolylineTo) => Self::PolylineTo(EmrPolyPointsL::read_data(data)?),
      Some(EmfRecordType::PolyDraw) => Self::PolyDraw(EmrPolyDrawL::read_data(data)?),
      Some(EmfRecordType::PolyPolyline) => {
        Self::PolyPolyline(EmrPolyPolygonL::read_polyline_data(data)?)
      }
      Some(EmfRecordType::PolyBezier16) => {
        let value = EmrPolyPointsS::read_data(data)?;
        validate_emr_poly_bezier_points(value.points.len(), "EMR_POLYBEZIER16")?;
        Self::PolyBezier16(value)
      }
      Some(EmfRecordType::Polygon16) => Self::Polygon16(EmrPolyPointsS::read_data(data)?),
      Some(EmfRecordType::Polyline16) => Self::Polyline16(EmrPolyPointsS::read_data(data)?),
      Some(EmfRecordType::PolyBezierTo16) => {
        let value = EmrPolyPointsS::read_data(data)?;
        validate_emr_poly_bezier_to_points(value.points.len(), "EMR_POLYBEZIERTO16")?;
        Self::PolyBezierTo16(value)
      }
      Some(EmfRecordType::PolylineTo16) => Self::PolylineTo16(EmrPolyPointsS::read_data(data)?),
      Some(EmfRecordType::PolyDraw16) => Self::PolyDraw16(EmrPolyDrawS::read_data(data)?),
      Some(EmfRecordType::PolyPolygon) => Self::PolyPolygon(EmrPolyPolygonL::read_data(data)?),
      Some(EmfRecordType::PolyPolyline16) => {
        Self::PolyPolyline16(EmrPolyPolygonS::read_polyline_data(data)?)
      }
      Some(EmfRecordType::PolyPolygon16) => Self::PolyPolygon16(EmrPolyPolygonS::read_data(data)?),
      Some(EmfRecordType::Rectangle) => Self::Rectangle(read_object(data)?),
      Some(EmfRecordType::Ellipse) => Self::Ellipse(read_object(data)?),
      Some(EmfRecordType::RealizePalette) => {
        ensure_no_data(data, "EMR_REALIZEPALETTE")?;
        Self::RealizePalette
      }
      Some(EmfRecordType::ExtTextOutA) => Self::ExtTextOutA(EmrExtTextOut::read_data(data, false)?),
      Some(EmfRecordType::ExtTextOutW) => Self::ExtTextOutW(EmrExtTextOut::read_data(data, true)?),
      Some(EmfRecordType::PolyTextOutA) => {
        Self::PolyTextOutA(EmrPolyTextOut::read_data(data, false)?)
      }
      Some(EmfRecordType::PolyTextOutW) => {
        Self::PolyTextOutW(EmrPolyTextOut::read_data(data, true)?)
      }
      Some(EmfRecordType::SmallTextOut) => Self::SmallTextOut(EmrSmallTextOut::read_data(data)?),
      Some(EmfRecordType::SetDiBitsToDevice) => {
        Self::SetDiBitsToDevice(EmrSetDiBitsToDevice::read_data(data)?)
      }
      Some(EmfRecordType::StretchDiBits) => Self::StretchDiBits(EmrStretchDiBits::read_data(data)?),
      Some(EmfRecordType::BitBlt) => Self::BitBlt(EmrBitBlt::read_data(data)?),
      Some(EmfRecordType::StretchBlt) => Self::StretchBlt(EmrStretchBlt::read_data(data)?),
      Some(EmfRecordType::MaskBlt) => Self::MaskBlt(EmrMaskBlt::read_data(data)?),
      Some(EmfRecordType::PlgBlt) => Self::PlgBlt(EmrPlgBlt::read_data(data)?),
      Some(EmfRecordType::AlphaBlend) => Self::AlphaBlend(EmrAlphaBlend::read_data(data)?),
      Some(EmfRecordType::TransparentBlt) => {
        Self::TransparentBlt(EmrTransparentBlt::read_data(data)?)
      }
      Some(EmfRecordType::GradientFill) => Self::GradientFill(EmrGradientFill::read_data(data)?),
      Some(EmfRecordType::GlsRecord) => Self::GlsRecord(EmrOpenGlRecord::read_data(data)?),
      Some(EmfRecordType::GlsBoundedRecord) => {
        Self::GlsBoundedRecord(EmrGlsBoundedRecord::read_data(data)?)
      }
      Some(EmfRecordType::PixelFormat) => {
        let value = read_object(data)?;
        validate_emr_pixel_format(&value)?;
        Self::PixelFormat(value)
      }
      Some(EmfRecordType::DrawEscape) => Self::DrawEscape(EmrEscape::read_data(data)?),
      Some(EmfRecordType::ExtEscape) => Self::ExtEscape(EmrEscape::read_data(data)?),
      Some(EmfRecordType::NamedEscape) => Self::NamedEscape(EmrNamedEscape::read_data(data)?),
      Some(EmfRecordType::ColorCorrectPalette) => {
        let value = read_object(data)?;
        validate_emr_color_correct_palette(&value)?;
        Self::ColorCorrectPalette(value)
      }
      Some(EmfRecordType::ForceUfiMapping) => Self::ForceUfiMapping(read_object(data)?),
      Some(EmfRecordType::SetIcmProfileA) => Self::SetIcmProfileA(EmrColorProfile::read_data(
        data,
        SdkEncoding::Windows1252,
        "EMR_SETICMPROFILEA",
      )?),
      Some(EmfRecordType::SetIcmProfileW) => Self::SetIcmProfileW(EmrColorProfile::read_data(
        data,
        SdkEncoding::Utf16Le,
        "EMR_SETICMPROFILEW",
      )?),
      Some(EmfRecordType::SetLinkedUfis) => Self::SetLinkedUfis(EmrSetLinkedUfis::read_data(data)?),
      Some(EmfRecordType::ColorMatchToTargetW) => {
        let value = EmrColorMatchToTargetW::read_data(data)?;
        validate_emr_color_match_to_target_w(&value)?;
        Self::ColorMatchToTargetW(value)
      }
      Some(EmfRecordType::SetIcmMode) => {
        let value = read_object(data)?;
        validate_emr_set_icm_mode(&value)?;
        Self::SetIcmMode(value)
      }
      Some(EmfRecordType::SetColorSpace) => {
        let value = read_object(data)?;
        validate_emr_set_color_space(&value)?;
        Self::SetColorSpace(value)
      }
      Some(EmfRecordType::DeleteColorSpace) => {
        let value = read_object(data)?;
        validate_emr_delete_color_space(&value)?;
        Self::DeleteColorSpace(value)
      }
      Some(EmfRecordType::SetLayout) => {
        let value = read_object(data)?;
        validate_emr_set_layout(&value)?;
        Self::SetLayout(value)
      }
      Some(EmfRecordType::SetTextJustification) => Self::SetTextJustification(read_object(data)?),
      Some(EmfRecordType::Comment) => Self::Comment(EmrComment::read_data(data)?),
      _ => Self::Unknown(record),
    })
  }

  pub fn validate_strict(&self) -> Result<()> {
    match self {
      Self::Header(value) => value.validate_strict(),
      Self::Eof(value) => {
        let data = value.to_data()?;
        validate_emr_eof_size_last(value, data.len() + 8)
      }
      Self::SetPixelV(value) => value.color.validate_strict(),
      Self::SetTextColor(value) => value.color.validate_strict(),
      Self::SetBkColor(value) => value.color.validate_strict(),
      Self::SetTextAlign(value) => validate_emr_set_text_align_strict(value),
      Self::ExtFloodFill(value) => value.color.validate_strict(),
      Self::CreatePen(value) => validate_emr_create_pen_strict(value),
      Self::CreateBrushIndirect(value) => validate_emr_create_brush_indirect_strict(value),
      Self::ExtCreatePen(value) => validate_emr_ext_create_pen_strict(value),
      Self::ExtCreateFontIndirectW(value) => validate_emr_ext_create_font_strict(&value.font),
      Self::CreateColorSpace(value) => validate_emr_create_color_space_strict(value),
      Self::BitBlt(value) => value.background_color_source.validate_strict(),
      Self::StretchBlt(value) => value.background_color_source.validate_strict(),
      Self::MaskBlt(value) => value.background_color_source.validate_strict(),
      Self::PlgBlt(value) => {
        value.background_color_source.validate_strict()?;
        validate_emr_plg_blt_strict(value)
      }
      Self::AlphaBlend(value) => value.background_color_source.validate_strict(),
      Self::TransparentBlt(value) => {
        value.transparent_color.validate_strict()?;
        value.background_color_source.validate_strict()
      }
      Self::ExtTextOutA(value) => validate_emr_ext_text_out_strict(value, false, "EMR_EXTTEXTOUTA"),
      Self::ExtTextOutW(value) => validate_emr_ext_text_out_strict(value, true, "EMR_EXTTEXTOUTW"),
      Self::PolyTextOutA(value) => validate_emr_poly_text_out_strict(value, false),
      Self::PolyTextOutW(value) => validate_emr_poly_text_out_strict(value, true),
      Self::Comment(value) => value.validate_strict(),
      _ => Ok(()),
    }
  }

  pub fn to_record(&self) -> Result<EmfRecord> {
    match self {
      Self::Header(value) => Ok(EmfRecord::new(EMR_HEADER, value.to_record_data()?)),
      Self::Eof(value) => {
        let data = value.to_data()?;
        Ok(EmfRecord::new(EMR_EOF, data))
      }
      Self::PolyBezier(value) => Ok(EmfRecord::new(EmfRecordType::PolyBezier.raw(), {
        validate_emr_poly_bezier_points(value.points.len(), "EMR_POLYBEZIER")?;
        value.to_data()?
      })),
      Self::SetWindowExtEx(value) => object_record(EmfRecordType::SetWindowExtEx, value),
      Self::SetWindowOrgEx(value) => object_record(EmfRecordType::SetWindowOrgEx, value),
      Self::SetViewportExtEx(value) => object_record(EmfRecordType::SetViewportExtEx, value),
      Self::SetViewportOrgEx(value) => object_record(EmfRecordType::SetViewportOrgEx, value),
      Self::SetBrushOrgEx(value) => object_record(EmfRecordType::SetBrushOrgEx, value),
      Self::SetPixelV(value) => object_record(EmfRecordType::SetPixelV, value),
      Self::SetMapperFlags(value) => {
        validate_emr_set_mapper_flags(value)?;
        object_record(EmfRecordType::SetMapperFlags, value)
      }
      Self::SetMapMode(value) => {
        validate_emr_set_map_mode(value)?;
        object_record(EmfRecordType::SetMapMode, value)
      }
      Self::SetBkMode(value) => {
        validate_emr_set_bk_mode(value)?;
        object_record(EmfRecordType::SetBkMode, value)
      }
      Self::SetPolyFillMode(value) => {
        validate_emr_set_poly_fill_mode(value)?;
        object_record(EmfRecordType::SetPolyfillMode, value)
      }
      Self::SetRop2(value) => {
        validate_emr_set_rop2(value)?;
        object_record(EmfRecordType::SetRop2, value)
      }
      Self::SetStretchBltMode(value) => {
        validate_emr_set_stretch_blt_mode(value)?;
        object_record(EmfRecordType::SetStretchBltMode, value)
      }
      Self::SetTextAlign(value) => {
        validate_emr_set_text_align(value)?;
        object_record(EmfRecordType::SetTextAlign, value)
      }
      Self::SetColorAdjustment(value) => {
        validate_emr_set_color_adjustment(value)?;
        object_record(EmfRecordType::SetColorAdjustment, value)
      }
      Self::SetTextColor(value) => object_record(EmfRecordType::SetTextColor, value),
      Self::SetBkColor(value) => object_record(EmfRecordType::SetBkColor, value),
      Self::OffsetClipRgn(value) => object_record(EmfRecordType::OffsetClipRgn, value),
      Self::SetMetaRgn => Ok(no_data_record(EmfRecordType::SetMetaRgn)),
      Self::ExcludeClipRect(value) => object_record(EmfRecordType::ExcludeClipRect, value),
      Self::IntersectClipRect(value) => object_record(EmfRecordType::IntersectClipRect, value),
      Self::ScaleViewportExtEx(value) => {
        validate_emr_scale_ext(
          value.x_num,
          value.x_denom,
          value.y_num,
          value.y_denom,
          "EMR_SCALEVIEWPORTEXTEX",
        )?;
        object_record(EmfRecordType::ScaleViewportExtEx, value)
      }
      Self::ScaleWindowExtEx(value) => {
        validate_emr_scale_ext(
          value.x_num,
          value.x_denom,
          value.y_num,
          value.y_denom,
          "EMR_SCALEWINDOWEXTEX",
        )?;
        object_record(EmfRecordType::ScaleWindowExtEx, value)
      }
      Self::SaveDc => Ok(no_data_record(EmfRecordType::SaveDc)),
      Self::RestoreDc(value) => {
        validate_emr_restore_dc(value)?;
        object_record(EmfRecordType::RestoreDc, value)
      }
      Self::SetWorldTransform(value) => object_record(EmfRecordType::SetWorldTransform, value),
      Self::ModifyWorldTransform(value) => {
        validate_emr_modify_world_transform(value)?;
        object_record(EmfRecordType::ModifyWorldTransform, value)
      }
      Self::SelectObject(value) => {
        validate_emr_select_object(value)?;
        object_record(EmfRecordType::SelectObject, value)
      }
      Self::SelectPalette(value) => {
        validate_emr_select_palette(value)?;
        object_record(EmfRecordType::SelectPalette, value)
      }
      Self::ResizePalette(value) => {
        validate_emr_resize_palette(value)?;
        object_record(EmfRecordType::ResizePalette, value)
      }
      Self::DeleteObject(value) => {
        validate_emr_delete_object(value)?;
        object_record(EmfRecordType::DeleteObject, value)
      }
      Self::MoveToEx(value) => object_record(EmfRecordType::MoveToEx, value),
      Self::LineTo(value) => object_record(EmfRecordType::LineTo, value),
      Self::AngleArc(value) => object_record(EmfRecordType::AngleArc, value),
      Self::RoundRect(value) => object_record(EmfRecordType::RoundRect, value),
      Self::Arc(value) => object_record(EmfRecordType::Arc, value),
      Self::ArcTo(value) => object_record(EmfRecordType::ArcTo, value),
      Self::Chord(value) => object_record(EmfRecordType::Chord, value),
      Self::Pie(value) => object_record(EmfRecordType::Pie, value),
      Self::ExtFloodFill(value) => {
        validate_emr_ext_flood_fill(value)?;
        object_record(EmfRecordType::ExtFloodFill, value)
      }
      Self::SetArcDirection(value) => {
        validate_emr_set_arc_direction(value)?;
        object_record(EmfRecordType::SetArcDirection, value)
      }
      Self::SetMiterLimit(value) => object_record(EmfRecordType::SetMiterLimit, value),
      Self::BeginPath => Ok(no_data_record(EmfRecordType::BeginPath)),
      Self::EndPath => Ok(no_data_record(EmfRecordType::EndPath)),
      Self::CloseFigure => Ok(no_data_record(EmfRecordType::CloseFigure)),
      Self::FillPath(value) => object_record(EmfRecordType::FillPath, value),
      Self::StrokeAndFillPath(value) => object_record(EmfRecordType::StrokeAndFillPath, value),
      Self::StrokePath(value) => object_record(EmfRecordType::StrokePath, value),
      Self::FlattenPath => Ok(no_data_record(EmfRecordType::FlattenPath)),
      Self::WidenPath => Ok(no_data_record(EmfRecordType::WidenPath)),
      Self::SelectClipPath(value) => {
        validate_emr_select_clip_path(value)?;
        object_record(EmfRecordType::SelectClipPath, value)
      }
      Self::AbortPath => Ok(no_data_record(EmfRecordType::AbortPath)),
      Self::FillRgn(value) => Ok(EmfRecord::new(
        EmfRecordType::FillRgn.raw(),
        value.to_data()?,
      )),
      Self::FrameRgn(value) => Ok(EmfRecord::new(
        EmfRecordType::FrameRgn.raw(),
        value.to_data()?,
      )),
      Self::InvertRgn(value) => Ok(EmfRecord::new(
        EmfRecordType::InvertRgn.raw(),
        value.to_data()?,
      )),
      Self::PaintRgn(value) => Ok(EmfRecord::new(
        EmfRecordType::PaintRgn.raw(),
        value.to_data()?,
      )),
      Self::ExtSelectClipRgn(value) => Ok(EmfRecord::new(
        EmfRecordType::ExtSelectClipRgn.raw(),
        value.to_data()?,
      )),
      Self::CreatePen(value) => {
        validate_emr_create_pen(value)?;
        object_record(EmfRecordType::CreatePen, value)
      }
      Self::CreateBrushIndirect(value) => {
        validate_emr_create_brush_indirect(value)?;
        object_record(EmfRecordType::CreateBrushIndirect, value)
      }
      Self::CreatePalette(value) => Ok(EmfRecord::new(
        EmfRecordType::CreatePalette.raw(),
        value.to_data()?,
      )),
      Self::SetPaletteEntries(value) => Ok(EmfRecord::new(
        EmfRecordType::SetPaletteEntries.raw(),
        value.to_data()?,
      )),
      Self::ExtCreatePen(value) => {
        validate_emr_ext_create_pen(value)?;
        Ok(EmfRecord::new(
          EmfRecordType::ExtCreatePen.raw(),
          value.to_data()?,
        ))
      }
      Self::ExtCreateFontIndirectW(value) => Ok(EmfRecord::new(
        EmfRecordType::ExtCreateFontIndirectW.raw(),
        value.to_data()?,
      )),
      Self::CreateColorSpace(value) => Ok(EmfRecord::new(
        EmfRecordType::CreateColorSpace.raw(),
        value.to_data()?,
      )),
      Self::CreateColorSpaceW(value) => Ok(EmfRecord::new(
        EmfRecordType::CreateColorSpaceW.raw(),
        value.to_data()?,
      )),
      Self::CreateMonoBrush(value) => Ok(EmfRecord::new(
        EmfRecordType::CreateMonoBrush.raw(),
        value.to_data()?,
      )),
      Self::CreateDibPatternBrushPt(value) => Ok(EmfRecord::new(
        EmfRecordType::CreateDibPatternBrushPt.raw(),
        value.to_data()?,
      )),
      Self::Polygon(value) => Ok(EmfRecord::new(
        EmfRecordType::Polygon.raw(),
        value.to_data()?,
      )),
      Self::Polyline(value) => Ok(EmfRecord::new(
        EmfRecordType::Polyline.raw(),
        value.to_data()?,
      )),
      Self::PolyBezierTo(value) => Ok(EmfRecord::new(EmfRecordType::PolyBezierTo.raw(), {
        validate_emr_poly_bezier_to_points(value.points.len(), "EMR_POLYBEZIERTO")?;
        value.to_data()?
      })),
      Self::PolylineTo(value) => Ok(EmfRecord::new(
        EmfRecordType::PolylineTo.raw(),
        value.to_data()?,
      )),
      Self::PolyDraw(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyDraw.raw(),
        value.to_data()?,
      )),
      Self::Polygon16(value) => Ok(EmfRecord::new(
        EmfRecordType::Polygon16.raw(),
        value.to_data()?,
      )),
      Self::Polyline16(value) => Ok(EmfRecord::new(
        EmfRecordType::Polyline16.raw(),
        value.to_data()?,
      )),
      Self::PolyBezier16(value) => Ok(EmfRecord::new(EmfRecordType::PolyBezier16.raw(), {
        validate_emr_poly_bezier_points(value.points.len(), "EMR_POLYBEZIER16")?;
        value.to_data()?
      })),
      Self::PolyBezierTo16(value) => Ok(EmfRecord::new(EmfRecordType::PolyBezierTo16.raw(), {
        validate_emr_poly_bezier_to_points(value.points.len(), "EMR_POLYBEZIERTO16")?;
        value.to_data()?
      })),
      Self::PolylineTo16(value) => Ok(EmfRecord::new(
        EmfRecordType::PolylineTo16.raw(),
        value.to_data()?,
      )),
      Self::PolyDraw16(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyDraw16.raw(),
        value.to_data()?,
      )),
      Self::PolyPolyline(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyPolyline.raw(),
        value.to_polyline_data()?,
      )),
      Self::PolyPolygon(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyPolygon.raw(),
        value.to_data()?,
      )),
      Self::PolyPolyline16(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyPolyline16.raw(),
        value.to_polyline_data()?,
      )),
      Self::PolyPolygon16(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyPolygon16.raw(),
        value.to_data()?,
      )),
      Self::Rectangle(value) => object_record(EmfRecordType::Rectangle, value),
      Self::Ellipse(value) => object_record(EmfRecordType::Ellipse, value),
      Self::RealizePalette => Ok(no_data_record(EmfRecordType::RealizePalette)),
      Self::ExtTextOutA(value) => Ok(EmfRecord::new(
        EmfRecordType::ExtTextOutA.raw(),
        value.to_data(false)?,
      )),
      Self::ExtTextOutW(value) => Ok(EmfRecord::new(
        EmfRecordType::ExtTextOutW.raw(),
        value.to_data(true)?,
      )),
      Self::PolyTextOutA(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyTextOutA.raw(),
        value.to_data(false)?,
      )),
      Self::PolyTextOutW(value) => Ok(EmfRecord::new(
        EmfRecordType::PolyTextOutW.raw(),
        value.to_data(true)?,
      )),
      Self::SmallTextOut(value) => Ok(EmfRecord::new(
        EmfRecordType::SmallTextOut.raw(),
        value.to_data()?,
      )),
      Self::SetDiBitsToDevice(value) => Ok(EmfRecord::new(
        EmfRecordType::SetDiBitsToDevice.raw(),
        value.to_data()?,
      )),
      Self::StretchDiBits(value) => Ok(EmfRecord::new(
        EmfRecordType::StretchDiBits.raw(),
        value.to_data()?,
      )),
      Self::BitBlt(value) => Ok(EmfRecord::new(
        EmfRecordType::BitBlt.raw(),
        value.to_data()?,
      )),
      Self::StretchBlt(value) => Ok(EmfRecord::new(
        EmfRecordType::StretchBlt.raw(),
        value.to_data()?,
      )),
      Self::MaskBlt(value) => Ok(EmfRecord::new(
        EmfRecordType::MaskBlt.raw(),
        value.to_data()?,
      )),
      Self::PlgBlt(value) => Ok(EmfRecord::new(
        EmfRecordType::PlgBlt.raw(),
        value.to_data()?,
      )),
      Self::AlphaBlend(value) => Ok(EmfRecord::new(
        EmfRecordType::AlphaBlend.raw(),
        value.to_data()?,
      )),
      Self::TransparentBlt(value) => Ok(EmfRecord::new(
        EmfRecordType::TransparentBlt.raw(),
        value.to_data()?,
      )),
      Self::GradientFill(value) => Ok(EmfRecord::new(
        EmfRecordType::GradientFill.raw(),
        value.to_data()?,
      )),
      Self::GlsRecord(value) => Ok(EmfRecord::new(
        EmfRecordType::GlsRecord.raw(),
        value.to_data()?,
      )),
      Self::GlsBoundedRecord(value) => Ok(EmfRecord::new(
        EmfRecordType::GlsBoundedRecord.raw(),
        value.to_data()?,
      )),
      Self::PixelFormat(value) => {
        validate_emr_pixel_format(value)?;
        object_record(EmfRecordType::PixelFormat, value)
      }
      Self::DrawEscape(value) => Ok(EmfRecord::new(
        EmfRecordType::DrawEscape.raw(),
        value.to_data()?,
      )),
      Self::ExtEscape(value) => Ok(EmfRecord::new(
        EmfRecordType::ExtEscape.raw(),
        value.to_data()?,
      )),
      Self::NamedEscape(value) => Ok(EmfRecord::new(
        EmfRecordType::NamedEscape.raw(),
        value.to_data()?,
      )),
      Self::ColorCorrectPalette(value) => {
        validate_emr_color_correct_palette(value)?;
        object_record(EmfRecordType::ColorCorrectPalette, value)
      }
      Self::ForceUfiMapping(value) => object_record(EmfRecordType::ForceUfiMapping, value),
      Self::SetIcmProfileA(value) => Ok(EmfRecord::new(
        EmfRecordType::SetIcmProfileA.raw(),
        value.to_data()?,
      )),
      Self::SetIcmProfileW(value) => Ok(EmfRecord::new(
        EmfRecordType::SetIcmProfileW.raw(),
        value.to_data()?,
      )),
      Self::SetLinkedUfis(value) => Ok(EmfRecord::new(
        EmfRecordType::SetLinkedUfis.raw(),
        value.to_data()?,
      )),
      Self::ColorMatchToTargetW(value) => {
        Ok(EmfRecord::new(EmfRecordType::ColorMatchToTargetW.raw(), {
          validate_emr_color_match_to_target_w(value)?;
          value.to_data()?
        }))
      }
      Self::SetIcmMode(value) => {
        validate_emr_set_icm_mode(value)?;
        object_record(EmfRecordType::SetIcmMode, value)
      }
      Self::SetColorSpace(value) => {
        validate_emr_set_color_space(value)?;
        object_record(EmfRecordType::SetColorSpace, value)
      }
      Self::DeleteColorSpace(value) => {
        validate_emr_delete_color_space(value)?;
        object_record(EmfRecordType::DeleteColorSpace, value)
      }
      Self::SetLayout(value) => {
        validate_emr_set_layout(value)?;
        object_record(EmfRecordType::SetLayout, value)
      }
      Self::SetTextJustification(value) => {
        object_record(EmfRecordType::SetTextJustification, value)
      }
      Self::Comment(value) => Ok(EmfRecord::new(
        EmfRecordType::Comment.raw(),
        value.to_data()?,
      )),
      Self::Unknown(record) => {
        validate_unknown_emf_record(record.record_type)?;
        Ok(EmfRecordRef::into_owned(*record))
      }
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct LogPaletteEntry {
  pub reserved: u8,
  pub blue: u8,
  pub green: u8,
  pub red: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogPalette {
  pub version: u16,
  pub entries: Vec<LogPaletteEntry>,
}

impl LogPalette {
  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Self::read_from_with_end(reader, None)
  }

  fn read_from_with_end<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    end: Option<u64>,
  ) -> Result<Self> {
    let version = reader.read_u16()?;
    let entry_count = reader.read_u16()? as usize;
    if let Some(end) = end {
      let entry_bytes = checked_record_array_bytes(entry_count, 4, "LogPalette entries")?;
      ensure_record_remaining(reader, end, entry_bytes, "LogPalette entries")?;
    }
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
      entries.push(LogPaletteEntry::read_from(reader)?);
    }
    let value = Self { version, entries };
    validate_log_palette(&value)?;
    Ok(value)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_log_palette(self)?;
    writer.write_u16(self.version)?;
    writer.write_u16(
      u16::try_from(self.entries.len())
        .map_err(|_| Error::invalid(0, "LogPalette entry count exceeds u16::MAX"))?,
    )?;
    for entry in &self.entries {
      entry.write_to(writer)?;
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrEof {
  pub palette_entries_offset: u32,
  pub palette_prefix: Vec<u8>,
  pub palette_entries: Vec<LogPaletteEntry>,
  pub palette_suffix: Vec<u8>,
  pub size_last: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCreatePalette {
  pub palette_index: u32,
  pub log_palette: LogPalette,
}

impl EmrCreatePalette {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let value = Self {
      palette_index: reader.read_u32()?,
      log_palette: LogPalette::read_from_with_end(&mut reader, Some(data.len() as u64))?,
    };
    ensure_reader_end(&mut reader, data.len() as u64, "EMR_CREATEPALETTE")?;
    validate_emr_create_palette(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_create_palette(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      8 + self.log_palette.entries.len() * 4,
    )));
    writer.write_u32(self.palette_index)?;
    self.log_palette.write_to(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrSetPaletteEntries {
  pub palette_index: u32,
  pub start: u32,
  pub entries: Vec<LogPaletteEntry>,
}

impl EmrSetPaletteEntries {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let palette_index = reader.read_u32()?;
    let start = reader.read_u32()?;
    let entry_count = reader.read_u32()? as usize;
    let entry_bytes = checked_record_array_bytes(entry_count, 4, "EMR_SETPALETTEENTRIES")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      entry_bytes,
      "EMR_SETPALETTEENTRIES entries",
    )?;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
      entries.push(LogPaletteEntry::read_from(&mut reader)?);
    }
    let value = Self {
      palette_index,
      start,
      entries,
    };
    ensure_reader_end(&mut reader, data.len() as u64, "EMR_SETPALETTEENTRIES")?;
    validate_emr_set_palette_entries(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_set_palette_entries(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(12 + self.entries.len() * 4)));
    writer.write_u32(self.palette_index)?;
    writer.write_u32(self.start)?;
    writer.write_u32(usize_to_u32(
      self.entries.len(),
      "EMR_SETPALETTEENTRIES entry count",
    )?)?;
    for entry in &self.entries {
      entry.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

impl EmrEof {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 12 {
      return Err(Error::invalid(8, "EMR_EOF record data is too small"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let palette_entry_count = reader.read_u32()? as usize;
    let palette_entries_offset = reader.read_u32()?;
    let size_last_start = data
      .len()
      .checked_sub(4)
      .ok_or_else(|| Error::invalid(8, "EMR_EOF record data is too small"))?;
    let size_last = u32::from_le_bytes(
      data[size_last_start..]
        .try_into()
        .expect("slice length checked"),
    );

    if palette_entry_count == 0 {
      return Ok(Self {
        palette_entries_offset,
        palette_prefix: data[8..size_last_start].to_vec(),
        palette_entries: Vec::new(),
        palette_suffix: Vec::new(),
        size_last,
      });
    }

    let entries_start = record_relative_data_offset(palette_entries_offset as usize)?;
    if entries_start < 8 {
      return Err(Error::invalid(
        8,
        "EMR_EOF palette entries overlap fixed fields",
      ));
    }
    let entries_len = palette_entry_count
      .checked_mul(4)
      .ok_or_else(|| Error::invalid(8, "EMR_EOF palette entry size overflows"))?;
    let entries_end = entries_start
      .checked_add(entries_len)
      .ok_or_else(|| Error::invalid(8, "EMR_EOF palette entry range overflows"))?;
    if entries_end > size_last_start {
      return Err(Error::invalid(
        8,
        "EMR_EOF palette entries exceed record payload",
      ));
    }

    let mut entries_reader = Reader::new(Cursor::new(&data[entries_start..entries_end]));
    let mut palette_entries = Vec::with_capacity(palette_entry_count);
    for _ in 0..palette_entry_count {
      palette_entries.push(LogPaletteEntry::read_from(&mut entries_reader)?);
    }

    Ok(Self {
      palette_entries_offset,
      palette_prefix: data[8..entries_start].to_vec(),
      palette_entries,
      palette_suffix: data[entries_end..size_last_start].to_vec(),
      size_last,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      12 + self.palette_prefix.len() + self.palette_entries.len() * 4,
    )));
    writer.write_u32(usize_to_u32(
      self.palette_entries.len(),
      "EMR_EOF palette entry count",
    )?)?;
    writer.write_u32(self.palette_entries_offset)?;

    if self.palette_entries.is_empty() {
      writer.write_all(&self.palette_prefix)?;
    } else {
      let entries_start = record_relative_data_offset(self.palette_entries_offset as usize)?;
      if entries_start < 8 {
        return Err(Error::invalid(
          8,
          "EMR_EOF palette entries overlap fixed fields",
        ));
      }
      let prefix_target_len = entries_start - 8;
      if self.palette_prefix.len() > prefix_target_len {
        return Err(Error::invalid(
          8,
          "EMR_EOF palette prefix exceeds palette entry offset",
        ));
      }
      writer.write_all(&self.palette_prefix)?;
      writer.write_all(&vec![0; prefix_target_len - self.palette_prefix.len()])?;
      for entry in &self.palette_entries {
        entry.write_to(&mut writer)?;
      }
    }

    writer.write_all(&self.palette_suffix)?;
    writer.write_u32(self.size_last)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetWindowExtEx {
  pub size: SizeL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetWindowOrgEx {
  pub origin: PointL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetViewportExtEx {
  pub size: SizeL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetViewportOrgEx {
  pub origin: PointL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetBrushOrgEx {
  pub origin: PointL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetPixelV {
  pub pixel: PointL,
  pub color: ColorRef,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_mapper_flags")]
pub struct EmrSetMapperFlags {
  pub flags: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_map_mode")]
pub struct EmrSetMapMode {
  pub map_mode: u32,
}

impl EmrSetMapMode {
  pub fn map_mode_kind(&self) -> Option<EmrMapMode> {
    EmrMapMode::from_raw(self.map_mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_bk_mode")]
pub struct EmrSetBkMode {
  pub background_mode: u32,
}

impl EmrSetBkMode {
  pub fn background_mode_kind(&self) -> Option<EmrBackgroundMode> {
    EmrBackgroundMode::from_raw(self.background_mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_poly_fill_mode")]
pub struct EmrSetPolyFillMode {
  pub polygon_fill_mode: u32,
}

impl EmrSetPolyFillMode {
  pub fn polygon_fill_mode_kind(&self) -> Option<EmrPolygonFillMode> {
    EmrPolygonFillMode::from_raw(self.polygon_fill_mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_rop2")]
pub struct EmrSetRop2 {
  pub rop2_mode: u32,
}

impl EmrSetRop2 {
  pub fn binary_raster_operation_kind(&self) -> Option<WmfBinaryRasterOperation> {
    u16::try_from(self.rop2_mode)
      .ok()
      .and_then(WmfBinaryRasterOperation::from_raw)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetStretchBltMode {
  pub stretch_mode: u32,
}

impl EmrSetStretchBltMode {
  pub fn stretch_mode_kind(&self) -> Option<EmrStretchMode> {
    EmrStretchMode::from_raw(self.stretch_mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_text_align")]
pub struct EmrSetTextAlign {
  pub text_alignment_mode: u32,
}

impl EmrSetTextAlign {
  pub fn text_alignment_flags(&self) -> WmfTextAlignmentModeFlags {
    WmfTextAlignmentModeFlags::from_bits_retain(self.text_alignment_mode as u16)
  }

  pub fn vertical_text_alignment_flags(&self) -> WmfVerticalTextAlignmentModeFlags {
    WmfVerticalTextAlignmentModeFlags::from_bits_retain(self.text_alignment_mode as u16)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_color_adjustment")]
pub struct EmrSetColorAdjustment {
  pub size: u16,
  pub values: u16,
  pub illuminant_index: u16,
  pub red_gamma: u16,
  pub green_gamma: u16,
  pub blue_gamma: u16,
  pub reference_black: u16,
  pub reference_white: u16,
  pub contrast: i16,
  pub brightness: i16,
  pub colorfulness: i16,
  pub red_green_tint: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmrColorAdjustmentGamma {
  pub raw: u16,
}

impl EmrColorAdjustmentGamma {
  pub const NO_CORRECTION: u16 = 10_000;
  pub const RECOMMENDED_MIN: u16 = 2_500;
  pub const RECOMMENDED_MAX: u16 = 65_000;

  pub const fn new(raw: u16) -> Self {
    Self { raw }
  }

  pub const fn is_no_correction(self) -> bool {
    self.raw == Self::NO_CORRECTION
  }

  pub fn is_in_recommended_range(self) -> bool {
    (Self::RECOMMENDED_MIN..=Self::RECOMMENDED_MAX).contains(&self.raw)
  }

  pub fn factor(self) -> f32 {
    f32::from(self.raw) / f32::from(Self::NO_CORRECTION)
  }
}

impl EmrSetColorAdjustment {
  pub fn color_adjustment_flags(&self) -> EmrColorAdjustmentFlags {
    EmrColorAdjustmentFlags::from_bits_retain(self.values)
  }

  pub fn illuminant_kind(&self) -> Option<EmrIlluminant> {
    EmrIlluminant::from_raw(self.illuminant_index)
  }

  pub const fn red_gamma_value(&self) -> EmrColorAdjustmentGamma {
    EmrColorAdjustmentGamma::new(self.red_gamma)
  }

  pub const fn green_gamma_value(&self) -> EmrColorAdjustmentGamma {
    EmrColorAdjustmentGamma::new(self.green_gamma)
  }

  pub const fn blue_gamma_value(&self) -> EmrColorAdjustmentGamma {
    EmrColorAdjustmentGamma::new(self.blue_gamma)
  }

  pub fn reference_black_in_recommended_range(&self) -> bool {
    (0..=4_000).contains(&self.reference_black)
  }

  pub fn reference_white_in_recommended_range(&self) -> bool {
    (6_000..=10_000).contains(&self.reference_white)
  }

  pub fn contrast_in_recommended_range(&self) -> bool {
    (-100..=100).contains(&self.contrast)
  }

  pub fn brightness_in_recommended_range(&self) -> bool {
    (-100..=100).contains(&self.brightness)
  }

  pub fn colorfulness_in_recommended_range(&self) -> bool {
    (-100..=100).contains(&self.colorfulness)
  }

  pub fn red_green_tint_in_recommended_range(&self) -> bool {
    (-100..=100).contains(&self.red_green_tint)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetTextColor {
  pub color: ColorRef,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetBkColor {
  pub color: ColorRef,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrOffsetClipRgn {
  pub offset: PointL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrExcludeClipRect {
  pub rect: RectL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrIntersectClipRect {
  pub rect: RectL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_scale_viewport_ext_ex")]
pub struct EmrScaleViewportExtEx {
  pub x_num: i32,
  pub x_denom: i32,
  pub y_num: i32,
  pub y_denom: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_scale_window_ext_ex")]
pub struct EmrScaleWindowExtEx {
  pub x_num: i32,
  pub x_denom: i32,
  pub y_num: i32,
  pub y_denom: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_restore_dc")]
pub struct EmrRestoreDc {
  pub saved_dc: i32,
}

#[derive(Clone, Debug, PartialEq, SdkObject)]
pub struct EmrSetWorldTransform {
  pub transform: XForm,
}

#[derive(Clone, Debug, PartialEq, SdkObject)]
#[sdk(validate = "validate_emr_modify_world_transform")]
pub struct EmrModifyWorldTransform {
  pub transform: XForm,
  pub mode: u32,
}

impl EmrModifyWorldTransform {
  pub fn mode_kind(&self) -> Option<EmrModifyWorldTransformMode> {
    EmrModifyWorldTransformMode::from_raw(self.mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_select_object")]
pub struct EmrSelectObject {
  pub object_index: u32,
}

impl EmrSelectObject {
  pub fn stock_object_kind(&self) -> Option<EmrStockObject> {
    EmrStockObject::from_raw(self.object_index)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_select_palette")]
pub struct EmrSelectPalette {
  pub palette_index: u32,
}

impl EmrSelectPalette {
  pub fn stock_object_kind(&self) -> Option<EmrStockObject> {
    EmrStockObject::from_raw(self.palette_index)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_resize_palette")]
pub struct EmrResizePalette {
  pub palette_index: u32,
  pub number_of_entries: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_delete_object")]
pub struct EmrDeleteObject {
  pub object_index: u32,
}

impl EmrDeleteObject {
  pub fn stock_object_kind(&self) -> Option<EmrStockObject> {
    EmrStockObject::from_raw(self.object_index)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrLineTo {
  pub point: PointL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrMoveToEx {
  pub point: PointL,
}

#[derive(Clone, Debug, PartialEq, SdkObject)]
pub struct EmrAngleArc {
  pub center: PointL,
  pub radius: u32,
  pub start_angle: f32,
  pub sweep_angle: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrRoundRect {
  pub bounds: RectL,
  pub corner: SizeL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrArc {
  pub box_bounds: RectL,
  pub start: PointL,
  pub end: PointL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_ext_flood_fill")]
pub struct EmrExtFloodFill {
  pub start: PointL,
  pub color: ColorRef,
  pub flood_fill_mode: u32,
}

impl EmrExtFloodFill {
  pub fn flood_fill_mode_kind(&self) -> Option<EmrFloodFillMode> {
    EmrFloodFillMode::from_raw(self.flood_fill_mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_arc_direction")]
pub struct EmrSetArcDirection {
  pub arc_direction: u32,
}

impl EmrSetArcDirection {
  pub fn arc_direction_kind(&self) -> Option<EmrArcDirection> {
    EmrArcDirection::from_raw(self.arc_direction)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetMiterLimit {
  pub miter_limit: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_select_clip_path")]
pub struct EmrSelectClipPath {
  pub region_mode: u32,
}

impl EmrSelectClipPath {
  pub fn region_mode_kind(&self) -> Option<EmrRegionMode> {
    EmrRegionMode::from_raw(self.region_mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrFillPath {
  pub bounds: RectL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrStrokeAndFillPath {
  pub bounds: RectL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrStrokePath {
  pub bounds: RectL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_region_data_header")]
pub struct RegionDataHeader {
  pub size: u32,
  pub region_type: u32,
  pub count_rects: u32,
  pub region_size: u32,
  pub bounds: RectL,
}

impl RegionDataHeader {
  pub const SIZE: u32 = 32;
  pub const TYPE_RECTANGLES: u32 = 1;
  pub const RECT_SIZE: u32 = 16;

  pub fn validate(&self) -> Result<()> {
    if self.size != Self::SIZE {
      return Err(Error::invalid(0, "RegionDataHeader Size must be 32"));
    }
    if self.region_type != Self::TYPE_RECTANGLES {
      return Err(Error::invalid(0, "RegionDataHeader Type must be 1"));
    }
    let expected_size = self
      .count_rects
      .checked_mul(Self::RECT_SIZE)
      .ok_or_else(|| Error::invalid(0, "RegionDataHeader RgnSize overflows"))?;
    if self.region_size != expected_size {
      return Err(Error::invalid(
        0,
        "RegionDataHeader RgnSize does not match CountRects",
      ));
    }
    Ok(())
  }
}

fn validate_region_data_header(value: &RegionDataHeader) -> Result<()> {
  value.validate()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionData {
  pub header: RegionDataHeader,
  pub rectangles: Vec<RectL>,
}

impl RegionData {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < RegionDataHeader::SIZE as usize {
      return Err(Error::invalid(0, "RegionData is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let header = RegionDataHeader::read_from(&mut reader)?;
    header.validate()?;
    let count = header.count_rects as usize;
    let rect_bytes = count
      .checked_mul(RegionDataHeader::RECT_SIZE as usize)
      .ok_or_else(|| Error::invalid(0, "RegionData rectangle data overflows"))?;
    if data.len() != RegionDataHeader::SIZE as usize + rect_bytes {
      return Err(Error::invalid(
        0,
        "RegionData length does not match RegionDataHeader",
      ));
    }
    let mut rectangles = Vec::with_capacity(count);
    for _ in 0..count {
      rectangles.push(RectL::read_from(&mut reader)?);
    }
    ensure_reader_end(&mut reader, data.len() as u64, "RegionData")?;
    Ok(Self { header, rectangles })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    self.header.validate()?;
    if self.header.count_rects as usize != self.rectangles.len() {
      return Err(Error::invalid(
        0,
        "RegionData CountRects does not match rectangle count",
      ));
    }
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      RegionDataHeader::SIZE as usize
        + self.rectangles.len() * RegionDataHeader::RECT_SIZE as usize,
    )));
    self.header.write_to(&mut writer)?;
    for rectangle in &self.rectangles {
      rectangle.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrRgnDataRecord {
  pub bounds: RectL,
  pub region_data: Vec<u8>,
}

impl EmrRgnDataRecord {
  pub fn typed_region_data(&self) -> Result<RegionData> {
    RegionData::read_data(&self.region_data)
  }

  pub fn read_data(data: &[u8], name: &str) -> Result<Self> {
    if data.len() < 20 {
      return Err(Error::invalid(0, format!("{name} record is too short")));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let region_data_size = reader.read_u32()? as usize;
    if data.len() < 20 + region_data_size {
      return Err(Error::invalid(
        0,
        format!("{name} region data is truncated"),
      ));
    }
    let region_data = reader.read_vec(region_data_size)?;
    ensure_reader_end(&mut reader, data.len() as u64, name)?;
    let value = Self {
      bounds,
      region_data,
    };
    validate_emr_rgn_data_record(&value, name)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_rgn_data_record(self, "EMR_RGN_DATA_RECORD")?;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.region_data.len(), "region data size")?)?;
    writer.write_all(&self.region_data)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrFillRgn {
  pub bounds: RectL,
  pub brush_index: u32,
  pub region_data: Vec<u8>,
}

impl EmrFillRgn {
  pub fn typed_region_data(&self) -> Result<RegionData> {
    RegionData::read_data(&self.region_data)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 24 {
      return Err(Error::invalid(0, "EMR_FILLRGN record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let region_data_size = reader.read_u32()? as usize;
    let brush_index = reader.read_u32()?;
    if data.len() < 24 + region_data_size {
      return Err(Error::invalid(0, "EMR_FILLRGN region data is truncated"));
    }
    let region_data = reader.read_vec(region_data_size)?;
    ensure_reader_end(&mut reader, data.len() as u64, "EMR_FILLRGN")?;
    let value = Self {
      bounds,
      brush_index,
      region_data,
    };
    validate_emr_fill_rgn(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_fill_rgn(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.region_data.len(), "region data size")?)?;
    writer.write_u32(self.brush_index)?;
    writer.write_all(&self.region_data)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrFrameRgn {
  pub bounds: RectL,
  pub brush_index: u32,
  pub width: i32,
  pub height: i32,
  pub region_data: Vec<u8>,
}

impl EmrFrameRgn {
  pub fn typed_region_data(&self) -> Result<RegionData> {
    RegionData::read_data(&self.region_data)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 32 {
      return Err(Error::invalid(0, "EMR_FRAMERGN record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let region_data_size = reader.read_u32()? as usize;
    let brush_index = reader.read_u32()?;
    let width = reader.read_i32()?;
    let height = reader.read_i32()?;
    if data.len() < 32 + region_data_size {
      return Err(Error::invalid(0, "EMR_FRAMERGN region data is truncated"));
    }
    let region_data = reader.read_vec(region_data_size)?;
    ensure_reader_end(&mut reader, data.len() as u64, "EMR_FRAMERGN")?;
    let value = Self {
      bounds,
      brush_index,
      width,
      height,
      region_data,
    };
    validate_emr_frame_rgn(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_frame_rgn(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.region_data.len(), "region data size")?)?;
    writer.write_u32(self.brush_index)?;
    writer.write_i32(self.width)?;
    writer.write_i32(self.height)?;
    writer.write_all(&self.region_data)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrExtSelectClipRgn {
  pub region_mode: u32,
  pub region_data: Vec<u8>,
}

impl EmrExtSelectClipRgn {
  pub fn region_mode_kind(&self) -> Option<EmrRegionMode> {
    EmrRegionMode::from_raw(self.region_mode)
  }

  pub fn typed_region_data(&self) -> Result<RegionData> {
    RegionData::read_data(&self.region_data)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 8 {
      return Err(Error::invalid(
        0,
        "EMR_EXTSELECTCLIPRGN record is too short",
      ));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let region_data_size = reader.read_u32()? as usize;
    let region_mode = reader.read_u32()?;
    if data.len() < 8 + region_data_size {
      return Err(Error::invalid(
        0,
        "EMR_EXTSELECTCLIPRGN region data is truncated",
      ));
    }
    let region_data = reader.read_vec(region_data_size)?;
    ensure_reader_end(&mut reader, data.len() as u64, "EMR_EXTSELECTCLIPRGN")?;
    let value = Self {
      region_mode,
      region_data,
    };
    validate_emr_ext_select_clip_rgn(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_ext_select_clip_rgn(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u32(usize_to_u32(self.region_data.len(), "region data size")?)?;
    writer.write_u32(self.region_mode)?;
    writer.write_all(&self.region_data)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_log_pen")]
pub struct LogPen {
  pub pen_style: u32,
  pub width: PointL,
  pub color: ColorRef,
}

impl LogPen {
  pub fn pen_style_flags(&self) -> EmrPenStyleFlags {
    EmrPenStyleFlags::from_bits_retain(self.pen_style)
  }

  pub const fn pen_line_style_raw(&self) -> u32 {
    self.pen_style & 0x0000_000F
  }

  pub fn pen_line_style_kind(&self) -> Option<EmrPenLineStyle> {
    EmrPenLineStyle::from_raw(self.pen_line_style_raw())
  }

  pub const fn pen_end_cap_raw(&self) -> u32 {
    self.pen_style & 0x0000_0F00
  }

  pub fn pen_end_cap_kind(&self) -> Option<EmrPenEndCap> {
    EmrPenEndCap::from_raw(self.pen_end_cap_raw())
  }

  pub const fn pen_join_raw(&self) -> u32 {
    self.pen_style & 0x0000_F000
  }

  pub fn pen_join_kind(&self) -> Option<EmrPenJoin> {
    EmrPenJoin::from_raw(self.pen_join_raw())
  }

  pub const fn pen_type_raw(&self) -> u32 {
    self.pen_style & 0x000F_0000
  }

  pub fn pen_type_kind(&self) -> Option<EmrPenType> {
    EmrPenType::from_raw(self.pen_type_raw())
  }

  pub const fn pen_reserved_bits(&self) -> u32 {
    self.pen_style & !(0x0000_000F | 0x0000_0F00 | 0x0000_F000 | 0x000F_0000)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogPenEx {
  pub pen_style: u32,
  pub width: u32,
  pub brush_style: u32,
  pub color: ColorRef,
  pub brush_hatch: u32,
  pub style_entries: Vec<u32>,
}

impl LogPenEx {
  pub const FIXED_SIZE: usize = 24;

  pub fn pen_style_flags(&self) -> EmrPenStyleFlags {
    EmrPenStyleFlags::from_bits_retain(self.pen_style)
  }

  pub const fn pen_line_style_raw(&self) -> u32 {
    self.pen_style & 0x0000_000F
  }

  pub fn pen_line_style_kind(&self) -> Option<EmrPenLineStyle> {
    EmrPenLineStyle::from_raw(self.pen_line_style_raw())
  }

  pub const fn pen_end_cap_raw(&self) -> u32 {
    self.pen_style & 0x0000_0F00
  }

  pub fn pen_end_cap_kind(&self) -> Option<EmrPenEndCap> {
    EmrPenEndCap::from_raw(self.pen_end_cap_raw())
  }

  pub const fn pen_join_raw(&self) -> u32 {
    self.pen_style & 0x0000_F000
  }

  pub fn pen_join_kind(&self) -> Option<EmrPenJoin> {
    EmrPenJoin::from_raw(self.pen_join_raw())
  }

  pub const fn pen_type_raw(&self) -> u32 {
    self.pen_style & 0x000F_0000
  }

  pub fn pen_type_kind(&self) -> Option<EmrPenType> {
    EmrPenType::from_raw(self.pen_type_raw())
  }

  pub const fn pen_reserved_bits(&self) -> u32 {
    self.pen_style & !(0x0000_000F | 0x0000_0F00 | 0x0000_F000 | 0x000F_0000)
  }

  pub fn brush_style_kind(&self) -> Option<WmfBrushStyle> {
    WmfBrushStyle::from_raw(self.brush_style as u16)
  }

  pub fn brush_hatch_kind(&self) -> Option<EmrHatchStyle> {
    EmrHatchStyle::from_raw(self.brush_hatch)
  }

  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Self::read_from_with_end(reader, None)
  }

  fn read_from_with_end<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    end: Option<u64>,
  ) -> Result<Self> {
    let pen_style = reader.read_u32()?;
    let width = reader.read_u32()?;
    let brush_style = reader.read_u32()?;
    let color = ColorRef::read_from(reader)?;
    let brush_hatch = reader.read_u32()?;
    let style_count = reader.read_u32()? as usize;
    if let Some(end) = end {
      let style_bytes = checked_record_array_bytes(style_count, 4, "LogPenEx StyleEntry array")?;
      ensure_record_remaining(reader, end, style_bytes, "LogPenEx StyleEntry array")?;
    }
    let mut style_entries = Vec::with_capacity(style_count);
    for _ in 0..style_count {
      style_entries.push(reader.read_u32()?);
    }
    let value = Self {
      pen_style,
      width,
      brush_style,
      color,
      brush_hatch,
      style_entries,
    };
    validate_log_pen_ex(&value)?;
    Ok(value)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_log_pen_ex(self)?;
    writer.write_u32(self.pen_style)?;
    writer.write_u32(self.width)?;
    writer.write_u32(self.brush_style)?;
    self.color.write_to(writer)?;
    writer.write_u32(self.brush_hatch)?;
    writer.write_u32(usize_to_u32(
      self.style_entries.len(),
      "LogPenEx NumStyleEntries",
    )?)?;
    for entry in &self.style_entries {
      writer.write_u32(*entry)?;
    }
    Ok(())
  }

  pub fn sdk_size(&self) -> usize {
    Self::FIXED_SIZE + self.style_entries.len() * 4
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_create_pen")]
pub struct EmrCreatePen {
  pub object_index: u32,
  pub pen_style: u32,
  pub width: PointL,
  pub color: ColorRef,
}

impl EmrCreatePen {
  pub fn log_pen(&self) -> LogPen {
    LogPen {
      pen_style: self.pen_style,
      width: self.width,
      color: self.color,
    }
  }

  pub fn pen_style_flags(&self) -> EmrPenStyleFlags {
    EmrPenStyleFlags::from_bits_retain(self.pen_style)
  }

  pub const fn pen_line_style_raw(&self) -> u32 {
    self.pen_style & 0x0000_000F
  }

  pub fn pen_line_style_kind(&self) -> Option<EmrPenLineStyle> {
    EmrPenLineStyle::from_raw(self.pen_line_style_raw())
  }

  pub const fn pen_end_cap_raw(&self) -> u32 {
    self.pen_style & 0x0000_0F00
  }

  pub fn pen_end_cap_kind(&self) -> Option<EmrPenEndCap> {
    EmrPenEndCap::from_raw(self.pen_end_cap_raw())
  }

  pub const fn pen_join_raw(&self) -> u32 {
    self.pen_style & 0x0000_F000
  }

  pub fn pen_join_kind(&self) -> Option<EmrPenJoin> {
    EmrPenJoin::from_raw(self.pen_join_raw())
  }

  pub const fn pen_type_raw(&self) -> u32 {
    self.pen_style & 0x000F_0000
  }

  pub fn pen_type_kind(&self) -> Option<EmrPenType> {
    EmrPenType::from_raw(self.pen_type_raw())
  }

  pub const fn pen_reserved_bits(&self) -> u32 {
    self.pen_style & !(0x0000_000F | 0x0000_0F00 | 0x0000_F000 | 0x000F_0000)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_log_brush_ex")]
pub struct LogBrushEx {
  pub brush_style: u32,
  pub color: ColorRef,
  pub brush_hatch: u32,
}

impl LogBrushEx {
  pub fn brush_style_kind(&self) -> Option<WmfBrushStyle> {
    WmfBrushStyle::from_raw(self.brush_style as u16)
  }

  pub fn brush_hatch_kind(&self) -> Option<EmrHatchStyle> {
    EmrHatchStyle::from_raw(self.brush_hatch)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_create_brush_indirect")]
pub struct EmrCreateBrushIndirect {
  pub object_index: u32,
  pub brush_style: u32,
  pub color: ColorRef,
  pub brush_hatch: u32,
}

impl EmrCreateBrushIndirect {
  pub fn log_brush_ex(&self) -> LogBrushEx {
    LogBrushEx {
      brush_style: self.brush_style,
      color: self.color,
      brush_hatch: self.brush_hatch,
    }
  }

  pub fn brush_style_kind(&self) -> Option<WmfBrushStyle> {
    self.log_brush_ex().brush_style_kind()
  }

  pub fn brush_hatch_kind(&self) -> Option<EmrHatchStyle> {
    self.log_brush_ex().brush_hatch_kind()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrExtCreatePen {
  pub object_index: u32,
  pub bitmap_info_offset: u32,
  pub bitmap_info_size: u32,
  pub bitmap_bits_offset: u32,
  pub bitmap_bits_size: u32,
  pub pen_style: u32,
  pub width: u32,
  pub brush_style: u32,
  pub color: ColorRef,
  pub brush_hatch: u32,
  pub style_entries: Vec<u32>,
  pub bitmap_buffer: Vec<u8>,
}

impl EmrExtCreatePen {
  fn bitmap_buffer_data_start(&self) -> usize {
    20 + self.log_pen_ex().sdk_size()
  }

  fn reconstructed_record_data(&self) -> Result<Vec<u8>> {
    let log_pen_ex = self.log_pen_ex();
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      self.bitmap_buffer_data_start() + self.bitmap_buffer.len(),
    )));
    writer.write_u32(self.object_index)?;
    writer.write_u32(self.bitmap_info_offset)?;
    writer.write_u32(self.bitmap_info_size)?;
    writer.write_u32(self.bitmap_bits_offset)?;
    writer.write_u32(self.bitmap_bits_size)?;
    log_pen_ex.write_to(&mut writer)?;
    writer.write_all(&self.bitmap_buffer)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn bitmap(&self) -> Result<Option<EmrBitmapBuffer>> {
    if self.bitmap_info_size == 0 && self.bitmap_bits_size == 0 {
      return Ok(None);
    }
    let data = self.reconstructed_record_data()?;
    let (bitmap, _) = read_bitmap_buffer(
      &data,
      self.bitmap_buffer_data_start(),
      self.bitmap_info_offset as usize,
      self.bitmap_info_size as usize,
      self.bitmap_bits_offset as usize,
      self.bitmap_bits_size as usize,
      "EMR_EXTCREATEPEN",
    )?;
    Ok(Some(bitmap))
  }

  pub fn log_pen_ex(&self) -> LogPenEx {
    LogPenEx {
      pen_style: self.pen_style,
      width: self.width,
      brush_style: self.brush_style,
      color: self.color,
      brush_hatch: self.brush_hatch,
      style_entries: self.style_entries.clone(),
    }
  }

  pub fn pen_style_flags(&self) -> EmrPenStyleFlags {
    EmrPenStyleFlags::from_bits_retain(self.pen_style)
  }

  pub const fn pen_line_style_raw(&self) -> u32 {
    self.pen_style & 0x0000_000F
  }

  pub fn pen_line_style_kind(&self) -> Option<EmrPenLineStyle> {
    EmrPenLineStyle::from_raw(self.pen_line_style_raw())
  }

  pub const fn pen_end_cap_raw(&self) -> u32 {
    self.pen_style & 0x0000_0F00
  }

  pub fn pen_end_cap_kind(&self) -> Option<EmrPenEndCap> {
    EmrPenEndCap::from_raw(self.pen_end_cap_raw())
  }

  pub const fn pen_join_raw(&self) -> u32 {
    self.pen_style & 0x0000_F000
  }

  pub fn pen_join_kind(&self) -> Option<EmrPenJoin> {
    EmrPenJoin::from_raw(self.pen_join_raw())
  }

  pub const fn pen_type_raw(&self) -> u32 {
    self.pen_style & 0x000F_0000
  }

  pub fn pen_type_kind(&self) -> Option<EmrPenType> {
    EmrPenType::from_raw(self.pen_type_raw())
  }

  pub const fn pen_reserved_bits(&self) -> u32 {
    self.pen_style & !(0x0000_000F | 0x0000_0F00 | 0x0000_F000 | 0x000F_0000)
  }

  pub fn brush_style_kind(&self) -> Option<WmfBrushStyle> {
    WmfBrushStyle::from_raw(self.brush_style as u16)
  }

  pub fn brush_hatch_kind(&self) -> Option<EmrHatchStyle> {
    EmrHatchStyle::from_raw(self.brush_hatch)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let object_index = reader.read_u32()?;
    let bitmap_info_offset = reader.read_u32()?;
    let bitmap_info_size = reader.read_u32()?;
    let bitmap_bits_offset = reader.read_u32()?;
    let bitmap_bits_size = reader.read_u32()?;
    let log_pen_ex = LogPenEx::read_from_with_end(&mut reader, Some(data.len() as u64))?;
    let position = reader.position()? as usize;
    let value = Self {
      object_index,
      bitmap_info_offset,
      bitmap_info_size,
      bitmap_bits_offset,
      bitmap_bits_size,
      pen_style: log_pen_ex.pen_style,
      width: log_pen_ex.width,
      brush_style: log_pen_ex.brush_style,
      color: log_pen_ex.color,
      brush_hatch: log_pen_ex.brush_hatch,
      style_entries: log_pen_ex.style_entries,
      bitmap_buffer: data[position..].to_vec(),
    };
    validate_emr_ext_create_pen(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_ext_create_pen(self)?;
    let log_pen_ex = self.log_pen_ex();
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      20 + log_pen_ex.sdk_size() + self.bitmap_buffer.len(),
    )));
    writer.write_u32(self.object_index)?;
    writer.write_u32(self.bitmap_info_offset)?;
    writer.write_u32(self.bitmap_info_size)?;
    writer.write_u32(self.bitmap_bits_offset)?;
    writer.write_u32(self.bitmap_bits_size)?;
    log_pen_ex.write_to(&mut writer)?;
    writer.write_all(&self.bitmap_buffer)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrRectangle {
  pub bounds: RectL,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrEllipse {
  pub bounds: RectL,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogFontW {
  pub height: i32,
  pub width: i32,
  pub escapement: i32,
  pub orientation: i32,
  pub weight: i32,
  pub italic: u8,
  pub underline: u8,
  pub strike_out: u8,
  pub char_set: u8,
  pub out_precision: u8,
  pub clip_precision: u8,
  pub quality: u8,
  pub pitch_and_family: u8,
  pub face_name: SdkString,
}

impl LogFontW {
  pub const SIZE: usize = 92;

  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let value = Self {
      height: reader.read_i32()?,
      width: reader.read_i32()?,
      escapement: reader.read_i32()?,
      orientation: reader.read_i32()?,
      weight: reader.read_i32()?,
      italic: reader.read_u8()?,
      underline: reader.read_u8()?,
      strike_out: reader.read_u8()?,
      char_set: reader.read_u8()?,
      out_precision: reader.read_u8()?,
      clip_precision: reader.read_u8()?,
      quality: reader.read_u8()?,
      pitch_and_family: reader.read_u8()?,
      face_name: SdkString::read_bytes(reader, LOGFONT_FACE_NAME_CHARS * 2, SdkEncoding::Utf16Le)?,
    };
    validate_log_font_w(&value)?;
    Ok(value)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_log_font_w(self)?;
    writer.write_i32(self.height)?;
    writer.write_i32(self.width)?;
    writer.write_i32(self.escapement)?;
    writer.write_i32(self.orientation)?;
    writer.write_i32(self.weight)?;
    writer.write_u8(self.italic)?;
    writer.write_u8(self.underline)?;
    writer.write_u8(self.strike_out)?;
    writer.write_u8(self.char_set)?;
    writer.write_u8(self.out_precision)?;
    writer.write_u8(self.clip_precision)?;
    writer.write_u8(self.quality)?;
    writer.write_u8(self.pitch_and_family)?;
    let bytes = self.face_name.encoded_bytes()?;
    write_fixed_bytes(writer, &bytes, LOGFONT_FACE_NAME_CHARS * 2)
  }

  pub fn sdk_size(&self) -> u64 {
    Self::SIZE as u64
  }

  pub fn char_set_kind(&self) -> Option<WmfCharacterSet> {
    WmfCharacterSet::from_raw(self.char_set)
  }

  pub fn out_precision_kind(&self) -> Option<WmfOutPrecision> {
    WmfOutPrecision::from_raw(self.out_precision)
  }

  pub fn clip_precision_flags(&self) -> WmfClipPrecisionFlags {
    WmfClipPrecisionFlags::from_bits_retain(self.clip_precision)
  }

  pub fn invalid_clip_precision_bits(&self) -> u8 {
    self.clip_precision & !WmfClipPrecisionFlags::all().bits()
  }

  pub fn quality_kind(&self) -> Option<WmfFontQuality> {
    WmfFontQuality::from_raw(self.quality)
  }

  pub fn pitch_kind(&self) -> Option<WmfPitchFont> {
    self.pitch_and_family_object().pitch_kind()
  }

  pub fn family_kind(&self) -> Option<WmfFamilyFont> {
    self.pitch_and_family_object().family_kind()
  }

  pub fn pitch_and_family_object(&self) -> WmfPitchAndFamily {
    WmfPitchAndFamily {
      value: self.pitch_and_family,
    }
  }
}

fn validate_log_font_w(_value: &LogFontW) -> Result<()> {
  Ok(())
}

fn validate_log_font_w_strict(value: &LogFontW) -> Result<()> {
  if !(0..=1000).contains(&value.weight) {
    return Err(Error::invalid(0, "LogFont Weight must be 0 through 1000"));
  }
  if value.italic > 1 {
    return Err(Error::invalid(0, "LogFont Italic must be a Boolean"));
  }
  if value.underline > 1 {
    return Err(Error::invalid(0, "LogFont Underline must be a Boolean"));
  }
  if value.strike_out > 1 {
    return Err(Error::invalid(0, "LogFont StrikeOut must be a Boolean"));
  }
  if value.char_set_kind().is_none() {
    return Err(Error::invalid(
      0,
      "LogFont CharSet is not a valid CharacterSet",
    ));
  }
  if value.out_precision_kind().is_none() {
    return Err(Error::invalid(
      0,
      "LogFont OutPrecision is not a valid OutPrecision",
    ));
  }
  if value.invalid_clip_precision_bits() != 0 {
    return Err(Error::invalid(
      0,
      "LogFont ClipPrecision contains invalid flags",
    ));
  }
  if value.quality_kind().is_none() {
    return Err(Error::invalid(
      0,
      "LogFont Quality is not a valid FontQuality",
    ));
  }
  if value.pitch_kind().is_none() {
    return Err(Error::invalid(
      0,
      "LogFont PitchAndFamily pitch is not a valid PitchFont",
    ));
  }
  if value.pitch_and_family_object().reserved_bits() != 0 {
    return Err(Error::invalid(
      0,
      "LogFont PitchAndFamily reserved bits are nonzero",
    ));
  }
  if value.family_kind().is_none() {
    return Err(Error::invalid(
      0,
      "LogFont PitchAndFamily family is not a valid FamilyFont",
    ));
  }
  Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_panose")]
pub struct Panose {
  pub family_type: u8,
  pub serif_style: u8,
  pub weight: u8,
  pub proportion: u8,
  pub contrast: u8,
  pub stroke_variation: u8,
  pub arm_style: u8,
  pub letterform: u8,
  pub midline: u8,
  pub x_height: u8,
}

impl Panose {
  pub fn family_type_kind(&self) -> Option<EmrPanoseFamilyType> {
    EmrPanoseFamilyType::from_raw(self.family_type)
  }

  pub fn serif_style_kind(&self) -> Option<EmrPanoseSerifType> {
    EmrPanoseSerifType::from_raw(self.serif_style)
  }

  pub fn weight_kind(&self) -> Option<EmrPanoseWeight> {
    EmrPanoseWeight::from_raw(self.weight)
  }

  pub fn proportion_kind(&self) -> Option<EmrPanoseProportion> {
    EmrPanoseProportion::from_raw(self.proportion)
  }

  pub fn contrast_kind(&self) -> Option<EmrPanoseContrast> {
    EmrPanoseContrast::from_raw(self.contrast)
  }

  pub fn stroke_variation_kind(&self) -> Option<EmrPanoseStrokeVariation> {
    EmrPanoseStrokeVariation::from_raw(self.stroke_variation)
  }

  pub fn arm_style_kind(&self) -> Option<EmrPanoseArmStyle> {
    EmrPanoseArmStyle::from_raw(self.arm_style)
  }

  pub fn letterform_kind(&self) -> Option<EmrPanoseLetterform> {
    EmrPanoseLetterform::from_raw(self.letterform)
  }

  pub fn midline_kind(&self) -> Option<EmrPanoseMidLine> {
    EmrPanoseMidLine::from_raw(self.midline)
  }

  pub fn x_height_kind(&self) -> Option<EmrPanoseXHeight> {
    EmrPanoseXHeight::from_raw(self.x_height)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    <Self as SdkWrite>::write_to(self, writer)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogFontEx {
  pub log_font: LogFontW,
  pub full_name: SdkString,
  pub style: SdkString,
  pub script: SdkString,
}

impl LogFontEx {
  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self {
      log_font: LogFontW::read_from(reader)?,
      full_name: SdkString::read_bytes(
        reader,
        LOGFONT_EX_FULL_NAME_CHARS * 2,
        SdkEncoding::Utf16Le,
      )?,
      style: SdkString::read_bytes(reader, LOGFONT_EX_STYLE_CHARS * 2, SdkEncoding::Utf16Le)?,
      script: SdkString::read_bytes(reader, LOGFONT_EX_SCRIPT_CHARS * 2, SdkEncoding::Utf16Le)?,
    })
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.log_font.write_to(writer)?;
    write_fixed_bytes(
      writer,
      &self.full_name.encoded_bytes()?,
      LOGFONT_EX_FULL_NAME_CHARS * 2,
    )?;
    write_fixed_bytes(
      writer,
      &self.style.encoded_bytes()?,
      LOGFONT_EX_STYLE_CHARS * 2,
    )?;
    write_fixed_bytes(
      writer,
      &self.script.encoded_bytes()?,
      LOGFONT_EX_SCRIPT_CHARS * 2,
    )
  }

  pub fn sdk_size(&self) -> u64 {
    LOGFONT_EX_SIZE as u64
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesignVector {
  pub signature: u32,
  pub values: Vec<i32>,
}

impl DesignVector {
  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let signature = reader.read_u32()?;
    let axis_count = reader.read_u32()?;
    if axis_count > 16 {
      return Err(Error::invalid(
        reader.position()?,
        "DesignVector axis count exceeds 16",
      ));
    }
    if signature != DESIGN_VECTOR_SIGNATURE {
      return Err(Error::invalid(
        reader.position()?,
        "DesignVector Signature must be 0x08007664",
      ));
    }
    let mut values = Vec::with_capacity(axis_count as usize);
    for _ in 0..axis_count {
      values.push(reader.read_i32()?);
    }
    let value = Self { signature, values };
    validate_design_vector(&value)?;
    Ok(value)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_design_vector(self)?;
    writer.write_u32(self.signature)?;
    writer.write_u32(usize_to_u32(self.values.len(), "DesignVector axis count")?)?;
    for value in &self.values {
      writer.write_i32(*value)?;
    }
    Ok(())
  }

  pub fn sdk_size(&self) -> u64 {
    8 + self.values.len() as u64 * 4
  }

  pub fn is_ms_signature(&self) -> bool {
    self.signature == DESIGN_VECTOR_SIGNATURE
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogFontExDv {
  pub log_font_ex: LogFontEx,
  pub design_vector: DesignVector,
}

impl LogFontExDv {
  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self {
      log_font_ex: LogFontEx::read_from(reader)?,
      design_vector: DesignVector::read_from(reader)?,
    })
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.log_font_ex.write_to(writer)?;
    self.design_vector.write_to(writer)
  }

  pub fn sdk_size(&self) -> u64 {
    self.log_font_ex.sdk_size() + self.design_vector.sdk_size()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogFontPanose {
  pub log_font: LogFontW,
  pub full_name: SdkString,
  pub style: SdkString,
  pub version: u32,
  pub style_size: u32,
  pub match_value: u32,
  pub reserved: u32,
  pub vendor_id: u32,
  pub culture: u32,
  pub panose: Panose,
  pub padding: [u8; 2],
}

impl LogFontPanose {
  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let log_font = LogFontW::read_from(reader)?;
    let full_name =
      SdkString::read_bytes(reader, LOGFONT_EX_FULL_NAME_CHARS * 2, SdkEncoding::Utf16Le)?;
    let style = SdkString::read_bytes(reader, LOGFONT_EX_STYLE_CHARS * 2, SdkEncoding::Utf16Le)?;
    let version = reader.read_u32()?;
    let style_size = reader.read_u32()?;
    let match_value = reader.read_u32()?;
    let reserved = reader.read_u32()?;
    let vendor_id = reader.read_u32()?;
    let culture = reader.read_u32()?;
    let panose = Panose::read_from(reader)?;
    let padding = [reader.read_u8()?, reader.read_u8()?];
    let value = Self {
      log_font,
      full_name,
      style,
      version,
      style_size,
      match_value,
      reserved,
      vendor_id,
      culture,
      panose,
      padding,
    };
    validate_log_font_panose(&value)?;
    Ok(value)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_log_font_panose(self)?;
    self.log_font.write_to(writer)?;
    write_fixed_bytes(
      writer,
      &self.full_name.encoded_bytes()?,
      LOGFONT_EX_FULL_NAME_CHARS * 2,
    )?;
    write_fixed_bytes(
      writer,
      &self.style.encoded_bytes()?,
      LOGFONT_EX_STYLE_CHARS * 2,
    )?;
    writer.write_u32(self.version)?;
    writer.write_u32(self.style_size)?;
    writer.write_u32(self.match_value)?;
    writer.write_u32(self.reserved)?;
    writer.write_u32(self.vendor_id)?;
    writer.write_u32(self.culture)?;
    self.panose.write_to(writer)?;
    writer.write_all(&self.padding)
  }

  pub fn sdk_size(&self) -> u64 {
    LOGFONT_PANOSE_SIZE as u64
  }
}

fn validate_log_font_panose(value: &LogFontPanose) -> Result<()> {
  validate_log_font_w(&value.log_font)?;
  if value.reserved != 0 {
    return Err(Error::invalid(0, "LogFontPanose Reserved must be zero"));
  }
  if value.culture != 0 {
    return Err(Error::invalid(0, "LogFontPanose Culture must be zero"));
  }
  validate_panose(&value.panose)?;
  Ok(())
}

fn validate_panose(value: &Panose) -> Result<()> {
  if value.family_type_kind().is_none() {
    return Err(Error::invalid(0, "Panose FamilyType is invalid"));
  }
  if value.serif_style_kind().is_none() {
    return Err(Error::invalid(0, "Panose SerifStyle is invalid"));
  }
  if value.weight_kind().is_none() {
    return Err(Error::invalid(0, "Panose Weight is invalid"));
  }
  if value.proportion_kind().is_none() {
    return Err(Error::invalid(0, "Panose Proportion is invalid"));
  }
  if value.contrast_kind().is_none() {
    return Err(Error::invalid(0, "Panose Contrast is invalid"));
  }
  if value.stroke_variation_kind().is_none() {
    return Err(Error::invalid(0, "Panose StrokeVariation is invalid"));
  }
  if value.arm_style_kind().is_none() {
    return Err(Error::invalid(0, "Panose ArmStyle is invalid"));
  }
  if value.letterform_kind().is_none() {
    return Err(Error::invalid(0, "Panose Letterform is invalid"));
  }
  if value.midline_kind().is_none() {
    return Err(Error::invalid(0, "Panose Midline is invalid"));
  }
  if value.x_height_kind().is_none() {
    return Err(Error::invalid(0, "Panose XHeight is invalid"));
  }
  Ok(())
}

fn validate_design_vector(value: &DesignVector) -> Result<()> {
  if value.signature != DESIGN_VECTOR_SIGNATURE {
    return Err(Error::invalid(
      0,
      "DesignVector Signature must be 0x08007664",
    ));
  }
  if value.values.len() > 16 {
    return Err(Error::invalid(0, "DesignVector axis count exceeds 16"));
  }
  Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmrExtCreateFont {
  LogFont(LogFontW),
  LogFontPanose(LogFontPanose),
  LogFontExDv(LogFontExDv),
  Raw(Vec<u8>),
}

impl EmrExtCreateFont {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() == LogFontW::SIZE {
      let mut reader = Reader::new(Cursor::new(data));
      return Ok(Self::LogFont(LogFontW::read_from(&mut reader)?));
    }
    if data.len() == LOGFONT_PANOSE_SIZE {
      let mut reader = Reader::new(Cursor::new(data));
      return Ok(Self::LogFontPanose(LogFontPanose::read_from(&mut reader)?));
    }
    if data.len() > LOGFONT_PANOSE_SIZE {
      let mut reader = Reader::new(Cursor::new(data));
      let font = LogFontExDv::read_from(&mut reader)?;
      ensure_reader_end(&mut reader, data.len() as u64, "LogFontExDv")?;
      return Ok(Self::LogFontExDv(font));
    }
    Ok(Self::Raw(data.to_vec()))
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_emr_ext_create_font(self)?;
    match self {
      Self::LogFont(value) => value.write_to(writer),
      Self::LogFontPanose(value) => value.write_to(writer),
      Self::LogFontExDv(value) => value.write_to(writer),
      Self::Raw(value) => writer.write_all(value),
    }
  }

  pub fn sdk_size(&self) -> u64 {
    match self {
      Self::LogFont(value) => value.sdk_size(),
      Self::LogFontPanose(value) => value.sdk_size(),
      Self::LogFontExDv(value) => value.sdk_size(),
      Self::Raw(value) => value.len() as u64,
    }
  }

  pub fn log_font(&self) -> Option<&LogFontW> {
    match self {
      Self::LogFont(value) => Some(value),
      Self::LogFontPanose(value) => Some(&value.log_font),
      Self::LogFontExDv(value) => Some(&value.log_font_ex.log_font),
      Self::Raw(_) => None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrExtCreateFontIndirectW {
  pub object_index: u32,
  pub font: EmrExtCreateFont,
}

impl EmrExtCreateFontIndirectW {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 4 {
      return Err(Error::invalid(8, "EMR_EXTCREATEFONTINDIRECTW is too small"));
    }
    let mut reader = Reader::new(Cursor::new(&data[..4]));
    let object_index = reader.read_u32()?;
    let font = EmrExtCreateFont::read_data(&data[4..])?;
    let value = Self { object_index, font };
    validate_emr_ext_create_font_indirect_w(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_ext_create_font_indirect_w(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      4 + self.font.sdk_size() as usize,
    )));
    writer.write_u32(self.object_index)?;
    self.font.write_to(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn log_font(&self) -> Option<&LogFontW> {
    self.font.log_font()
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct CieXyz {
  pub x: i32,
  pub y: i32,
  pub z: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct CieXyzTriple {
  pub red: CieXyz,
  pub green: CieXyz,
  pub blue: CieXyz,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogColorSpaceGamma {
  pub raw: u32,
}

impl LogColorSpaceGamma {
  pub const fn new(raw: u32) -> Self {
    Self { raw }
  }

  pub const fn from_parts(integer: u8, fraction: u8) -> Self {
    Self {
      raw: ((integer as u32) << 16) | ((fraction as u32) << 8),
    }
  }

  pub const fn integer(self) -> u8 {
    (self.raw >> 16) as u8
  }

  pub const fn fraction(self) -> u8 {
    (self.raw >> 8) as u8
  }

  pub const fn reserved_bits(self) -> u32 {
    self.raw & 0xFF00_00FF
  }

  pub fn real_value(self) -> f32 {
    f32::from(self.integer()) + f32::from(self.fraction()) / 256.0
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogColorSpace {
  pub signature: u32,
  pub version: u32,
  pub size: u32,
  pub color_space_type: i32,
  pub intent: i32,
  pub endpoints: CieXyzTriple,
  pub gamma_red: u32,
  pub gamma_green: u32,
  pub gamma_blue: u32,
  pub filename: SdkString,
}

impl LogColorSpace {
  pub fn signature_kind(&self) -> Option<EmrLogColorSpaceSignature> {
    EmrLogColorSpaceSignature::from_raw(self.signature)
  }

  pub fn color_space_type_kind(&self) -> Option<EmrLogicalColorSpace> {
    EmrLogicalColorSpace::from_raw(self.color_space_type)
  }

  pub fn intent_kind(&self) -> Option<EmrGamutMappingIntent> {
    EmrGamutMappingIntent::from_raw(self.intent)
  }

  pub const fn gamma_red_value(&self) -> LogColorSpaceGamma {
    LogColorSpaceGamma::new(self.gamma_red)
  }

  pub const fn gamma_green_value(&self) -> LogColorSpaceGamma {
    LogColorSpaceGamma::new(self.gamma_green)
  }

  pub const fn gamma_blue_value(&self) -> LogColorSpaceGamma {
    LogColorSpaceGamma::new(self.gamma_blue)
  }

  pub fn read_from<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    encoding: SdkEncoding,
    filename_bytes: usize,
  ) -> Result<Self> {
    let value = Self {
      signature: reader.read_u32()?,
      version: reader.read_u32()?,
      size: reader.read_u32()?,
      color_space_type: reader.read_i32()?,
      intent: reader.read_i32()?,
      endpoints: CieXyzTriple::read_from(reader)?,
      gamma_red: reader.read_u32()?,
      gamma_green: reader.read_u32()?,
      gamma_blue: reader.read_u32()?,
      filename: SdkString::read_bytes(reader, filename_bytes, encoding)?,
    };
    validate_log_color_space(&value, filename_bytes)?;
    Ok(value)
  }

  fn read_from_compatible<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    encoding: SdkEncoding,
    filename_bytes: usize,
  ) -> Result<Self> {
    Ok(Self {
      signature: reader.read_u32()?,
      version: reader.read_u32()?,
      size: reader.read_u32()?,
      color_space_type: reader.read_i32()?,
      intent: reader.read_i32()?,
      endpoints: CieXyzTriple::read_from(reader)?,
      gamma_red: reader.read_u32()?,
      gamma_green: reader.read_u32()?,
      gamma_blue: reader.read_u32()?,
      filename: SdkString::read_bytes(reader, filename_bytes, encoding)?,
    })
  }

  pub fn write_to<W: std::io::Write>(
    &self,
    writer: &mut Writer<W>,
    filename_bytes: usize,
  ) -> Result<()> {
    validate_log_color_space(self, filename_bytes)?;
    writer.write_u32(self.signature)?;
    writer.write_u32(self.version)?;
    writer.write_u32(self.size)?;
    writer.write_i32(self.color_space_type)?;
    writer.write_i32(self.intent)?;
    self.endpoints.write_to(writer)?;
    writer.write_u32(self.gamma_red)?;
    writer.write_u32(self.gamma_green)?;
    writer.write_u32(self.gamma_blue)?;
    write_fixed_bytes(writer, &self.filename.encoded_bytes()?, filename_bytes)
  }

  fn write_compatible<W: std::io::Write>(
    &self,
    writer: &mut Writer<W>,
    filename_bytes: usize,
  ) -> Result<()> {
    writer.write_u32(self.signature)?;
    writer.write_u32(self.version)?;
    writer.write_u32(self.size)?;
    writer.write_i32(self.color_space_type)?;
    writer.write_i32(self.intent)?;
    self.endpoints.write_to(writer)?;
    writer.write_u32(self.gamma_red)?;
    writer.write_u32(self.gamma_green)?;
    writer.write_u32(self.gamma_blue)?;
    write_fixed_bytes(writer, &self.filename.encoded_bytes()?, filename_bytes)
  }

  pub fn sdk_size(filename_bytes: usize) -> usize {
    68 + filename_bytes
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCreateColorSpace {
  pub color_space_index: u32,
  pub log_color_space: LogColorSpace,
  pub extension: Vec<u8>,
}

impl EmrCreateColorSpace {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    const FIXED_DATA_SIZE: usize = 4 + 68;
    if data.len() < FIXED_DATA_SIZE {
      return Err(Error::invalid(
        0,
        "EMR_CREATECOLORSPACE record is shorter than its fixed fields",
      ));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let color_space_index = reader.read_u32()?;
    let filename_bytes = (data.len() - FIXED_DATA_SIZE).min(260);
    let log_color_space =
      LogColorSpace::read_from_compatible(&mut reader, SdkEncoding::Windows1252, filename_bytes)?;
    let extension = read_remaining(&mut reader, data)?;
    let value = Self {
      color_space_index,
      log_color_space,
      extension,
    };
    validate_emr_create_color_space(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_create_color_space(self)?;
    let filename_bytes = self
      .log_color_space
      .filename
      .raw_bytes()
      .map_or(260, |bytes| bytes.len().min(260));
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      4 + LogColorSpace::sdk_size(filename_bytes) + self.extension.len(),
    )));
    writer.write_u32(self.color_space_index)?;
    self
      .log_color_space
      .write_compatible(&mut writer, filename_bytes)?;
    writer.write_all(&self.extension)?;
    pad_writer_to_4(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCreateColorSpaceW {
  pub color_space_index: u32,
  pub log_color_space: LogColorSpace,
  pub flags: u32,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrCreateColorSpaceW {
  pub fn flags(&self) -> EmrCreateColorSpaceWFlags {
    EmrCreateColorSpaceWFlags::from_bits_retain(self.flags)
  }

  pub const fn invalid_flag_bits(&self) -> u32 {
    self.flags & !EmrCreateColorSpaceWFlags::all().bits()
  }

  pub const fn contains_color_profile_data(&self) -> bool {
    self.flags & EmrCreateColorSpaceWFlags::COLOR_PROFILE_DATA.bits() != 0
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let color_space_index = reader.read_u32()?;
    let log_color_space = LogColorSpace::read_from(&mut reader, SdkEncoding::Utf16Le, 520)?;
    let flags = reader.read_u32()?;
    let data_size = reader.read_u32()? as usize;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      data_size,
      "EMR_CREATECOLORSPACEW profile data",
    )?;
    let profile_data = reader.read_vec(data_size)?;
    let padding = read_remaining(&mut reader, data)?;
    let value = Self {
      color_space_index,
      log_color_space,
      flags,
      data: profile_data,
      padding,
    };
    validate_emr_create_color_space_w(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_create_color_space_w(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      12 + LogColorSpace::sdk_size(520) + self.data.len() + self.padding.len(),
    )));
    writer.write_u32(self.color_space_index)?;
    self.log_color_space.write_to(&mut writer, 520)?;
    writer.write_u32(self.flags)?;
    writer.write_u32(usize_to_u32(
      self.data.len(),
      "EMR_CREATECOLORSPACEW profile data size",
    )?)?;
    writer.write_all(&self.data)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_CREATECOLORSPACEW")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrPolyPointsL {
  pub bounds: RectL,
  pub points: Vec<PointL>,
}

impl EmrPolyPointsL {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let count = reader.read_u32()? as usize;
    let required = checked_record_array_bytes(count, 8, "EMF point array")?;
    ensure_record_remaining(&mut reader, data.len() as u64, required, "EMF point array")?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
      points.push(PointL::read_from(&mut reader)?);
    }
    ensure_reader_end(&mut reader, data.len() as u64, "EMF point array")?;
    Ok(Self { bounds, points })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(20 + self.points.len() * 8)));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.points.len(), "EMF point count")?)?;
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrPolyPointsS {
  pub bounds: RectL,
  pub points: Vec<PointS>,
}

impl EmrPolyPointsS {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let count = reader.read_u32()? as usize;
    let required = checked_record_array_bytes(count, 4, "EMF point array")?;
    ensure_record_remaining(&mut reader, data.len() as u64, required, "EMF point array")?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
      points.push(PointS::read_from(&mut reader)?);
    }
    ensure_reader_end(&mut reader, data.len() as u64, "EMF point array")?;
    Ok(Self { bounds, points })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(20 + self.points.len() * 4)));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.points.len(), "EMF point count")?)?;
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrPolyDrawL {
  pub bounds: RectL,
  pub points: Vec<PointL>,
  pub point_types: Vec<EmrPointTypeValue>,
  pub padding: Vec<u8>,
}

impl EmrPolyDrawL {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let count = reader.read_u32()? as usize;
    let required = checked_record_array_bytes(count, 8, "EMR_POLYDRAW points")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      required,
      "EMR_POLYDRAW points",
    )?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
      points.push(PointL::read_from(&mut reader)?);
    }
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      count,
      "EMR_POLYDRAW point types",
    )?;
    let point_types = read_emr_point_type_values(&mut reader, count)?;
    validate_emr_poly_draw_point_types(&point_types, "EMR_POLYDRAW")?;
    let padding = read_remaining(&mut reader, data)?;
    validate_emf_record_alignment_padding(
      &padding,
      20 + points.len() * 8 + point_types.len(),
      "EMR_POLYDRAW",
    )?;
    Ok(Self {
      bounds,
      points,
      point_types,
      padding,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    if self.points.len() != self.point_types.len() {
      return Err(Error::invalid(
        0,
        "EMR_POLYDRAW point and point type counts differ",
      ));
    }
    validate_emr_poly_draw_point_types(&self.point_types, "EMR_POLYDRAW")?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      20 + self.points.len() * 9 + self.padding.len() + 3,
    )));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.points.len(), "EMR_POLYDRAW point count")?)?;
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    write_emr_point_type_values(&mut writer, &self.point_types)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_POLYDRAW")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrPolyDrawS {
  pub bounds: RectL,
  pub points: Vec<PointS>,
  pub point_types: Vec<EmrPointTypeValue>,
  pub padding: Vec<u8>,
}

impl EmrPolyDrawS {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let count = reader.read_u32()? as usize;
    let required = checked_record_array_bytes(count, 4, "EMR_POLYDRAW16 points")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      required,
      "EMR_POLYDRAW16 points",
    )?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
      points.push(PointS::read_from(&mut reader)?);
    }
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      count,
      "EMR_POLYDRAW16 point types",
    )?;
    let point_types = read_emr_point_type_values(&mut reader, count)?;
    validate_emr_poly_draw_point_types(&point_types, "EMR_POLYDRAW16")?;
    let padding = read_remaining(&mut reader, data)?;
    validate_emf_record_alignment_padding(
      &padding,
      20 + points.len() * 4 + point_types.len(),
      "EMR_POLYDRAW16",
    )?;
    Ok(Self {
      bounds,
      points,
      point_types,
      padding,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    if self.points.len() != self.point_types.len() {
      return Err(Error::invalid(
        0,
        "EMR_POLYDRAW16 point and point type counts differ",
      ));
    }
    validate_emr_poly_draw_point_types(&self.point_types, "EMR_POLYDRAW16")?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      20 + self.points.len() * 5 + self.padding.len() + 3,
    )));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(
      self.points.len(),
      "EMR_POLYDRAW16 point count",
    )?)?;
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    write_emr_point_type_values(&mut writer, &self.point_types)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_POLYDRAW16")?;
    Ok(writer.into_inner().into_inner())
  }
}

fn read_emr_point_type_values<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  count: usize,
) -> Result<Vec<EmrPointTypeValue>> {
  let mut values = Vec::with_capacity(count);
  for _ in 0..count {
    values.push(EmrPointTypeValue::new(reader.read_u8()?)?);
  }
  Ok(values)
}

fn write_emr_point_type_values<W: std::io::Write>(
  writer: &mut Writer<W>,
  values: &[EmrPointTypeValue],
) -> Result<()> {
  for value in values {
    validate_emr_point_type_value(value.value)?;
    writer.write_u8(value.value)?;
  }
  Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrPolyPolygonL {
  pub bounds: RectL,
  pub counts: Vec<u32>,
  pub points: Vec<PointL>,
}

impl EmrPolyPolygonL {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    Self::read_data_with_options(data, false)
  }

  pub fn read_polyline_data(data: &[u8]) -> Result<Self> {
    Self::read_data_with_options(data, true)
  }

  fn read_data_with_options(data: &[u8], require_polyline_counts: bool) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let polygon_count = reader.read_u32()? as usize;
    let point_count = reader.read_u32()? as usize;
    let count_bytes = checked_record_array_bytes(polygon_count, 4, "EMF polygon counts")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      count_bytes,
      "EMF polygon counts",
    )?;
    let mut counts = Vec::with_capacity(polygon_count);
    for _ in 0..polygon_count {
      counts.push(reader.read_u32()?);
    }
    let point_bytes = checked_record_array_bytes(point_count, 8, "EMF polygon points")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      point_bytes,
      "EMF polygon points",
    )?;
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
      points.push(PointL::read_from(&mut reader)?);
    }
    let value = Self {
      bounds,
      counts,
      points,
    };
    validate_emr_poly_polygon_l(&value, require_polyline_counts)?;
    ensure_reader_end(&mut reader, data.len() as u64, "EMF polygon points")?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    self.to_data_with_options(false)
  }

  pub fn to_polyline_data(&self) -> Result<Vec<u8>> {
    self.to_data_with_options(true)
  }

  fn to_data_with_options(&self, require_polyline_counts: bool) -> Result<Vec<u8>> {
    validate_emr_poly_polygon_l(self, require_polyline_counts)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      24 + self.counts.len() * 4 + self.points.len() * 8,
    )));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.counts.len(), "EMF polygon count")?)?;
    writer.write_u32(usize_to_u32(self.points.len(), "EMF total point count")?)?;
    for count in &self.counts {
      writer.write_u32(*count)?;
    }
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrPolyPolygonS {
  pub bounds: RectL,
  pub counts: Vec<u32>,
  pub points: Vec<PointS>,
}

impl EmrPolyPolygonS {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    Self::read_data_with_options(data, false)
  }

  pub fn read_polyline_data(data: &[u8]) -> Result<Self> {
    Self::read_data_with_options(data, true)
  }

  fn read_data_with_options(data: &[u8], require_polyline_counts: bool) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let polygon_count = reader.read_u32()? as usize;
    let point_count = reader.read_u32()? as usize;
    let count_bytes = checked_record_array_bytes(polygon_count, 4, "EMF polygon counts")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      count_bytes,
      "EMF polygon counts",
    )?;
    let mut counts = Vec::with_capacity(polygon_count);
    for _ in 0..polygon_count {
      counts.push(reader.read_u32()?);
    }
    let point_bytes = checked_record_array_bytes(point_count, 4, "EMF polygon points")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      point_bytes,
      "EMF polygon points",
    )?;
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
      points.push(PointS::read_from(&mut reader)?);
    }
    let value = Self {
      bounds,
      counts,
      points,
    };
    validate_emr_poly_polygon_s(&value, require_polyline_counts)?;
    ensure_reader_end(&mut reader, data.len() as u64, "EMF polygon points")?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    self.to_data_with_options(false)
  }

  pub fn to_polyline_data(&self) -> Result<Vec<u8>> {
    self.to_data_with_options(true)
  }

  fn to_data_with_options(&self, require_polyline_counts: bool) -> Result<Vec<u8>> {
    validate_emr_poly_polygon_s(self, require_polyline_counts)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      24 + self.counts.len() * 4 + self.points.len() * 4,
    )));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(self.counts.len(), "EMF polygon count")?)?;
    writer.write_u32(usize_to_u32(self.points.len(), "EMF total point count")?)?;
    for count in &self.counts {
      writer.write_u32(*count)?;
    }
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrGradientRectangle {
  pub upper_left: u32,
  pub lower_right: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrGradientTriangle {
  pub vertex1: u32,
  pub vertex2: u32,
  pub vertex3: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmrGradientFillMesh {
  Rectangles {
    rectangles: Vec<EmrGradientRectangle>,
    padding: Vec<u8>,
  },
  Triangles(Vec<EmrGradientTriangle>),
  Raw {
    mesh_count: u32,
    data: Vec<u8>,
  },
}

impl EmrGradientFillMesh {
  fn mesh_count(&self) -> Result<u32> {
    match self {
      Self::Rectangles { rectangles, .. } => {
        usize_to_u32(rectangles.len(), "EMR_GRADIENTFILL rectangle count")
      }
      Self::Triangles(triangles) => {
        usize_to_u32(triangles.len(), "EMR_GRADIENTFILL triangle count")
      }
      Self::Raw { mesh_count, .. } => Ok(*mesh_count),
    }
  }

  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Rectangles {
        rectangles,
        padding,
      } => {
        for rectangle in rectangles {
          rectangle.write_to(writer)?;
        }
        writer.write_all(padding)
      }
      Self::Triangles(triangles) => {
        for triangle in triangles {
          triangle.write_to(writer)?;
        }
        Ok(())
      }
      Self::Raw { data, .. } => writer.write_all(data),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrGradientFill {
  pub bounds: RectL,
  pub mode: u32,
  pub vertices: Vec<TriVertex>,
  pub mesh: EmrGradientFillMesh,
}

impl EmrGradientFill {
  pub fn mode_kind(&self) -> Option<EmrGradientFillMode> {
    EmrGradientFillMode::from_raw(self.mode)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let vertex_count = reader.read_u32()? as usize;
    let mesh_count = reader.read_u32()?;
    let mode = reader.read_u32()?;
    let vertex_bytes = checked_record_array_bytes(vertex_count, 16, "EMR_GRADIENTFILL vertices")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      vertex_bytes,
      "EMR_GRADIENTFILL vertices",
    )?;
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
      vertices.push(TriVertex::read_from(&mut reader)?);
    }
    let mesh_count_usize = mesh_count as usize;
    let mesh = match mode {
      0 | 1 => {
        let rectangle_bytes =
          checked_record_array_bytes(mesh_count_usize, 8, "EMR_GRADIENTFILL rectangles")?;
        ensure_record_remaining(
          &mut reader,
          data.len() as u64,
          rectangle_bytes,
          "EMR_GRADIENTFILL rectangles",
        )?;
        let mut rectangles = Vec::with_capacity(mesh_count_usize);
        for _ in 0..mesh_count_usize {
          rectangles.push(EmrGradientRectangle::read_from(&mut reader)?);
        }
        let padding_len = mesh_count_usize
          .checked_mul(4)
          .ok_or_else(|| Error::invalid(0, "EMR_GRADIENTFILL rectangle padding overflows"))?;
        ensure_record_remaining(
          &mut reader,
          data.len() as u64,
          padding_len,
          "EMR_GRADIENTFILL rectangle padding",
        )?;
        let padding = reader.read_vec(padding_len)?;
        ensure_reader_end(&mut reader, data.len() as u64, "EMR_GRADIENTFILL")?;
        EmrGradientFillMesh::Rectangles {
          rectangles,
          padding,
        }
      }
      2 => {
        let triangle_bytes =
          checked_record_array_bytes(mesh_count_usize, 12, "EMR_GRADIENTFILL triangles")?;
        ensure_record_remaining(
          &mut reader,
          data.len() as u64,
          triangle_bytes,
          "EMR_GRADIENTFILL triangles",
        )?;
        let mut triangles = Vec::with_capacity(mesh_count_usize);
        for _ in 0..mesh_count_usize {
          triangles.push(EmrGradientTriangle::read_from(&mut reader)?);
        }
        ensure_reader_end(&mut reader, data.len() as u64, "EMR_GRADIENTFILL")?;
        EmrGradientFillMesh::Triangles(triangles)
      }
      _ => EmrGradientFillMesh::Raw {
        mesh_count,
        data: read_remaining(&mut reader, data)?,
      },
    };
    let value = Self {
      bounds,
      mode,
      vertices,
      mesh,
    };
    validate_emr_gradient_fill(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_gradient_fill(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(
      self.vertices.len(),
      "EMR_GRADIENTFILL vertex count",
    )?)?;
    writer.write_u32(self.mesh.mesh_count()?)?;
    writer.write_u32(self.mode)?;
    for vertex in &self.vertices {
      vertex.write_to(&mut writer)?;
    }
    self.mesh.write_to(&mut writer)?;
    pad_writer_to_4(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrEscape {
  pub escape: u32,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrEscape {
  pub fn read_data(record_data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(record_data));
    let escape = reader.read_u32()?;
    let data_size = reader.read_u32()? as usize;
    let data = reader.read_vec(data_size)?;
    let padding = read_remaining(&mut reader, record_data)?;
    let value = Self {
      escape,
      data,
      padding,
    };
    validate_emr_escape(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_escape(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      8 + self.data.len() + self.padding.len() + 3,
    )));
    writer.write_u32(self.escape)?;
    writer.write_u32(usize_to_u32(self.data.len(), "EMR_ESCAPE data size")?)?;
    writer.write_all(&self.data)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_ESCAPE")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrNamedEscape {
  pub escape: u32,
  pub driver_name: SdkString,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrNamedEscape {
  pub fn read_data(record_data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(record_data));
    let escape = reader.read_u32()?;
    let driver_size = reader.read_u32()? as usize;
    let data_size = reader.read_u32()? as usize;
    let driver_name = SdkString::raw(reader.read_vec(driver_size)?, SdkEncoding::Utf16Le);
    let data = reader.read_vec(data_size)?;
    let padding = read_remaining(&mut reader, record_data)?;
    let value = Self {
      escape,
      driver_name,
      data,
      padding,
    };
    validate_emr_named_escape(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_named_escape(self)?;
    let driver_name = self.driver_name.encoded_bytes()?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      12 + driver_name.len() + self.data.len() + self.padding.len() + 3,
    )));
    writer.write_u32(self.escape)?;
    writer.write_u32(usize_to_u32(
      driver_name.len(),
      "EMR_NAMEDESCAPE driver name size",
    )?)?;
    writer.write_u32(usize_to_u32(self.data.len(), "EMR_NAMEDESCAPE data size")?)?;
    writer.write_all(&driver_name)?;
    writer.write_all(&self.data)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_NAMEDESCAPE")?;
    Ok(writer.into_inner().into_inner())
  }
}

fn validate_emr_escape(value: &EmrEscape) -> Result<()> {
  validate_emr_escape_function(value.escape)?;
  validate_emr_escape_padding(&value.padding, 8 + value.data.len(), "EMR_ESCAPE")
}

fn validate_emr_named_escape(value: &EmrNamedEscape) -> Result<()> {
  validate_emr_escape_function(value.escape)?;
  let driver_name = value.driver_name.encoded_bytes()?;
  if !driver_name.len().is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      "EMR_NAMEDESCAPE driver name byte count must be even",
    ));
  }
  if driver_name.len() < 2 || driver_name[driver_name.len() - 2..] != [0, 0] {
    return Err(Error::invalid(
      0,
      "EMR_NAMEDESCAPE driver name must be UTF-16 null-terminated",
    ));
  }
  validate_emr_escape_padding(
    &value.padding,
    12 + driver_name.len() + value.data.len(),
    "EMR_NAMEDESCAPE",
  )
}

fn validate_emr_escape_padding(
  padding: &[u8],
  unpadded_size: usize,
  record_name: &str,
) -> Result<()> {
  validate_emf_record_alignment_padding(padding, unpadded_size, record_name)
}

fn validate_log_pen(value: &LogPen) -> Result<()> {
  validate_emr_pen_style(
    value.pen_line_style_kind(),
    value.pen_end_cap_kind(),
    value.pen_join_kind(),
    value.pen_type_kind(),
    value.pen_reserved_bits(),
  )
}

fn validate_log_pen_strict(value: &LogPen) -> Result<()> {
  validate_log_pen(value)?;
  value.color.validate_strict()?;
  if value.pen_type_kind() == Some(EmrPenType::Cosmetic) && value.width.x != 1 {
    return Err(Error::invalid(0, "LogPen cosmetic pen width must be 1"));
  }
  Ok(())
}

fn validate_emr_create_pen(value: &EmrCreatePen) -> Result<()> {
  validate_emr_created_object_index(value.object_index, "EMR_CREATEPEN", "ihPen")?;
  validate_log_pen(&value.log_pen())
}

fn validate_emr_create_pen_strict(value: &EmrCreatePen) -> Result<()> {
  validate_emr_created_object_index(value.object_index, "EMR_CREATEPEN", "ihPen")?;
  validate_log_pen_strict(&value.log_pen())
}

fn validate_emr_create_brush_indirect(value: &EmrCreateBrushIndirect) -> Result<()> {
  validate_emr_created_object_index(value.object_index, "EMR_CREATEBRUSHINDIRECT", "ihBrush")?;
  validate_log_brush_ex(&value.log_brush_ex())
}

fn validate_emr_create_brush_indirect_strict(value: &EmrCreateBrushIndirect) -> Result<()> {
  validate_emr_create_brush_indirect(value)?;
  value.color.validate_strict()
}

fn validate_log_brush_ex(value: &LogBrushEx) -> Result<()> {
  let Some(brush_style) = value.brush_style_kind() else {
    return Err(Error::invalid(
      0,
      "LogBrushEx BrushStyle is not a valid BrushStyle",
    ));
  };
  match brush_style {
    WmfBrushStyle::Solid | WmfBrushStyle::Null => Ok(()),
    WmfBrushStyle::Hatched => {
      if value.brush_hatch_kind().is_none() {
        return Err(Error::invalid(
          0,
          "LogBrushEx BrushHatch is not a valid HatchStyle for BS_HATCHED",
        ));
      }
      Ok(())
    }
    _ => Err(Error::invalid(0, "LogBrushEx BrushStyle is not supported")),
  }
}

fn validate_emr_create_palette(value: &EmrCreatePalette) -> Result<()> {
  validate_emr_created_object_index(value.palette_index, "EMR_CREATEPALETTE", "ihPal")?;
  validate_log_palette(&value.log_palette)?;
  if value.log_palette.entries.is_empty() {
    return Err(Error::invalid(
      0,
      "EMR_CREATEPALETTE NumberOfEntries must be nonzero",
    ));
  }
  Ok(())
}

fn validate_log_palette(value: &LogPalette) -> Result<()> {
  if value.version != 0x0300 {
    return Err(Error::invalid(0, "LogPalette Version must be 0x0300"));
  }
  if value.entries.len() > u16::MAX as usize {
    return Err(Error::invalid(0, "LogPalette entry count exceeds u16::MAX"));
  }
  Ok(())
}

fn validate_emr_created_object_index(
  value: u32,
  record_name: &str,
  field_name: &str,
) -> Result<()> {
  if value == 0 {
    return Err(Error::invalid(
      0,
      format!("{record_name} {field_name} must not be zero"),
    ));
  }
  if EmrStockObject::from_raw(value).is_some() {
    return Err(Error::invalid(
      0,
      format!("{record_name} {field_name} must not be a stock object index"),
    ));
  }
  Ok(())
}

fn validate_emr_set_mapper_flags(value: &EmrSetMapperFlags) -> Result<()> {
  if value.flags > 1 {
    return Err(Error::invalid(0, "EMR_SETMAPPERFLAGS Flags must be 0 or 1"));
  }
  Ok(())
}

fn validate_emr_set_map_mode(value: &EmrSetMapMode) -> Result<()> {
  if value.map_mode_kind().is_none() {
    return Err(Error::invalid(0, "EMR_SETMAPMODE MapMode is invalid"));
  }
  Ok(())
}

fn validate_emr_set_bk_mode(value: &EmrSetBkMode) -> Result<()> {
  if value.background_mode_kind().is_none() {
    return Err(Error::invalid(0, "EMR_SETBKMODE BackgroundMode is invalid"));
  }
  Ok(())
}

fn validate_emr_set_poly_fill_mode(value: &EmrSetPolyFillMode) -> Result<()> {
  if value.polygon_fill_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_SETPOLYFILLMODE PolygonFillMode is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_set_rop2(value: &EmrSetRop2) -> Result<()> {
  if value.binary_raster_operation_kind().is_none() {
    return Err(Error::invalid(0, "EMR_SETROP2 ROP2Mode is invalid"));
  }
  Ok(())
}

fn validate_emr_set_stretch_blt_mode(_value: &EmrSetStretchBltMode) -> Result<()> {
  Ok(())
}

fn validate_emr_set_text_align(value: &EmrSetTextAlign) -> Result<()> {
  let _ = value;
  Ok(())
}

fn validate_emr_set_text_align_strict(value: &EmrSetTextAlign) -> Result<()> {
  let text_alignment_mode = u16::try_from(value.text_alignment_mode).map_err(|_| {
    Error::invalid(
      0,
      "EMR_SETTEXTALIGN TextAlignmentMode contains invalid flags",
    )
  })?;
  validate_wmf_text_alignment_value(text_alignment_mode, "EMR_SETTEXTALIGN")
}

fn validate_emr_scale_ext(
  x_num: i32,
  x_denom: i32,
  y_num: i32,
  y_denom: i32,
  name: &str,
) -> Result<()> {
  if x_num == 0 || x_denom == 0 || y_num == 0 || y_denom == 0 {
    return Err(Error::invalid(
      0,
      format!("{name} scale numerator and denominator fields must be nonzero"),
    ));
  }
  Ok(())
}

fn validate_emr_scale_viewport_ext_ex(value: &EmrScaleViewportExtEx) -> Result<()> {
  validate_emr_scale_ext(
    value.x_num,
    value.x_denom,
    value.y_num,
    value.y_denom,
    "EMR_SCALEVIEWPORTEXTEX",
  )
}

fn validate_emr_scale_window_ext_ex(value: &EmrScaleWindowExtEx) -> Result<()> {
  validate_emr_scale_ext(
    value.x_num,
    value.x_denom,
    value.y_num,
    value.y_denom,
    "EMR_SCALEWINDOWEXTEX",
  )
}

fn validate_emr_set_color_adjustment(value: &EmrSetColorAdjustment) -> Result<()> {
  if value.size != 24 {
    return Err(Error::invalid(0, "EMR_SETCOLORADJUSTMENT Size must be 24"));
  }
  if value.values & !EmrColorAdjustmentFlags::all().bits() != 0 {
    return Err(Error::invalid(
      0,
      "EMR_SETCOLORADJUSTMENT Values contains invalid ColorAdjustment flags",
    ));
  }
  if value.illuminant_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_SETCOLORADJUSTMENT IlluminantIndex is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_set_arc_direction(value: &EmrSetArcDirection) -> Result<()> {
  if value.arc_direction_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_SETARCDIRECTION ArcDirection is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_modify_world_transform(value: &EmrModifyWorldTransform) -> Result<()> {
  if value.mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_MODIFYWORLDTRANSFORM ModifyWorldTransformMode is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_restore_dc(value: &EmrRestoreDc) -> Result<()> {
  if value.saved_dc >= 0 {
    return Err(Error::invalid(0, "EMR_RESTOREDC SavedDC must be negative"));
  }
  Ok(())
}

fn validate_emr_select_object(value: &EmrSelectObject) -> Result<()> {
  if value.object_index == 0 {
    return Err(Error::invalid(
      0,
      "EMR_SELECTOBJECT ihObject must not be zero",
    ));
  }
  Ok(())
}

fn validate_emr_select_palette(value: &EmrSelectPalette) -> Result<()> {
  if value.palette_index == 0 {
    return Err(Error::invalid(
      0,
      "EMR_SELECTPALETTE ihPal must not be zero",
    ));
  }
  Ok(())
}

fn validate_emr_resize_palette(value: &EmrResizePalette) -> Result<()> {
  if value.palette_index == 0 {
    return Err(Error::invalid(
      0,
      "EMR_RESIZEPALETTE ihPal must not be zero",
    ));
  }
  if value.number_of_entries == 0 || value.number_of_entries > 0x0000_0400 {
    return Err(Error::invalid(
      0,
      "EMR_RESIZEPALETTE NumberOfEntries must be in 1..=1024",
    ));
  }
  Ok(())
}

fn validate_emr_set_palette_entries(value: &EmrSetPaletteEntries) -> Result<()> {
  if value.palette_index == 0 {
    return Err(Error::invalid(
      0,
      "EMR_SETPALETTEENTRIES ihPal must not be zero",
    ));
  }
  Ok(())
}

fn validate_emr_color_correct_palette(value: &EmrColorCorrectPalette) -> Result<()> {
  if value.palette_index == 0 {
    return Err(Error::invalid(
      0,
      "EMR_COLORCORRECTPALETTE ihPalette must not be zero",
    ));
  }
  Ok(())
}

fn validate_emr_delete_object(value: &EmrDeleteObject) -> Result<()> {
  if value.object_index == 0 {
    return Err(Error::invalid(
      0,
      "EMR_DELETEOBJECT ihObject must not be zero",
    ));
  }
  if value.stock_object_kind().is_some() {
    return Err(Error::invalid(
      0,
      "EMR_DELETEOBJECT ihObject must not be a stock object index",
    ));
  }
  Ok(())
}

fn validate_emr_select_clip_path(value: &EmrSelectClipPath) -> Result<()> {
  if value.region_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_SELECTCLIPPATH RegionMode is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_rgn_data_record(value: &EmrRgnDataRecord, record_name: &str) -> Result<()> {
  value
    .typed_region_data()
    .map(|_| ())
    .map_err(|err| Error::invalid(0, format!("{record_name} RgnData is invalid: {err}")))
}

fn validate_emr_fill_rgn(value: &EmrFillRgn) -> Result<()> {
  if value.brush_index == 0 {
    return Err(Error::invalid(0, "EMR_FILLRGN ihBrush must not be zero"));
  }
  value
    .typed_region_data()
    .map(|_| ())
    .map_err(|err| Error::invalid(0, format!("EMR_FILLRGN RgnData is invalid: {err}")))
}

fn validate_emr_frame_rgn(value: &EmrFrameRgn) -> Result<()> {
  if value.brush_index == 0 {
    return Err(Error::invalid(0, "EMR_FRAMERGN ihBrush must not be zero"));
  }
  value
    .typed_region_data()
    .map(|_| ())
    .map_err(|err| Error::invalid(0, format!("EMR_FRAMERGN RgnData is invalid: {err}")))
}

fn validate_emr_ext_select_clip_rgn(value: &EmrExtSelectClipRgn) -> Result<()> {
  let Some(region_mode) = value.region_mode_kind() else {
    return Err(Error::invalid(
      0,
      "EMR_EXTSELECTCLIPRGN RegionMode is invalid",
    ));
  };
  if value.region_data.is_empty() {
    if region_mode == EmrRegionMode::Copy {
      return Ok(());
    }
    return Err(Error::invalid(
      0,
      "EMR_EXTSELECTCLIPRGN RgnData can be omitted only for RGN_COPY",
    ));
  }
  value.typed_region_data()?;
  Ok(())
}

fn validate_emr_ext_flood_fill(value: &EmrExtFloodFill) -> Result<()> {
  if value.flood_fill_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_EXTFLOODFILL FloodFillMode is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_set_icm_mode(value: &EmrSetIcmMode) -> Result<()> {
  if value.icm_mode_kind().is_none() {
    return Err(Error::invalid(0, "EMR_SETICMMODE ICMMode is invalid"));
  }
  Ok(())
}

fn validate_emr_set_layout(value: &EmrSetLayout) -> Result<()> {
  if value.invalid_layout_bits() != 0 {
    return Err(Error::invalid(
      0,
      "EMR_SETLAYOUT LayoutMode contains invalid flags",
    ));
  }
  Ok(())
}

fn validate_emr_set_color_space(value: &EmrSetColorSpace) -> Result<()> {
  if value.color_space_index == 0 {
    return Err(Error::invalid(0, "EMR_SETCOLORSPACE ihCS must not be zero"));
  }
  Ok(())
}

fn validate_emr_delete_color_space(value: &EmrDeleteColorSpace) -> Result<()> {
  if value.color_space_index == 0 {
    return Err(Error::invalid(
      0,
      "EMR_DELETECOLORSPACE ihCS must not be zero",
    ));
  }
  Ok(())
}

fn validate_ext_text_out_options(options: ExtTextOutOptions, record_name: &str) -> Result<()> {
  let invalid_bits = options.bits() & !ExtTextOutOptions::all().bits();
  if invalid_bits != 0 {
    return Err(Error::invalid(
      0,
      format!("{record_name} text options contain invalid flags"),
    ));
  }
  Ok(())
}

fn validate_emr_text(value: &EmrText, wide: bool, record_name: &str) -> Result<()> {
  validate_ext_text_out_options(value.options, record_name)?;
  let text_bytes = value.text.encoded_bytes()?;
  if wide && !text_bytes.len().is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      format!("{record_name} UTF-16 text byte length is odd"),
    ));
  }
  let char_count = if wide {
    text_bytes.len() / 2
  } else {
    text_bytes.len()
  };
  if value.dx_buffer_present {
    let expected_dx = char_count
      .checked_mul(if value.options.contains(ExtTextOutOptions::PDY) {
        2
      } else {
        1
      })
      .ok_or_else(|| Error::invalid(0, format!("{record_name} dx count overflows")))?;
    if value.dx.len() != expected_dx {
      return Err(Error::invalid(
        0,
        format!("{record_name} dx count does not match character count"),
      ));
    }
  } else {
    if !value.dx.is_empty() {
      return Err(Error::invalid(
        0,
        format!("{record_name} DxBuffer values require a nonzero offDx"),
      ));
    }
    if !value.undefined_space_before_dx.is_empty() {
      return Err(Error::invalid(
        0,
        format!("{record_name} UndefinedSpace before Dx requires a DxBuffer"),
      ));
    }
  }
  Ok(())
}

fn validate_emr_text_strict(value: &EmrText, wide: bool, record_name: &str) -> Result<()> {
  validate_emr_text(value, wide, record_name)?;
  if value.options.contains(ExtTextOutOptions::NO_RECT) {
    if value.rectangle.is_some() {
      return Err(Error::invalid(
        0,
        format!("{record_name} rectangle supplied with ETO_NO_RECT"),
      ));
    }
    if emr_text_requires_rectangle(value.options) {
      return Err(Error::invalid(
        0,
        format!("{record_name} ETO_NO_RECT cannot be combined with rectangle options"),
      ));
    }
  }
  if emr_text_requires_rectangle(value.options) && value.rectangle.is_none() {
    return Err(Error::invalid(
      0,
      format!("{record_name} rectangle missing for ETO_OPAQUE or ETO_CLIPPED"),
    ));
  }

  Ok(())
}

fn validate_emr_ext_text_out(value: &EmrExtTextOut, wide: bool, record_name: &str) -> Result<()> {
  if value.graphics_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      format!("{record_name} iGraphicsMode is invalid"),
    ));
  }
  validate_emr_text(&value.text, wide, record_name)
}

fn validate_emr_ext_text_out_strict(
  value: &EmrExtTextOut,
  wide: bool,
  record_name: &str,
) -> Result<()> {
  if value.graphics_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      format!("{record_name} iGraphicsMode is invalid"),
    ));
  }
  validate_emr_text_strict(&value.text, wide, record_name)
}

fn validate_emr_poly_text_out(value: &EmrPolyTextOut, wide: bool) -> Result<()> {
  if value.graphics_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_POLYTEXTOUT iGraphicsMode is invalid",
    ));
  }
  for text in &value.texts {
    validate_emr_text(text, wide, "EMR_POLYTEXTOUT")?;
  }
  Ok(())
}

fn validate_emr_poly_text_out_strict(value: &EmrPolyTextOut, wide: bool) -> Result<()> {
  if value.graphics_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_POLYTEXTOUT iGraphicsMode is invalid",
    ));
  }
  for text in &value.texts {
    validate_emr_text_strict(text, wide, "EMR_POLYTEXTOUT")?;
  }
  Ok(())
}

fn validate_emr_small_text_out(value: &EmrSmallTextOut) -> Result<()> {
  validate_ext_text_out_options(value.options, "EMR_SMALLTEXTOUT")?;
  if value.graphics_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_SMALLTEXTOUT iGraphicsMode is invalid",
    ));
  }
  if value.options.contains(ExtTextOutOptions::NO_RECT) {
    if value.bounds.is_some() {
      return Err(Error::invalid(
        0,
        "EMR_SMALLTEXTOUT bounds supplied with ETO_NO_RECT",
      ));
    }
  } else if value.bounds.is_none() {
    return Err(Error::invalid(
      0,
      "EMR_SMALLTEXTOUT bounds missing without ETO_NO_RECT",
    ));
  }
  let text_bytes = value.text.encoded_bytes()?;
  if !value.options.contains(ExtTextOutOptions::SMALL_CHARS) && !text_bytes.len().is_multiple_of(2)
  {
    return Err(Error::invalid(
      0,
      "EMR_SMALLTEXTOUT UTF-16 text byte length is odd",
    ));
  }
  Ok(())
}

fn validate_emf_record_alignment_padding(
  padding: &[u8],
  unpadded_size: usize,
  record_name: &str,
) -> Result<()> {
  if padding.len() > 3 {
    return Err(Error::invalid(
      0,
      format!("{record_name} alignment padding exceeds 3 bytes"),
    ));
  }
  let expected_padding = (4 - (unpadded_size % 4)) % 4;
  if padding.len() != expected_padding {
    return Err(Error::invalid(
      0,
      format!(
        "{record_name} alignment padding has {} bytes; expected {}",
        padding.len(),
        expected_padding
      ),
    ));
  }
  Ok(())
}

fn write_emf_record_alignment_padding<W: std::io::Write>(
  writer: &mut Writer<W>,
  padding: &[u8],
  record_name: &str,
) -> Result<()> {
  if padding.is_empty() {
    return pad_writer_to_4(writer);
  }
  if padding.len() > 3 {
    return Err(Error::invalid(
      0,
      format!("{record_name} alignment padding exceeds 3 bytes"),
    ));
  }
  writer.write_all(padding)?;
  if !writer.position()?.is_multiple_of(4) {
    return Err(Error::invalid(
      0,
      format!("{record_name} alignment padding does not align the record"),
    ));
  }
  Ok(())
}

fn validate_log_color_space(value: &LogColorSpace, filename_bytes: usize) -> Result<()> {
  if value.signature_kind().is_none() {
    return Err(Error::invalid(
      0,
      "LogColorSpace Signature is not a valid LogColorSpaceSignature",
    ));
  }
  if value.version != 0x0000_0400 {
    return Err(Error::invalid(0, "LogColorSpace Version must be 0x0400"));
  }
  let expected_size = usize_to_u32(
    LogColorSpace::sdk_size(filename_bytes),
    "LogColorSpace Size",
  )?;
  if value.size != expected_size {
    return Err(Error::invalid(
      0,
      "LogColorSpace Size does not match the encoded filename width",
    ));
  }
  if value.color_space_type_kind().is_none() {
    return Err(Error::invalid(0, "LogColorSpace ColorSpaceType is invalid"));
  }
  if value.intent_kind().is_none() {
    return Err(Error::invalid(0, "LogColorSpace Intent is invalid"));
  }
  Ok(())
}

fn validate_emr_ext_create_font_indirect_w(value: &EmrExtCreateFontIndirectW) -> Result<()> {
  validate_emr_created_object_index(value.object_index, "EMR_EXTCREATEFONTINDIRECTW", "ihFonts")?;
  validate_emr_ext_create_font(&value.font)
}

fn validate_emr_ext_create_font(value: &EmrExtCreateFont) -> Result<()> {
  match value {
    EmrExtCreateFont::LogFont(value) => validate_log_font_w(value),
    EmrExtCreateFont::LogFontPanose(value) => validate_log_font_panose(value),
    EmrExtCreateFont::LogFontExDv(value) => {
      validate_log_font_w(&value.log_font_ex.log_font)?;
      validate_design_vector(&value.design_vector)
    }
    EmrExtCreateFont::Raw(data) => {
      if data.len() == LogFontW::SIZE {
        return Err(Error::invalid(
          0,
          "EMR_EXTCREATEFONTINDIRECTW raw elw uses LogFont size",
        ));
      }
      if data.len() >= LOGFONT_PANOSE_SIZE {
        return Err(Error::invalid(
          0,
          "EMR_EXTCREATEFONTINDIRECTW raw elw uses a known LogFontPanose/LogFontExDv shape",
        ));
      }
      Ok(())
    }
  }
}

fn validate_emr_ext_create_font_strict(value: &EmrExtCreateFont) -> Result<()> {
  validate_emr_ext_create_font(value)?;
  match value {
    EmrExtCreateFont::LogFont(value) => validate_log_font_w_strict(value),
    EmrExtCreateFont::LogFontPanose(value) => validate_log_font_w_strict(&value.log_font),
    EmrExtCreateFont::LogFontExDv(value) => validate_log_font_w_strict(&value.log_font_ex.log_font),
    EmrExtCreateFont::Raw(_) => Ok(()),
  }
}

fn validate_emr_create_color_space(value: &EmrCreateColorSpace) -> Result<()> {
  validate_emr_created_object_index(value.color_space_index, "EMR_CREATECOLORSPACE", "ihCS")
}

fn validate_emr_create_color_space_strict(value: &EmrCreateColorSpace) -> Result<()> {
  validate_emr_create_color_space(value)?;
  validate_log_color_space(&value.log_color_space, 260)?;
  if value
    .log_color_space
    .filename
    .raw_bytes()
    .is_some_and(|bytes| bytes.len() != 260)
  {
    return Err(Error::invalid(
      0,
      "LogColorSpace Filename must occupy 260 bytes",
    ));
  }
  Ok(())
}

fn validate_emr_create_color_space_w(value: &EmrCreateColorSpaceW) -> Result<()> {
  validate_emr_created_object_index(value.color_space_index, "EMR_CREATECOLORSPACEW", "ihCS")?;
  if value.invalid_flag_bits() != 0 {
    return Err(Error::invalid(
      0,
      "EMR_CREATECOLORSPACEW dwFlags contains invalid bits",
    ));
  }
  validate_emf_record_alignment_padding(
    &value.padding,
    12 + LogColorSpace::sdk_size(520) + value.data.len(),
    "EMR_CREATECOLORSPACEW",
  )?;
  Ok(())
}

fn validate_emr_color_match_to_target_w(value: &EmrColorMatchToTargetW) -> Result<()> {
  if value.action_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_COLORMATCHTOTARGETW dwAction is invalid",
    ));
  }
  if value.flags_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_COLORMATCHTOTARGETW dwFlags is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_comment_multi_formats(value: &EmrCommentMultiFormats) -> Result<()> {
  let mut total_size = 0usize;
  let format_data_start = value.format_data_start_offset()?;
  let mut ranges = Vec::with_capacity(value.formats.len());
  for (index, format) in value.formats.iter().enumerate() {
    let signature = format.signature_kind();
    if signature == Some(EmrFormatSignature::Eps) && format.version != 1 {
      return Err(Error::invalid(
        0,
        "EMR_COMMENT_MULTIFORMATS EPS Version must be 1",
      ));
    }
    if !format.data_offset.is_multiple_of(4) {
      return Err(Error::invalid(
        0,
        "EMR_COMMENT_MULTIFORMATS offData must be 32-bit aligned",
      ));
    }
    let format_data = value.format_data_slice(index)?;
    let start = (format.data_offset - format_data_start) as usize;
    let end = start
      .checked_add(format.size_data as usize)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_MULTIFORMATS data range overflows"))?;
    ranges.push((start, end));
    if signature == Some(EmrFormatSignature::Eps) {
      EmrEpsData::read_data(format_data)?;
    }
    total_size = total_size
      .checked_add(format.size_data as usize)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_MULTIFORMATS data size overflows"))?;
  }
  if total_size != value.format_data.len() {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_MULTIFORMATS FormatData length does not match SizeData",
    ));
  }
  ranges.sort_unstable();
  let mut next = 0usize;
  for (start, end) in ranges {
    if start != next {
      return Err(Error::invalid(
        0,
        "EMR_COMMENT_MULTIFORMATS data ranges overlap or leave gaps",
      ));
    }
    next = end;
  }
  Ok(())
}

fn validate_emr_comment_multi_formats_strict(value: &EmrCommentMultiFormats) -> Result<()> {
  validate_emr_comment_multi_formats(value)?;
  if value
    .formats
    .iter()
    .any(|format| format.signature_kind().is_none())
  {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_MULTIFORMATS Signature is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_comment_windows_metafile(value: &EmrCommentWindowsMetafile) -> Result<()> {
  if value.version_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_WINDOWS_METAFILE Version is invalid",
    ));
  }
  if value.reserved != 0 {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_WINDOWS_METAFILE Reserved must be zero",
    ));
  }
  if value.flags != 0 {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_WINDOWS_METAFILE Flags must be zero",
    ));
  }
  if value.metafile_size as usize != value.metafile.len() {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_WINDOWS_METAFILE WinMetafileSize does not match data length",
    ));
  }
  Ok(())
}

fn validate_emr_comment_begin_group(value: &EmrCommentBeginGroup) -> Result<()> {
  let description = value.description.encoded_bytes()?;
  let expected_len = (value.description_chars as usize)
    .checked_mul(2)
    .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_BEGINGROUP description size overflows"))?;
  if description.len() != expected_len {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_BEGINGROUP nDescription does not match Description length",
    ));
  }
  if value.description_chars != 0 && !description.ends_with(&[0, 0]) {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_BEGINGROUP Description must be null-terminated UTF-16LE",
    ));
  }
  Ok(())
}

fn validate_emr_comment_emf_plus(
  records: &[crate::emfplus::EmfPlusRecord],
  _emf_plus_trailing_data: &[u8],
  alignment_padding: &[u8],
) -> Result<()> {
  if records.is_empty() {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_EMFPLUS must contain at least one EMF+ record",
    ));
  }
  validate_emr_comment_alignment_padding(alignment_padding)
}

fn validate_emr_comment_emf_spool(
  spool_identifier: u32,
  data: &[u8],
  alignment_padding: &[u8],
) -> Result<()> {
  if spool_identifier != EMR_COMMENT_EMFSPOOL_FONT_DEFINITION {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_EMFSPOOL identifier must be EMFSPOOL font definition",
    ));
  }
  if data.is_empty() {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT_EMFSPOOL must contain at least one font definition record",
    ));
  }
  validate_emr_comment_alignment_padding(alignment_padding)
}

fn validate_unknown_public_comment_identifier(identifier: u32, offset: u64) -> Result<()> {
  if EmrPublicCommentIdentifier::from_raw(identifier).is_some() {
    return Err(Error::invalid(
      offset,
      "EMR_COMMENT_PUBLIC Unknown comment requires an unknown identifier",
    ));
  }
  Ok(())
}

fn validate_emr_private_data(data: &[u8], alignment_padding: &[u8]) -> Result<()> {
  if data.len() >= 4 {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT PrivateData without CommentIdentifier must be shorter than 4 bytes",
    ));
  }
  validate_emr_comment_alignment_padding(alignment_padding)?;
  let record_data_size = 4usize
    .checked_add(data.len())
    .and_then(|size| size.checked_add(alignment_padding.len()))
    .ok_or_else(|| Error::invalid(0, "EMR_COMMENT size overflows"))?;
  if !alignment_padding.is_empty() && !record_data_size.is_multiple_of(4) {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT alignment padding does not align the record",
    ));
  }
  Ok(())
}

fn validate_emr_raw_comment(
  data_size: u32,
  identifier: u32,
  data: &[u8],
  alignment_padding: &[u8],
) -> Result<()> {
  if matches!(
    identifier,
    EMR_COMMENT_EMFSPOOL | EMR_COMMENT_EMFPLUS | EMR_COMMENT_PUBLIC
  ) {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT private data must not use a predefined comment identifier",
    ));
  }
  let expected_data_size = data
    .len()
    .checked_add(4)
    .ok_or_else(|| Error::invalid(0, "EMR_COMMENT data size overflows"))?;
  if data_size as usize != expected_data_size {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT DataSize must match identifier plus private data",
    ));
  }
  validate_emr_comment_alignment_padding(alignment_padding)?;
  let record_data_size = 4usize
    .checked_add(data_size as usize)
    .and_then(|size| size.checked_add(alignment_padding.len()))
    .ok_or_else(|| Error::invalid(0, "EMR_COMMENT size overflows"))?;
  if !record_data_size.is_multiple_of(4) {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT alignment padding does not align the record",
    ));
  }
  Ok(())
}

fn validate_emr_comment_alignment_padding(_alignment_padding: &[u8]) -> Result<()> {
  Ok(())
}

fn validate_emr_comment_alignment_padding_strict(alignment_padding: &[u8]) -> Result<()> {
  if alignment_padding.len() > 3 {
    return Err(Error::invalid(
      0,
      "EMR_COMMENT alignment padding exceeds 3 bytes",
    ));
  }
  Ok(())
}

fn write_emr_comment_alignment_padding<W: std::io::Write>(
  writer: &mut Writer<W>,
  alignment_padding: &[u8],
) -> Result<()> {
  if alignment_padding.is_empty() {
    pad_writer_to_4(writer)
  } else {
    validate_emr_comment_alignment_padding(alignment_padding)?;
    writer.write_all(alignment_padding)?;
    if !writer.position()?.is_multiple_of(4) {
      return Err(Error::invalid(
        0,
        "EMR_COMMENT alignment padding does not align the record",
      ));
    }
    Ok(())
  }
}

fn validate_emr_eof_size_last(value: &EmrEof, record_size: usize) -> Result<()> {
  let record_size = usize_to_u32(record_size, "EMR_EOF record size")?;
  if value.size_last != record_size {
    return Err(Error::invalid(0, "EMR_EOF SizeLast must match record Size"));
  }
  Ok(())
}

fn validate_emr_poly_bezier_points(count: usize, name: &str) -> Result<()> {
  if count < 4 || !(count - 1).is_multiple_of(3) {
    return Err(Error::invalid(
      0,
      format!("{name} point count must be 1 plus a multiple of 3"),
    ));
  }
  Ok(())
}

fn validate_emr_poly_bezier_to_points(count: usize, name: &str) -> Result<()> {
  if count < 3 || !count.is_multiple_of(3) {
    return Err(Error::invalid(
      0,
      format!("{name} point count must be a positive multiple of 3"),
    ));
  }
  Ok(())
}

fn validate_emf_header(value: &EmfHeader) -> Result<()> {
  if value.signature != EMF_SIGNATURE {
    return Err(Error::invalid(
      0,
      "EMR_HEADER RecordSignature must be ENHMETA_SIGNATURE",
    ));
  }
  if value.reserved != 0 {
    return Err(Error::invalid(0, "EMR_HEADER Reserved must be zero"));
  }
  if let Some(extension1) = value.header_extension1()?
    && extension1.opengl_present().is_none()
  {
    return Err(Error::invalid(0, "EMR_HEADER bOpenGL must be 0 or 1"));
  }
  if let Some(extension1) = value.header_extension1()?
    && (extension1.pixel_format_size == 0) != (extension1.pixel_format_offset == 0)
  {
    return Err(Error::invalid(
      0,
      "EMR_HEADER cbPixelFormat and offPixelFormat must both be zero or both be present",
    ));
  }
  value.pixel_format_descriptor()?;
  Ok(())
}

fn validate_emf_header_strict(value: &EmfHeader) -> Result<()> {
  validate_emf_header(value)?;
  if let Some(description) = value.description()? {
    let bytes = description.encoded_bytes()?;
    if bytes.len() < 2 || bytes[bytes.len() - 2..] != [0, 0] {
      return Err(Error::invalid(
        0,
        "EMR_HEADER description must be UTF-16 null-terminated",
      ));
    }
  }
  Ok(())
}

fn validate_emr_pixel_format(value: &EmrPixelFormat) -> Result<()> {
  if value.n_size != EmrPixelFormat::SIZE {
    return Err(Error::invalid(0, "EMR_PIXELFORMAT nSize must be 40"));
  }
  if value.n_version != 1 {
    return Err(Error::invalid(0, "EMR_PIXELFORMAT nVersion must be 1"));
  }
  if value.invalid_flag_bits() != 0 {
    return Err(Error::invalid(
      0,
      "EMR_PIXELFORMAT dwFlags contains invalid PixelFormatDescriptor flags",
    ));
  }
  let flags = value.flags();
  if flags.contains(EmrPixelFormatFlags::DOUBLEBUFFER)
    && flags.contains(EmrPixelFormatFlags::SUPPORT_GDI)
  {
    return Err(Error::invalid(
      0,
      "EMR_PIXELFORMAT PFD_DOUBLEBUFFER and PFD_SUPPORT_GDI must not both be set",
    ));
  }
  if value.pixel_type_kind().is_none() {
    return Err(Error::invalid(0, "EMR_PIXELFORMAT iPixelType is invalid"));
  }
  Ok(())
}

fn validate_emr_poly_counts(
  counts: &[u32],
  point_count: usize,
  require_polyline_counts: bool,
) -> Result<()> {
  let mut total = 0usize;
  for count in counts {
    if require_polyline_counts && *count < 2 {
      return Err(Error::invalid(
        0,
        "EMR_POLYPOLYLINE point counts must be at least 2",
      ));
    }
    total = total
      .checked_add(*count as usize)
      .ok_or_else(|| Error::invalid(0, "EMF poly point count overflow"))?;
  }
  if total != point_count {
    return Err(Error::invalid(
      0,
      "EMF poly point counts must match the point array length",
    ));
  }
  Ok(())
}

fn validate_emr_poly_polygon_l(
  value: &EmrPolyPolygonL,
  require_polyline_counts: bool,
) -> Result<()> {
  validate_emr_poly_counts(&value.counts, value.points.len(), require_polyline_counts)
}

fn validate_emr_poly_polygon_s(
  value: &EmrPolyPolygonS,
  require_polyline_counts: bool,
) -> Result<()> {
  validate_emr_poly_counts(&value.counts, value.points.len(), require_polyline_counts)
}

fn validate_emr_alpha_blend(value: &EmrAlphaBlend) -> Result<()> {
  validate_emr_blend_function(&value.blend_function)?;
  if value.bitmap.is_none() {
    return Err(Error::invalid(
      0,
      "EMR_ALPHABLEND source bitmap is required",
    ));
  }
  if value.dest_size.cx <= 0 || value.dest_size.cy <= 0 {
    return Err(Error::invalid(
      0,
      "EMR_ALPHABLEND destination size must be positive",
    ));
  }
  if value.source_size.cx <= 0 || value.source_size.cy <= 0 {
    return Err(Error::invalid(
      0,
      "EMR_ALPHABLEND source size must be positive",
    ));
  }
  if value.color_usage_kind().is_none() {
    return Err(Error::invalid(0, "EMR_ALPHABLEND ColorUsage is invalid"));
  }
  Ok(())
}

fn validate_emr_gradient_fill(value: &EmrGradientFill) -> Result<()> {
  let vertex_count = value.vertices.len() as u32;
  match (value.mode_kind(), &value.mesh) {
    (
      Some(EmrGradientFillMode::RectangleHorizontal | EmrGradientFillMode::RectangleVertical),
      EmrGradientFillMesh::Rectangles {
        rectangles,
        padding,
      },
    ) => {
      let expected_padding = rectangles
        .len()
        .checked_mul(4)
        .ok_or_else(|| Error::invalid(0, "EMR_GRADIENTFILL rectangle padding overflows"))?;
      if padding.len() != expected_padding {
        return Err(Error::invalid(
          0,
          "EMR_GRADIENTFILL rectangle padding must be nTri * 4 bytes",
        ));
      }
      for rectangle in rectangles {
        if rectangle.upper_left >= vertex_count || rectangle.lower_right >= vertex_count {
          return Err(Error::invalid(
            0,
            "EMR_GRADIENTFILL rectangle vertex indexes must be smaller than nVer",
          ));
        }
      }
    }
    (Some(EmrGradientFillMode::Triangle), EmrGradientFillMesh::Triangles(triangles)) => {
      for triangle in triangles {
        if triangle.vertex1 >= vertex_count
          || triangle.vertex2 >= vertex_count
          || triangle.vertex3 >= vertex_count
        {
          return Err(Error::invalid(
            0,
            "EMR_GRADIENTFILL triangle vertex indexes must be smaller than nVer",
          ));
        }
      }
    }
    (None, _) => {
      return Err(Error::invalid(0, "EMR_GRADIENTFILL ulMode is invalid"));
    }
    _ => {
      return Err(Error::invalid(
        0,
        "EMR_GRADIENTFILL mesh type does not match ulMode",
      ));
    }
  }
  Ok(())
}

fn validate_emr_transparent_blt(value: &EmrTransparentBlt) -> Result<()> {
  if value.bitmap.is_none() {
    return Err(Error::invalid(
      0,
      "EMR_TRANSPARENTBLT source bitmap is required",
    ));
  }
  if !value.transparent_color.is_reserved_zero() {
    return Err(Error::invalid(
      0,
      "EMR_TRANSPARENTBLT TransparentColor Reserved must be 0",
    ));
  }
  if value.color_usage_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_TRANSPARENTBLT ColorUsage is invalid",
    ));
  }
  Ok(())
}

fn validate_emr_mask_blt(value: &EmrMaskBlt) -> Result<()> {
  validate_dib_color_usage(value.source_color_usage, "EMR_MASKBLT source ColorUsage")?;
  validate_dib_color_usage(value.mask_color_usage, "EMR_MASKBLT mask ColorUsage")?;
  validate_required_bitmap(value.source_bitmap.as_ref(), "EMR_MASKBLT", "source bitmap")?;
  validate_required_monochrome_mask_bitmap(value.mask_bitmap.as_ref(), "EMR_MASKBLT")
}

fn validate_emr_plg_blt(value: &EmrPlgBlt) -> Result<()> {
  validate_dib_color_usage(value.source_color_usage, "EMR_PLGBLT source ColorUsage")?;
  validate_dib_color_usage(value.mask_color_usage, "EMR_PLGBLT mask ColorUsage")?;
  validate_required_bitmap(value.source_bitmap.as_ref(), "EMR_PLGBLT", "source bitmap")
}

fn validate_emr_plg_blt_strict(value: &EmrPlgBlt) -> Result<()> {
  validate_emr_plg_blt(value)?;
  validate_required_monochrome_mask_bitmap(value.mask_bitmap.as_ref(), "EMR_PLGBLT")
}

fn validate_required_bitmap(
  bitmap: Option<&EmrBitmapBuffer>,
  record_name: &str,
  field_name: &str,
) -> Result<()> {
  if bitmap.is_some() {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{record_name} {field_name} is required"),
    ))
  }
}

fn validate_required_monochrome_mask_bitmap(
  bitmap: Option<&EmrBitmapBuffer>,
  record_name: &str,
) -> Result<()> {
  let Some(bitmap) = bitmap else {
    return Err(Error::invalid(
      0,
      format!("{record_name} mask bitmap is required"),
    ));
  };
  let info = bitmap.dib_info()?;
  if info.header.bit_count_kind() == Some(BitmapBitCount::One) {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{record_name} mask bitmap must be monochrome"),
    ))
  }
}

fn validate_dib_color_usage(value: u32, name: &str) -> Result<()> {
  if DibColorUsage::from_raw(value).is_some() {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} is invalid")))
  }
}

fn validate_emr_point_type_value(value: u8) -> Result<()> {
  if EmrPointType::from_raw(value).is_some()
    || matches!(
        value,
        value if value == (EmrPointType::CloseFigure.raw() | EmrPointType::LineTo.raw())
            || value == (EmrPointType::CloseFigure.raw() | EmrPointType::BezierTo.raw())
    )
  {
    Ok(())
  } else {
    Err(Error::invalid(0, "EMF Point type value is invalid"))
  }
}

fn validate_emr_poly_draw_point_types(values: &[EmrPointTypeValue], name: &str) -> Result<()> {
  let mut index = 0;
  while index < values.len() {
    if values[index].point_type() != Some(EmrPointType::BezierTo) {
      index += 1;
      continue;
    }
    let start = index;
    while index < values.len() && values[index].point_type() == Some(EmrPointType::BezierTo) {
      index += 1;
    }
    if !(index - start).is_multiple_of(3) {
      return Err(Error::invalid(
        0,
        format!("{name} PT_BEZIERTO values must occur in sets of three"),
      ));
    }
  }
  Ok(())
}

fn validate_emr_blend_function(value: &EmrBlendFunction) -> Result<()> {
  if value.blend_operation_kind().is_none() {
    return Err(Error::invalid(
      0,
      "EMR_ALPHABLEND BlendOperation is invalid",
    ));
  }
  if value.alpha_format_kind().is_none() {
    return Err(Error::invalid(0, "EMR_ALPHABLEND AlphaFormat is invalid"));
  }
  Ok(())
}

fn validate_log_pen_ex(_value: &LogPenEx) -> Result<()> {
  Ok(())
}

fn validate_log_pen_ex_strict(value: &LogPenEx) -> Result<()> {
  validate_emr_pen_style(
    value.pen_line_style_kind(),
    value.pen_end_cap_kind(),
    value.pen_join_kind(),
    value.pen_type_kind(),
    value.pen_reserved_bits(),
  )?;
  if value.pen_type_kind() == Some(EmrPenType::Cosmetic) && value.width != 1 {
    return Err(Error::invalid(0, "LogPenEx cosmetic pen width must be 1"));
  }

  let Some(brush_style) = value.brush_style_kind() else {
    return Err(Error::invalid(
      0,
      "LogPenEx BrushStyle is not a valid BrushStyle",
    ));
  };
  if value.pen_type_kind() == Some(EmrPenType::Geometric) {
    match brush_style {
      WmfBrushStyle::Solid | WmfBrushStyle::Hatched => {}
      WmfBrushStyle::Null if value.pen_line_style_kind() == Some(EmrPenLineStyle::Null) => {}
      _ => {
        return Err(Error::invalid(
          0,
          "LogPenEx geometric pen BrushStyle must be BS_SOLID, BS_HATCHED, or BS_NULL with PS_NULL",
        ));
      }
    }
  }
  if brush_style == WmfBrushStyle::Hatched && value.brush_hatch_kind().is_none() {
    return Err(Error::invalid(
      0,
      "LogPenEx BrushHatch is not a valid HatchStyle for BS_HATCHED",
    ));
  }
  if brush_style == WmfBrushStyle::Hatched
    && value.pen_type_kind() != Some(EmrPenType::Geometric)
    && !matches!(
      value.brush_hatch_kind(),
      Some(EmrHatchStyle::SolidTextColor | EmrHatchStyle::SolidBackgroundColor)
    )
  {
    return Err(Error::invalid(
      0,
      "LogPenEx non-geometric hatched pen BrushHatch must be HS_SOLIDTEXTCLR or HS_SOLIDBKCLR",
    ));
  }
  Ok(())
}

fn validate_emr_ext_create_pen(value: &EmrExtCreatePen) -> Result<()> {
  validate_emr_created_object_index(value.object_index, "EMR_EXTCREATEPEN", "ihPen")?;
  validate_log_pen_ex(&value.log_pen_ex())?;
  value.bitmap()?;
  Ok(())
}

fn validate_emr_ext_create_pen_strict(value: &EmrExtCreatePen) -> Result<()> {
  validate_emr_ext_create_pen(value)?;
  validate_log_pen_ex_strict(&value.log_pen_ex())?;
  value.color.validate_strict()
}

fn validate_emr_pen_style(
  line_style: Option<EmrPenLineStyle>,
  end_cap: Option<EmrPenEndCap>,
  join: Option<EmrPenJoin>,
  pen_type: Option<EmrPenType>,
  reserved_bits: u32,
) -> Result<()> {
  if line_style.is_none() {
    return Err(Error::invalid(
      0,
      "EMF PenStyle line style is not a valid PenStyle",
    ));
  }
  if end_cap.is_none() {
    return Err(Error::invalid(
      0,
      "EMF PenStyle end cap is not a valid PenStyle",
    ));
  }
  if join.is_none() {
    return Err(Error::invalid(
      0,
      "EMF PenStyle join is not a valid PenStyle",
    ));
  }
  if pen_type.is_none() {
    return Err(Error::invalid(
      0,
      "EMF PenStyle type is not a valid PenStyle",
    ));
  }
  if reserved_bits != 0 {
    return Err(Error::invalid(0, "EMF PenStyle reserved bits must be zero"));
  }
  Ok(())
}

fn validate_emr_escape_function(value: u32) -> Result<()> {
  if value <= u16::MAX as u32 && WmfMetafileEscape::from_raw(value as u16).is_some() {
    Ok(())
  } else {
    Err(Error::invalid(0, "EMR_ESCAPE iEscape is invalid"))
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrOpenGlRecord {
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrOpenGlRecord {
  pub fn read_data(record_data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(record_data));
    let data_size = reader.read_u32()? as usize;
    ensure_record_remaining(
      &mut reader,
      record_data.len() as u64,
      data_size,
      "EMR_GLSRECORD Data",
    )?;
    let data = reader.read_vec(data_size)?;
    let padding = read_remaining(&mut reader, record_data)?;
    validate_emf_record_alignment_padding(&padding, 4 + data_size, "EMR_GLSRECORD")?;
    Ok(Self { data, padding })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      4 + self.data.len() + self.padding.len() + 3,
    )));
    writer.write_u32(usize_to_u32(self.data.len(), "EMR_GLSRECORD data size")?)?;
    writer.write_all(&self.data)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_GLSRECORD")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrGlsBoundedRecord {
  pub bounds: RectL,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrGlsBoundedRecord {
  pub fn read_data(record_data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(record_data));
    let bounds = RectL::read_from(&mut reader)?;
    let data_size = reader.read_u32()? as usize;
    ensure_record_remaining(
      &mut reader,
      record_data.len() as u64,
      data_size,
      "EMR_GLSBOUNDEDRECORD Data",
    )?;
    let data = reader.read_vec(data_size)?;
    let padding = read_remaining(&mut reader, record_data)?;
    validate_emf_record_alignment_padding(&padding, 20 + data_size, "EMR_GLSBOUNDEDRECORD")?;
    Ok(Self {
      bounds,
      data,
      padding,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      20 + self.data.len() + self.padding.len() + 3,
    )));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(
      self.data.len(),
      "EMR_GLSBOUNDEDRECORD data size",
    )?)?;
    writer.write_all(&self.data)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_GLSBOUNDEDRECORD")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_pixel_format")]
pub struct EmrPixelFormat {
  pub n_size: u16,
  pub n_version: u16,
  pub flags: u32,
  pub pixel_type: u8,
  pub color_bits: u8,
  pub red_bits: u8,
  pub red_shift: u8,
  pub green_bits: u8,
  pub green_shift: u8,
  pub blue_bits: u8,
  pub blue_shift: u8,
  pub alpha_bits: u8,
  pub alpha_shift: u8,
  pub accum_bits: u8,
  pub accum_red_bits: u8,
  pub accum_green_bits: u8,
  pub accum_blue_bits: u8,
  pub accum_alpha_bits: u8,
  pub depth_bits: u8,
  pub stencil_bits: u8,
  pub aux_buffers: u8,
  pub layer_type: u8,
  pub reserved: u8,
  pub layer_mask: u32,
  pub visible_mask: u32,
  pub damage_mask: u32,
}

impl EmrPixelFormat {
  pub const SIZE: u16 = 40;

  pub fn flags(&self) -> EmrPixelFormatFlags {
    EmrPixelFormatFlags::from_bits_retain(self.flags)
  }

  pub fn invalid_flag_bits(&self) -> u32 {
    self.flags & !EmrPixelFormatFlags::all().bits()
  }

  pub fn pixel_type_kind(&self) -> Option<EmrPixelFormatType> {
    EmrPixelFormatType::from_raw(self.pixel_type)
  }

  pub const fn overlay_plane_count(&self) -> u8 {
    self.reserved & 0x0F
  }

  pub const fn underlay_plane_count(&self) -> u8 {
    self.reserved >> 4
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrForceUfiMapping {
  pub checksum: u32,
  pub index: u32,
}

pub type UniversalFontId = EmrForceUfiMapping;

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_color_correct_palette")]
pub struct EmrColorCorrectPalette {
  pub palette_index: u32,
  pub first_entry: u32,
  pub palette_entries: u32,
  pub reserved: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrSetLinkedUfis {
  pub ufis: Vec<EmrForceUfiMapping>,
  pub reserved: [u8; 8],
}

impl EmrSetLinkedUfis {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let count = reader.read_u32()? as usize;
    let ufis_bytes = checked_record_array_bytes(count, 8, "EMR_SETLINKEDUFIS UFIs")?;
    let required = ufis_bytes
      .checked_add(8)
      .ok_or_else(|| Error::invalid(0, "EMR_SETLINKEDUFIS size overflows"))?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      required,
      "EMR_SETLINKEDUFIS UFIs and reserved data",
    )?;
    let mut ufis = Vec::with_capacity(count);
    for _ in 0..count {
      ufis.push(EmrForceUfiMapping::read_from(&mut reader)?);
    }
    let reserved = reader.read_array::<8>()?;
    ensure_reader_end(&mut reader, data.len() as u64, "EMR_SETLINKEDUFIS")?;
    Ok(Self { ufis, reserved })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(12 + self.ufis.len() * 8)));
    writer.write_u32(usize_to_u32(
      self.ufis.len(),
      "EMR_SETLINKEDUFIS UFI count",
    )?)?;
    for ufi in &self.ufis {
      ufi.write_to(&mut writer)?;
    }
    writer.write_all(&self.reserved)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrColorProfile {
  pub flags: u32,
  pub name: SdkString,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrColorProfile {
  pub fn read_data(record_data: &[u8], encoding: SdkEncoding, name: &str) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(record_data));
    let flags = reader.read_u32()?;
    let name_size = reader.read_u32()? as usize;
    let data_size = reader.read_u32()? as usize;
    if encoding == SdkEncoding::Utf16Le && !name_size.is_multiple_of(2) {
      return Err(Error::invalid(4, format!("{name} UTF-16 name size is odd")));
    }
    let variable_size = name_size
      .checked_add(data_size)
      .ok_or_else(|| Error::invalid(0, format!("{name} profile data size overflows")))?;
    ensure_record_remaining(&mut reader, record_data.len() as u64, variable_size, name)?;
    let profile_name = SdkString::raw(reader.read_vec(name_size)?, encoding);
    let profile_data = reader.read_vec(data_size)?;
    let padding = read_remaining(&mut reader, record_data)?;
    validate_emf_record_alignment_padding(&padding, 12 + variable_size, name)?;
    Ok(Self {
      flags,
      name: profile_name,
      data: profile_data,
      padding,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    let name = self.name.encoded_bytes()?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      12 + name.len() + self.data.len() + self.padding.len() + 3,
    )));
    writer.write_u32(self.flags)?;
    writer.write_u32(usize_to_u32(name.len(), "EMR_SETICMPROFILE name size")?)?;
    writer.write_u32(usize_to_u32(
      self.data.len(),
      "EMR_SETICMPROFILE profile data size",
    )?)?;
    writer.write_all(&name)?;
    writer.write_all(&self.data)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_SETICMPROFILE")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrColorMatchToTargetW {
  pub action: u32,
  pub flags: u32,
  pub name: SdkString,
  pub data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrColorMatchToTargetW {
  pub fn action_kind(&self) -> Option<EmrColorSpaceMode> {
    EmrColorSpaceMode::from_raw(self.action)
  }

  pub fn flags_kind(&self) -> Option<EmrColorMatchToTarget> {
    EmrColorMatchToTarget::from_raw(self.flags)
  }

  pub fn read_data(record_data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(record_data));
    let action = reader.read_u32()?;
    let flags = reader.read_u32()?;
    let name_size = reader.read_u32()? as usize;
    let data_size = reader.read_u32()? as usize;
    if !name_size.is_multiple_of(2) {
      return Err(Error::invalid(
        8,
        "EMR_COLORMATCHTOTARGETW UTF-16 name size is odd",
      ));
    }
    let variable_size = name_size.checked_add(data_size).ok_or_else(|| {
      Error::invalid(
        0,
        "EMR_COLORMATCHTOTARGETW target profile data size overflows",
      )
    })?;
    ensure_record_remaining(
      &mut reader,
      record_data.len() as u64,
      variable_size,
      "EMR_COLORMATCHTOTARGETW target profile data",
    )?;
    let name = SdkString::raw(reader.read_vec(name_size)?, SdkEncoding::Utf16Le);
    let data = reader.read_vec(data_size)?;
    let padding = read_remaining(&mut reader, record_data)?;
    validate_emf_record_alignment_padding(&padding, 16 + variable_size, "EMR_COLORMATCHTOTARGETW")?;
    Ok(Self {
      action,
      flags,
      name,
      data,
      padding,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_color_match_to_target_w(self)?;
    let name = self.name.encoded_bytes()?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      16 + name.len() + self.data.len() + self.padding.len() + 3,
    )));
    writer.write_u32(self.action)?;
    writer.write_u32(self.flags)?;
    writer.write_u32(usize_to_u32(
      name.len(),
      "EMR_COLORMATCHTOTARGETW name size",
    )?)?;
    writer.write_u32(usize_to_u32(
      self.data.len(),
      "EMR_COLORMATCHTOTARGETW profile data size",
    )?)?;
    writer.write_all(&name)?;
    writer.write_all(&self.data)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_COLORMATCHTOTARGETW")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrText {
  pub reference: PointL,
  pub options: ExtTextOutOptions,
  pub rectangle: Option<RectL>,
  pub text: SdkString,
  pub undefined_space_before_string: Vec<u8>,
  pub dx_buffer_present: bool,
  pub undefined_space_before_dx: Vec<u8>,
  pub dx: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrExtTextOut {
  pub bounds: RectL,
  pub graphics_mode: u32,
  pub ex_scale: f32,
  pub ey_scale: f32,
  pub text: EmrText,
  pub padding: Vec<u8>,
}

impl EmrExtTextOut {
  pub fn graphics_mode_kind(&self) -> Option<EmrGraphicsMode> {
    EmrGraphicsMode::from_raw(self.graphics_mode)
  }

  pub fn read_data(data: &[u8], wide: bool) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let graphics_mode = reader.read_u32()?;
    let ex_scale = reader.read_f32()?;
    let ey_scale = reader.read_f32()?;
    let (mut text, buffer_ranges) = read_emr_text(&mut reader, data, wide, "EMR_EXTTEXTOUT")?;
    let descriptor_end = reader.position()? as usize;
    if buffer_ranges.string_start < descriptor_end {
      return Err(Error::invalid(
        0,
        "EMR_EXTTEXTOUT string overlaps the fixed text descriptor",
      ));
    }
    text.undefined_space_before_string = data[descriptor_end..buffer_ranges.string_start].to_vec();
    text.undefined_space_before_dx = if let Some(dx_start) = buffer_ranges.dx_start {
      if dx_start < buffer_ranges.string_end {
        return Err(Error::invalid(
          0,
          "EMR_EXTTEXTOUT dx buffer overlaps or precedes the string buffer",
        ));
      }
      data[buffer_ranges.string_end..dx_start].to_vec()
    } else {
      Vec::new()
    };
    let highest_consumed = buffer_ranges.consumed_end().max(descriptor_end);
    let padding = data
      .get(highest_consumed..)
      .ok_or_else(|| Error::invalid(0, "EMR_EXTTEXTOUT data range is out of bounds"))?
      .to_vec();
    validate_emf_record_alignment_padding(&padding, highest_consumed, "EMR_EXTTEXTOUT")?;

    let value = Self {
      bounds,
      graphics_mode,
      ex_scale,
      ey_scale,
      text,
      padding,
    };
    validate_emr_ext_text_out(&value, wide, "EMR_EXTTEXTOUT")?;
    Ok(value)
  }

  pub fn to_data(&self, wide: bool) -> Result<Vec<u8>> {
    validate_emr_ext_text_out(self, wide, "EMR_EXTTEXTOUT")?;
    let text_bytes = self.text.text.encoded_bytes()?;
    let has_rect = self.text.rectangle.is_some();
    let fixed_size = 16 + 4 + 4 + 4 + 8 + 4 + 4 + 4 + if has_rect { 16 } else { 0 } + 4;
    let string_offset = 8usize
      .checked_add(fixed_size)
      .and_then(|value| value.checked_add(self.text.undefined_space_before_string.len()))
      .ok_or_else(|| Error::invalid(0, "EMR_EXTTEXTOUT string offset overflows"))?;
    validate_record_relative_alignment(
      string_offset,
      if wide { 2 } else { 1 },
      "EMR_EXTTEXTOUT offString",
    )?;
    let dx_offset = if !self.text.dx_buffer_present {
      0
    } else if self.text.undefined_space_before_dx.is_empty() {
      align_to_u32(
        string_offset
          .checked_add(text_bytes.len())
          .ok_or_else(|| Error::invalid(0, "EMR_EXTTEXTOUT dx offset overflows"))?,
      )
    } else {
      let offset = string_offset
        .checked_add(text_bytes.len())
        .and_then(|value| value.checked_add(self.text.undefined_space_before_dx.len()))
        .ok_or_else(|| Error::invalid(0, "EMR_EXTTEXTOUT dx offset overflows"))?;
      validate_record_relative_alignment(offset, 4, "EMR_EXTTEXTOUT offDx")?;
      offset
    };
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      fixed_size + text_bytes.len() + self.text.dx.len() * 4 + 4,
    )));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(self.graphics_mode)?;
    writer.write_f32(self.ex_scale)?;
    writer.write_f32(self.ey_scale)?;
    self.text.reference.write_to(&mut writer)?;
    let char_count = if wide {
      text_bytes.len() / 2
    } else {
      text_bytes.len()
    };
    writer.write_u32(usize_to_u32(char_count, "EMR_EXTTEXTOUT character count")?)?;
    writer.write_u32(usize_to_u32(string_offset, "EMR_EXTTEXTOUT string offset")?)?;
    writer.write_u32(self.text.options.bits())?;
    if let Some(rectangle) = &self.text.rectangle {
      rectangle.write_to(&mut writer)?;
    }
    writer.write_u32(usize_to_u32(dx_offset, "EMR_EXTTEXTOUT dx offset")?)?;
    writer.write_all(&self.text.undefined_space_before_string)?;
    writer.write_all(&text_bytes)?;
    if dx_offset != 0 {
      if self.text.undefined_space_before_dx.is_empty() {
        pad_writer_to_record_offset(&mut writer, dx_offset)?;
      } else {
        writer.write_all(&self.text.undefined_space_before_dx)?;
      }
      for value in &self.text.dx {
        writer.write_u32(*value)?;
      }
    }
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_EXTTEXTOUT")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrPolyTextOut {
  pub bounds: RectL,
  pub graphics_mode: u32,
  pub ex_scale: f32,
  pub ey_scale: f32,
  pub texts: Vec<EmrText>,
  pub padding: Vec<u8>,
}

impl EmrPolyTextOut {
  pub fn graphics_mode_kind(&self) -> Option<EmrGraphicsMode> {
    EmrGraphicsMode::from_raw(self.graphics_mode)
  }

  pub fn read_data(data: &[u8], wide: bool) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let graphics_mode = reader.read_u32()?;
    let ex_scale = reader.read_f32()?;
    let ey_scale = reader.read_f32()?;
    let strings = reader.read_u32()? as usize;
    let minimum_text_bytes =
      checked_record_array_bytes(strings, 20, "EMR_POLYTEXTOUT text headers")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      minimum_text_bytes,
      "EMR_POLYTEXTOUT text headers",
    )?;
    let mut texts = Vec::with_capacity(strings);
    let mut buffer_ranges = Vec::with_capacity(strings);

    for _ in 0..strings {
      let (text, ranges) = read_emr_text(&mut reader, data, wide, "EMR_POLYTEXTOUT")?;
      texts.push(text);
      buffer_ranges.push(ranges);
    }

    let descriptor_end = reader.position()? as usize;
    let mut highest_consumed = descriptor_end;
    for (text, ranges) in texts.iter_mut().zip(buffer_ranges) {
      if ranges.string_start < highest_consumed {
        return Err(Error::invalid(
          0,
          "EMR_POLYTEXTOUT string buffers overlap or precede their descriptors",
        ));
      }
      text.undefined_space_before_string = data[highest_consumed..ranges.string_start].to_vec();
      highest_consumed = ranges.string_end;
      if let Some(dx_start) = ranges.dx_start {
        if dx_start < highest_consumed {
          return Err(Error::invalid(
            0,
            "EMR_POLYTEXTOUT dx buffers overlap or precede their strings",
          ));
        }
        text.undefined_space_before_dx = data[highest_consumed..dx_start].to_vec();
        highest_consumed = ranges.dx_end.expect("dx end is present with dx start");
      }
    }
    let padding = data
      .get(highest_consumed..)
      .ok_or_else(|| Error::invalid(0, "EMR_POLYTEXTOUT data range is out of bounds"))?
      .to_vec();
    validate_emf_record_alignment_padding(&padding, highest_consumed, "EMR_POLYTEXTOUT")?;

    let value = Self {
      bounds,
      graphics_mode,
      ex_scale,
      ey_scale,
      texts,
      padding,
    };
    validate_emr_poly_text_out(&value, wide)?;
    Ok(value)
  }

  pub fn to_data(&self, wide: bool) -> Result<Vec<u8>> {
    validate_emr_poly_text_out(self, wide)?;
    let layouts = layout_emr_texts(&self.texts, wide, 8 + 32)?;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    self.bounds.write_to(&mut writer)?;
    writer.write_u32(self.graphics_mode)?;
    writer.write_f32(self.ex_scale)?;
    writer.write_f32(self.ey_scale)?;
    writer.write_u32(usize_to_u32(
      self.texts.len(),
      "EMR_POLYTEXTOUT string count",
    )?)?;

    for (text, layout) in self.texts.iter().zip(&layouts) {
      text.reference.write_to(&mut writer)?;
      writer.write_u32(usize_to_u32(
        layout.char_count,
        "EMR_POLYTEXTOUT character count",
      )?)?;
      writer.write_u32(usize_to_u32(
        layout.string_offset,
        "EMR_POLYTEXTOUT string offset",
      )?)?;
      writer.write_u32(text.options.bits())?;
      if let Some(rectangle) = &text.rectangle {
        rectangle.write_to(&mut writer)?;
      } else if emr_text_requires_rectangle(text.options) {
        return Err(Error::invalid(
          0,
          "EMR_POLYTEXTOUT rectangle missing for text options",
        ));
      }
      writer.write_u32(usize_to_u32(
        layout.dx_offset.unwrap_or(0),
        "EMR_POLYTEXTOUT dx offset",
      )?)?;
    }

    for (text, layout) in self.texts.iter().zip(&layouts) {
      if text.undefined_space_before_string.is_empty() {
        pad_writer_to_record_offset(&mut writer, layout.string_offset)?;
      } else {
        writer.write_all(&text.undefined_space_before_string)?;
      }
      writer.write_all(&layout.text_bytes)?;
      if let Some(dx_offset) = layout.dx_offset {
        if text.undefined_space_before_dx.is_empty() {
          pad_writer_to_record_offset(&mut writer, dx_offset)?;
        } else {
          writer.write_all(&text.undefined_space_before_dx)?;
        }
        for value in &text.dx {
          writer.write_u32(*value)?;
        }
      }
    }
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_POLYTEXTOUT")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrSmallTextOut {
  pub reference: PointL,
  pub options: ExtTextOutOptions,
  pub graphics_mode: u32,
  pub ex_scale: f32,
  pub ey_scale: f32,
  pub bounds: Option<RectL>,
  pub text: SdkString,
  pub padding: Vec<u8>,
}

impl EmrSmallTextOut {
  pub fn graphics_mode_kind(&self) -> Option<EmrGraphicsMode> {
    EmrGraphicsMode::from_raw(self.graphics_mode)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let reference = PointL {
      x: reader.read_i32()?,
      y: reader.read_i32()?,
    };
    let chars = reader.read_u32()? as usize;
    let options = ExtTextOutOptions::from_bits_retain(reader.read_u32()?);
    let graphics_mode = reader.read_u32()?;
    let ex_scale = reader.read_f32()?;
    let ey_scale = reader.read_f32()?;
    let bounds = if options.contains(ExtTextOutOptions::NO_RECT) {
      None
    } else {
      Some(RectL::read_from(&mut reader)?)
    };
    let (encoding, text_len) = if options.contains(ExtTextOutOptions::SMALL_CHARS) {
      (SdkEncoding::UnicodeLowByte, chars)
    } else {
      (
        SdkEncoding::Utf16Le,
        chars
          .checked_mul(2)
          .ok_or_else(|| Error::invalid(0, "EMR_SMALLTEXTOUT text length overflows"))?,
      )
    };
    let text = SdkString::raw(reader.read_vec(text_len)?, encoding);
    let padding = read_remaining(&mut reader, data)?;
    validate_emf_record_alignment_padding(
      &padding,
      28 + if bounds.is_some() { 16 } else { 0 } + text_len,
      "EMR_SMALLTEXTOUT",
    )?;

    let value = Self {
      reference,
      options,
      graphics_mode,
      ex_scale,
      ey_scale,
      bounds,
      text,
      padding,
    };
    validate_emr_small_text_out(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_small_text_out(self)?;
    let text_bytes = self.text.encoded_bytes()?;
    let char_count = if self.options.contains(ExtTextOutOptions::SMALL_CHARS) {
      text_bytes.len()
    } else {
      if !text_bytes.len().is_multiple_of(2) {
        return Err(Error::invalid(
          0,
          "EMR_SMALLTEXTOUT UTF-16 text byte length is odd",
        ));
      }
      text_bytes.len() / 2
    };
    let has_bounds = !self.options.contains(ExtTextOutOptions::NO_RECT);
    let bounds =
      if has_bounds {
        Some(self.bounds.ok_or_else(|| {
          Error::invalid(0, "EMR_SMALLTEXTOUT bounds missing without ETO_NO_RECT")
        })?)
      } else {
        None
      };
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      28 + if has_bounds { 16 } else { 0 } + text_bytes.len() + self.padding.len() + 3,
    )));
    writer.write_i32(self.reference.x)?;
    writer.write_i32(self.reference.y)?;
    writer.write_u32(usize_to_u32(
      char_count,
      "EMR_SMALLTEXTOUT character count",
    )?)?;
    writer.write_u32(self.options.bits())?;
    writer.write_u32(self.graphics_mode)?;
    writer.write_f32(self.ex_scale)?;
    writer.write_f32(self.ey_scale)?;
    if let Some(bounds) = bounds {
      bounds.write_to(&mut writer)?;
    }
    writer.write_all(&text_bytes)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_SMALLTEXTOUT")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrBitmapBuffer {
  pub undefined_space_before_bitmap_info: Vec<u8>,
  pub bitmap_info: Vec<u8>,
  pub undefined_space_before_bitmap_bits: Vec<u8>,
  pub bitmap_bits: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmrBitmapBufferOrder {
  #[default]
  SourceThenMask,
  MaskThenSource,
}

impl EmrBitmapBuffer {
  pub fn dib_info(&self) -> Result<DibBitmapInfo> {
    DibBitmapInfo::read_from_slice(&self.bitmap_info)
  }

  pub fn device_independent_bitmap(&self) -> Result<DeviceIndependentBitmap> {
    DeviceIndependentBitmap::from_parts(&self.bitmap_info, &self.bitmap_bits)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCreateMonoBrush {
  pub brush_index: u32,
  pub color_usage: u32,
  pub bitmap: EmrBitmapBuffer,
  pub padding: Vec<u8>,
}

impl EmrCreateMonoBrush {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let (brush_index, color_usage, bitmap, padding) =
      read_dib_brush_data(data, "EMR_CREATEMONOBRUSH")?;
    Ok(Self {
      brush_index,
      color_usage,
      bitmap,
      padding,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    write_dib_brush_data(
      self.brush_index,
      self.color_usage,
      &self.bitmap,
      &self.padding,
      "EMR_CREATEMONOBRUSH",
    )
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCreateDibPatternBrushPt {
  pub brush_index: u32,
  pub color_usage: u32,
  pub bitmap: EmrBitmapBuffer,
  pub padding: Vec<u8>,
}

impl EmrCreateDibPatternBrushPt {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let (brush_index, color_usage, bitmap, padding) =
      read_dib_brush_data(data, "EMR_CREATEDIBPATTERNBRUSHPT")?;
    Ok(Self {
      brush_index,
      color_usage,
      bitmap,
      padding,
    })
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    write_dib_brush_data(
      self.brush_index,
      self.color_usage,
      &self.bitmap,
      &self.padding,
      "EMR_CREATEDIBPATTERNBRUSHPT",
    )
  }
}

fn read_dib_brush_data(
  data: &[u8],
  record_name: &str,
) -> Result<(u32, u32, EmrBitmapBuffer, Vec<u8>)> {
  let mut reader = Reader::new(Cursor::new(data));
  let brush_index = reader.read_u32()?;
  validate_emr_created_object_index(brush_index, record_name, "ihBrush")?;
  let color_usage = reader.read_u32()?;
  validate_dib_color_usage(color_usage, "EMF DIB brush ColorUsage")?;
  let off_bmi = reader.read_u32()? as usize;
  let cb_bmi = reader.read_u32()? as usize;
  let off_bits = reader.read_u32()? as usize;
  let cb_bits = reader.read_u32()? as usize;
  let (bitmap, bitmap_end) =
    read_bitmap_buffer(data, 24, off_bmi, cb_bmi, off_bits, cb_bits, record_name)?;
  let padding = read_bitmap_record_padding(data, bitmap_end, record_name)?;
  Ok((brush_index, color_usage, bitmap, padding))
}

fn write_dib_brush_data(
  brush_index: u32,
  color_usage: u32,
  bitmap: &EmrBitmapBuffer,
  padding: &[u8],
  record_name: &str,
) -> Result<Vec<u8>> {
  validate_emr_created_object_index(brush_index, record_name, "ihBrush")?;
  validate_dib_color_usage(color_usage, "EMF DIB brush ColorUsage")?;
  let layout = layout_bitmap_buffer(24, bitmap, record_name)?;
  let mut writer = Writer::new(Cursor::new(Vec::with_capacity(layout.data_end + 3)));
  writer.write_u32(brush_index)?;
  writer.write_u32(color_usage)?;
  writer.write_u32(usize_to_u32(
    layout.off_bmi,
    format!("{record_name} bitmap info offset"),
  )?)?;
  writer.write_u32(usize_to_u32(bitmap.bitmap_info.len(), "bitmap info size")?)?;
  writer.write_u32(usize_to_u32(
    layout.off_bits,
    format!("{record_name} bitmap bits offset"),
  )?)?;
  writer.write_u32(usize_to_u32(bitmap.bitmap_bits.len(), "bitmap bits size")?)?;
  write_bitmap_buffer(&mut writer, bitmap)?;
  write_emf_record_alignment_padding(&mut writer, padding, record_name)?;
  Ok(writer.into_inner().into_inner())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrSetDiBitsToDevice {
  pub bounds: RectL,
  pub dest: PointL,
  pub source: BitmapSourceBounds,
  pub color_usage: u32,
  pub start_scan: u32,
  pub scan_lines: u32,
  pub bitmap: EmrBitmapBuffer,
  pub padding: Vec<u8>,
}

impl EmrSetDiBitsToDevice {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = PointL::read_from(&mut reader)?;
    let source = BitmapSourceBounds::read_from(&mut reader)?;
    let off_bmi = reader.read_u32()? as usize;
    let cb_bmi = reader.read_u32()? as usize;
    let off_bits = reader.read_u32()? as usize;
    let cb_bits = reader.read_u32()? as usize;
    let color_usage = reader.read_u32()?;
    let start_scan = reader.read_u32()?;
    let scan_lines = reader.read_u32()?;
    let (bitmap, bitmap_end) = read_bitmap_buffer(
      data,
      68,
      off_bmi,
      cb_bmi,
      off_bits,
      cb_bits,
      "EMR_SETDIBITSTODEVICE",
    )?;
    let value = Self {
      bounds,
      dest,
      source,
      color_usage,
      start_scan,
      scan_lines,
      bitmap,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_SETDIBITSTODEVICE")?,
    };
    validate_dib_color_usage(value.color_usage, "EMR_SETDIBITSTODEVICE ColorUsage")?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_dib_color_usage(self.color_usage, "EMR_SETDIBITSTODEVICE ColorUsage")?;
    let layout = layout_bitmap_buffer(68, &self.bitmap, "EMR_SETDIBITSTODEVICE")?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      layout.data_end + self.padding.len() + 3,
    )));
    self.bounds.write_to(&mut writer)?;
    self.dest.write_to(&mut writer)?;
    self.source.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(
      layout.off_bmi,
      "EMR_SETDIBITSTODEVICE bitmap info offset",
    )?)?;
    writer.write_u32(usize_to_u32(
      self.bitmap.bitmap_info.len(),
      "bitmap info size",
    )?)?;
    writer.write_u32(usize_to_u32(
      layout.off_bits,
      "EMR_SETDIBITSTODEVICE bitmap bits offset",
    )?)?;
    writer.write_u32(usize_to_u32(
      self.bitmap.bitmap_bits.len(),
      "bitmap bits size",
    )?)?;
    writer.write_u32(self.color_usage)?;
    writer.write_u32(self.start_scan)?;
    writer.write_u32(self.scan_lines)?;
    write_bitmap_buffer(&mut writer, &self.bitmap)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_SETDIBITSTODEVICE")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrStretchDiBits {
  pub bounds: RectL,
  pub dest: PointL,
  pub source: BitmapSourceBounds,
  pub color_usage: u32,
  pub raster_operation: u32,
  pub dest_size: SizeL,
  pub bitmap: EmrBitmapBuffer,
  pub padding: Vec<u8>,
}

impl EmrStretchDiBits {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = PointL::read_from(&mut reader)?;
    let source = BitmapSourceBounds::read_from(&mut reader)?;
    let off_bmi = reader.read_u32()? as usize;
    let cb_bmi = reader.read_u32()? as usize;
    let off_bits = reader.read_u32()? as usize;
    let cb_bits = reader.read_u32()? as usize;
    let color_usage = reader.read_u32()?;
    let raster_operation = reader.read_u32()?;
    let dest_size = SizeL::read_from(&mut reader)?;
    let (bitmap, bitmap_end) = read_bitmap_buffer(
      data,
      72,
      off_bmi,
      cb_bmi,
      off_bits,
      cb_bits,
      "EMR_STRETCHDIBITS",
    )?;
    let value = Self {
      bounds,
      dest,
      source,
      color_usage,
      raster_operation,
      dest_size,
      bitmap,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_STRETCHDIBITS")?,
    };
    validate_dib_color_usage(value.color_usage, "EMR_STRETCHDIBITS ColorUsage")?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_dib_color_usage(self.color_usage, "EMR_STRETCHDIBITS ColorUsage")?;
    let layout = layout_bitmap_buffer(72, &self.bitmap, "EMR_STRETCHDIBITS")?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      layout.data_end + self.padding.len() + 3,
    )));
    self.bounds.write_to(&mut writer)?;
    self.dest.write_to(&mut writer)?;
    self.source.write_to(&mut writer)?;
    writer.write_u32(usize_to_u32(
      layout.off_bmi,
      "EMR_STRETCHDIBITS bitmap info offset",
    )?)?;
    writer.write_u32(usize_to_u32(
      self.bitmap.bitmap_info.len(),
      "bitmap info size",
    )?)?;
    writer.write_u32(usize_to_u32(
      layout.off_bits,
      "EMR_STRETCHDIBITS bitmap bits offset",
    )?)?;
    writer.write_u32(usize_to_u32(
      self.bitmap.bitmap_bits.len(),
      "bitmap bits size",
    )?)?;
    writer.write_u32(self.color_usage)?;
    writer.write_u32(self.raster_operation)?;
    self.dest_size.write_to(&mut writer)?;
    write_bitmap_buffer(&mut writer, &self.bitmap)?;
    write_emf_record_alignment_padding(&mut writer, &self.padding, "EMR_STRETCHDIBITS")?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrBitBlt {
  pub bounds: RectL,
  pub dest: PointL,
  pub dest_size: SizeL,
  pub raster_operation: u32,
  pub source: PointL,
  pub xform_source: XForm,
  pub background_color_source: ColorRef,
  pub color_usage: u32,
  pub bitmap: Option<EmrBitmapBuffer>,
  pub padding: Vec<u8>,
}

impl EmrBitBlt {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = PointL::read_from(&mut reader)?;
    let dest_size = SizeL::read_from(&mut reader)?;
    let raster_operation = reader.read_u32()?;
    let source = PointL::read_from(&mut reader)?;
    let xform_source = XForm::read_from(&mut reader)?;
    let background_color_source = ColorRef::read_from(&mut reader)?;
    let color_usage = reader.read_u32()?;
    let off_bmi = reader.read_u32()? as usize;
    let cb_bmi = reader.read_u32()? as usize;
    let off_bits = reader.read_u32()? as usize;
    let cb_bits = reader.read_u32()? as usize;
    let (bitmap, bitmap_end) =
      read_optional_bitmap_buffer(data, 92, off_bmi, cb_bmi, off_bits, cb_bits)?;
    let value = Self {
      bounds,
      dest,
      dest_size,
      raster_operation,
      source,
      xform_source,
      background_color_source,
      color_usage,
      bitmap,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_BITBLT")?,
    };
    validate_dib_color_usage(value.color_usage, "EMR_BITBLT ColorUsage")?;
    validate_optional_source_bitmap(value.raster_operation, value.bitmap.is_some(), "EMR_BITBLT")?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_dib_color_usage(self.color_usage, "EMR_BITBLT ColorUsage")?;
    validate_optional_source_bitmap(self.raster_operation, self.bitmap.is_some(), "EMR_BITBLT")?;
    write_bit_blt_data(
      self.bounds,
      self.dest,
      self.dest_size,
      self.raster_operation.to_le_bytes(),
      self.source,
      self.xform_source,
      self.background_color_source,
      self.color_usage,
      None,
      self.bitmap.as_ref(),
      &self.padding,
      "EMR_BITBLT",
    )
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrStretchBlt {
  pub bounds: RectL,
  pub dest: PointL,
  pub dest_size: SizeL,
  pub raster_operation: u32,
  pub source: PointL,
  pub xform_source: XForm,
  pub background_color_source: ColorRef,
  pub color_usage: u32,
  pub source_size: SizeL,
  pub bitmap: Option<EmrBitmapBuffer>,
  pub padding: Vec<u8>,
}

impl EmrStretchBlt {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = PointL::read_from(&mut reader)?;
    let dest_size = SizeL::read_from(&mut reader)?;
    let raster_operation = reader.read_u32()?;
    let source = PointL::read_from(&mut reader)?;
    let xform_source = XForm::read_from(&mut reader)?;
    let background_color_source = ColorRef::read_from(&mut reader)?;
    let color_usage = reader.read_u32()?;
    let off_bmi = reader.read_u32()? as usize;
    let cb_bmi = reader.read_u32()? as usize;
    let off_bits = reader.read_u32()? as usize;
    let cb_bits = reader.read_u32()? as usize;
    let source_size = SizeL::read_from(&mut reader)?;
    let (bitmap, bitmap_end) =
      read_optional_bitmap_buffer(data, 100, off_bmi, cb_bmi, off_bits, cb_bits)?;
    let value = Self {
      bounds,
      dest,
      dest_size,
      raster_operation,
      source,
      xform_source,
      background_color_source,
      color_usage,
      source_size,
      bitmap,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_STRETCHBLT")?,
    };
    validate_dib_color_usage(value.color_usage, "EMR_STRETCHBLT ColorUsage")?;
    validate_optional_source_bitmap(
      value.raster_operation,
      value.bitmap.is_some(),
      "EMR_STRETCHBLT",
    )?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_dib_color_usage(self.color_usage, "EMR_STRETCHBLT ColorUsage")?;
    validate_optional_source_bitmap(
      self.raster_operation,
      self.bitmap.is_some(),
      "EMR_STRETCHBLT",
    )?;
    write_bit_blt_data(
      self.bounds,
      self.dest,
      self.dest_size,
      self.raster_operation.to_le_bytes(),
      self.source,
      self.xform_source,
      self.background_color_source,
      self.color_usage,
      Some(self.source_size),
      self.bitmap.as_ref(),
      &self.padding,
      "EMR_STRETCHBLT",
    )
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "emf")]
pub struct EmrRop4 {
  pub reserved: u16,
  pub background_rop3: u8,
  pub foreground_rop3: u8,
}

impl EmrRop4 {
  pub const fn background_rop3_code(&self) -> WmfTernaryRasterOperationCode {
    WmfTernaryRasterOperationCode::from_raw(self.background_rop3)
  }

  pub const fn foreground_rop3_code(&self) -> WmfTernaryRasterOperationCode {
    WmfTernaryRasterOperationCode::from_raw(self.foreground_rop3)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrMaskBlt {
  pub bounds: RectL,
  pub dest: PointL,
  pub dest_size: SizeL,
  pub raster_operation: EmrRop4,
  pub source: PointL,
  pub xform_source: XForm,
  pub background_color_source: ColorRef,
  pub source_color_usage: u32,
  pub source_bitmap: Option<EmrBitmapBuffer>,
  pub mask: PointL,
  pub mask_color_usage: u32,
  pub mask_bitmap: Option<EmrBitmapBuffer>,
  pub bitmap_order: EmrBitmapBufferOrder,
  pub padding: Vec<u8>,
}

impl EmrMaskBlt {
  pub fn source_color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.source_color_usage)
  }

  pub fn mask_color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.mask_color_usage)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = PointL::read_from(&mut reader)?;
    let dest_size = SizeL::read_from(&mut reader)?;
    let raster_operation = EmrRop4::read_from(&mut reader)?;
    let source = PointL::read_from(&mut reader)?;
    let xform_source = XForm::read_from(&mut reader)?;
    let background_color_source = ColorRef::read_from(&mut reader)?;
    let source_color_usage = reader.read_u32()?;
    let off_bmi_src = reader.read_u32()? as usize;
    let cb_bmi_src = reader.read_u32()? as usize;
    let off_bits_src = reader.read_u32()? as usize;
    let cb_bits_src = reader.read_u32()? as usize;
    let mask = PointL::read_from(&mut reader)?;
    let mask_color_usage = reader.read_u32()?;
    let off_bmi_mask = reader.read_u32()? as usize;
    let cb_bmi_mask = reader.read_u32()? as usize;
    let off_bits_mask = reader.read_u32()? as usize;
    let cb_bits_mask = reader.read_u32()? as usize;
    let (source_bitmap, mask_bitmap, bitmap_order, bitmap_end) = read_two_bitmap_buffers(
      data,
      120,
      (off_bmi_src, cb_bmi_src, off_bits_src, cb_bits_src),
      (off_bmi_mask, cb_bmi_mask, off_bits_mask, cb_bits_mask),
      "EMR_MASKBLT",
    )?;

    let value = Self {
      bounds,
      dest,
      dest_size,
      raster_operation,
      source,
      xform_source,
      background_color_source,
      source_color_usage,
      source_bitmap,
      mask,
      mask_color_usage,
      mask_bitmap,
      bitmap_order,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_MASKBLT")?,
    };
    validate_emr_mask_blt(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_mask_blt(self)?;
    let fixed = 120usize;
    let (source_layout, mask_layout) = match self.bitmap_order {
      EmrBitmapBufferOrder::SourceThenMask => layout_two_bitmap_buffers(
        fixed,
        self.source_bitmap.as_ref(),
        self.mask_bitmap.as_ref(),
      )?,
      EmrBitmapBufferOrder::MaskThenSource => {
        let (mask_layout, source_layout) = layout_two_bitmap_buffers(
          fixed,
          self.mask_bitmap.as_ref(),
          self.source_bitmap.as_ref(),
        )?;
        (source_layout, mask_layout)
      }
    };
    let data_end = source_layout
      .into_iter()
      .chain(mask_layout)
      .fold(fixed, |end, layout| end.max(layout.data_end));
    let capacity = data_end
      .checked_add(self.padding.len())
      .and_then(|size| size.checked_add(3))
      .ok_or_else(|| Error::invalid(0, "EMR_MASKBLT serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    self.bounds.write_to(&mut writer)?;
    self.dest.write_to(&mut writer)?;
    self.dest_size.write_to(&mut writer)?;
    self.raster_operation.write_to(&mut writer)?;
    self.source.write_to(&mut writer)?;
    self.xform_source.write_to(&mut writer)?;
    self.background_color_source.write_to(&mut writer)?;
    writer.write_u32(self.source_color_usage)?;
    write_bitmap_layout(
      &mut writer,
      source_layout,
      self.source_bitmap.as_ref(),
      "EMR_MASKBLT source",
    )?;
    self.mask.write_to(&mut writer)?;
    writer.write_u32(self.mask_color_usage)?;
    write_bitmap_layout(
      &mut writer,
      mask_layout,
      self.mask_bitmap.as_ref(),
      "EMR_MASKBLT mask",
    )?;
    match self.bitmap_order {
      EmrBitmapBufferOrder::SourceThenMask => write_two_bitmap_buffers(
        &mut writer,
        self.source_bitmap.as_ref(),
        self.mask_bitmap.as_ref(),
        &self.padding,
        "EMR_MASKBLT",
      )?,
      EmrBitmapBufferOrder::MaskThenSource => write_two_bitmap_buffers(
        &mut writer,
        self.mask_bitmap.as_ref(),
        self.source_bitmap.as_ref(),
        &self.padding,
        "EMR_MASKBLT",
      )?,
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrPlgBlt {
  pub bounds: RectL,
  pub dest: [PointL; 3],
  pub source: BitmapSourceBounds,
  pub xform_source: XForm,
  pub background_color_source: ColorRef,
  pub source_color_usage: u32,
  pub source_bitmap: Option<EmrBitmapBuffer>,
  pub mask: PointL,
  pub mask_color_usage: u32,
  pub mask_bitmap: Option<EmrBitmapBuffer>,
  pub bitmap_order: EmrBitmapBufferOrder,
  pub padding: Vec<u8>,
}

impl EmrPlgBlt {
  pub fn source_color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.source_color_usage)
  }

  pub fn mask_color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.mask_color_usage)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = [
      PointL::read_from(&mut reader)?,
      PointL::read_from(&mut reader)?,
      PointL::read_from(&mut reader)?,
    ];
    let source = BitmapSourceBounds::read_from(&mut reader)?;
    let xform_source = XForm::read_from(&mut reader)?;
    let background_color_source = ColorRef::read_from(&mut reader)?;
    let source_color_usage = reader.read_u32()?;
    let off_bmi_src = reader.read_u32()? as usize;
    let cb_bmi_src = reader.read_u32()? as usize;
    let off_bits_src = reader.read_u32()? as usize;
    let cb_bits_src = reader.read_u32()? as usize;
    let mask = PointL::read_from(&mut reader)?;
    let mask_color_usage = reader.read_u32()?;
    let off_bmi_mask = reader.read_u32()? as usize;
    let cb_bmi_mask = reader.read_u32()? as usize;
    let off_bits_mask = reader.read_u32()? as usize;
    let cb_bits_mask = reader.read_u32()? as usize;
    let (source_bitmap, mask_bitmap, bitmap_order, bitmap_end) = read_two_bitmap_buffers(
      data,
      132,
      (off_bmi_src, cb_bmi_src, off_bits_src, cb_bits_src),
      (off_bmi_mask, cb_bmi_mask, off_bits_mask, cb_bits_mask),
      "EMR_PLGBLT",
    )?;

    let value = Self {
      bounds,
      dest,
      source,
      xform_source,
      background_color_source,
      source_color_usage,
      source_bitmap,
      mask,
      mask_color_usage,
      mask_bitmap,
      bitmap_order,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_PLGBLT")?,
    };
    validate_emr_plg_blt(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_plg_blt(self)?;
    let fixed = 132usize;
    let (source_layout, mask_layout) = match self.bitmap_order {
      EmrBitmapBufferOrder::SourceThenMask => layout_two_bitmap_buffers(
        fixed,
        self.source_bitmap.as_ref(),
        self.mask_bitmap.as_ref(),
      )?,
      EmrBitmapBufferOrder::MaskThenSource => {
        let (mask_layout, source_layout) = layout_two_bitmap_buffers(
          fixed,
          self.mask_bitmap.as_ref(),
          self.source_bitmap.as_ref(),
        )?;
        (source_layout, mask_layout)
      }
    };
    let data_end = source_layout
      .into_iter()
      .chain(mask_layout)
      .fold(fixed, |end, layout| end.max(layout.data_end));
    let capacity = data_end
      .checked_add(self.padding.len())
      .and_then(|size| size.checked_add(3))
      .ok_or_else(|| Error::invalid(0, "EMR_PLGBLT serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    self.bounds.write_to(&mut writer)?;
    for point in self.dest {
      point.write_to(&mut writer)?;
    }
    self.source.write_to(&mut writer)?;
    self.xform_source.write_to(&mut writer)?;
    self.background_color_source.write_to(&mut writer)?;
    writer.write_u32(self.source_color_usage)?;
    write_bitmap_layout(
      &mut writer,
      source_layout,
      self.source_bitmap.as_ref(),
      "EMR_PLGBLT source",
    )?;
    self.mask.write_to(&mut writer)?;
    writer.write_u32(self.mask_color_usage)?;
    write_bitmap_layout(
      &mut writer,
      mask_layout,
      self.mask_bitmap.as_ref(),
      "EMR_PLGBLT mask",
    )?;
    match self.bitmap_order {
      EmrBitmapBufferOrder::SourceThenMask => write_two_bitmap_buffers(
        &mut writer,
        self.source_bitmap.as_ref(),
        self.mask_bitmap.as_ref(),
        &self.padding,
        "EMR_PLGBLT",
      )?,
      EmrBitmapBufferOrder::MaskThenSource => write_two_bitmap_buffers(
        &mut writer,
        self.mask_bitmap.as_ref(),
        self.source_bitmap.as_ref(),
        &self.padding,
        "EMR_PLGBLT",
      )?,
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "emf")]
pub struct EmrBlendFunction {
  pub blend_operation: u8,
  pub blend_flags: u8,
  pub source_constant_alpha: u8,
  pub alpha_format: u8,
}

impl EmrBlendFunction {
  pub fn blend_operation_kind(&self) -> Option<EmrBlendOperation> {
    EmrBlendOperation::from_raw(self.blend_operation)
  }

  pub fn alpha_format_kind(&self) -> Option<EmrAlphaFormat> {
    EmrAlphaFormat::from_raw(self.alpha_format)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrAlphaBlend {
  pub bounds: RectL,
  pub dest: PointL,
  pub dest_size: SizeL,
  pub blend_function: EmrBlendFunction,
  pub source: PointL,
  pub xform_source: XForm,
  pub background_color_source: ColorRef,
  pub color_usage: u32,
  pub source_size: SizeL,
  pub bitmap: Option<EmrBitmapBuffer>,
  pub padding: Vec<u8>,
}

impl EmrAlphaBlend {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = PointL::read_from(&mut reader)?;
    let dest_size = SizeL::read_from(&mut reader)?;
    let blend_function = EmrBlendFunction::read_from(&mut reader)?;
    let source = PointL::read_from(&mut reader)?;
    let xform_source = XForm::read_from(&mut reader)?;
    let background_color_source = ColorRef::read_from(&mut reader)?;
    let color_usage = reader.read_u32()?;
    let off_bmi = reader.read_u32()? as usize;
    let cb_bmi = reader.read_u32()? as usize;
    let off_bits = reader.read_u32()? as usize;
    let cb_bits = reader.read_u32()? as usize;
    let source_size = SizeL::read_from(&mut reader)?;
    let (bitmap, bitmap_end) =
      read_optional_bitmap_buffer(data, 100, off_bmi, cb_bmi, off_bits, cb_bits)?;
    let value = Self {
      bounds,
      dest,
      dest_size,
      blend_function,
      source,
      xform_source,
      background_color_source,
      color_usage,
      source_size,
      bitmap,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_ALPHABLEND")?,
    };
    validate_emr_alpha_blend(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_alpha_blend(self)?;
    write_bit_blt_data(
      self.bounds,
      self.dest,
      self.dest_size,
      [
        self.blend_function.blend_operation,
        self.blend_function.blend_flags,
        self.blend_function.source_constant_alpha,
        self.blend_function.alpha_format,
      ],
      self.source,
      self.xform_source,
      self.background_color_source,
      self.color_usage,
      Some(self.source_size),
      self.bitmap.as_ref(),
      &self.padding,
      "EMR_ALPHABLEND",
    )
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmrTransparentBlt {
  pub bounds: RectL,
  pub dest: PointL,
  pub dest_size: SizeL,
  pub transparent_color: ColorRef,
  pub source: PointL,
  pub xform_source: XForm,
  pub background_color_source: ColorRef,
  pub color_usage: u32,
  pub source_size: SizeL,
  pub bitmap: Option<EmrBitmapBuffer>,
  pub padding: Vec<u8>,
}

impl EmrTransparentBlt {
  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_raw(self.color_usage)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let bounds = RectL::read_from(&mut reader)?;
    let dest = PointL::read_from(&mut reader)?;
    let dest_size = SizeL::read_from(&mut reader)?;
    let transparent_color = ColorRef::read_from(&mut reader)?;
    let source = PointL::read_from(&mut reader)?;
    let xform_source = XForm::read_from(&mut reader)?;
    let background_color_source = ColorRef::read_from(&mut reader)?;
    let color_usage = reader.read_u32()?;
    let off_bmi = reader.read_u32()? as usize;
    let cb_bmi = reader.read_u32()? as usize;
    let off_bits = reader.read_u32()? as usize;
    let cb_bits = reader.read_u32()? as usize;
    let source_size = SizeL::read_from(&mut reader)?;
    let (bitmap, bitmap_end) =
      read_optional_bitmap_buffer(data, 100, off_bmi, cb_bmi, off_bits, cb_bits)?;
    let value = Self {
      bounds,
      dest,
      dest_size,
      transparent_color,
      source,
      xform_source,
      background_color_source,
      color_usage,
      source_size,
      bitmap,
      padding: read_bitmap_record_padding(data, bitmap_end, "EMR_TRANSPARENTBLT")?,
    };
    validate_emr_transparent_blt(&value)?;
    Ok(value)
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    validate_emr_transparent_blt(self)?;
    write_bit_blt_data(
      self.bounds,
      self.dest,
      self.dest_size,
      [
        self.transparent_color.red,
        self.transparent_color.green,
        self.transparent_color.blue,
        self.transparent_color.reserved,
      ],
      self.source,
      self.xform_source,
      self.background_color_source,
      self.color_usage,
      Some(self.source_size),
      self.bitmap.as_ref(),
      &self.padding,
      "EMR_TRANSPARENTBLT",
    )
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "emf")]
pub struct BitmapSourceBounds {
  pub x: i32,
  pub y: i32,
  pub width: i32,
  pub height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_icm_mode")]
pub struct EmrSetIcmMode {
  pub icm_mode: u32,
}

impl EmrSetIcmMode {
  pub fn icm_mode_kind(&self) -> Option<EmrIcmMode> {
    EmrIcmMode::from_raw(self.icm_mode)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_color_space")]
pub struct EmrSetColorSpace {
  pub color_space_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_delete_color_space")]
pub struct EmrDeleteColorSpace {
  pub color_space_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_emr_set_layout")]
pub struct EmrSetLayout {
  pub layout_mode: u32,
}

impl EmrSetLayout {
  pub fn layout_flags(&self) -> EmrLayoutModeFlags {
    EmrLayoutModeFlags::from_bits_retain(self.layout_mode)
  }

  pub const fn invalid_layout_bits(&self) -> u32 {
    self.layout_mode & !EmrLayoutModeFlags::all().bits()
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrSetTextJustification {
  pub break_extra: i32,
  pub break_count: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
pub struct EmrFormat {
  pub signature: u32,
  pub version: u32,
  pub size_data: u32,
  pub data_offset: u32,
}

impl EmrFormat {
  pub fn signature_kind(&self) -> Option<EmrFormatSignature> {
    EmrFormatSignature::from_raw(self.signature)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
pub struct EmrBitFix28_4 {
  pub raw: i32,
}

impl EmrBitFix28_4 {
  pub const fn int_value(&self) -> i32 {
    self.raw >> 4
  }

  pub const fn frac_value(&self) -> u8 {
    (self.raw & 0x0F) as u8
  }

  pub fn real_value(&self) -> f32 {
    self.int_value() as f32 + f32::from(self.frac_value()) / 16.0
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
pub struct EmrPoint28_4 {
  pub x: EmrBitFix28_4,
  pub y: EmrBitFix28_4,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrEpsData {
  pub size_data: u32,
  pub version: u32,
  pub points: [EmrPoint28_4; 3],
  pub postscript_data: Vec<u8>,
}

impl EmrEpsData {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 32 {
      return Err(Error::invalid(0, "EpsData is too small"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let size_data = reader.read_u32()?;
    let version = reader.read_u32()?;
    let points = [
      EmrPoint28_4::read_from(&mut reader)?,
      EmrPoint28_4::read_from(&mut reader)?,
      EmrPoint28_4::read_from(&mut reader)?,
    ];
    if size_data as usize != data.len() {
      return Err(Error::invalid(
        0,
        "EpsData SizeData does not match data length",
      ));
    }
    if version != 1 {
      return Err(Error::invalid(0, "EpsData Version must be 1"));
    }
    Ok(Self {
      size_data,
      version,
      points,
      postscript_data: data[32..].to_vec(),
    })
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    if self.size_data as usize != 32 + self.postscript_data.len() {
      return Err(Error::invalid(
        0,
        "EpsData SizeData does not match data length",
      ));
    }
    if self.version != 1 {
      return Err(Error::invalid(0, "EpsData Version must be 1"));
    }
    writer.write_u32(self.size_data)?;
    writer.write_u32(self.version)?;
    for point in &self.points {
      point.write_to(writer)?;
    }
    writer.write_all(&self.postscript_data)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(self.size_data as usize)));
    self.write_to(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCommentBeginGroup {
  pub rectangle: RectL,
  pub description_chars: u32,
  pub description: SdkString,
  pub padding: Vec<u8>,
}

impl EmrCommentBeginGroup {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let rectangle = RectL::read_from(&mut reader)?;
    let description_chars = reader.read_u32()?;
    let description_len = (description_chars as usize)
      .checked_mul(2)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_BEGINGROUP description overflows"))?;
    let position = reader.position()? as usize;
    let description_end = position
      .checked_add(description_len)
      .ok_or_else(|| Error::invalid(position as u64, "description end overflows"))?;
    if description_end > data.len() {
      return Err(Error::invalid(
        position as u64,
        "EMR_COMMENT_BEGINGROUP description is out of bounds",
      ));
    }
    let description = SdkString::raw(
      data[position..description_end].to_vec(),
      SdkEncoding::Utf16Le,
    );
    let value = Self {
      rectangle,
      description_chars,
      description,
      padding: data[description_end..].to_vec(),
    };
    validate_emr_comment_begin_group(&value)?;
    Ok(value)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_emr_comment_begin_group(self)?;
    self.rectangle.write_to(writer)?;
    writer.write_u32(self.description_chars)?;
    writer.write_all(&self.description.encoded_bytes()?)?;
    writer.write_all(&self.padding)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCommentMultiFormats {
  pub output_rect: RectL,
  pub formats: Vec<EmrFormat>,
  pub format_data: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrCommentMultiFormats {
  fn format_data_start_offset(&self) -> Result<u32> {
    let formats_size = self
      .formats
      .len()
      .checked_mul(16)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_MULTIFORMATS size overflows"))?;
    usize_to_u32(
      8 + 16 + 4 + formats_size,
      "EMR_COMMENT_MULTIFORMATS FormatData offset",
    )
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let output_rect = RectL::read_from(&mut reader)?;
    let format_count = reader.read_u32()? as usize;
    let format_bytes =
      checked_record_array_bytes(format_count, 16, "EMR_COMMENT_MULTIFORMATS formats")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      format_bytes,
      "EMR_COMMENT_MULTIFORMATS formats",
    )?;
    let mut formats = Vec::with_capacity(format_count);
    for _ in 0..format_count {
      formats.push(EmrFormat::read_from(&mut reader)?);
    }
    let format_data_len = formats.iter().try_fold(0usize, |sum, format| {
      sum
        .checked_add(format.size_data as usize)
        .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_MULTIFORMATS data size overflows"))
    })?;
    let position = reader.position()? as usize;
    let format_data_end = position
      .checked_add(format_data_len)
      .ok_or_else(|| Error::invalid(position as u64, "format data end overflows"))?;
    if format_data_end > data.len() {
      return Err(Error::invalid(
        position as u64,
        "EMR_COMMENT_MULTIFORMATS format data is out of bounds",
      ));
    }
    let value = Self {
      output_rect,
      formats,
      format_data: data[position..format_data_end].to_vec(),
      padding: data[format_data_end..].to_vec(),
    };
    validate_emr_comment_multi_formats(&value)?;
    Ok(value)
  }

  pub fn format_data_slice(&self, index: usize) -> Result<&[u8]> {
    let format = self
      .formats
      .get(index)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_MULTIFORMATS format index is invalid"))?;
    let format_data_start = self.format_data_start_offset()?;
    if format.data_offset < format_data_start {
      return Err(Error::invalid(
        0,
        "EMR_COMMENT_MULTIFORMATS offData points before FormatData",
      ));
    }
    let start = (format.data_offset - format_data_start) as usize;
    let end = start
      .checked_add(format.size_data as usize)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_MULTIFORMATS data range overflows"))?;
    self.format_data.get(start..end).ok_or_else(|| {
      Error::invalid(
        0,
        "EMR_COMMENT_MULTIFORMATS offData/SizeData is out of bounds",
      )
    })
  }

  pub fn eps_data(&self, index: usize) -> Result<Option<EmrEpsData>> {
    let format = self
      .formats
      .get(index)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_MULTIFORMATS format index is invalid"))?;
    if format.signature_kind() == Some(EmrFormatSignature::Eps) {
      Ok(Some(EmrEpsData::read_data(self.format_data_slice(index)?)?))
    } else {
      Ok(None)
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_emr_comment_multi_formats(self)?;
    self.output_rect.write_to(writer)?;
    writer.write_u32(usize_to_u32(
      self.formats.len(),
      "EMR_COMMENT_MULTIFORMATS format count",
    )?)?;
    for format in &self.formats {
      format.write_to(writer)?;
    }
    writer.write_all(&self.format_data)?;
    writer.write_all(&self.padding)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmrCommentWindowsMetafile {
  pub version: u16,
  pub reserved: u16,
  pub checksum: u32,
  pub flags: u32,
  pub metafile_size: u32,
  pub metafile: Vec<u8>,
  pub padding: Vec<u8>,
}

impl EmrCommentWindowsMetafile {
  pub fn version_kind(&self) -> Option<WmfMetafileVersion> {
    WmfMetafileVersion::from_raw(self.version)
  }

  pub fn metafile_len(&self) -> usize {
    self.metafile.len()
  }

  pub fn metafile_size_matches_data(&self) -> bool {
    self.metafile_size as usize == self.metafile.len()
  }

  pub fn has_padding(&self) -> bool {
    !self.padding.is_empty()
  }

  pub fn windows_metafile(&self) -> Result<WmfMetafile> {
    WmfMetafile::from_bytes(&self.metafile)
  }

  pub fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let version = reader.read_u16()?;
    let reserved = reader.read_u16()?;
    let checksum = reader.read_u32()?;
    let flags = reader.read_u32()?;
    let metafile_size = reader.read_u32()?;
    let position = reader.position()? as usize;
    let metafile_end = position
      .checked_add(metafile_size as usize)
      .ok_or_else(|| Error::invalid(position as u64, "WMF comment data end overflows"))?;
    if metafile_end > data.len() {
      return Err(Error::invalid(
        position as u64,
        "EMR_COMMENT_WINDOWS_METAFILE data is out of bounds",
      ));
    }
    let value = Self {
      version,
      reserved,
      checksum,
      flags,
      metafile_size,
      metafile: data[position..metafile_end].to_vec(),
      padding: data[metafile_end..].to_vec(),
    };
    validate_emr_comment_windows_metafile(&value)?;
    Ok(value)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_emr_comment_windows_metafile(self)?;
    writer.write_u16(self.version)?;
    writer.write_u16(self.reserved)?;
    writer.write_u32(self.checksum)?;
    writer.write_u32(self.flags)?;
    writer.write_u32(self.metafile_size)?;
    writer.write_all(&self.metafile)?;
    writer.write_all(&self.padding)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmrPublicComment {
  BeginGroup(EmrCommentBeginGroup),
  EndGroup,
  MultiFormats(EmrCommentMultiFormats),
  WindowsMetafile(EmrCommentWindowsMetafile),
  Unknown { identifier: u32, data: Vec<u8> },
}

impl EmrPublicComment {
  pub fn read_data(identifier: u32, data: &[u8]) -> Result<Self> {
    match EmrPublicCommentIdentifier::from_raw(identifier) {
      Some(EmrPublicCommentIdentifier::BeginGroup) => {
        Ok(Self::BeginGroup(EmrCommentBeginGroup::read_data(data)?))
      }
      Some(EmrPublicCommentIdentifier::EndGroup) => {
        if data.is_empty() {
          Ok(Self::EndGroup)
        } else {
          Err(Error::invalid(0, "EMR_COMMENT_ENDGROUP must not have data"))
        }
      }
      Some(EmrPublicCommentIdentifier::MultiFormats) => {
        Ok(Self::MultiFormats(EmrCommentMultiFormats::read_data(data)?))
      }
      Some(EmrPublicCommentIdentifier::WindowsMetafile) => Ok(Self::WindowsMetafile(
        EmrCommentWindowsMetafile::read_data(data)?,
      )),
      Some(EmrPublicCommentIdentifier::UnicodeString)
      | Some(EmrPublicCommentIdentifier::UnicodeEnd) => Err(Error::invalid(
        0,
        "EMR_COMMENT_UNICODE_STRING and EMR_COMMENT_UNICODE_END are reserved",
      )),
      _ => Ok(Self::Unknown {
        identifier,
        data: data.to_vec(),
      }),
    }
  }

  pub fn identifier(&self) -> u32 {
    match self {
      Self::BeginGroup(_) => EmrPublicCommentIdentifier::BeginGroup.raw(),
      Self::EndGroup => EmrPublicCommentIdentifier::EndGroup.raw(),
      Self::MultiFormats(_) => EmrPublicCommentIdentifier::MultiFormats.raw(),
      Self::WindowsMetafile(_) => EmrPublicCommentIdentifier::WindowsMetafile.raw(),
      Self::Unknown { identifier, .. } => *identifier,
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::BeginGroup(value) => value.write_to(writer),
      Self::EndGroup => Ok(()),
      Self::MultiFormats(value) => value.write_to(writer),
      Self::WindowsMetafile(value) => value.write_to(writer),
      Self::Unknown { identifier, data } => {
        validate_unknown_public_comment_identifier(*identifier, writer.position().unwrap_or(0))?;
        writer.write_all(data)
      }
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmrComment {
  EmfPlus {
    records: Vec<crate::emfplus::EmfPlusRecord>,
    emf_plus_trailing_data: Vec<u8>,
    alignment_padding: Vec<u8>,
  },
  EmfSpool {
    spool_identifier: u32,
    data: Vec<u8>,
    alignment_padding: Vec<u8>,
  },
  Public {
    comment: EmrPublicComment,
    alignment_padding: Vec<u8>,
  },
  PrivateData {
    data: Vec<u8>,
    alignment_padding: Vec<u8>,
  },
  Raw {
    data_size: u32,
    identifier: u32,
    data: Vec<u8>,
    alignment_padding: Vec<u8>,
  },
}

impl EmrComment {
  pub fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 4 {
      return Err(Error::invalid(0, "EMR_COMMENT data is too small"));
    }
    if !data.len().is_multiple_of(4) {
      return Err(Error::invalid(0, "EMR_COMMENT data must be 32-bit aligned"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let data_size = reader.read_u32()?;
    if data_size < 4 {
      let payload_end = 4usize
        .checked_add(data_size as usize)
        .ok_or_else(|| Error::invalid(0, "EMR_COMMENT payload range overflows"))?;
      let payload = data
        .get(4..payload_end)
        .ok_or_else(|| Error::invalid(0, "EMR_COMMENT payload is out of bounds"))?;
      let alignment_padding = data[payload_end..].to_vec();
      validate_emr_private_data(payload, &alignment_padding)?;
      return Ok(Self::PrivateData {
        data: payload.to_vec(),
        alignment_padding,
      });
    }
    let identifier = reader.read_u32()?;
    let payload_len = data_size as usize - 4;
    let payload_end = 8usize
      .checked_add(payload_len)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT payload range overflows"))?;
    let payload = data
      .get(8..payload_end)
      .ok_or_else(|| Error::invalid(0, "EMR_COMMENT payload is out of bounds"))?;
    let alignment_padding = data[payload_end..].to_vec();
    validate_emr_comment_alignment_padding(&alignment_padding)?;
    if identifier == EMR_COMMENT_EMFPLUS {
      let (records, emf_plus_trailing_data) = crate::emfplus::read_records_with_trailing(payload)?;
      validate_emr_comment_emf_plus(&records, &emf_plus_trailing_data, &alignment_padding)?;
      Ok(Self::EmfPlus {
        records,
        emf_plus_trailing_data,
        alignment_padding,
      })
    } else if identifier == EMR_COMMENT_EMFSPOOL {
      if payload.len() < 4 {
        return Err(Error::invalid(
          8,
          "EMR_COMMENT_EMFSPOOL payload is too small",
        ));
      }
      let mut reader = Reader::new(Cursor::new(payload));
      let spool_identifier = reader.read_u32()?;
      validate_emr_comment_emf_spool(spool_identifier, &payload[4..], &alignment_padding)?;
      Ok(Self::EmfSpool {
        spool_identifier,
        data: payload[4..].to_vec(),
        alignment_padding,
      })
    } else if identifier == EMR_COMMENT_PUBLIC {
      if payload.len() < 4 {
        return Err(Error::invalid(8, "EMR_COMMENT_PUBLIC payload is too small"));
      }
      let mut reader = Reader::new(Cursor::new(payload));
      let public_identifier = reader.read_u32()?;
      Ok(Self::Public {
        comment: EmrPublicComment::read_data(public_identifier, &payload[4..])?,
        alignment_padding,
      })
    } else {
      Ok(Self::Raw {
        data_size,
        identifier,
        data: payload.to_vec(),
        alignment_padding,
      })
    }
  }

  pub fn to_data(&self) -> Result<Vec<u8>> {
    match self {
      Self::EmfPlus {
        records,
        emf_plus_trailing_data,
        alignment_padding,
      } => {
        validate_emr_comment_emf_plus(records, emf_plus_trailing_data, alignment_padding)?;
        let payload_len = records
          .iter()
          .try_fold(0usize, |total, record| {
            let record_size = usize::try_from(record.sdk_size())
              .map_err(|_| Error::invalid(0, "EMF+ record size overflows usize"))?;
            total
              .checked_add(record_size)
              .ok_or_else(|| Error::invalid(0, "EMF+ comment payload size overflows"))
          })?
          .checked_add(emf_plus_trailing_data.len())
          .ok_or_else(|| Error::invalid(0, "EMF+ comment payload size overflows"))?;
        let capacity = 8usize
          .checked_add(payload_len)
          .and_then(|size| size.checked_add(alignment_padding.len()))
          .ok_or_else(|| Error::invalid(0, "EMF+ comment size overflows usize"))?;
        let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
        writer.write_u32(usize_to_u32(payload_len + 4, "EMR_COMMENT data size")?)?;
        writer.write_u32(EMR_COMMENT_EMFPLUS)?;
        for record in records {
          record.write_to(&mut writer)?;
        }
        writer.write_all(emf_plus_trailing_data)?;
        write_emr_comment_alignment_padding(&mut writer, alignment_padding)?;
        Ok(writer.into_inner().into_inner())
      }
      Self::EmfSpool {
        spool_identifier,
        data,
        alignment_padding,
      } => {
        validate_emr_comment_emf_spool(*spool_identifier, data, alignment_padding)?;
        let payload_len = 8usize
          .checked_add(data.len())
          .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_EMFSPOOL size overflows"))?;
        let mut writer = Writer::new(Cursor::new(Vec::with_capacity(payload_len + 4)));
        writer.write_u32(usize_to_u32(payload_len, "EMR_COMMENT_EMFSPOOL data size")?)?;
        writer.write_u32(EMR_COMMENT_EMFSPOOL)?;
        writer.write_u32(*spool_identifier)?;
        writer.write_all(data)?;
        write_emr_comment_alignment_padding(&mut writer, alignment_padding)?;
        Ok(writer.into_inner().into_inner())
      }
      Self::Public {
        comment,
        alignment_padding,
      } => {
        let mut payload = Writer::new(Cursor::new(Vec::new()));
        payload.write_u32(comment.identifier())?;
        comment.write_to(&mut payload)?;
        let payload = payload.into_inner().into_inner();
        let data_size = payload
          .len()
          .checked_add(4)
          .ok_or_else(|| Error::invalid(0, "EMR_COMMENT_PUBLIC size overflows"))?;
        let mut writer = Writer::new(Cursor::new(Vec::with_capacity(4 + data_size)));
        writer.write_u32(usize_to_u32(data_size, "EMR_COMMENT_PUBLIC data size")?)?;
        writer.write_u32(EMR_COMMENT_PUBLIC)?;
        writer.write_all(&payload)?;
        write_emr_comment_alignment_padding(&mut writer, alignment_padding)?;
        Ok(writer.into_inner().into_inner())
      }
      Self::PrivateData {
        data,
        alignment_padding,
      } => {
        validate_emr_private_data(data, alignment_padding)?;
        let mut writer = Writer::new(Cursor::new(Vec::with_capacity(4 + data.len())));
        writer.write_u32(usize_to_u32(data.len(), "EMR_COMMENT data size")?)?;
        writer.write_all(data)?;
        write_emr_comment_alignment_padding(&mut writer, alignment_padding)?;
        Ok(writer.into_inner().into_inner())
      }
      Self::Raw {
        data_size,
        identifier,
        data,
        alignment_padding,
      } => {
        validate_emr_raw_comment(*data_size, *identifier, data, alignment_padding)?;
        let mut writer = Writer::new(Cursor::new(Vec::with_capacity(8 + data.len())));
        writer.write_u32(*data_size)?;
        writer.write_u32(*identifier)?;
        writer.write_all(data)?;
        write_emr_comment_alignment_padding(&mut writer, alignment_padding)?;
        Ok(writer.into_inner().into_inner())
      }
    }
  }

  pub fn alignment_padding(&self) -> &[u8] {
    match self {
      Self::EmfPlus {
        alignment_padding, ..
      }
      | Self::EmfSpool {
        alignment_padding, ..
      }
      | Self::Public {
        alignment_padding, ..
      }
      | Self::PrivateData {
        alignment_padding, ..
      }
      | Self::Raw {
        alignment_padding, ..
      } => alignment_padding,
    }
  }

  pub fn validate_strict(&self) -> Result<()> {
    validate_emr_comment_alignment_padding_strict(self.alignment_padding())?;
    if let Self::EmfPlus {
      emf_plus_trailing_data,
      ..
    } = self
      && !emf_plus_trailing_data.is_empty()
    {
      return Err(Error::invalid(0, "EMR_COMMENT_EMFPLUS has trailing data"));
    }
    if let Self::Public {
      comment: EmrPublicComment::MultiFormats(value),
      ..
    } = self
    {
      validate_emr_comment_multi_formats_strict(value)?;
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmfHeader {
  pub bounds: RectL,
  pub frame: RectL,
  pub signature: u32,
  pub version: u32,
  pub bytes: u32,
  pub records: u32,
  pub handles: u16,
  pub reserved: u16,
  pub description_chars: u32,
  pub description_offset: u32,
  pub palette_entries: u32,
  pub device: SizeL,
  pub millimeters: SizeL,
  pub extension: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
pub struct EmfHeaderExtension1 {
  pub pixel_format_size: u32,
  pub pixel_format_offset: u32,
  pub opengl: u32,
}

impl EmfHeaderExtension1 {
  pub fn opengl_present(&self) -> Option<bool> {
    match self.opengl {
      0 => Some(false),
      1 => Some(true),
      _ => None,
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
pub struct EmfHeaderExtension2 {
  pub micrometers_x: u32,
  pub micrometers_y: u32,
}

impl EmfHeader {
  pub fn validate_strict(&self) -> Result<()> {
    validate_emf_header_strict(self)
  }

  pub fn version_kind(&self) -> Option<EmrMetafileVersion> {
    EmrMetafileVersion::from_raw(self.version)
  }

  pub fn from_record_data(data: &[u8]) -> Result<Self> {
    if data.len() < (EMF_HEADER_MIN_SIZE as usize - 8) {
      return Err(Error::invalid(8, "EMR_HEADER record data is too small"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let mut header = Self::read_from(&mut reader)?;
    let position = reader.position()? as usize;
    header.extension = data[position..].to_vec();
    validate_emf_header(&header)?;
    Ok(header)
  }

  pub fn to_record_data(&self) -> Result<Vec<u8>> {
    validate_emf_header(self)?;
    self.to_record_data_unchecked()
  }

  fn to_record_data_unchecked(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    self.write_to(&mut writer)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn fixed_header_record_size(&self) -> Result<usize> {
    let mut header_size = 8usize
      .checked_add(self.sdk_size() as usize)
      .ok_or_else(|| Error::invalid(0, "EMR_HEADER size overflows"))?;
    if self.description_chars != 0 && self.description_offset != 0 {
      header_size = header_size.min(self.description_offset as usize);
    }
    if self.extension.len() >= 12 && header_size >= 100 {
      let mut reader = Reader::new(Cursor::new(&self.extension[..12]));
      let extension1 = EmfHeaderExtension1::read_from(&mut reader)?;
      if extension1.pixel_format_size != 0 && extension1.pixel_format_offset != 0 {
        if extension1.pixel_format_offset < 100 {
          return Err(Error::invalid(
            0,
            "EMR_HEADER offPixelFormat points into fixed header",
          ));
        }
        header_size = header_size.min(extension1.pixel_format_offset as usize);
      }
    }
    Ok(header_size)
  }

  pub fn header_extension1(&self) -> Result<Option<EmfHeaderExtension1>> {
    if self.fixed_header_record_size()? < 100 || self.extension.len() < 12 {
      return Ok(None);
    }
    let mut reader = Reader::new(Cursor::new(&self.extension[..12]));
    Ok(Some(EmfHeaderExtension1::read_from(&mut reader)?))
  }

  pub fn header_extension2(&self) -> Result<Option<EmfHeaderExtension2>> {
    if self.fixed_header_record_size()? < 108 || self.extension.len() < 20 {
      return Ok(None);
    }
    let mut reader = Reader::new(Cursor::new(&self.extension[12..20]));
    Ok(Some(EmfHeaderExtension2::read_from(&mut reader)?))
  }

  pub fn bounds_width(&self) -> i64 {
    i64::from(self.bounds.right) - i64::from(self.bounds.left)
  }

  pub fn bounds_height(&self) -> i64 {
    i64::from(self.bounds.bottom) - i64::from(self.bounds.top)
  }

  pub fn frame_width_01mm(&self) -> i64 {
    i64::from(self.frame.right) - i64::from(self.frame.left)
  }

  pub fn frame_height_01mm(&self) -> i64 {
    i64::from(self.frame.bottom) - i64::from(self.frame.top)
  }

  pub fn frame_width_mm(&self) -> f64 {
    self.frame_width_01mm() as f64 / 100.0
  }

  pub fn frame_height_mm(&self) -> f64 {
    self.frame_height_01mm() as f64 / 100.0
  }

  pub fn device_size_pixels(&self) -> SizeL {
    self.device
  }

  pub fn device_size_millimeters(&self) -> SizeL {
    self.millimeters
  }

  pub fn device_size_micrometers(&self) -> Result<Option<(u32, u32)>> {
    Ok(
      self
        .header_extension2()?
        .map(|extension2| (extension2.micrometers_x, extension2.micrometers_y)),
    )
  }

  pub fn opengl_present(&self) -> Result<Option<bool>> {
    Ok(
      self
        .header_extension1()?
        .and_then(|extension1| extension1.opengl_present()),
    )
  }

  pub fn description(&self) -> Result<Option<SdkString>> {
    if self.description_chars == 0 || self.description_offset == 0 {
      return Ok(None);
    }
    let description_len = (self.description_chars as usize)
      .checked_mul(2)
      .ok_or_else(|| Error::invalid(0, "EMR_HEADER description length overflows"))?;
    let description_data = self.extension_range_from_record_offset(
      self.description_offset,
      description_len,
      "EMR_HEADER description",
    )?;
    Ok(Some(SdkString::raw(
      description_data.to_vec(),
      SdkEncoding::Utf16Le,
    )))
  }

  pub fn pixel_format_descriptor(&self) -> Result<Option<EmrPixelFormat>> {
    let Some(extension1) = self.header_extension1()? else {
      return Ok(None);
    };
    if extension1.pixel_format_size == 0 || extension1.pixel_format_offset == 0 {
      return Ok(None);
    }
    if extension1.pixel_format_size != u32::from(EmrPixelFormat::SIZE) {
      return Err(Error::invalid(
        0,
        "EMR_HEADER cbPixelFormat must be 40 when present",
      ));
    }
    let pixel_format_data = self.extension_range_from_record_offset(
      extension1.pixel_format_offset,
      extension1.pixel_format_size as usize,
      "EMR_HEADER PixelFormat",
    )?;
    let value = read_object(pixel_format_data)?;
    validate_emr_pixel_format(&value)?;
    Ok(Some(value))
  }

  fn extension_range_from_record_offset(
    &self,
    record_offset: u32,
    byte_len: usize,
    name: &str,
  ) -> Result<&[u8]> {
    let record_data_offset = record_offset
      .checked_sub(8)
      .ok_or_else(|| Error::invalid(0, format!("{name} offset points before data")))?;
    let extension_offset = (record_data_offset as usize)
      .checked_sub(EMF_HEADER_FIXED_DATA_SIZE)
      .ok_or_else(|| Error::invalid(0, format!("{name} offset points into fixed header")))?;
    let end = extension_offset
      .checked_add(byte_len)
      .ok_or_else(|| Error::invalid(0, format!("{name} range overflows")))?;
    self
      .extension
      .get(extension_offset..end)
      .ok_or_else(|| Error::invalid(0, format!("{name} range is out of bounds")))
  }
}

impl SdkRead for EmfHeader {
  fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self {
      bounds: RectL::read_from(reader)?,
      frame: RectL::read_from(reader)?,
      signature: reader.read_u32()?,
      version: reader.read_u32()?,
      bytes: reader.read_u32()?,
      records: reader.read_u32()?,
      handles: reader.read_u16()?,
      reserved: reader.read_u16()?,
      description_chars: reader.read_u32()?,
      description_offset: reader.read_u32()?,
      palette_entries: reader.read_u32()?,
      device: SizeL::read_from(reader)?,
      millimeters: SizeL::read_from(reader)?,
      extension: Vec::new(),
    })
  }
}

impl SdkWrite for EmfHeader {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.bounds.write_to(writer)?;
    self.frame.write_to(writer)?;
    writer.write_u32(self.signature)?;
    writer.write_u32(self.version)?;
    writer.write_u32(self.bytes)?;
    writer.write_u32(self.records)?;
    writer.write_u16(self.handles)?;
    writer.write_u16(self.reserved)?;
    writer.write_u32(self.description_chars)?;
    writer.write_u32(self.description_offset)?;
    writer.write_u32(self.palette_entries)?;
    self.device.write_to(writer)?;
    self.millimeters.write_to(writer)?;
    writer.write_all(&self.extension)
  }
}

impl SdkSize for EmfHeader {
  fn sdk_size(&self) -> u64 {
    80 + self.extension.len() as u64
  }
}

fn read_object<T: SdkRead>(data: &[u8]) -> Result<T> {
  let mut reader = Reader::new(Cursor::new(data));
  let value = T::read_from(&mut reader)?;
  ensure_reader_end(&mut reader, data.len() as u64, std::any::type_name::<T>())?;
  Ok(value)
}

fn object_record<T: SdkWrite + SdkSize>(
  record_type: EmfRecordType,
  value: &T,
) -> Result<EmfRecord> {
  let capacity = usize::try_from(value.sdk_size())
    .map_err(|_| Error::invalid(0, "EMF object size overflows usize"))?;
  let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
  value.write_to(&mut writer)?;
  Ok(EmfRecord::new(
    record_type.raw(),
    writer.into_inner().into_inner(),
  ))
}

fn no_data_record(record_type: EmfRecordType) -> EmfRecord {
  EmfRecord::new(record_type.raw(), Vec::new())
}

fn ensure_no_data(data: &[u8], record_name: &str) -> Result<()> {
  if data.is_empty() {
    Ok(())
  } else {
    Err(Error::invalid(
      8,
      format!("{record_name} record data must be empty"),
    ))
  }
}

fn ensure_reader_end<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  end: u64,
  name: &str,
) -> Result<()> {
  let position = reader.position()?;
  if position == end {
    Ok(())
  } else {
    Err(Error::invalid(
      position,
      format!("{name} record has trailing data"),
    ))
  }
}

fn checked_record_array_bytes(count: usize, element_size: usize, name: &str) -> Result<usize> {
  count
    .checked_mul(element_size)
    .ok_or_else(|| Error::invalid(0, format!("{name} size overflows usize")))
}

fn ensure_record_remaining<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  end: u64,
  required: usize,
  name: &str,
) -> Result<()> {
  let position = reader.position()?;
  if position
    .checked_add(required as u64)
    .is_some_and(|required_end| required_end <= end)
  {
    Ok(())
  } else {
    Err(Error::invalid(
      position,
      format!("{name} extends past record data"),
    ))
  }
}

fn read_remaining<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data: &[u8],
) -> Result<Vec<u8>> {
  let position = reader.position()? as usize;
  Ok(
    data
      .get(position..)
      .ok_or_else(|| Error::invalid(position as u64, "reader is past record data"))?
      .to_vec(),
  )
}

fn write_fixed_bytes<W: std::io::Write>(
  writer: &mut Writer<W>,
  bytes: &[u8],
  len: usize,
) -> Result<()> {
  if bytes.len() >= len {
    writer.write_all(&bytes[..len])
  } else {
    writer.write_all(bytes)?;
    writer.write_all(&vec![0; len - bytes.len()])
  }
}

fn record_relative_data_offset(offset: usize) -> Result<usize> {
  offset
    .checked_sub(8)
    .ok_or_else(|| Error::invalid(0, "record-relative offset points into record header"))
}

fn validate_record_relative_alignment(offset: usize, alignment: usize, name: &str) -> Result<()> {
  if offset.is_multiple_of(alignment) {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} must be {alignment}-byte aligned"),
    ))
  }
}

fn emr_text_requires_rectangle(options: ExtTextOutOptions) -> bool {
  options.intersects(ExtTextOutOptions::OPAQUE | ExtTextOutOptions::CLIPPED)
}

fn read_emr_text<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data: &[u8],
  wide: bool,
  record_name: &str,
) -> Result<(EmrText, EmrTextBufferRanges)> {
  let descriptor_start = reader.position()?;
  let descriptor = peek_emr_text_descriptor(reader)?;
  if descriptor.options.contains(ExtTextOutOptions::NO_RECT) {
    reader.seek(descriptor_start)?;
    match read_emr_text_with_rectangle(reader, data, wide, record_name, false) {
      Ok(value) => return Ok(value),
      Err(no_rectangle_error) => {
        reader.seek(descriptor_start)?;
        return match read_emr_text_with_rectangle(reader, data, wide, record_name, true) {
          Ok(value) => Ok(value),
          Err(_) => {
            reader.seek(descriptor_start)?;
            Err(no_rectangle_error)
          }
        };
      }
    }
  }
  if emr_text_requires_rectangle(descriptor.options) {
    reader.seek(descriptor_start)?;
    return read_emr_text_with_rectangle(reader, data, wide, record_name, true);
  }

  let string_start = record_relative_data_offset(descriptor.string_offset)?;
  if string_start >= descriptor_start as usize + 40 {
    reader.seek(descriptor_start)?;
    match read_emr_text_with_rectangle(reader, data, wide, record_name, true) {
      Ok(value) => return Ok(value),
      Err(with_rectangle_error) => {
        reader.seek(descriptor_start)?;
        match read_emr_text_with_rectangle(reader, data, wide, record_name, false) {
          Ok(value) => return Ok(value),
          Err(_) => {
            reader.seek(descriptor_start)?;
            return Err(with_rectangle_error);
          }
        }
      }
    }
  }

  reader.seek(descriptor_start)?;
  match read_emr_text_with_rectangle(reader, data, wide, record_name, false) {
    Ok(value) => Ok(value),
    Err(no_rectangle_error) => {
      reader.seek(descriptor_start)?;
      match read_emr_text_with_rectangle(reader, data, wide, record_name, true) {
        Ok(value) => Ok(value),
        Err(_) => {
          reader.seek(descriptor_start)?;
          Err(no_rectangle_error)
        }
      }
    }
  }
}

struct EmrTextDescriptorHeader {
  string_offset: usize,
  options: ExtTextOutOptions,
}

fn peek_emr_text_descriptor<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
) -> Result<EmrTextDescriptorHeader> {
  let descriptor_start = reader.position()?;
  reader.seek(descriptor_start + 12)?;
  let string_offset = reader.read_u32()? as usize;
  let options = ExtTextOutOptions::from_bits_retain(reader.read_u32()?);
  reader.seek(descriptor_start)?;
  Ok(EmrTextDescriptorHeader {
    string_offset,
    options,
  })
}

fn read_emr_text_with_rectangle<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  data: &[u8],
  wide: bool,
  record_name: &str,
  include_rectangle: bool,
) -> Result<(EmrText, EmrTextBufferRanges)> {
  let reference = PointL::read_from(reader)?;
  let chars = reader.read_u32()? as usize;
  let string_offset = reader.read_u32()? as usize;
  let options = ExtTextOutOptions::from_bits_retain(reader.read_u32()?);
  let rectangle = if include_rectangle {
    Some(RectL::read_from(reader)?)
  } else {
    None
  };
  let dx_offset = reader.read_u32()? as usize;

  let string_len = chars
    .checked_mul(if wide { 2 } else { 1 })
    .ok_or_else(|| Error::invalid(0, format!("{record_name} string length overflows")))?;
  validate_record_relative_alignment(
    string_offset,
    if wide { 2 } else { 1 },
    &format!("{record_name} offString"),
  )?;
  let string_start = record_relative_data_offset(string_offset)?;
  let string_end = string_start
    .checked_add(string_len)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} string range overflows")))?;
  let text = SdkString::raw(
    data
      .get(string_start..string_end)
      .ok_or_else(|| Error::invalid(0, format!("{record_name} string range is out of bounds")))?
      .to_vec(),
    if wide {
      SdkEncoding::Utf16Le
    } else {
      SdkEncoding::Windows1252
    },
  );
  let mut dx_range = None;

  let dx = if dx_offset == 0 {
    Vec::new()
  } else {
    let dx_count = chars
      .checked_mul(if options.contains(ExtTextOutOptions::PDY) {
        2
      } else {
        1
      })
      .ok_or_else(|| Error::invalid(0, format!("{record_name} dx count overflows")))?;
    validate_record_relative_alignment(dx_offset, 4, &format!("{record_name} offDx"))?;
    let dx_start = record_relative_data_offset(dx_offset)?;
    let dx_len = dx_count
      .checked_mul(4)
      .ok_or_else(|| Error::invalid(0, format!("{record_name} dx length overflows")))?;
    let dx_end = dx_start
      .checked_add(dx_len)
      .ok_or_else(|| Error::invalid(0, format!("{record_name} dx range overflows")))?;
    let mut dx_reader =
      Reader::new(Cursor::new(data.get(dx_start..dx_end).ok_or_else(
        || Error::invalid(0, format!("{record_name} dx range is out of bounds")),
      )?));
    let mut values = Vec::with_capacity(dx_count);
    for _ in 0..dx_count {
      values.push(dx_reader.read_u32()?);
    }
    dx_range = Some((dx_start, dx_end));
    values
  };

  Ok((
    EmrText {
      reference,
      options,
      rectangle,
      text,
      undefined_space_before_string: Vec::new(),
      dx_buffer_present: dx_offset != 0,
      undefined_space_before_dx: Vec::new(),
      dx,
    },
    EmrTextBufferRanges {
      string_start,
      string_end,
      dx_start: dx_range.map(|(start, _)| start),
      dx_end: dx_range.map(|(_, end)| end),
    },
  ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EmrTextBufferRanges {
  string_start: usize,
  string_end: usize,
  dx_start: Option<usize>,
  dx_end: Option<usize>,
}

impl EmrTextBufferRanges {
  fn consumed_end(self) -> usize {
    self.string_end.max(self.dx_end.unwrap_or(0))
  }
}

struct EmrTextLayout {
  text_bytes: Vec<u8>,
  char_count: usize,
  string_offset: usize,
  dx_offset: Option<usize>,
}

fn layout_emr_texts(
  texts: &[EmrText],
  wide: bool,
  first_descriptor_record_offset: usize,
) -> Result<Vec<EmrTextLayout>> {
  let mut current = first_descriptor_record_offset;
  let mut encoded = Vec::with_capacity(texts.len());
  for text in texts {
    current = current
      .checked_add(if text.rectangle.is_some() { 40 } else { 24 })
      .ok_or_else(|| Error::invalid(0, "EMR text descriptor range overflows"))?;
    let text_bytes = text.text.encoded_bytes()?;
    if wide && !text_bytes.len().is_multiple_of(2) {
      return Err(Error::invalid(0, "EMR UTF-16 text byte length is odd"));
    }
    let char_count = if wide {
      text_bytes.len() / 2
    } else {
      text_bytes.len()
    };
    if text.dx_buffer_present {
      let expected_dx = char_count
        .checked_mul(if text.options.contains(ExtTextOutOptions::PDY) {
          2
        } else {
          1
        })
        .ok_or_else(|| Error::invalid(0, "EMR text dx count overflows"))?;
      if text.dx.len() != expected_dx {
        return Err(Error::invalid(
          0,
          "EMR text dx count does not match character count",
        ));
      }
    }
    encoded.push((text_bytes.into_owned(), char_count));
  }

  let mut layouts = Vec::with_capacity(texts.len());
  for ((text_bytes, char_count), text) in encoded.into_iter().zip(texts) {
    if text.undefined_space_before_string.is_empty() && wide {
      current = align_to_u16(current);
    } else {
      current = current
        .checked_add(text.undefined_space_before_string.len())
        .ok_or_else(|| Error::invalid(0, "EMR text string offset overflows"))?;
    }
    validate_record_relative_alignment(current, if wide { 2 } else { 1 }, "EMR text offString")?;
    let string_offset = current;
    current = current
      .checked_add(text_bytes.len())
      .ok_or_else(|| Error::invalid(0, "EMR text string range overflows"))?;
    let dx_offset = if !text.dx_buffer_present {
      None
    } else {
      if text.undefined_space_before_dx.is_empty() {
        current = align_to_u32(current);
      } else {
        current = current
          .checked_add(text.undefined_space_before_dx.len())
          .ok_or_else(|| Error::invalid(0, "EMR text dx offset overflows"))?;
      }
      validate_record_relative_alignment(current, 4, "EMR text offDx")?;
      let dx_offset = current;
      current = current
        .checked_add(
          text
            .dx
            .len()
            .checked_mul(4)
            .ok_or_else(|| Error::invalid(0, "EMR text dx byte length overflows"))?,
        )
        .ok_or_else(|| Error::invalid(0, "EMR text dx range overflows"))?;
      Some(dx_offset)
    };
    layouts.push(EmrTextLayout {
      text_bytes,
      char_count,
      string_offset,
      dx_offset,
    });
  }

  Ok(layouts)
}

fn read_bitmap_buffer(
  data: &[u8],
  prefix_end: usize,
  off_bmi: usize,
  cb_bmi: usize,
  off_bits: usize,
  cb_bits: usize,
  record_name: &str,
) -> Result<(EmrBitmapBuffer, usize)> {
  let bmi_start = bitmap_range_start(off_bmi, cb_bmi, prefix_end)?;
  if bmi_start < prefix_end {
    return Err(Error::invalid(
      0,
      format!("{record_name} bitmap info overlaps preceding fields"),
    ));
  }
  let bmi_end = bmi_start
    .checked_add(cb_bmi)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap info range overflows")))?;
  let bits_start = bitmap_range_start(off_bits, cb_bits, bmi_end)?;
  if bits_start < bmi_end {
    return Err(Error::invalid(
      0,
      format!("{record_name} bitmap bits overlap or precede bitmap info"),
    ));
  }
  let bits_end = bits_start
    .checked_add(cb_bits)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap bits range overflows")))?;
  let undefined_space_before_bitmap_info = data
    .get(prefix_end..bmi_start)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap info offset is invalid")))?
    .to_vec();
  let bitmap_info = data
    .get(bmi_start..bmi_end)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap info range is invalid")))?
    .to_vec();
  let undefined_space_before_bitmap_bits = data
    .get(bmi_end..bits_start)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap bits offset is invalid")))?
    .to_vec();
  let bitmap_bits = data
    .get(bits_start..bits_end)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap bits range is invalid")))?
    .to_vec();
  Ok((
    EmrBitmapBuffer {
      undefined_space_before_bitmap_info,
      bitmap_info,
      undefined_space_before_bitmap_bits,
      bitmap_bits,
    },
    bits_end,
  ))
}

fn bitmap_range_start(offset: usize, size: usize, empty_fallback: usize) -> Result<usize> {
  if offset == 0 && size == 0 {
    Ok(empty_fallback)
  } else {
    record_relative_data_offset(offset)
  }
}

fn read_optional_bitmap_buffer(
  data: &[u8],
  prefix_end: usize,
  off_bmi: usize,
  cb_bmi: usize,
  off_bits: usize,
  cb_bits: usize,
) -> Result<(Option<EmrBitmapBuffer>, usize)> {
  if cb_bmi == 0 && cb_bits == 0 {
    return Ok((None, prefix_end));
  }
  let (bitmap, end) = read_bitmap_buffer(
    data,
    prefix_end,
    off_bmi,
    cb_bmi,
    off_bits,
    cb_bits,
    "EMF bitmap record",
  )?;
  Ok((Some(bitmap), end))
}

type BitmapBufferFields = (usize, usize, usize, usize);

fn bitmap_buffer_start(fields: BitmapBufferFields) -> Result<Option<usize>> {
  let (off_bmi, cb_bmi, off_bits, cb_bits) = fields;
  if cb_bmi != 0 {
    Ok(Some(record_relative_data_offset(off_bmi)?))
  } else if cb_bits != 0 {
    Ok(Some(record_relative_data_offset(off_bits)?))
  } else {
    Ok(None)
  }
}

fn read_two_bitmap_buffers(
  data: &[u8],
  prefix_end: usize,
  source_fields: BitmapBufferFields,
  mask_fields: BitmapBufferFields,
  record_name: &str,
) -> Result<(
  Option<EmrBitmapBuffer>,
  Option<EmrBitmapBuffer>,
  EmrBitmapBufferOrder,
  usize,
)> {
  let source_start = bitmap_buffer_start(source_fields)?;
  let mask_start = bitmap_buffer_start(mask_fields)?;
  let mask_first = matches!((source_start, mask_start), (Some(source), Some(mask)) if mask < source)
    || matches!((source_start, mask_start), (None, Some(_)));

  if mask_first {
    let (mask_bitmap, mask_end) = read_optional_bitmap_buffer(
      data,
      prefix_end,
      mask_fields.0,
      mask_fields.1,
      mask_fields.2,
      mask_fields.3,
    )?;
    let (source_bitmap, source_end) = read_optional_bitmap_buffer(
      data,
      mask_end,
      source_fields.0,
      source_fields.1,
      source_fields.2,
      source_fields.3,
    )?;
    Ok((
      source_bitmap,
      mask_bitmap,
      EmrBitmapBufferOrder::MaskThenSource,
      source_end,
    ))
  } else {
    let (source_bitmap, source_end) = read_optional_bitmap_buffer(
      data,
      prefix_end,
      source_fields.0,
      source_fields.1,
      source_fields.2,
      source_fields.3,
    )?;
    let (mask_bitmap, mask_end) = read_optional_bitmap_buffer(
      data,
      source_end,
      mask_fields.0,
      mask_fields.1,
      mask_fields.2,
      mask_fields.3,
    )?;
    if source_start == mask_start && source_start.is_some() {
      return Err(Error::invalid(
        0,
        format!("{record_name} source and mask bitmap buffers overlap"),
      ));
    }
    Ok((
      source_bitmap,
      mask_bitmap,
      EmrBitmapBufferOrder::SourceThenMask,
      mask_end,
    ))
  }
}

fn read_bitmap_record_padding(
  data: &[u8],
  unpadded_end: usize,
  record_name: &str,
) -> Result<Vec<u8>> {
  let padding = data
    .get(unpadded_end..)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap range is out of bounds")))?
    .to_vec();
  validate_emf_record_alignment_padding(&padding, unpadded_end, record_name)?;
  Ok(padding)
}

fn validate_optional_source_bitmap(
  raster_operation: u32,
  bitmap_present: bool,
  record_name: &str,
) -> Result<()> {
  if !bitmap_present && WmfTernaryRasterOperation::new(raster_operation).uses_source() {
    return Err(Error::invalid(
      0,
      format!("{record_name} source-dependent raster operation requires a source bitmap"),
    ));
  }
  Ok(())
}

#[derive(Clone, Copy)]
struct BitmapBufferLayout {
  off_bmi: usize,
  off_bits: usize,
  data_end: usize,
}

fn layout_bitmap_buffer(
  prefix_end: usize,
  bitmap: &EmrBitmapBuffer,
  record_name: &str,
) -> Result<BitmapBufferLayout> {
  let bmi_start = prefix_end
    .checked_add(bitmap.undefined_space_before_bitmap_info.len())
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap info offset overflows")))?;
  let off_bmi = bmi_start
    .checked_add(8)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap info offset overflows")))?;
  let bits_start = bmi_start
    .checked_add(bitmap.bitmap_info.len())
    .and_then(|value| value.checked_add(bitmap.undefined_space_before_bitmap_bits.len()))
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap bits offset overflows")))?;
  let off_bits = bits_start
    .checked_add(8)
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap bits offset overflows")))?;
  let data_end = bits_start
    .checked_add(bitmap.bitmap_bits.len())
    .ok_or_else(|| Error::invalid(0, format!("{record_name} bitmap range overflows")))?;
  Ok(BitmapBufferLayout {
    off_bmi,
    off_bits,
    data_end,
  })
}

fn layout_two_bitmap_buffers(
  fixed_size: usize,
  first: Option<&EmrBitmapBuffer>,
  second: Option<&EmrBitmapBuffer>,
) -> Result<(Option<BitmapBufferLayout>, Option<BitmapBufferLayout>)> {
  let first_layout = first
    .map(|bitmap| layout_bitmap_buffer(fixed_size, bitmap, "EMF source bitmap"))
    .transpose()?;
  let second_prefix = first_layout.map_or(fixed_size, |layout| layout.data_end);
  let second_layout = second
    .map(|bitmap| layout_bitmap_buffer(second_prefix, bitmap, "EMF mask bitmap"))
    .transpose()?;
  Ok((first_layout, second_layout))
}

fn write_bitmap_layout<W: std::io::Write>(
  writer: &mut Writer<W>,
  layout: Option<BitmapBufferLayout>,
  bitmap: Option<&EmrBitmapBuffer>,
  record_name: &str,
) -> Result<()> {
  writer.write_u32(usize_to_u32(
    layout.map_or(0, |layout| layout.off_bmi),
    format!("{record_name} bitmap info offset"),
  )?)?;
  writer.write_u32(usize_to_u32(
    bitmap.map_or(0, |bitmap| bitmap.bitmap_info.len()),
    format!("{record_name} bitmap info size"),
  )?)?;
  writer.write_u32(usize_to_u32(
    layout.map_or(0, |layout| layout.off_bits),
    format!("{record_name} bitmap bits offset"),
  )?)?;
  writer.write_u32(usize_to_u32(
    bitmap.map_or(0, |bitmap| bitmap.bitmap_bits.len()),
    format!("{record_name} bitmap bits size"),
  )?)?;
  Ok(())
}

fn write_two_bitmap_buffers<W: std::io::Write>(
  writer: &mut Writer<W>,
  first: Option<&EmrBitmapBuffer>,
  second: Option<&EmrBitmapBuffer>,
  padding: &[u8],
  record_name: &str,
) -> Result<()> {
  if let Some(bitmap) = first {
    write_bitmap_buffer(writer, bitmap)?;
  }
  if let Some(bitmap) = second {
    write_bitmap_buffer(writer, bitmap)?;
  }
  write_emf_record_alignment_padding(writer, padding, record_name)
}

fn write_bitmap_buffer<W: std::io::Write>(
  writer: &mut Writer<W>,
  bitmap: &EmrBitmapBuffer,
) -> Result<()> {
  writer.write_all(&bitmap.undefined_space_before_bitmap_info)?;
  writer.write_all(&bitmap.bitmap_info)?;
  writer.write_all(&bitmap.undefined_space_before_bitmap_bits)?;
  writer.write_all(&bitmap.bitmap_bits)
}

#[allow(clippy::too_many_arguments)]
fn write_bit_blt_data(
  bounds: RectL,
  dest: PointL,
  dest_size: SizeL,
  operation: [u8; 4],
  source: PointL,
  xform_source: XForm,
  background_color_source: ColorRef,
  color_usage: u32,
  source_size: Option<SizeL>,
  bitmap: Option<&EmrBitmapBuffer>,
  padding: &[u8],
  record_name: &str,
) -> Result<Vec<u8>> {
  let fixed = if source_size.is_some() {
    100usize
  } else {
    92usize
  };
  let layout = bitmap
    .map(|bitmap| layout_bitmap_buffer(fixed, bitmap, record_name))
    .transpose()?;
  let capacity = layout
    .map_or(fixed, |layout| layout.data_end)
    .checked_add(padding.len())
    .and_then(|size| size.checked_add(3))
    .ok_or_else(|| Error::invalid(0, format!("{record_name} serialized size overflows")))?;
  let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
  bounds.write_to(&mut writer)?;
  dest.write_to(&mut writer)?;
  dest_size.write_to(&mut writer)?;
  writer.write_all(&operation)?;
  source.write_to(&mut writer)?;
  xform_source.write_to(&mut writer)?;
  background_color_source.write_to(&mut writer)?;
  writer.write_u32(color_usage)?;
  writer.write_u32(usize_to_u32(
    layout.map_or(0, |layout| layout.off_bmi),
    format!("{record_name} bitmap info offset"),
  )?)?;
  writer.write_u32(usize_to_u32(
    bitmap.map_or(0, |bitmap| bitmap.bitmap_info.len()),
    "bitmap info size",
  )?)?;
  writer.write_u32(usize_to_u32(
    layout.map_or(0, |layout| layout.off_bits),
    format!("{record_name} bitmap bits offset"),
  )?)?;
  writer.write_u32(usize_to_u32(
    bitmap.map_or(0, |bitmap| bitmap.bitmap_bits.len()),
    "bitmap bits size",
  )?)?;
  if let Some(source_size) = source_size {
    source_size.write_to(&mut writer)?;
  }
  if let Some(bitmap) = bitmap {
    write_bitmap_buffer(&mut writer, bitmap)?;
  }
  write_emf_record_alignment_padding(&mut writer, padding, record_name)?;
  Ok(writer.into_inner().into_inner())
}

fn align_to_u32(value: usize) -> usize {
  (value + 3) & !3
}

fn align_to_u16(value: usize) -> usize {
  (value + 1) & !1
}

fn pad_writer_to_4<W: std::io::Write>(writer: &mut Writer<W>) -> Result<()> {
  let padding = (4 - (writer.position()? as usize % 4)) % 4;
  if padding != 0 {
    writer.write_all(&[0; 3][..padding])?;
  }
  Ok(())
}

fn pad_writer_to_record_offset<W: std::io::Write>(
  writer: &mut Writer<W>,
  record_offset: usize,
) -> Result<()> {
  let current_record_offset = writer
    .position()?
    .checked_add(8)
    .ok_or_else(|| Error::invalid(0, "writer record offset overflows"))?
    as usize;
  if current_record_offset > record_offset {
    return Err(Error::invalid(
      writer.position()?,
      "writer has passed requested record offset",
    ));
  }
  writer.write_all(&vec![0; record_offset - current_record_offset])
}

fn usize_to_u32(value: usize, context: impl std::fmt::Display) -> Result<u32> {
  u32::try_from(value).map_err(|_| Error::invalid(0, format!("{context} exceeds u32::MAX")))
}

pub fn looks_like_emf(bytes: &[u8]) -> bool {
  if bytes.len() < 44 {
    return false;
  }
  let record_type = u32::from_le_bytes(bytes[0..4].try_into().expect("slice length checked"));
  let header_size = u32::from_le_bytes(bytes[4..8].try_into().expect("slice length checked"));
  let signature = u32::from_le_bytes(bytes[40..44].try_into().expect("slice length checked"));
  record_type == EMR_HEADER && header_size >= EMF_HEADER_MIN_SIZE && signature == EMF_SIGNATURE
}

#[cfg(test)]
mod tests {
  use super::*;

  fn minimal_emf() -> Vec<u8> {
    let mut bytes = vec![0; 88];
    bytes[0..4].copy_from_slice(&EMR_HEADER.to_le_bytes());
    bytes[4..8].copy_from_slice(&88u32.to_le_bytes());
    bytes[40..44].copy_from_slice(&EMF_SIGNATURE.to_le_bytes());
    bytes[48..52].copy_from_slice(&108u32.to_le_bytes());
    bytes[52..56].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&EMR_EOF.to_le_bytes());
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes
  }

  fn identity_xform() -> XForm {
    XForm {
      m11: 1.0,
      m12: 0.0,
      m21: 0.0,
      m22: 1.0,
      dx: 0.0,
      dy: 0.0,
    }
  }

  fn point_type(value: u8) -> EmrPointTypeValue {
    EmrPointTypeValue::new(value).unwrap()
  }

  fn point_types(values: &[u8]) -> Vec<EmrPointTypeValue> {
    values.iter().copied().map(point_type).collect()
  }

  fn rgb_bitmap(width: i32, height: i32, bit_count: u16, bits: Vec<u8>) -> EmrBitmapBuffer {
    let mut bitmap_info = Vec::new();
    bitmap_info.extend_from_slice(&40u32.to_le_bytes());
    bitmap_info.extend_from_slice(&width.to_le_bytes());
    bitmap_info.extend_from_slice(&height.to_le_bytes());
    bitmap_info.extend_from_slice(&1u16.to_le_bytes());
    bitmap_info.extend_from_slice(&bit_count.to_le_bytes());
    bitmap_info.extend_from_slice(&0u32.to_le_bytes());
    bitmap_info.extend_from_slice(&0u32.to_le_bytes());
    bitmap_info.extend_from_slice(&0u32.to_le_bytes());
    bitmap_info.extend_from_slice(&0u32.to_le_bytes());
    bitmap_info.extend_from_slice(&0u32.to_le_bytes());
    bitmap_info.extend_from_slice(&0u32.to_le_bytes());
    EmrBitmapBuffer {
      undefined_space_before_bitmap_info: Vec::new(),
      bitmap_info,
      undefined_space_before_bitmap_bits: Vec::new(),
      bitmap_bits: bits,
    }
  }

  fn fixed_utf16_ascii(text: &[u8], chars: usize) -> SdkString {
    let mut bytes = vec![0; chars * 2];
    for (index, value) in text.iter().take(chars).enumerate() {
      bytes[index * 2] = *value;
    }
    SdkString::raw(bytes, SdkEncoding::Utf16Le)
  }

  fn test_log_font() -> LogFontW {
    LogFontW {
      height: -12,
      width: 0,
      escapement: 0,
      orientation: 0,
      weight: 700,
      italic: 1,
      underline: 0,
      strike_out: 0,
      char_set: WmfCharacterSet::Ansi.raw(),
      out_precision: WmfOutPrecision::TrueType.raw(),
      clip_precision: WmfClipPrecisionFlags::STROKE.bits(),
      quality: WmfFontQuality::ClearType.raw(),
      pitch_and_family: (WmfFamilyFont::Swiss.raw() << 4) | WmfPitchFont::Variable.raw(),
      face_name: fixed_utf16_ascii(b"Arial", LOGFONT_FACE_NAME_CHARS),
    }
  }

  #[test]
  fn emf_roundtrip_preserves_bytes() {
    let bytes = minimal_emf();
    let metafile = EmfMetafile::from_bytes(&bytes).unwrap();
    assert_eq!(metafile.records.len(), 2);
    assert_eq!(metafile.computed_bytes().unwrap(), 108);
    assert_eq!(metafile.computed_record_count().unwrap(), 2);
    assert!(metafile.validate_header_metrics().is_ok());
    assert_eq!(metafile.to_bytes().unwrap(), bytes);

    let mut object_header = EmfHeader::from_record_data(&metafile.records[0].data).unwrap();
    object_header.bytes = 136;
    object_header.records = 3;
    object_header.handles = 1;
    let create_pen = EmfRecordData::CreatePen(EmrCreatePen {
      object_index: 1,
      pen_style: EmrPenLineStyle::Solid.raw() | EmrPenType::Cosmetic.raw(),
      width: PointL { x: 1, y: 0 },
      color: ColorRef {
        red: 1,
        green: 2,
        blue: 3,
        reserved: 0,
      },
    })
    .to_record()
    .unwrap();
    assert_eq!(emf_record_size(&create_pen).unwrap(), 28);
    let object_metafile = EmfMetafile {
      records: vec![
        EmfRecordData::Header(object_header.clone())
          .to_record()
          .unwrap(),
        create_pen,
        metafile.records[1].clone(),
      ],
      trailing_data: Vec::new(),
    };
    assert!(object_metafile.validate_header_metrics().is_ok());
    let object_bytes = object_metafile.to_bytes().unwrap();
    assert_eq!(
      EmfMetafile::from_bytes(&object_bytes)
        .unwrap()
        .computed_record_count()
        .unwrap(),
      3
    );
    let mut invalid_handles = object_bytes.clone();
    invalid_handles[56..58].copy_from_slice(&0_u16.to_le_bytes());
    let invalid_handles_metafile = EmfMetafile::from_bytes(&invalid_handles).unwrap();
    assert!(invalid_handles_metafile.validate_header_metrics().is_err());
    object_header.handles = 0;
    let invalid_object_metafile = EmfMetafile {
      records: vec![
        EmfRecordData::Header(object_header).to_record().unwrap(),
        object_metafile.records[1].clone(),
        object_metafile.records[2].clone(),
      ],
      trailing_data: Vec::new(),
    };
    assert!(invalid_object_metafile.validate_header_metrics().is_err());
    assert!(invalid_object_metafile.to_bytes().is_ok());

    let mut selected_header =
      EmfHeader::from_record_data(&object_metafile.records[0].data).unwrap();
    selected_header.bytes = 148;
    selected_header.records = 4;
    let selected_object_metafile = EmfMetafile {
      records: vec![
        EmfRecordData::Header(selected_header).to_record().unwrap(),
        object_metafile.records[1].clone(),
        EmfRecordData::SelectObject(EmrSelectObject { object_index: 1 })
          .to_record()
          .unwrap(),
        object_metafile.records[2].clone(),
      ],
      trailing_data: Vec::new(),
    };
    assert!(selected_object_metafile.validate_header_metrics().is_ok());
    let mut invalid_selected_object_metafile = selected_object_metafile.clone();
    invalid_selected_object_metafile.records[2] =
      EmfRecordData::SelectObject(EmrSelectObject { object_index: 2 })
        .to_record()
        .unwrap();
    assert!(
      invalid_selected_object_metafile
        .validate_header_metrics()
        .is_err()
    );
    let mut stock_selected_object_metafile = selected_object_metafile.clone();
    stock_selected_object_metafile.records[2] = EmfRecordData::SelectObject(EmrSelectObject {
      object_index: EmrStockObject::DcPen.raw(),
    })
    .to_record()
    .unwrap();
    assert!(
      stock_selected_object_metafile
        .validate_header_metrics()
        .is_ok()
    );

    let begin_group = EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::BeginGroup(EmrCommentBeginGroup {
        rectangle: RectL::default(),
        description_chars: 1,
        description: SdkString::raw(vec![0, 0], SdkEncoding::Utf16Le),
        padding: Vec::new(),
      }),
      alignment_padding: vec![0, 0],
    })
    .to_record()
    .unwrap();
    let end_group = EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::EndGroup,
      alignment_padding: Vec::new(),
    })
    .to_record()
    .unwrap();
    let mut group_header = EmfHeader::from_record_data(&metafile.records[0].data).unwrap();
    group_header.bytes =
      88 + emf_record_size(&begin_group).unwrap() + emf_record_size(&end_group).unwrap() + 20;
    group_header.records = 4;
    let group_metafile = EmfMetafile {
      records: vec![
        EmfRecordData::Header(group_header.clone())
          .to_record()
          .unwrap(),
        begin_group.clone(),
        end_group.clone(),
        metafile.records[1].clone(),
      ],
      trailing_data: Vec::new(),
    };
    assert!(group_metafile.validate_header_metrics().is_ok());
    let mut unmatched_begin = group_metafile.clone();
    unmatched_begin.records.remove(2);
    let mut unmatched_begin_header = group_header.clone();
    unmatched_begin_header.bytes = 88 + emf_record_size(&begin_group).unwrap() + 20;
    unmatched_begin_header.records = 3;
    unmatched_begin.records[0] = EmfRecordData::Header(unmatched_begin_header)
      .to_record()
      .unwrap();
    assert!(unmatched_begin.validate_header_metrics().is_err());
    let mut unmatched_end = group_metafile.clone();
    unmatched_end.records.remove(1);
    let mut unmatched_end_header = group_header;
    unmatched_end_header.bytes = 88 + emf_record_size(&end_group).unwrap() + 20;
    unmatched_end_header.records = 3;
    unmatched_end.records[0] = EmfRecordData::Header(unmatched_end_header)
      .to_record()
      .unwrap();
    assert!(unmatched_end.validate_header_metrics().is_err());

    let assert_out_of_range_reference = |record_data: EmfRecordData<'_>| {
      let reference_record = record_data.to_record().unwrap();
      let mut reference_header = EmfHeader::from_record_data(&metafile.records[0].data).unwrap();
      reference_header.bytes = 88 + emf_record_size(&reference_record).unwrap() + 20;
      reference_header.records = 3;
      reference_header.handles = 1;
      let reference_metafile = EmfMetafile {
        records: vec![
          EmfRecordData::Header(reference_header).to_record().unwrap(),
          reference_record,
          metafile.records[1].clone(),
        ],
        trailing_data: Vec::new(),
      };
      assert!(reference_metafile.validate_header_metrics().is_err());
    };
    assert_out_of_range_reference(EmfRecordData::ResizePalette(EmrResizePalette {
      palette_index: 2,
      number_of_entries: 1,
    }));
    assert_out_of_range_reference(EmfRecordData::SetPaletteEntries(EmrSetPaletteEntries {
      palette_index: 2,
      start: 0,
      entries: vec![LogPaletteEntry {
        reserved: 0,
        blue: 1,
        green: 2,
        red: 3,
      }],
    }));
    assert_out_of_range_reference(EmfRecordData::ColorCorrectPalette(EmrColorCorrectPalette {
      palette_index: 2,
      first_entry: 0,
      palette_entries: 1,
      reserved: 0,
    }));
    assert_out_of_range_reference(EmfRecordData::SetColorSpace(EmrSetColorSpace {
      color_space_index: 2,
    }));
    assert_out_of_range_reference(EmfRecordData::DeleteColorSpace(EmrDeleteColorSpace {
      color_space_index: 2,
    }));

    let mut invalid_bytes = bytes.clone();
    invalid_bytes[48..52].copy_from_slice(&104u32.to_le_bytes());
    let invalid_metafile = EmfMetafile::from_bytes(&invalid_bytes).unwrap();
    assert!(invalid_metafile.validate_header_metrics().is_err());

    let mut invalid_records = bytes.clone();
    invalid_records[52..56].copy_from_slice(&1u32.to_le_bytes());
    let invalid_records_metafile = EmfMetafile::from_bytes(&invalid_records).unwrap();
    assert!(invalid_records_metafile.validate_header_metrics().is_err());

    let mut trailing = bytes.clone();
    trailing.extend_from_slice(&[0; 4]);
    assert_eq!(
      EmfMetafile::from_bytes(&trailing)
        .unwrap()
        .to_bytes()
        .unwrap(),
      trailing
    );

    let missing_eof = bytes[..88].to_vec();
    assert!(EmfMetafile::from_bytes(&missing_eof).is_err());
  }

  #[test]
  fn emf_borrowed_view_uses_input_storage_and_materializes_explicitly() {
    let bytes = minimal_emf();
    let view = EmfMetafileRef::from_bytes(&bytes).unwrap();
    assert_eq!(view.record_count(), 2);
    assert!(view.trailing_data().is_empty());
    assert_eq!(view.header().record_type, EMR_HEADER);

    let mut records = view.records();
    assert_eq!(records.len(), 2);
    let header = records.next().unwrap();
    assert_eq!(header.data.as_ptr(), bytes[8..].as_ptr());
    assert!(matches!(
      header.parse_data().unwrap(),
      EmfRecordData::Header(_)
    ));
    assert_eq!(header.rebuild_typed().unwrap().as_ref(), header);
    assert_eq!(records.len(), 1);

    let owned = view.into_owned();
    assert_eq!(owned.to_bytes().unwrap(), bytes);

    let mut invalid_late_record = bytes;
    invalid_late_record[92..96].copy_from_slice(&24u32.to_le_bytes());
    assert!(EmfMetafileRef::from_bytes(&invalid_late_record).is_err());
  }

  #[test]
  fn detects_emf_signature() {
    assert!(looks_like_emf(&minimal_emf()));
  }

  #[test]
  fn parses_typed_emf_header() {
    let metafile = EmfMetafile::from_bytes(&minimal_emf()).unwrap();
    let header = metafile.header().unwrap().as_header().unwrap().unwrap();
    assert_eq!(header.signature, EMF_SIGNATURE);
    assert_eq!(header.sdk_size(), 80);
    assert_eq!(header.fixed_header_record_size().unwrap(), 88);
    assert_eq!(header.bounds_width(), 0);
    assert_eq!(header.bounds_height(), 0);
    assert_eq!(header.frame_width_01mm(), 0);
    assert_eq!(header.frame_height_01mm(), 0);
    assert_eq!(header.description().unwrap(), None);
    assert_eq!(
      header.to_record_data().unwrap(),
      metafile.header().unwrap().data
    );
  }

  #[test]
  fn emf_header_description_accessor_keeps_original_header_layout() {
    let description_bytes = vec![
      b'L', 0, b'o', 0, b'n', 0, b'g', 0, b'N', 0, b'a', 0, b'm', 0, b'e', 0, 0, 0,
    ];
    let mut header = EmfHeader {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 1,
        bottom: 1,
      },
      frame: RectL {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
      },
      signature: EMF_SIGNATURE,
      version: 0x0001_0000,
      bytes: 0,
      records: 0,
      handles: 0,
      reserved: 0,
      description_chars: 9,
      description_offset: 88,
      palette_entries: 0,
      device: SizeL { cx: 1, cy: 1 },
      millimeters: SizeL { cx: 1, cy: 1 },
      extension: description_bytes.clone(),
    };

    assert_eq!(header.fixed_header_record_size().unwrap(), 88);
    assert_eq!(header.header_extension1().unwrap(), None);
    assert_eq!(header.bounds_width(), 1);
    assert_eq!(header.bounds_height(), 1);
    assert_eq!(header.frame_width_01mm(), 100);
    assert_eq!(header.frame_height_01mm(), 100);
    assert_eq!(header.frame_width_mm(), 1.0);
    assert_eq!(header.frame_height_mm(), 1.0);
    assert_eq!(header.version_kind(), Some(EmrMetafileVersion::Enhanced));
    assert_eq!(header.device_size_pixels(), SizeL { cx: 1, cy: 1 });
    assert_eq!(header.device_size_millimeters(), SizeL { cx: 1, cy: 1 });
    assert_eq!(header.device_size_micrometers().unwrap(), None);
    assert_eq!(header.opengl_present().unwrap(), None);
    assert_eq!(
      header
        .description()
        .unwrap()
        .unwrap()
        .encoded_bytes()
        .unwrap(),
      description_bytes
    );
    assert!(header.to_record_data().is_ok());

    header.signature = 0;
    assert!(header.to_record_data().is_err());
    header.signature = EMF_SIGNATURE;

    let last = header.extension.len() - 1;
    header.extension[last] = b'!';
    let compatible_bytes = header.to_record_data().unwrap();
    let reparsed = EmfHeader::from_record_data(&compatible_bytes).unwrap();
    assert_eq!(reparsed.to_record_data().unwrap(), compatible_bytes);
    assert!(reparsed.validate_strict().is_err());
    header.extension[last] = 0;

    header.description_offset = 999;
    assert!(header.to_record_data().is_err());
  }

  #[test]
  fn emf_header_extensions_expose_pixel_format_descriptor() {
    let pixel_format = EmrPixelFormat {
      n_size: 40,
      n_version: 1,
      flags: (EmrPixelFormatFlags::DRAW_TO_WINDOW | EmrPixelFormatFlags::SUPPORT_OPENGL).bits(),
      pixel_type: EmrPixelFormatType::Rgba.raw(),
      color_bits: 32,
      red_bits: 8,
      red_shift: 16,
      green_bits: 8,
      green_shift: 8,
      blue_bits: 8,
      blue_shift: 0,
      alpha_bits: 8,
      alpha_shift: 24,
      accum_bits: 0,
      accum_red_bits: 0,
      accum_green_bits: 0,
      accum_blue_bits: 0,
      accum_alpha_bits: 0,
      depth_bits: 24,
      stencil_bits: 8,
      aux_buffers: 0,
      layer_type: 0,
      reserved: 0,
      layer_mask: 0,
      visible_mask: 0,
      damage_mask: 0,
    };
    let mut pixel_format_bytes = Vec::new();
    pixel_format
      .write_to(&mut Writer::new(&mut pixel_format_bytes))
      .unwrap();

    let mut extension = Vec::new();
    extension.extend_from_slice(&40_u32.to_le_bytes());
    extension.extend_from_slice(&108_u32.to_le_bytes());
    extension.extend_from_slice(&1_u32.to_le_bytes());
    extension.extend_from_slice(&300_000_u32.to_le_bytes());
    extension.extend_from_slice(&200_000_u32.to_le_bytes());
    extension.extend_from_slice(&pixel_format_bytes);

    let mut header = EmfHeader {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 1,
        bottom: 1,
      },
      frame: RectL {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
      },
      signature: EMF_SIGNATURE,
      version: 0x0001_0000,
      bytes: 0,
      records: 0,
      handles: 0,
      reserved: 0,
      description_chars: 0,
      description_offset: 0,
      palette_entries: 0,
      device: SizeL { cx: 1, cy: 1 },
      millimeters: SizeL { cx: 1, cy: 1 },
      extension,
    };

    let extension1 = header.header_extension1().unwrap().unwrap();
    assert_eq!(extension1.pixel_format_size, 40);
    assert_eq!(extension1.pixel_format_offset, 108);
    assert_eq!(extension1.opengl_present(), Some(true));
    assert_eq!(header.opengl_present().unwrap(), Some(true));
    let extension2 = header.header_extension2().unwrap().unwrap();
    assert_eq!(extension2.micrometers_x, 300_000);
    assert_eq!(extension2.micrometers_y, 200_000);
    assert_eq!(
      header.device_size_micrometers().unwrap(),
      Some((300_000, 200_000))
    );
    assert_eq!(
      header.pixel_format_descriptor().unwrap(),
      Some(pixel_format.clone())
    );

    header.extension[0..4].copy_from_slice(&39_u32.to_le_bytes());
    assert!(header.pixel_format_descriptor().is_err());
    header.extension[0..4].copy_from_slice(&40_u32.to_le_bytes());

    header.extension[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(header.to_record_data().is_err());
    header.extension[0..4].copy_from_slice(&40_u32.to_le_bytes());
    header.extension[4..8].copy_from_slice(&0_u32.to_le_bytes());
    assert!(header.to_record_data().is_err());
    header.extension[4..8].copy_from_slice(&108_u32.to_le_bytes());

    header.extension[4..8].copy_from_slice(&4_u32.to_le_bytes());
    assert!(header.pixel_format_descriptor().is_err());
    header.extension[4..8].copy_from_slice(&108_u32.to_le_bytes());
    header.extension[8..12].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(
      header
        .header_extension1()
        .unwrap()
        .unwrap()
        .opengl_present(),
      None
    );
    assert!(header.to_record_data().is_err());
  }

  #[test]
  fn maps_emf_record_type_enum() {
    let record = EmfRecord::new(EMR_HEADER, Vec::new());
    assert_eq!(record.record_kind(), Some(EmfRecordType::Header));
    assert_eq!(EmfRecordType::ExtTextOutW.raw(), 0x0000_0054);
    assert_eq!(EmrDibColors::RgbColors.raw(), 0x0000);
    assert_eq!(EmrDibColors::PalColors.raw(), 0x0001);
    assert_eq!(EmrDibColors::PalIndices.raw(), 0x0002);
    assert_eq!(EmrArmStyle::BentDoubleSerif.raw(), 0x0B);
    assert_eq!(EmrContrast::VeryHigh.raw(), 0x09);
    assert_eq!(EmrFamilyType::Pictorial.raw(), 0x05);
    assert_eq!(EmrLetterform::ObliqueSquare.raw(), 0x0F);
    assert_eq!(EmrMidLine::LowSerifed.raw(), 0x0D);
    assert_eq!(EmrProportion::Monospaced.raw(), 0x09);
    assert_eq!(EmrSerifType::Rounded.raw(), 0x0F);
    assert_eq!(EmrStrokeVariation::InstantVertical.raw(), 0x08);
    assert_eq!(EmrWeight::Nord.raw(), 0x0B);
    assert_eq!(EmrXHeight::DuckingLarge.raw(), 0x07);
  }

  #[test]
  fn typed_bezier_records_validate_point_counts() {
    let bounds = RectL {
      left: 0,
      top: 0,
      right: 10,
      bottom: 10,
    };
    let bezier = EmfRecordData::PolyBezier(EmrPolyPointsL {
      bounds,
      points: vec![
        PointL { x: 0, y: 0 },
        PointL { x: 1, y: 2 },
        PointL { x: 3, y: 4 },
        PointL { x: 5, y: 6 },
      ],
    });
    let record = bezier.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), bezier);
    let mut trailing_point_record = record.clone();
    trailing_point_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_point_record.parse_data().is_err());

    let invalid_bezier = EmfRecordData::PolyBezier(EmrPolyPointsL {
      bounds,
      points: vec![
        PointL { x: 0, y: 0 },
        PointL { x: 1, y: 2 },
        PointL { x: 3, y: 4 },
      ],
    });
    assert!(invalid_bezier.to_record().is_err());
    assert!(
      EmfRecord::new(
        EmfRecordType::PolyBezier.raw(),
        match invalid_bezier {
          EmfRecordData::PolyBezier(value) => value.to_data().unwrap(),
          _ => unreachable!(),
        },
      )
      .parse_data()
      .is_err()
    );

    let bezier_to = EmfRecordData::PolyBezierTo(EmrPolyPointsL {
      bounds,
      points: vec![
        PointL { x: 1, y: 2 },
        PointL { x: 3, y: 4 },
        PointL { x: 5, y: 6 },
      ],
    });
    let record = bezier_to.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), bezier_to);

    let invalid_bezier_to = EmfRecordData::PolyBezierTo(EmrPolyPointsL {
      bounds,
      points: vec![PointL { x: 1, y: 2 }, PointL { x: 3, y: 4 }],
    });
    assert!(invalid_bezier_to.to_record().is_err());

    let bezier16 = EmfRecordData::PolyBezier16(EmrPolyPointsS {
      bounds,
      points: vec![
        PointS { x: 0, y: 0 },
        PointS { x: 1, y: 2 },
        PointS { x: 3, y: 4 },
        PointS { x: 5, y: 6 },
      ],
    });
    let record = bezier16.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), bezier16);
    let mut trailing_point16_record = record.clone();
    trailing_point16_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_point16_record.parse_data().is_err());

    let invalid_bezier_to16 = EmfRecordData::PolyBezierTo16(EmrPolyPointsS {
      bounds,
      points: vec![PointS { x: 1, y: 2 }, PointS { x: 3, y: 4 }],
    });
    assert!(invalid_bezier_to16.to_record().is_err());

    let oversized_point_record = EmfRecord::new(EmfRecordType::PolyBezier.raw(), {
      let mut data = Vec::new();
      data.extend_from_slice(&bounds.left.to_le_bytes());
      data.extend_from_slice(&bounds.top.to_le_bytes());
      data.extend_from_slice(&bounds.right.to_le_bytes());
      data.extend_from_slice(&bounds.bottom.to_le_bytes());
      data.extend_from_slice(&1_000_000_u32.to_le_bytes());
      data
    });
    assert!(oversized_point_record.parse_data().is_err());
  }

  #[test]
  fn derived_emf_object_roundtrips() {
    let value = EmrSetWindowOrgEx {
      origin: PointL { x: -10, y: 20 },
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    value.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    assert_eq!(bytes, [246, 255, 255, 255, 20, 0, 0, 0]);
    assert_eq!(value.sdk_size(), 8);

    let mut reader = Reader::new(Cursor::new(bytes));
    let parsed = EmrSetWindowOrgEx::read_from(&mut reader).unwrap();
    assert_eq!(parsed, value);
  }

  #[test]
  fn typed_polygon_record_roundtrips() {
    let value = EmfRecordData::Polygon(EmrPolyPointsL {
      bounds: RectL {
        left: 1,
        top: 2,
        right: 9,
        bottom: 10,
      },
      points: vec![PointL { x: 1, y: 2 }, PointL { x: 3, y: 4 }],
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::Polygon.raw());
    assert_eq!(record.parse_data().unwrap(), value);
  }

  #[test]
  fn typed_simple_state_records_roundtrip() {
    let values = [
      EmfRecordData::SetMapperFlags(EmrSetMapperFlags { flags: 1 }),
      EmfRecordData::SetMapMode(EmrSetMapMode { map_mode: 8 }),
      EmfRecordData::SetBkMode(EmrSetBkMode { background_mode: 2 }),
      EmfRecordData::SetRop2(EmrSetRop2 { rop2_mode: 13 }),
      EmfRecordData::SetArcDirection(EmrSetArcDirection { arc_direction: 2 }),
      EmfRecordData::SetIcmMode(EmrSetIcmMode { icm_mode: 1 }),
      EmfRecordData::SetColorSpace(EmrSetColorSpace {
        color_space_index: 2,
      }),
      EmfRecordData::DeleteColorSpace(EmrDeleteColorSpace {
        color_space_index: 2,
      }),
      EmfRecordData::SetLayout(EmrSetLayout {
        layout_mode: (EmrLayoutModeFlags::RTL | EmrLayoutModeFlags::BITMAP_ORIENTATION_PRESERVED)
          .bits(),
      }),
    ];

    for value in values {
      let record = value.to_record().unwrap();
      assert_eq!(record.data.len(), 4);
      assert_eq!(record.parse_data().unwrap(), value);
    }
    assert!(
      EmfRecord::new(
        EmfRecordType::SetMapMode.raw(),
        vec![8, 0, 0, 0, 0, 0, 0, 0]
      )
      .parse_data()
      .is_err()
    );
    assert!(
      EmfRecordData::SetMapperFlags(EmrSetMapperFlags { flags: 2 })
        .to_record()
        .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SetMapperFlags.raw(),
        2_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SetMapperFlags.raw(),
        [1_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat()
      )
      .parse_data()
      .is_err()
    );

    let text_color = EmfRecordData::SetTextColor(EmrSetTextColor {
      color: ColorRef {
        red: 1,
        green: 2,
        blue: 3,
        reserved: 0,
      },
    });
    let record = text_color.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetTextColor.raw());
    assert_eq!(record.parse_data().unwrap(), text_color);
    let mut invalid_text_color = record.clone();
    invalid_text_color.data[3] = 1;
    let parsed = invalid_text_color.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_text_color);
    let mut trailing_text_color = record;
    trailing_text_color
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_text_color.parse_data().is_err());

    let background_color = EmfRecordData::SetBkColor(EmrSetBkColor {
      color: ColorRef {
        red: 4,
        green: 5,
        blue: 6,
        reserved: 0,
      },
    });
    let record = background_color.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetBkColor.raw());
    assert_eq!(record.parse_data().unwrap(), background_color);
    let mut invalid_background_color = record;
    invalid_background_color.data[3] = 1;
    let parsed = invalid_background_color.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_background_color);

    let restore_dc = EmfRecordData::RestoreDc(EmrRestoreDc { saved_dc: -1 });
    let record = restore_dc.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::RestoreDc.raw());
    assert_eq!(record.parse_data().unwrap(), restore_dc);
    assert!(
      EmfRecordData::RestoreDc(EmrRestoreDc { saved_dc: 0 })
        .to_record()
        .is_err()
    );
    assert!(
      EmfRecord::new(EmfRecordType::RestoreDc.raw(), 1_i32.to_le_bytes().to_vec())
        .parse_data()
        .is_err()
    );

    let record = EmfRecordData::SetMapMode(EmrSetMapMode {
      map_mode: EmrMapMode::Anisotropic.raw(),
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetMapMode(value) = parsed else {
      panic!("expected EMR_SETMAPMODE");
    };
    assert_eq!(value.map_mode_kind(), Some(EmrMapMode::Anisotropic));

    let record = EmfRecordData::SetBkMode(EmrSetBkMode {
      background_mode: EmrBackgroundMode::Opaque.raw(),
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetBkMode(value) = parsed else {
      panic!("expected EMR_SETBKMODE");
    };
    assert_eq!(
      value.background_mode_kind(),
      Some(EmrBackgroundMode::Opaque)
    );

    let record = EmfRecordData::SetPolyFillMode(EmrSetPolyFillMode {
      polygon_fill_mode: EmrPolygonFillMode::Winding.raw(),
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetPolyFillMode(value) = parsed else {
      panic!("expected EMR_SETPOLYFILLMODE");
    };
    assert_eq!(
      value.polygon_fill_mode_kind(),
      Some(EmrPolygonFillMode::Winding)
    );

    let record = EmfRecordData::SetRop2(EmrSetRop2 {
      rop2_mode: WmfBinaryRasterOperation::CopyPen.raw() as u32,
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetRop2(value) = parsed else {
      panic!("expected EMR_SETROP2");
    };
    assert_eq!(
      value.binary_raster_operation_kind(),
      Some(WmfBinaryRasterOperation::CopyPen)
    );

    let record = EmfRecordData::SetStretchBltMode(EmrSetStretchBltMode {
      stretch_mode: EmrStretchMode::Halftone.raw(),
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetStretchBltMode(value) = parsed else {
      panic!("expected EMR_SETSTRETCHBLTMODE");
    };
    assert_eq!(value.stretch_mode_kind(), Some(EmrStretchMode::Halftone));
    assert!(
      EmfRecordData::SetMapMode(EmrSetMapMode {
        map_mode: 0xFFFF_FFFF,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::SetBkMode(EmrSetBkMode {
        background_mode: 0xFFFF_FFFF,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::SetPolyFillMode(EmrSetPolyFillMode {
        polygon_fill_mode: 0xFFFF_FFFF,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::SetRop2(EmrSetRop2 {
        rop2_mode: 0xFFFF_FFFF,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SetRop2.raw(),
        0xFFFF_FFFF_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );
    let record = EmfRecordData::SetStretchBltMode(EmrSetStretchBltMode {
      stretch_mode: 0xFFFF_FFFF,
    })
    .to_record()
    .unwrap();
    assert_eq!(record.data, 0xFFFF_FFFF_u32.to_le_bytes());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetStretchBltMode(value) = parsed else {
      panic!("expected EMR_SETSTRETCHBLTMODE");
    };
    assert_eq!(value.stretch_mode, 0xFFFF_FFFF);
    assert_eq!(value.stretch_mode_kind(), None);
    assert!(
      EmfRecordData::SetIcmMode(EmrSetIcmMode {
        icm_mode: 0xFFFF_FFFF,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SetIcmMode.raw(),
        0xFFFF_FFFF_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );
    let record = EmfRecordData::SetLayout(EmrSetLayout {
      layout_mode: (EmrLayoutModeFlags::RTL | EmrLayoutModeFlags::BITMAP_ORIENTATION_PRESERVED)
        .bits(),
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetLayout(value) = parsed else {
      panic!("expected EMR_SETLAYOUT");
    };
    assert!(value.layout_flags().contains(EmrLayoutModeFlags::RTL));
    assert!(
      value
        .layout_flags()
        .contains(EmrLayoutModeFlags::BITMAP_ORIENTATION_PRESERVED)
    );
    assert_eq!(value.invalid_layout_bits(), 0);
    assert!(
      EmfRecordData::SetLayout(EmrSetLayout { layout_mode: 2 })
        .to_record()
        .is_err()
    );
    assert!(
      EmfRecord::new(EmfRecordType::SetLayout.raw(), 2_u32.to_le_bytes().to_vec())
        .parse_data()
        .is_err()
    );
    assert!(
      EmfRecordData::SetColorSpace(EmrSetColorSpace {
        color_space_index: 0,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SetColorSpace.raw(),
        0_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );
    assert!(
      EmfRecordData::DeleteColorSpace(EmrDeleteColorSpace {
        color_space_index: 0,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::DeleteColorSpace.raw(),
        0_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );

    let record = EmfRecordData::SetTextAlign(EmrSetTextAlign {
      text_alignment_mode: (WmfTextAlignmentModeFlags::UPDATE_CP
        | WmfTextAlignmentModeFlags::BASELINE
        | WmfTextAlignmentModeFlags::RTL_READING)
        .bits() as u32,
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetTextAlign(value) = parsed else {
      panic!("expected EMR_SETTEXTALIGN");
    };
    assert!(
      value
        .text_alignment_flags()
        .contains(WmfTextAlignmentModeFlags::UPDATE_CP)
    );
    assert!(
      value
        .vertical_text_alignment_flags()
        .contains(WmfVerticalTextAlignmentModeFlags::BASELINE)
    );
    let invalid_alignment = EmfRecordData::SetTextAlign(EmrSetTextAlign {
      text_alignment_mode: 0x0004,
    });
    assert!(invalid_alignment.validate_strict().is_err());
    assert!(invalid_alignment.to_record().is_ok());
    let invalid_alignment_record = EmfRecord::new(
      EmfRecordType::SetTextAlign.raw(),
      0x0010_u32.to_le_bytes().to_vec(),
    );
    let parsed = invalid_alignment_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_alignment_record);
    let oversized_alignment = EmfRecordData::SetTextAlign(EmrSetTextAlign {
      text_alignment_mode: 0x0001_0000,
    });
    assert!(oversized_alignment.validate_strict().is_err());
    assert!(oversized_alignment.to_record().is_ok());

    let record = EmfRecordData::SetArcDirection(EmrSetArcDirection {
      arc_direction: EmrArcDirection::Clockwise.raw(),
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetArcDirection(value) = parsed else {
      panic!("expected EMR_SETARCDIRECTION");
    };
    assert_eq!(value.arc_direction_kind(), Some(EmrArcDirection::Clockwise));
    let mut trailing_set_arc_direction = record.clone();
    trailing_set_arc_direction
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_set_arc_direction.parse_data().is_err());
    assert!(
      EmfRecordData::SetArcDirection(EmrSetArcDirection {
        arc_direction: 0xFFFF_FFFF,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SetArcDirection.raw(),
        0xFFFF_FFFF_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );

    let record = EmfRecordData::SetIcmMode(EmrSetIcmMode {
      icm_mode: EmrIcmMode::On.raw(),
    })
    .to_record()
    .unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetIcmMode(value) = parsed else {
      panic!("expected EMR_SETICMMODE");
    };
    assert_eq!(value.icm_mode_kind(), Some(EmrIcmMode::On));

    let color_adjustment = EmfRecordData::SetColorAdjustment(EmrSetColorAdjustment {
      size: 24,
      values: EmrColorAdjustmentFlags::NEGATIVE.bits(),
      illuminant_index: EmrIlluminant::B.raw(),
      red_gamma: 10_000,
      green_gamma: 10_001,
      blue_gamma: 10_002,
      reference_black: 0,
      reference_white: 10_000,
      contrast: -1,
      brightness: 2,
      colorfulness: -3,
      red_green_tint: 4,
    });
    let record = color_adjustment.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetColorAdjustment.raw());
    assert_eq!(record.data.len(), 24);
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SetColorAdjustment(value) = &parsed else {
      panic!("expected EMR_SETCOLORADJUSTMENT");
    };
    assert!(
      value
        .color_adjustment_flags()
        .contains(EmrColorAdjustmentFlags::NEGATIVE)
    );
    assert_eq!(value.illuminant_kind(), Some(EmrIlluminant::B));
    assert!(value.red_gamma_value().is_no_correction());
    assert!(value.red_gamma_value().is_in_recommended_range());
    assert!((value.green_gamma_value().factor() - 1.0001).abs() < f32::EPSILON);
    assert!(value.blue_gamma_value().is_in_recommended_range());
    assert!(value.reference_black_in_recommended_range());
    assert!(value.reference_white_in_recommended_range());
    assert!(value.contrast_in_recommended_range());
    assert!(value.brightness_in_recommended_range());
    assert!(value.colorfulness_in_recommended_range());
    assert!(value.red_green_tint_in_recommended_range());
    assert_eq!(parsed, color_adjustment);
    let mut invalid_size = color_adjustment.clone();
    let EmfRecordData::SetColorAdjustment(value) = &mut invalid_size else {
      unreachable!();
    };
    value.size = 23;
    assert!(invalid_size.to_record().is_err());
    let mut invalid_size_record = record.clone();
    invalid_size_record.data[0..2].copy_from_slice(&23_u16.to_le_bytes());
    assert!(invalid_size_record.parse_data().is_err());

    let mut invalid_flags = color_adjustment.clone();
    let EmfRecordData::SetColorAdjustment(value) = &mut invalid_flags else {
      unreachable!();
    };
    value.values = 0x8000;
    assert!(invalid_flags.to_record().is_err());
    let mut invalid_flags_record = record.clone();
    invalid_flags_record.data[2..4].copy_from_slice(&0x8000_u16.to_le_bytes());
    assert!(invalid_flags_record.parse_data().is_err());

    let mut invalid_illuminant = color_adjustment.clone();
    let EmfRecordData::SetColorAdjustment(value) = &mut invalid_illuminant else {
      unreachable!();
    };
    value.illuminant_index = 0xFFFF;
    assert!(invalid_illuminant.to_record().is_err());
    let mut invalid_illuminant_record = record.clone();
    invalid_illuminant_record.data[4..6].copy_from_slice(&0xFFFF_u16.to_le_bytes());
    assert!(invalid_illuminant_record.parse_data().is_err());
  }

  #[test]
  fn typed_eof_record_preserves_palette_spacing() {
    let value = EmfRecordData::Eof(EmrEof {
      palette_entries_offset: 20,
      palette_prefix: vec![0xAA, 0xBB, 0xCC, 0xDD],
      palette_entries: vec![LogPaletteEntry {
        reserved: 1,
        blue: 2,
        green: 3,
        red: 4,
      }],
      palette_suffix: vec![0xEE, 0xFF, 0x11, 0x22],
      size_last: 32,
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EMR_EOF);
    assert_eq!(
      record.data,
      vec![
        1, 0, 0, 0, // nPalEntries
        20, 0, 0, 0, // offPalEntries
        0xAA, 0xBB, 0xCC, 0xDD, // UndefinedSpace1
        1, 2, 3, 4, // LogPaletteEntry
        0xEE, 0xFF, 0x11, 0x22, // UndefinedSpace2
        32, 0, 0, 0, // SizeLast
      ]
    );
    assert_eq!(record.parse_data().unwrap(), value);

    let invalid = EmfRecordData::Eof(EmrEof {
      palette_entries_offset: 20,
      palette_prefix: vec![0xAA, 0xBB, 0xCC, 0xDD],
      palette_entries: vec![LogPaletteEntry {
        reserved: 1,
        blue: 2,
        green: 3,
        red: 4,
      }],
      palette_suffix: vec![0xEE, 0xFF, 0x11, 0x22],
      size_last: 28,
    });
    assert!(invalid.validate_strict().is_err());
    assert!(invalid.to_record().is_ok());
    let mut invalid_record = record.clone();
    invalid_record.data[20..24].copy_from_slice(&28u32.to_le_bytes());
    let parsed = invalid_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_record);
    let mut fixed_field_overlap_record = record.clone();
    fixed_field_overlap_record.data[4..8].copy_from_slice(&12u32.to_le_bytes());
    assert!(fixed_field_overlap_record.parse_data().is_err());
    let mut palette_range_overflow_record = record.clone();
    palette_range_overflow_record.data[4..8].copy_from_slice(&28u32.to_le_bytes());
    assert!(palette_range_overflow_record.parse_data().is_err());

    let invalid_prefix = EmfRecordData::Eof(EmrEof {
      palette_entries_offset: 20,
      palette_prefix: vec![0; 5],
      palette_entries: vec![LogPaletteEntry {
        reserved: 1,
        blue: 2,
        green: 3,
        red: 4,
      }],
      palette_suffix: Vec::new(),
      size_last: 25,
    });
    assert!(invalid_prefix.to_record().is_err());

    let empty_palette = EmfRecordData::Eof(EmrEof {
      palette_entries_offset: 0xFFFF_FFFF,
      palette_prefix: vec![0xAA, 0xBB, 0xCC, 0xDD],
      palette_entries: Vec::new(),
      palette_suffix: Vec::new(),
      size_last: 24,
    });
    let record = empty_palette.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), empty_palette);
  }

  #[test]
  fn typed_palette_object_records_roundtrip() {
    let log_palette = LogPalette {
      version: 0x0300,
      entries: vec![LogPaletteEntry {
        reserved: 0x7F,
        blue: 1,
        green: 2,
        red: 3,
      }],
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    log_palette.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    let mut reader = Reader::new(Cursor::new(bytes.clone()));
    assert_eq!(LogPalette::read_from(&mut reader).unwrap(), log_palette);

    let invalid_log_palette = LogPalette {
      version: 0x0200,
      entries: Vec::new(),
    };
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    assert!(invalid_log_palette.write_to(&mut writer).is_err());
    let mut invalid_bytes = bytes;
    invalid_bytes[0..2].copy_from_slice(&0x0200_u16.to_le_bytes());
    let mut reader = Reader::new(Cursor::new(invalid_bytes));
    assert!(LogPalette::read_from(&mut reader).is_err());

    let select = EmfRecordData::SelectPalette(EmrSelectPalette { palette_index: 2 });
    let record = select.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SelectPalette.raw());
    assert_eq!(record.parse_data().unwrap(), select);

    let select_stock = EmfRecordData::SelectPalette(EmrSelectPalette {
      palette_index: EmrStockObject::DefaultPalette.raw(),
    });
    let record = select_stock.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SelectPalette(value) = &parsed else {
      panic!("expected EMR_SELECTPALETTE");
    };
    assert_eq!(
      value.stock_object_kind(),
      Some(EmrStockObject::DefaultPalette)
    );
    assert_eq!(parsed, select_stock);
    assert!(
      EmfRecordData::SelectPalette(EmrSelectPalette { palette_index: 0 })
        .to_record()
        .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SelectPalette.raw(),
        0_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );

    let resize = EmfRecordData::ResizePalette(EmrResizePalette {
      palette_index: 2,
      number_of_entries: 256,
    });
    let record = resize.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ResizePalette.raw());
    assert_eq!(record.parse_data().unwrap(), resize);
    assert!(
      EmfRecordData::ResizePalette(EmrResizePalette {
        palette_index: 2,
        number_of_entries: 0,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::ResizePalette(EmrResizePalette {
        palette_index: 2,
        number_of_entries: 1025,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::ResizePalette(EmrResizePalette {
        palette_index: 0,
        number_of_entries: 1,
      })
      .to_record()
      .is_err()
    );
    let invalid_resize_record = EmfRecord::new(
      EmfRecordType::ResizePalette.raw(),
      [2_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat(),
    );
    assert!(invalid_resize_record.parse_data().is_err());
    let invalid_resize_index_record = EmfRecord::new(
      EmfRecordType::ResizePalette.raw(),
      [0_u32.to_le_bytes(), 1_u32.to_le_bytes()].concat(),
    );
    assert!(invalid_resize_index_record.parse_data().is_err());
  }

  #[test]
  fn typed_object_and_transform_enum_accessors_map_spec_values() {
    let select_stock = EmfRecordData::SelectObject(EmrSelectObject {
      object_index: EmrStockObject::DcPen.raw(),
    });
    let record = select_stock.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SelectObject(value) = &parsed else {
      panic!("expected EMR_SELECTOBJECT");
    };
    assert_eq!(value.stock_object_kind(), Some(EmrStockObject::DcPen));
    assert_eq!(parsed, select_stock);
    assert!(
      EmfRecordData::SelectObject(EmrSelectObject { object_index: 0 })
        .to_record()
        .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::SelectObject.raw(),
        0_u32.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );

    let delete_object = EmfRecordData::DeleteObject(EmrDeleteObject { object_index: 1 });
    let record = delete_object.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::DeleteObject(value) = &parsed else {
      panic!("expected EMR_DELETEOBJECT");
    };
    assert_eq!(value.stock_object_kind(), None);
    assert_eq!(parsed, delete_object);

    let delete_zero = EmfRecordData::DeleteObject(EmrDeleteObject { object_index: 0 });
    assert!(delete_zero.to_record().is_err());
    let delete_stock = EmfRecordData::DeleteObject(EmrDeleteObject {
      object_index: EmrStockObject::DcPen.raw(),
    });
    assert!(delete_stock.to_record().is_err());
    let delete_zero_record = EmfRecord::new(
      EmfRecordType::DeleteObject.raw(),
      0_u32.to_le_bytes().to_vec(),
    );
    assert!(delete_zero_record.parse_data().is_err());
    let delete_stock_record = EmfRecord::new(
      EmfRecordType::DeleteObject.raw(),
      EmrStockObject::DcPen.raw().to_le_bytes().to_vec(),
    );
    assert!(delete_stock_record.parse_data().is_err());

    let transform = EmfRecordData::ModifyWorldTransform(EmrModifyWorldTransform {
      transform: XForm {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 2.0,
        dy: 3.0,
      },
      mode: EmrModifyWorldTransformMode::RightMultiply.raw(),
    });
    let record = transform.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ModifyWorldTransform(value) = &parsed else {
      panic!("expected EMR_MODIFYWORLDTRANSFORM");
    };
    assert_eq!(
      value.mode_kind(),
      Some(EmrModifyWorldTransformMode::RightMultiply)
    );
    assert_eq!(parsed, transform);
    let mut invalid_transform = transform.clone();
    let EmfRecordData::ModifyWorldTransform(value) = &mut invalid_transform else {
      unreachable!();
    };
    value.mode = 0xFFFF_FFFF;
    assert!(invalid_transform.to_record().is_err());
    let mut invalid_transform_record = record.clone();
    invalid_transform_record.data[24..28].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_transform_record.parse_data().is_err());

    let create_pen = EmfRecordData::CreatePen(EmrCreatePen {
      object_index: 2,
      pen_style: (EmrPenStyleFlags::DASH
        | EmrPenStyleFlags::END_CAP_SQUARE
        | EmrPenStyleFlags::JOIN_MITER)
        .bits(),
      width: PointL { x: 1, y: 0 },
      color: ColorRef {
        red: 1,
        green: 2,
        blue: 3,
        reserved: 0,
      },
    });
    let record = create_pen.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::CreatePen(value) = &parsed else {
      panic!("expected EMR_CREATEPEN");
    };
    assert!(value.pen_style_flags().contains(EmrPenStyleFlags::DASH));
    assert!(
      value
        .pen_style_flags()
        .contains(EmrPenStyleFlags::END_CAP_SQUARE)
    );
    assert!(
      value
        .pen_style_flags()
        .contains(EmrPenStyleFlags::JOIN_MITER)
    );
    assert_eq!(value.pen_line_style_kind(), Some(EmrPenLineStyle::Dash));
    assert_eq!(value.pen_end_cap_kind(), Some(EmrPenEndCap::Square));
    assert_eq!(value.pen_join_kind(), Some(EmrPenJoin::Miter));
    assert_eq!(value.pen_type_kind(), Some(EmrPenType::Cosmetic));
    assert_eq!(value.pen_reserved_bits(), 0);
    let log_pen = value.log_pen();
    assert_eq!(log_pen.pen_line_style_kind(), Some(EmrPenLineStyle::Dash));
    assert_eq!(log_pen.pen_type_kind(), Some(EmrPenType::Cosmetic));
    assert_eq!(parsed, create_pen);
    let mut zero_create_pen_index = create_pen.clone();
    let EmfRecordData::CreatePen(value) = &mut zero_create_pen_index else {
      unreachable!();
    };
    value.object_index = 0;
    assert!(zero_create_pen_index.to_record().is_err());
    let mut stock_create_pen_index = create_pen.clone();
    let EmfRecordData::CreatePen(value) = &mut stock_create_pen_index else {
      unreachable!();
    };
    value.object_index = EmrStockObject::DcPen.raw();
    assert!(stock_create_pen_index.to_record().is_err());
    let mut zero_create_pen_index_record = record.clone();
    zero_create_pen_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_create_pen_index_record.parse_data().is_err());
    let mut stock_create_pen_index_record = record.clone();
    stock_create_pen_index_record.data[0..4]
      .copy_from_slice(&EmrStockObject::DcPen.raw().to_le_bytes());
    assert!(stock_create_pen_index_record.parse_data().is_err());
    let mut invalid_create_pen_record = record.clone();
    invalid_create_pen_record.data[4..8].copy_from_slice(&0x0000_0010_u32.to_le_bytes());
    assert!(invalid_create_pen_record.parse_data().is_err());
    assert!(
      EmfRecordData::CreatePen(EmrCreatePen {
        object_index: 2,
        pen_style: 0x0000_000F,
        width: PointL { x: 1, y: 0 },
        color: ColorRef {
          red: 1,
          green: 2,
          blue: 3,
          reserved: 0,
        },
      })
      .to_record()
      .is_err()
    );
    let compatible_width_pen = EmfRecordData::CreatePen(EmrCreatePen {
      object_index: 2,
      pen_style: EmrPenStyleFlags::DASH.bits(),
      width: PointL { x: 2, y: 0 },
      color: ColorRef {
        red: 1,
        green: 2,
        blue: 3,
        reserved: 0,
      },
    });
    let compatible_width_record = compatible_width_pen.to_record().unwrap();
    assert_eq!(
      compatible_width_record.parse_data().unwrap(),
      compatible_width_pen
    );
    assert!(compatible_width_pen.validate_strict().is_err());

    let create_brush = EmfRecordData::CreateBrushIndirect(EmrCreateBrushIndirect {
      object_index: 3,
      brush_style: WmfBrushStyle::Hatched.raw() as u32,
      color: ColorRef {
        red: 10,
        green: 20,
        blue: 30,
        reserved: 0,
      },
      brush_hatch: EmrHatchStyle::SolidTextColor.raw(),
    });
    let record = create_brush.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::CreateBrushIndirect(value) = &parsed else {
      panic!("expected EMR_CREATEBRUSHINDIRECT");
    };
    assert_eq!(value.brush_style_kind(), Some(WmfBrushStyle::Hatched));
    assert_eq!(
      value.brush_hatch_kind(),
      Some(EmrHatchStyle::SolidTextColor)
    );
    let log_brush_ex = value.log_brush_ex();
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    log_brush_ex.write_to(&mut writer).unwrap();
    let bytes = writer.into_inner().into_inner();
    let mut reader = Reader::new(Cursor::new(bytes));
    assert_eq!(LogBrushEx::read_from(&mut reader).unwrap(), log_brush_ex);
    assert_eq!(parsed, create_brush);
    let mut zero_create_brush_index = create_brush.clone();
    let EmfRecordData::CreateBrushIndirect(value) = &mut zero_create_brush_index else {
      unreachable!();
    };
    value.object_index = 0;
    assert!(zero_create_brush_index.to_record().is_err());
    let mut zero_create_brush_index_record = record.clone();
    zero_create_brush_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_create_brush_index_record.parse_data().is_err());
    let mut invalid_create_brush_record = record.clone();
    invalid_create_brush_record.data[4..8].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_create_brush_record.parse_data().is_err());
    assert!(
      EmfRecordData::CreateBrushIndirect(EmrCreateBrushIndirect {
        object_index: 3,
        brush_style: WmfBrushStyle::DibPatternPt.raw() as u32,
        color: ColorRef {
          red: 10,
          green: 20,
          blue: 30,
          reserved: 0,
        },
        brush_hatch: 0,
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::CreateBrushIndirect(EmrCreateBrushIndirect {
        object_index: 3,
        brush_style: WmfBrushStyle::Hatched.raw() as u32,
        color: ColorRef {
          red: 10,
          green: 20,
          blue: 30,
          reserved: 0,
        },
        brush_hatch: 0xFFFF_FFFF,
      })
      .to_record()
      .is_err()
    );

    let ext_create_pen = EmfRecordData::ExtCreatePen(EmrExtCreatePen {
      object_index: 4,
      bitmap_info_offset: 0,
      bitmap_info_size: 0,
      bitmap_bits_offset: 0,
      bitmap_bits_size: 0,
      pen_style: (EmrPenStyleFlags::GEOMETRIC | EmrPenStyleFlags::USER_STYLE).bits(),
      width: 5,
      brush_style: WmfBrushStyle::Hatched.raw() as u32,
      color: ColorRef {
        red: 40,
        green: 50,
        blue: 60,
        reserved: 0,
      },
      brush_hatch: EmrHatchStyle::DitheredBackgroundColor.raw(),
      style_entries: vec![0x0403_0201],
      bitmap_buffer: Vec::new(),
    });
    let record = ext_create_pen.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ExtCreatePen(value) = &parsed else {
      panic!("expected EMR_EXTCREATEPEN");
    };
    assert!(
      value
        .pen_style_flags()
        .contains(EmrPenStyleFlags::GEOMETRIC)
    );
    assert_eq!(
      value.pen_line_style_kind(),
      Some(EmrPenLineStyle::UserStyle)
    );
    assert_eq!(value.pen_end_cap_kind(), Some(EmrPenEndCap::Round));
    assert_eq!(value.pen_join_kind(), Some(EmrPenJoin::Round));
    assert_eq!(value.pen_type_kind(), Some(EmrPenType::Geometric));
    assert_eq!(value.pen_reserved_bits(), 0);
    assert_eq!(value.brush_style_kind(), Some(WmfBrushStyle::Hatched));
    assert_eq!(
      value.brush_hatch_kind(),
      Some(EmrHatchStyle::DitheredBackgroundColor)
    );
    assert_eq!(value.log_pen_ex().style_entries, vec![0x0403_0201]);
    assert_eq!(value.bitmap().unwrap(), None);
    assert_eq!(parsed, ext_create_pen);
    let mut zero_ext_create_pen_index = ext_create_pen.clone();
    let EmfRecordData::ExtCreatePen(value) = &mut zero_ext_create_pen_index else {
      unreachable!();
    };
    value.object_index = 0;
    assert!(zero_ext_create_pen_index.to_record().is_err());
    let mut zero_ext_create_pen_index_record = record.clone();
    zero_ext_create_pen_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_ext_create_pen_index_record.parse_data().is_err());
    let mut invalid_ext_create_pen_record = record.clone();
    invalid_ext_create_pen_record.data[20..24].copy_from_slice(&0x0000_0010_u32.to_le_bytes());
    let parsed = invalid_ext_create_pen_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_ext_create_pen_record);
    let mut oversized_style_count_record = record.clone();
    oversized_style_count_record.data[40..44].copy_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(oversized_style_count_record.parse_data().is_err());
    let mut invalid_bitmap_range_record = record.clone();
    invalid_bitmap_range_record.data[12..16].copy_from_slice(&1_u32.to_le_bytes());
    invalid_bitmap_range_record.data[16..20].copy_from_slice(&1_u32.to_le_bytes());
    assert!(invalid_bitmap_range_record.parse_data().is_err());
    let bitmap_ext_create_pen = EmfRecordData::ExtCreatePen(EmrExtCreatePen {
      object_index: 4,
      bitmap_info_offset: 0,
      bitmap_info_size: 0,
      bitmap_bits_offset: 52,
      bitmap_bits_size: 2,
      pen_style: (EmrPenStyleFlags::GEOMETRIC | EmrPenStyleFlags::USER_STYLE).bits(),
      width: 5,
      brush_style: WmfBrushStyle::Hatched.raw() as u32,
      color: ColorRef {
        red: 40,
        green: 50,
        blue: 60,
        reserved: 0,
      },
      brush_hatch: EmrHatchStyle::DitheredBackgroundColor.raw(),
      style_entries: Vec::new(),
      bitmap_buffer: vec![0xAA, 0xBB],
    });
    let EmfRecordData::ExtCreatePen(value) = &bitmap_ext_create_pen else {
      unreachable!();
    };
    assert_eq!(
      value.bitmap().unwrap(),
      Some(EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info: Vec::new(),
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits: vec![0xAA, 0xBB],
      })
    );
    assert_eq!(
      bitmap_ext_create_pen
        .to_record()
        .unwrap()
        .parse_data()
        .unwrap(),
      bitmap_ext_create_pen
    );
    assert!(
      EmfRecordData::ExtCreatePen(EmrExtCreatePen {
        object_index: 4,
        bitmap_info_offset: 0,
        bitmap_info_size: 0,
        bitmap_bits_offset: 0,
        bitmap_bits_size: 0,
        pen_style: EmrPenStyleFlags::DASH.bits(),
        width: 2,
        brush_style: WmfBrushStyle::Solid.raw() as u32,
        color: ColorRef {
          red: 1,
          green: 2,
          blue: 3,
          reserved: 0,
        },
        brush_hatch: 0,
        style_entries: Vec::new(),
        bitmap_buffer: Vec::new(),
      })
      .validate_strict()
      .is_err()
    );
    assert!(
      EmfRecordData::ExtCreatePen(EmrExtCreatePen {
        object_index: 4,
        bitmap_info_offset: 0,
        bitmap_info_size: 0,
        bitmap_bits_offset: 0,
        bitmap_bits_size: 0,
        pen_style: EmrPenStyleFlags::GEOMETRIC.bits(),
        width: 2,
        brush_style: WmfBrushStyle::DibPatternPt.raw() as u32,
        color: ColorRef {
          red: 1,
          green: 2,
          blue: 3,
          reserved: 0,
        },
        brush_hatch: 0,
        style_entries: Vec::new(),
        bitmap_buffer: Vec::new(),
      })
      .validate_strict()
      .is_err()
    );
    assert!(
      EmfRecordData::ExtCreatePen(EmrExtCreatePen {
        object_index: 4,
        bitmap_info_offset: 0,
        bitmap_info_size: 0,
        bitmap_bits_offset: 0,
        bitmap_bits_size: 0,
        pen_style: EmrPenStyleFlags::GEOMETRIC.bits(),
        width: 2,
        brush_style: WmfBrushStyle::Hatched.raw() as u32,
        color: ColorRef {
          red: 1,
          green: 2,
          blue: 3,
          reserved: 0,
        },
        brush_hatch: 0xFFFF_FFFF,
        style_entries: Vec::new(),
        bitmap_buffer: Vec::new(),
      })
      .validate_strict()
      .is_err()
    );
    let cosmetic_hatched_pen = EmfRecordData::ExtCreatePen(EmrExtCreatePen {
      object_index: 4,
      bitmap_info_offset: 0,
      bitmap_info_size: 0,
      bitmap_bits_offset: 0,
      bitmap_bits_size: 0,
      pen_style: EmrPenStyleFlags::USER_STYLE.bits(),
      width: 1,
      brush_style: WmfBrushStyle::Hatched.raw() as u32,
      color: ColorRef {
        red: 1,
        green: 2,
        blue: 3,
        reserved: 0,
      },
      brush_hatch: EmrHatchStyle::SolidBackgroundColor.raw(),
      style_entries: vec![1, 2],
      bitmap_buffer: Vec::new(),
    });
    assert!(cosmetic_hatched_pen.to_record().is_ok());
    assert!(
      EmfRecordData::ExtCreatePen(EmrExtCreatePen {
        object_index: 4,
        bitmap_info_offset: 0,
        bitmap_info_size: 0,
        bitmap_bits_offset: 0,
        bitmap_bits_size: 0,
        pen_style: EmrPenStyleFlags::USER_STYLE.bits(),
        width: 1,
        brush_style: WmfBrushStyle::Hatched.raw() as u32,
        color: ColorRef {
          red: 1,
          green: 2,
          blue: 3,
          reserved: 0,
        },
        brush_hatch: EmrHatchStyle::DitheredBackgroundColor.raw(),
        style_entries: vec![1, 2],
        bitmap_buffer: Vec::new(),
      })
      .validate_strict()
      .is_err()
    );
  }

  #[test]
  fn typed_ext_create_font_indirect_w_variants_roundtrip() {
    let log_font = test_log_font();
    let basic = EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
      object_index: 7,
      font: EmrExtCreateFont::LogFont(log_font.clone()),
    });
    let record = basic.to_record().unwrap();
    assert_eq!(
      record.record_type,
      EmfRecordType::ExtCreateFontIndirectW.raw()
    );
    assert_eq!(record.data.len(), 4 + LogFontW::SIZE);
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ExtCreateFontIndirectW(value) = &parsed else {
      panic!("expected EMR_EXTCREATEFONTINDIRECTW");
    };
    let parsed_log_font = value.log_font().unwrap();
    assert_eq!(parsed_log_font.char_set_kind(), Some(WmfCharacterSet::Ansi));
    assert_eq!(
      parsed_log_font.out_precision_kind(),
      Some(WmfOutPrecision::TrueType)
    );
    assert!(
      parsed_log_font
        .clip_precision_flags()
        .contains(WmfClipPrecisionFlags::STROKE)
    );
    assert_eq!(
      parsed_log_font.quality_kind(),
      Some(WmfFontQuality::ClearType)
    );
    assert_eq!(parsed_log_font.pitch_kind(), Some(WmfPitchFont::Variable));
    assert_eq!(parsed_log_font.family_kind(), Some(WmfFamilyFont::Swiss));
    assert_eq!(parsed, basic);
    let mut zero_font_index = basic.clone();
    let EmfRecordData::ExtCreateFontIndirectW(value) = &mut zero_font_index else {
      unreachable!();
    };
    value.object_index = 0;
    assert!(zero_font_index.to_record().is_err());
    let mut zero_font_index_record = record.clone();
    zero_font_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_font_index_record.parse_data().is_err());
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.weight = 1001;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.italic = 2;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.underline = 2;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.strike_out = 2;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.char_set = 0x03;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.out_precision = 0x02;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.clip_precision = 0x08;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.quality = 0x06;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.pitch_and_family = 0x04;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.pitch_and_family = 0x03;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let mut invalid_log_font = log_font.clone();
    invalid_log_font.pitch_and_family = (0x06 << 4) | WmfPitchFont::Variable.raw();
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 7,
        font: EmrExtCreateFont::LogFont(invalid_log_font),
      })
      .validate_strict()
      .is_err()
    );
    let weight_offset = 4 + 16;
    let italic_offset = 4 + 20;
    let underline_offset = italic_offset + 1;
    let strike_out_offset = italic_offset + 2;
    let char_set_offset = italic_offset + 3;
    let out_precision_offset = italic_offset + 4;
    let clip_precision_offset = italic_offset + 5;
    let quality_offset = italic_offset + 6;
    let pitch_and_family_offset = italic_offset + 7;
    let mut invalid_record = record.clone();
    invalid_record.data[weight_offset..weight_offset + 4].copy_from_slice(&1001_i32.to_le_bytes());
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[italic_offset] = 2;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[underline_offset] = 2;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[strike_out_offset] = 2;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[char_set_offset] = 0x03;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[out_precision_offset] = 0x02;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[clip_precision_offset] = 0x08;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[quality_offset] = 0x06;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[pitch_and_family_offset] = 0x04;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[pitch_and_family_offset] = 0x03;
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );
    let mut invalid_record = record.clone();
    invalid_record.data[pitch_and_family_offset] = (0x06 << 4) | WmfPitchFont::Variable.raw();
    assert!(
      invalid_record
        .parse_data()
        .unwrap()
        .validate_strict()
        .is_err()
    );

    let panose = LogFontPanose {
      log_font: log_font.clone(),
      full_name: fixed_utf16_ascii(b"Arial Bold", LOGFONT_EX_FULL_NAME_CHARS),
      style: fixed_utf16_ascii(b"Bold", LOGFONT_EX_STYLE_CHARS),
      version: 1,
      style_size: 12,
      match_value: 0x0102_0304,
      reserved: 0,
      vendor_id: 0x4142_4344,
      culture: 0,
      panose: Panose {
        family_type: EmrPanoseFamilyType::TextDisplay.raw(),
        serif_style: EmrPanoseSerifType::NormalSans.raw(),
        weight: EmrPanoseWeight::Bold.raw(),
        proportion: EmrPanoseProportion::Modern.raw(),
        contrast: EmrPanoseContrast::Medium.raw(),
        stroke_variation: EmrPanoseStrokeVariation::GradualVertical.raw(),
        arm_style: EmrPanoseArmStyle::StraightHorizontal.raw(),
        letterform: EmrPanoseLetterform::NormalRounded.raw(),
        midline: EmrPanoseMidLine::StandardTrimmed.raw(),
        x_height: EmrPanoseXHeight::ConstantStandard.raw(),
      },
      padding: [0, 0],
    };
    let panose_record = EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
      object_index: 8,
      font: EmrExtCreateFont::LogFontPanose(panose.clone()),
    });
    let record = panose_record.to_record().unwrap();
    assert_eq!(record.data.len(), 4 + LOGFONT_PANOSE_SIZE);
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ExtCreateFontIndirectW(value) = &parsed else {
      panic!("expected EMR_EXTCREATEFONTINDIRECTW");
    };
    let EmrExtCreateFont::LogFontPanose(value) = &value.font else {
      panic!("expected LogFontPanose");
    };
    assert_eq!(
      value.panose.family_type_kind(),
      Some(EmrPanoseFamilyType::TextDisplay)
    );
    assert_eq!(
      value.panose.serif_style_kind(),
      Some(EmrPanoseSerifType::NormalSans)
    );
    assert_eq!(value.panose.weight_kind(), Some(EmrPanoseWeight::Bold));
    assert_eq!(
      value.panose.proportion_kind(),
      Some(EmrPanoseProportion::Modern)
    );
    assert_eq!(
      value.panose.contrast_kind(),
      Some(EmrPanoseContrast::Medium)
    );
    assert_eq!(
      value.panose.stroke_variation_kind(),
      Some(EmrPanoseStrokeVariation::GradualVertical)
    );
    assert_eq!(
      value.panose.arm_style_kind(),
      Some(EmrPanoseArmStyle::StraightHorizontal)
    );
    assert_eq!(
      value.panose.letterform_kind(),
      Some(EmrPanoseLetterform::NormalRounded)
    );
    assert_eq!(
      value.panose.midline_kind(),
      Some(EmrPanoseMidLine::StandardTrimmed)
    );
    assert_eq!(
      value.panose.x_height_kind(),
      Some(EmrPanoseXHeight::ConstantStandard)
    );
    assert_eq!(parsed, panose_record);
    let mut invalid_panose = panose.clone();
    invalid_panose.reserved = 1;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 8,
        font: EmrExtCreateFont::LogFontPanose(invalid_panose),
      })
      .to_record()
      .is_err()
    );
    let mut invalid_panose = panose.clone();
    invalid_panose.culture = 1;
    assert!(
      EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
        object_index: 8,
        font: EmrExtCreateFont::LogFontPanose(invalid_panose),
      })
      .to_record()
      .is_err()
    );
    let reserved_offset =
      4 + LogFontW::SIZE + LOGFONT_EX_FULL_NAME_CHARS * 2 + LOGFONT_EX_STYLE_CHARS * 2 + 12;
    let culture_offset = reserved_offset + 8;
    let mut invalid_record = record.clone();
    invalid_record.data[reserved_offset..reserved_offset + 4].copy_from_slice(&1_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[culture_offset..culture_offset + 4].copy_from_slice(&1_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());

    let font_ex_dv = LogFontExDv {
      log_font_ex: LogFontEx {
        log_font,
        full_name: fixed_utf16_ascii(b"Arial Variable", LOGFONT_EX_FULL_NAME_CHARS),
        style: fixed_utf16_ascii(b"Regular", LOGFONT_EX_STYLE_CHARS),
        script: fixed_utf16_ascii(b"Western", LOGFONT_EX_SCRIPT_CHARS),
      },
      design_vector: DesignVector {
        signature: DESIGN_VECTOR_SIGNATURE,
        values: vec![100, 200],
      },
    };
    let ex_dv_record = EmfRecordData::ExtCreateFontIndirectW(EmrExtCreateFontIndirectW {
      object_index: 9,
      font: EmrExtCreateFont::LogFontExDv(font_ex_dv.clone()),
    });
    let record = ex_dv_record.to_record().unwrap();
    assert_eq!(
      record.data.len(),
      4 + LOGFONT_EX_SIZE + font_ex_dv.design_vector.sdk_size() as usize
    );
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ExtCreateFontIndirectW(value) = &parsed else {
      panic!("expected EMR_EXTCREATEFONTINDIRECTW");
    };
    let EmrExtCreateFont::LogFontExDv(value) = &value.font else {
      panic!("expected LogFontExDv");
    };
    assert!(value.design_vector.is_ms_signature());
    assert_eq!(value.design_vector.values, [100, 200]);
    assert_eq!(parsed, ex_dv_record);

    let mut invalid_design_vector = ex_dv_record.clone();
    let EmfRecordData::ExtCreateFontIndirectW(value) = &mut invalid_design_vector else {
      unreachable!();
    };
    let EmrExtCreateFont::LogFontExDv(value) = &mut value.font else {
      unreachable!();
    };
    value.design_vector.signature = 0xFFFF_FFFF;
    assert!(invalid_design_vector.to_record().is_err());

    let mut invalid_record = record.clone();
    let design_vector_offset = 4 + LOGFONT_EX_SIZE;
    invalid_record.data[design_vector_offset..design_vector_offset + 4]
      .copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());
  }

  #[test]
  fn typed_palette_entry_records_roundtrip() {
    let entries = vec![
      LogPaletteEntry {
        reserved: 0,
        blue: 10,
        green: 20,
        red: 30,
      },
      LogPaletteEntry {
        reserved: 1,
        blue: 40,
        green: 50,
        red: 60,
      },
    ];

    let create = EmfRecordData::CreatePalette(EmrCreatePalette {
      palette_index: 3,
      log_palette: LogPalette {
        version: 0x0300,
        entries: entries.clone(),
      },
    });
    let record = create.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::CreatePalette.raw());
    assert_eq!(record.parse_data().unwrap(), create);
    let mut zero_create_palette_index = create.clone();
    let EmfRecordData::CreatePalette(value) = &mut zero_create_palette_index else {
      unreachable!();
    };
    value.palette_index = 0;
    assert!(zero_create_palette_index.to_record().is_err());
    let mut zero_create_palette_index_record = record.clone();
    zero_create_palette_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_create_palette_index_record.parse_data().is_err());
    let mut invalid_create_palette_version = record.clone();
    invalid_create_palette_version.data[4..6].copy_from_slice(&0x0200_u16.to_le_bytes());
    assert!(invalid_create_palette_version.parse_data().is_err());
    let mut invalid_create_palette_empty = record.clone();
    invalid_create_palette_empty.data[6..8].copy_from_slice(&0_u16.to_le_bytes());
    invalid_create_palette_empty.data.truncate(8);
    assert!(invalid_create_palette_empty.parse_data().is_err());
    let mut oversized_create_palette = record.clone();
    oversized_create_palette.data[6..8].copy_from_slice(&100_u16.to_le_bytes());
    oversized_create_palette.data.truncate(8);
    assert!(oversized_create_palette.parse_data().is_err());
    let mut invalid_create_palette_tail = record.clone();
    invalid_create_palette_tail.data.push(0xEE);
    assert!(invalid_create_palette_tail.parse_data().is_err());
    let invalid_create = EmfRecordData::CreatePalette(EmrCreatePalette {
      palette_index: 3,
      log_palette: LogPalette {
        version: 0x0300,
        entries: Vec::new(),
      },
    });
    assert!(invalid_create.to_record().is_err());

    let set_entries = EmfRecordData::SetPaletteEntries(EmrSetPaletteEntries {
      palette_index: 3,
      start: 4,
      entries,
    });
    let record = set_entries.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetPaletteEntries.raw());
    assert_eq!(record.parse_data().unwrap(), set_entries);
    let mut oversized_set_entries = record.clone();
    oversized_set_entries.data[8..12].copy_from_slice(&1_000_000_u32.to_le_bytes());
    oversized_set_entries.data.truncate(12);
    assert!(oversized_set_entries.parse_data().is_err());
    let invalid_set_entries_index = EmfRecordData::SetPaletteEntries(EmrSetPaletteEntries {
      palette_index: 0,
      start: 4,
      entries: Vec::new(),
    });
    assert!(invalid_set_entries_index.to_record().is_err());
    let mut invalid_set_entries_index_record = record.clone();
    invalid_set_entries_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(invalid_set_entries_index_record.parse_data().is_err());
    let mut invalid_set_entries_tail = record.clone();
    invalid_set_entries_tail.data.push(0xEE);
    assert!(invalid_set_entries_tail.parse_data().is_err());

    let color_correct = EmfRecordData::ColorCorrectPalette(EmrColorCorrectPalette {
      palette_index: 3,
      first_entry: 4,
      palette_entries: 2,
      reserved: 0xAABB_CCDD,
    });
    let record = color_correct.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ColorCorrectPalette.raw());
    assert_eq!(record.data.len(), 16);
    assert_eq!(record.parse_data().unwrap(), color_correct);
    let mut invalid_color_correct = color_correct.clone();
    let EmfRecordData::ColorCorrectPalette(value) = &mut invalid_color_correct else {
      unreachable!();
    };
    value.palette_index = 0;
    assert!(invalid_color_correct.to_record().is_err());
    let mut invalid_color_correct_record = record.clone();
    invalid_color_correct_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(invalid_color_correct_record.parse_data().is_err());
  }

  #[test]
  fn typed_linked_ufi_records_roundtrip() {
    let value = EmfRecordData::SetLinkedUfis(EmrSetLinkedUfis {
      ufis: vec![
        EmrForceUfiMapping {
          checksum: 0x0102_0304,
          index: 5,
        },
        EmrForceUfiMapping {
          checksum: 0xAABB_CCDD,
          index: 6,
        },
      ],
      reserved: [0x11; 8],
    });
    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetLinkedUfis.raw());
    assert_eq!(record.data.len(), 28);
    assert_eq!(record.parse_data().unwrap(), value);
    let mut oversized_ufis = record.clone();
    oversized_ufis.data[0..4].copy_from_slice(&1_000_000_u32.to_le_bytes());
    oversized_ufis.data.truncate(4);
    assert!(oversized_ufis.parse_data().is_err());
  }

  #[test]
  fn typed_scale_and_text_justification_records_roundtrip() {
    fn assert_state_fixed_roundtrip(
      value: EmfRecordData,
      record_type: EmfRecordType,
      data_len: usize,
    ) {
      let record = value.to_record().unwrap();
      assert_eq!(record.record_type, record_type.raw());
      assert_eq!(record.data.len(), data_len);
      assert_eq!(record.parse_data().unwrap(), value);

      let mut trailing_record = record;
      trailing_record.data.extend_from_slice(&0_u32.to_le_bytes());
      assert!(trailing_record.parse_data().is_err());
    }

    assert_state_fixed_roundtrip(
      EmfRecordData::SetWindowExtEx(EmrSetWindowExtEx {
        size: SizeL { cx: 100, cy: 200 },
      }),
      EmfRecordType::SetWindowExtEx,
      8,
    );
    assert_state_fixed_roundtrip(
      EmfRecordData::SetWindowOrgEx(EmrSetWindowOrgEx {
        origin: PointL { x: -10, y: 20 },
      }),
      EmfRecordType::SetWindowOrgEx,
      8,
    );
    assert_state_fixed_roundtrip(
      EmfRecordData::SetViewportExtEx(EmrSetViewportExtEx {
        size: SizeL { cx: 300, cy: 400 },
      }),
      EmfRecordType::SetViewportExtEx,
      8,
    );
    assert_state_fixed_roundtrip(
      EmfRecordData::SetViewportOrgEx(EmrSetViewportOrgEx {
        origin: PointL { x: 30, y: -40 },
      }),
      EmfRecordType::SetViewportOrgEx,
      8,
    );
    assert_state_fixed_roundtrip(
      EmfRecordData::SetBrushOrgEx(EmrSetBrushOrgEx {
        origin: PointL { x: 5, y: 6 },
      }),
      EmfRecordType::SetBrushOrgEx,
      8,
    );
    assert_state_fixed_roundtrip(
      EmfRecordData::MoveToEx(EmrMoveToEx {
        point: PointL { x: 7, y: 8 },
      }),
      EmfRecordType::MoveToEx,
      8,
    );
    assert_state_fixed_roundtrip(
      EmfRecordData::SetMiterLimit(EmrSetMiterLimit { miter_limit: 10 }),
      EmfRecordType::SetMiterLimit,
      4,
    );

    let scale = EmfRecordData::ScaleViewportExtEx(EmrScaleViewportExtEx {
      x_num: 2,
      x_denom: 3,
      y_num: -4,
      y_denom: 5,
    });
    let record = scale.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ScaleViewportExtEx.raw());
    assert_eq!(record.parse_data().unwrap(), scale);
    let mut invalid_scale = scale.clone();
    let EmfRecordData::ScaleViewportExtEx(value) = &mut invalid_scale else {
      unreachable!();
    };
    value.x_num = 0;
    assert!(invalid_scale.to_record().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[0..4].copy_from_slice(&0_i32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());

    let scale_window = EmfRecordData::ScaleWindowExtEx(EmrScaleWindowExtEx {
      x_num: 2,
      x_denom: 3,
      y_num: 4,
      y_denom: -5,
    });
    let record = scale_window.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ScaleWindowExtEx.raw());
    assert_eq!(record.parse_data().unwrap(), scale_window);
    let mut invalid_scale_window = scale_window.clone();
    let EmfRecordData::ScaleWindowExtEx(value) = &mut invalid_scale_window else {
      unreachable!();
    };
    value.y_denom = 0;
    assert!(invalid_scale_window.to_record().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[12..16].copy_from_slice(&0_i32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());

    let justification = EmfRecordData::SetTextJustification(EmrSetTextJustification {
      break_extra: -12,
      break_count: 4,
    });
    let record = justification.to_record().unwrap();
    assert_eq!(
      record.record_type,
      EmfRecordType::SetTextJustification.raw()
    );
    assert_eq!(record.parse_data().unwrap(), justification);
    let mut trailing_justification = record;
    trailing_justification
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_justification.parse_data().is_err());
  }

  #[test]
  fn typed_no_parameter_records_roundtrip() {
    let values = [
      EmfRecordData::SaveDc,
      EmfRecordData::RealizePalette,
      EmfRecordData::BeginPath,
      EmfRecordData::EndPath,
      EmfRecordData::CloseFigure,
      EmfRecordData::FlattenPath,
      EmfRecordData::WidenPath,
      EmfRecordData::AbortPath,
    ];

    for value in values {
      let record = value.to_record().unwrap();
      assert!(record.data.is_empty());
      assert_eq!(record.parse_data().unwrap(), value);

      let mut trailing_record = record;
      trailing_record.data.extend_from_slice(&0_u32.to_le_bytes());
      assert!(trailing_record.parse_data().is_err());
    }

    let begin = EmfRecordData::BeginPath.to_record().unwrap();
    let end = EmfRecordData::EndPath.to_record().unwrap();
    let abort = EmfRecordData::AbortPath.to_record().unwrap();
    assert!(validate_emf_path_brackets(&[begin.clone(), end]).is_ok());
    assert!(validate_emf_path_brackets(&[begin.clone(), abort]).is_ok());
    assert!(validate_emf_path_brackets(&[begin.clone(), begin]).is_err());
    assert!(validate_emf_path_brackets(&[EmfRecordData::EndPath.to_record().unwrap()]).is_err());
    assert!(validate_emf_path_brackets(&[EmfRecordData::BeginPath.to_record().unwrap()]).is_err());
  }

  #[test]
  fn typed_poly_polyline16_record_roundtrips() {
    let value = EmfRecordData::PolyPolyline16(EmrPolyPolygonS {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 10,
        bottom: 10,
      },
      counts: vec![2, 3],
      points: vec![
        crate::types::PointS { x: 1, y: 2 },
        crate::types::PointS { x: 3, y: 4 },
        crate::types::PointS { x: 5, y: 6 },
        crate::types::PointS { x: 7, y: 8 },
        crate::types::PointS { x: 9, y: 10 },
      ],
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyPolyline16.raw());
    assert_eq!(record.parse_data().unwrap(), value);
    let mut trailing_polyline16_record = record.clone();
    trailing_polyline16_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_polyline16_record.parse_data().is_err());

    let mut invalid_polyline_count = value.clone();
    let EmfRecordData::PolyPolyline16(value) = &mut invalid_polyline_count else {
      unreachable!();
    };
    value.counts = vec![1, 4];
    assert!(invalid_polyline_count.to_record().is_err());

    let mut invalid_polyline_count_record = record.clone();
    invalid_polyline_count_record.data[24..28].copy_from_slice(&1_u32.to_le_bytes());
    invalid_polyline_count_record.data[28..32].copy_from_slice(&4_u32.to_le_bytes());
    assert!(invalid_polyline_count_record.parse_data().is_err());

    let polygon = EmfRecordData::PolyPolygon16(EmrPolyPolygonS {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 10,
        bottom: 10,
      },
      counts: vec![1],
      points: vec![crate::types::PointS { x: 1, y: 2 }],
    });
    let record = polygon.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyPolygon16.raw());
    assert_eq!(record.parse_data().unwrap(), polygon);

    let mut invalid_polygon_count = polygon.clone();
    let EmfRecordData::PolyPolygon16(value) = &mut invalid_polygon_count else {
      unreachable!();
    };
    value.counts = vec![2];
    assert!(invalid_polygon_count.to_record().is_err());
  }

  #[test]
  fn typed_poly_polyline_record_validates_counts() {
    let value = EmfRecordData::PolyPolyline(EmrPolyPolygonL {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 10,
        bottom: 10,
      },
      counts: vec![2],
      points: vec![PointL { x: 1, y: 2 }, PointL { x: 3, y: 4 }],
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyPolyline.raw());
    assert_eq!(record.parse_data().unwrap(), value);
    let mut trailing_polyline_record = record.clone();
    trailing_polyline_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_polyline_record.parse_data().is_err());

    let mut invalid_polyline_count = value.clone();
    let EmfRecordData::PolyPolyline(value) = &mut invalid_polyline_count else {
      unreachable!();
    };
    value.counts = vec![1];
    assert!(invalid_polyline_count.to_record().is_err());

    let polygon = EmfRecordData::PolyPolygon(EmrPolyPolygonL {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 10,
        bottom: 10,
      },
      counts: vec![1],
      points: vec![PointL { x: 1, y: 2 }],
    });
    let record = polygon.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyPolygon.raw());
    assert_eq!(record.parse_data().unwrap(), polygon);

    let mut invalid_polygon_count = polygon.clone();
    let EmfRecordData::PolyPolygon(value) = &mut invalid_polygon_count else {
      unreachable!();
    };
    value.counts = vec![2];
    assert!(invalid_polygon_count.to_record().is_err());

    let oversized_count_record = EmfRecord::new(EmfRecordType::PolyPolygon.raw(), {
      let mut data = Vec::new();
      data.extend_from_slice(&0_i32.to_le_bytes());
      data.extend_from_slice(&0_i32.to_le_bytes());
      data.extend_from_slice(&10_i32.to_le_bytes());
      data.extend_from_slice(&10_i32.to_le_bytes());
      data.extend_from_slice(&1_000_000_u32.to_le_bytes());
      data.extend_from_slice(&0_u32.to_le_bytes());
      data
    });
    assert!(oversized_count_record.parse_data().is_err());
  }

  #[test]
  fn typed_poly_draw_records_roundtrip() {
    let bounds = RectL {
      left: 0,
      top: 0,
      right: 20,
      bottom: 20,
    };
    let value = EmfRecordData::PolyDraw(EmrPolyDrawL {
      bounds,
      points: vec![
        PointL { x: 1, y: 2 },
        PointL { x: 3, y: 4 },
        PointL { x: 5, y: 6 },
      ],
      point_types: point_types(&[0x06, 0x02, 0x03]),
      padding: vec![0],
    });
    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyDraw.raw());
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, value);
    let mut trailing_padding_record = record.clone();
    trailing_padding_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_padding_record.parse_data().is_err());
    let EmfRecordData::PolyDraw(parsed) = parsed else {
      unreachable!();
    };
    assert!(parsed.point_types[2].close_figure());
    assert_eq!(
      parsed.point_types[2].point_type(),
      Some(EmrPointType::LineTo)
    );

    let bezier = EmfRecordData::PolyDraw(EmrPolyDrawL {
      bounds,
      points: vec![
        PointL { x: 1, y: 2 },
        PointL { x: 3, y: 4 },
        PointL { x: 5, y: 6 },
        PointL { x: 7, y: 8 },
      ],
      point_types: point_types(&[0x06, 0x04, 0x04, 0x05]),
      padding: Vec::new(),
    });
    let record = bezier.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), bezier);

    let invalid_bezier = EmfRecordData::PolyDraw(EmrPolyDrawL {
      bounds,
      points: vec![
        PointL { x: 1, y: 2 },
        PointL { x: 3, y: 4 },
        PointL { x: 5, y: 6 },
      ],
      point_types: point_types(&[0x06, 0x04, 0x04]),
      padding: vec![0],
    });
    assert!(invalid_bezier.to_record().is_err());
    let invalid_record = EmfRecord::new(EmfRecordType::PolyDraw.raw(), {
      let mut data = Vec::new();
      data.extend_from_slice(&bounds.left.to_le_bytes());
      data.extend_from_slice(&bounds.top.to_le_bytes());
      data.extend_from_slice(&bounds.right.to_le_bytes());
      data.extend_from_slice(&bounds.bottom.to_le_bytes());
      data.extend_from_slice(&3u32.to_le_bytes());
      for point in [
        PointL { x: 1, y: 2 },
        PointL { x: 3, y: 4 },
        PointL { x: 5, y: 6 },
      ] {
        data.extend_from_slice(&point.x.to_le_bytes());
        data.extend_from_slice(&point.y.to_le_bytes());
      }
      data.extend_from_slice(&[0x06, 0x04, 0x04, 0x00]);
      data
    });
    assert!(invalid_record.parse_data().is_err());

    let value = EmfRecordData::PolyDraw16(EmrPolyDrawS {
      bounds,
      points: vec![
        PointS { x: 1, y: 2 },
        PointS { x: 3, y: 4 },
        PointS { x: 5, y: 6 },
      ],
      point_types: point_types(&[0x06, 0x02, 0x03]),
      padding: vec![0],
    });
    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyDraw16.raw());
    assert_eq!(record.parse_data().unwrap(), value);
    let mut trailing_padding16_record = record.clone();
    trailing_padding16_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_padding16_record.parse_data().is_err());

    let invalid = EmfRecordData::PolyDraw(EmrPolyDrawL {
      bounds,
      points: vec![PointL { x: 1, y: 2 }],
      point_types: vec![EmrPointTypeValue { value: 0x07 }],
      padding: Vec::new(),
    });
    assert!(invalid.to_record().is_err());
  }

  #[test]
  fn typed_gradient_fill_records_roundtrip() {
    let bounds = RectL {
      left: 0,
      top: 0,
      right: 100,
      bottom: 100,
    };
    let vertices = vec![
      TriVertex {
        x: 0,
        y: 0,
        red: 0xFFFF,
        green: 0,
        blue: 0,
        alpha: 0,
      },
      TriVertex {
        x: 100,
        y: 100,
        red: 0,
        green: 0xFFFF,
        blue: 0,
        alpha: 0,
      },
      TriVertex {
        x: 0,
        y: 100,
        red: 0,
        green: 0,
        blue: 0xFFFF,
        alpha: 0,
      },
    ];
    let rectangle = EmfRecordData::GradientFill(EmrGradientFill {
      bounds,
      mode: 1,
      vertices: vertices[..2].to_vec(),
      mesh: EmrGradientFillMesh::Rectangles {
        rectangles: vec![EmrGradientRectangle {
          upper_left: 0,
          lower_right: 1,
        }],
        padding: vec![1, 2, 3, 4],
      },
    });
    let record = rectangle.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::GradientFill.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::GradientFill(value) = &parsed else {
      panic!("expected EMR_GRADIENTFILL");
    };
    assert_eq!(
      value.mode_kind(),
      Some(EmrGradientFillMode::RectangleVertical)
    );
    assert_eq!(parsed, rectangle);
    let mut invalid_rectangle = rectangle.clone();
    {
      let EmfRecordData::GradientFill(value) = &mut invalid_rectangle else {
        unreachable!();
      };
      let EmrGradientFillMesh::Rectangles { rectangles, .. } = &mut value.mesh else {
        unreachable!();
      };
      rectangles[0].lower_right = 2;
    }
    assert!(invalid_rectangle.to_record().is_err());
    let mut invalid_rectangle = rectangle.clone();
    {
      let EmfRecordData::GradientFill(value) = &mut invalid_rectangle else {
        unreachable!();
      };
      let EmrGradientFillMesh::Rectangles { padding, .. } = &mut value.mesh else {
        unreachable!();
      };
      padding.clear();
    }
    assert!(invalid_rectangle.to_record().is_err());
    let mut invalid_rectangle_record = record.clone();
    let rectangle_mesh_offset = 28 + 2 * 16;
    invalid_rectangle_record.data[rectangle_mesh_offset + 4..rectangle_mesh_offset + 8]
      .copy_from_slice(&2_u32.to_le_bytes());
    assert!(invalid_rectangle_record.parse_data().is_err());
    let mut trailing_rectangle_record = record.clone();
    trailing_rectangle_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_rectangle_record.parse_data().is_err());

    let triangle = EmfRecordData::GradientFill(EmrGradientFill {
      bounds,
      mode: EmrGradientFillMode::Triangle.raw(),
      vertices,
      mesh: EmrGradientFillMesh::Triangles(vec![EmrGradientTriangle {
        vertex1: 0,
        vertex2: 1,
        vertex3: 2,
      }]),
    });
    let record = triangle.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::GradientFill.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::GradientFill(value) = &parsed else {
      panic!("expected EMR_GRADIENTFILL");
    };
    assert_eq!(value.mode_kind(), Some(EmrGradientFillMode::Triangle));
    assert_eq!(parsed, triangle);
    let mut invalid_triangle = triangle.clone();
    {
      let EmfRecordData::GradientFill(value) = &mut invalid_triangle else {
        unreachable!();
      };
      let EmrGradientFillMesh::Triangles(triangles) = &mut value.mesh else {
        unreachable!();
      };
      triangles[0].vertex3 = 3;
    }
    assert!(invalid_triangle.to_record().is_err());
    let mut invalid_triangle_record = record.clone();
    let triangle_mesh_offset = 28 + 3 * 16;
    invalid_triangle_record.data[triangle_mesh_offset + 8..triangle_mesh_offset + 12]
      .copy_from_slice(&3_u32.to_le_bytes());
    assert!(invalid_triangle_record.parse_data().is_err());
    let mut trailing_triangle_record = record.clone();
    trailing_triangle_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_triangle_record.parse_data().is_err());
    assert!(
      EmfRecordData::GradientFill(EmrGradientFill {
        bounds,
        mode: EmrGradientFillMode::Triangle.raw(),
        vertices: vec![
          TriVertex {
            x: 0,
            y: 0,
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
          },
          TriVertex {
            x: 1,
            y: 1,
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
          },
        ],
        mesh: EmrGradientFillMesh::Rectangles {
          rectangles: vec![EmrGradientRectangle {
            upper_left: 0,
            lower_right: 1,
          }],
          padding: vec![0; 4],
        },
      })
      .to_record()
      .is_err()
    );

    let oversized_vertex_record = EmfRecord::new(EmfRecordType::GradientFill.raw(), {
      let mut data = Vec::new();
      data.extend_from_slice(&bounds.left.to_le_bytes());
      data.extend_from_slice(&bounds.top.to_le_bytes());
      data.extend_from_slice(&bounds.right.to_le_bytes());
      data.extend_from_slice(&bounds.bottom.to_le_bytes());
      data.extend_from_slice(&1_000_000_u32.to_le_bytes());
      data.extend_from_slice(&0_u32.to_le_bytes());
      data.extend_from_slice(&EmrGradientFillMode::Triangle.raw().to_le_bytes());
      data
    });
    assert!(oversized_vertex_record.parse_data().is_err());

    let invalid_mode_record = EmfRecord::new(EmfRecordType::GradientFill.raw(), {
      let mut data = Vec::new();
      data.extend_from_slice(&bounds.left.to_le_bytes());
      data.extend_from_slice(&bounds.top.to_le_bytes());
      data.extend_from_slice(&bounds.right.to_le_bytes());
      data.extend_from_slice(&bounds.bottom.to_le_bytes());
      data.extend_from_slice(&0_u32.to_le_bytes());
      data.extend_from_slice(&0_u32.to_le_bytes());
      data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
      data
    });
    assert!(invalid_mode_record.parse_data().is_err());
    assert!(
      EmfRecordData::GradientFill(EmrGradientFill {
        bounds,
        mode: 0xFFFF_FFFF,
        vertices: Vec::new(),
        mesh: EmrGradientFillMesh::Raw {
          mesh_count: 0,
          data: Vec::new(),
        },
      })
      .to_record()
      .is_err()
    );
  }

  #[test]
  fn typed_escape_opengl_pixel_format_and_color_profile_records_roundtrip() {
    let draw_escape = EmfRecordData::DrawEscape(EmrEscape {
      escape: u32::from(WmfMetafileEscape::PassThrough.raw()),
      data: vec![1, 2, 3],
      padding: vec![0],
    });
    let record = draw_escape.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::DrawEscape.raw());
    assert_eq!(record.parse_data().unwrap(), draw_escape);

    let ext_escape = EmfRecordData::ExtEscape(EmrEscape {
      escape: u32::from(WmfMetafileEscape::PostScriptData.raw()),
      data: vec![4, 5, 6],
      padding: vec![0],
    });
    let record = ext_escape.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ExtEscape.raw());
    assert_eq!(record.parse_data().unwrap(), ext_escape);

    let named_escape = EmfRecordData::NamedEscape(EmrNamedEscape {
      escape: u32::from(WmfMetafileEscape::PostScriptData.raw()),
      driver_name: SdkString::raw(vec![b'P', 0, b'S', 0, 0, 0], SdkEncoding::Utf16Le),
      data: vec![7, 8],
      padding: Vec::new(),
    });
    let record = named_escape.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::NamedEscape.raw());
    assert_eq!(record.parse_data().unwrap(), named_escape);
    assert!(
      EmfRecordData::DrawEscape(EmrEscape {
        escape: 0xFFFF_FFFF,
        data: Vec::new(),
        padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::NamedEscape(EmrNamedEscape {
        escape: u32::from(WmfMetafileEscape::PostScriptData.raw()),
        driver_name: SdkString::raw(vec![b'P', 0, b'S', 0], SdkEncoding::Utf16Le),
        data: Vec::new(),
        padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecord::new(
        EmfRecordType::DrawEscape.raw(),
        [
          0xFFFF_FFFF_u32.to_le_bytes().as_slice(),
          0_u32.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );

    let gls = EmfRecordData::GlsRecord(EmrOpenGlRecord {
      data: vec![0xAA, 0xBB, 0xCC],
      padding: vec![0],
    });
    let record = gls.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::GlsRecord.raw());
    assert_eq!(record.parse_data().unwrap(), gls);
    let mut invalid_gls_padding = record.clone();
    invalid_gls_padding
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(invalid_gls_padding.parse_data().is_err());
    let mut invalid_gls_size = record.clone();
    invalid_gls_size.data[0..4].copy_from_slice(&5_u32.to_le_bytes());
    assert!(invalid_gls_size.parse_data().is_err());
    assert!(
      EmfRecordData::GlsRecord(EmrOpenGlRecord {
        data: vec![0xAA, 0xBB, 0xCC],
        padding: vec![0, 0],
      })
      .to_record()
      .is_err()
    );
    let empty_gls = EmfRecordData::GlsRecord(EmrOpenGlRecord {
      data: Vec::new(),
      padding: Vec::new(),
    });
    assert_eq!(
      empty_gls.to_record().unwrap().parse_data().unwrap(),
      empty_gls
    );

    let bounds = RectL {
      left: 1,
      top: 2,
      right: 3,
      bottom: 4,
    };
    let gls_bounded = EmfRecordData::GlsBoundedRecord(EmrGlsBoundedRecord {
      bounds,
      data: vec![0xDD, 0xEE],
      padding: vec![0, 0],
    });
    let record = gls_bounded.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::GlsBoundedRecord.raw());
    assert_eq!(record.parse_data().unwrap(), gls_bounded);
    let mut invalid_gls_bounded_padding = record.clone();
    invalid_gls_bounded_padding
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(invalid_gls_bounded_padding.parse_data().is_err());
    let mut invalid_gls_bounded_size = record.clone();
    invalid_gls_bounded_size.data[16..20].copy_from_slice(&7_u32.to_le_bytes());
    assert!(invalid_gls_bounded_size.parse_data().is_err());
    assert!(
      EmfRecordData::GlsBoundedRecord(EmrGlsBoundedRecord {
        bounds,
        data: vec![0xDD, 0xEE],
        padding: vec![0],
      })
      .to_record()
      .is_err()
    );

    let pixel_format = EmfRecordData::PixelFormat(EmrPixelFormat {
      n_size: 40,
      n_version: 1,
      flags: 0x0000_0024,
      pixel_type: 0,
      color_bits: 32,
      red_bits: 8,
      red_shift: 16,
      green_bits: 8,
      green_shift: 8,
      blue_bits: 8,
      blue_shift: 0,
      alpha_bits: 8,
      alpha_shift: 24,
      accum_bits: 0,
      accum_red_bits: 0,
      accum_green_bits: 0,
      accum_blue_bits: 0,
      accum_alpha_bits: 0,
      depth_bits: 24,
      stencil_bits: 8,
      aux_buffers: 0,
      layer_type: 0,
      reserved: 0,
      layer_mask: 0,
      visible_mask: 0,
      damage_mask: 0,
    });
    let record = pixel_format.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PixelFormat.raw());
    assert_eq!(record.parse_data().unwrap(), pixel_format);
    let EmfRecordData::PixelFormat(value) = &pixel_format else {
      panic!("expected EMR_PIXELFORMAT");
    };
    assert_eq!(
      value.flags(),
      EmrPixelFormatFlags::DRAW_TO_WINDOW | EmrPixelFormatFlags::SUPPORT_OPENGL
    );
    assert_eq!(value.pixel_type_kind(), Some(EmrPixelFormatType::Rgba));
    assert_eq!(value.overlay_plane_count(), 0);
    assert_eq!(value.underlay_plane_count(), 0);

    let mut invalid_size = pixel_format.clone();
    let EmfRecordData::PixelFormat(value) = &mut invalid_size else {
      panic!("expected EMR_PIXELFORMAT");
    };
    value.n_size = 39;
    assert!(invalid_size.to_record().is_err());
    let mut invalid_size_record = record.clone();
    invalid_size_record.data[0..2].copy_from_slice(&39_u16.to_le_bytes());
    assert!(invalid_size_record.parse_data().is_err());

    let mut invalid_version = pixel_format.clone();
    let EmfRecordData::PixelFormat(value) = &mut invalid_version else {
      panic!("expected EMR_PIXELFORMAT");
    };
    value.n_version = 2;
    assert!(invalid_version.to_record().is_err());
    let mut invalid_version_record = record.clone();
    invalid_version_record.data[2..4].copy_from_slice(&2_u16.to_le_bytes());
    assert!(invalid_version_record.parse_data().is_err());

    let mut invalid_flags = pixel_format.clone();
    let EmfRecordData::PixelFormat(value) = &mut invalid_flags else {
      panic!("expected EMR_PIXELFORMAT");
    };
    value.flags = 0x8000_0000;
    assert!(invalid_flags.to_record().is_err());
    let mut invalid_flags_record = record.clone();
    invalid_flags_record.data[4..8].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
    assert!(invalid_flags_record.parse_data().is_err());

    let mut incompatible_flags = pixel_format.clone();
    let EmfRecordData::PixelFormat(value) = &mut incompatible_flags else {
      panic!("expected EMR_PIXELFORMAT");
    };
    value.flags = (EmrPixelFormatFlags::DOUBLEBUFFER | EmrPixelFormatFlags::SUPPORT_GDI).bits();
    assert!(incompatible_flags.to_record().is_err());
    let mut incompatible_flags_record = record.clone();
    incompatible_flags_record.data[4..8].copy_from_slice(
      &(EmrPixelFormatFlags::DOUBLEBUFFER | EmrPixelFormatFlags::SUPPORT_GDI)
        .bits()
        .to_le_bytes(),
    );
    assert!(incompatible_flags_record.parse_data().is_err());

    let mut invalid_pixel_type = pixel_format.clone();
    let EmfRecordData::PixelFormat(value) = &mut invalid_pixel_type else {
      panic!("expected EMR_PIXELFORMAT");
    };
    value.pixel_type = 2;
    assert!(invalid_pixel_type.to_record().is_err());
    let mut invalid_pixel_type_record = record.clone();
    invalid_pixel_type_record.data[8] = 2;
    assert!(invalid_pixel_type_record.parse_data().is_err());

    let ufi = EmfRecordData::ForceUfiMapping(EmrForceUfiMapping {
      checksum: 0x0102_0304,
      index: 5,
    });
    let record = ufi.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ForceUfiMapping.raw());
    assert_eq!(record.parse_data().unwrap(), ufi);
    let mut trailing_ufi = record;
    trailing_ufi.data.extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_ufi.parse_data().is_err());

    let profile_a = EmfRecordData::SetIcmProfileA(EmrColorProfile {
      flags: 1,
      name: SdkString::raw(b"s.icc\0".to_vec(), SdkEncoding::Windows1252),
      data: vec![1, 2],
      padding: Vec::new(),
    });
    let record = profile_a.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetIcmProfileA.raw());
    assert_eq!(record.parse_data().unwrap(), profile_a);
    let mut oversized_profile_a_name = record.clone();
    oversized_profile_a_name.data[4..8].copy_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(oversized_profile_a_name.parse_data().is_err());

    let profile_w = EmfRecordData::SetIcmProfileW(EmrColorProfile {
      flags: 2,
      name: SdkString::raw(vec![b'w', 0, 0, 0], SdkEncoding::Utf16Le),
      data: vec![3, 4, 5],
      padding: vec![0],
    });
    let record = profile_w.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetIcmProfileW.raw());
    assert_eq!(record.parse_data().unwrap(), profile_w);
    let mut oversized_profile_w_data = record.clone();
    oversized_profile_w_data.data[8..12].copy_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(oversized_profile_w_data.parse_data().is_err());
    let mut invalid_profile_w_padding = record.clone();
    invalid_profile_w_padding
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(invalid_profile_w_padding.parse_data().is_err());
    let mut invalid_profile_w_write_padding = profile_w.clone();
    let EmfRecordData::SetIcmProfileW(value) = &mut invalid_profile_w_write_padding else {
      unreachable!();
    };
    value.padding = vec![0, 0];
    assert!(invalid_profile_w_write_padding.to_record().is_err());

    let color_match = EmfRecordData::ColorMatchToTargetW(EmrColorMatchToTargetW {
      action: 1,
      flags: 1,
      name: SdkString::raw(vec![b'c', 0, 0, 0], SdkEncoding::Utf16Le),
      data: vec![6, 7],
      padding: vec![0, 0],
    });
    let record = color_match.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ColorMatchToTargetW.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ColorMatchToTargetW(value) = &parsed else {
      panic!("expected EMR_COLORMATCHTOTARGETW");
    };
    assert_eq!(value.action_kind(), Some(EmrColorSpaceMode::Enable));
    assert_eq!(value.flags_kind(), Some(EmrColorMatchToTarget::Embedded));
    assert_eq!(parsed, color_match);
    let mut invalid_color_match = color_match.clone();
    let EmfRecordData::ColorMatchToTargetW(value) = &mut invalid_color_match else {
      unreachable!();
    };
    value.action = 0xFFFF_FFFF;
    assert!(invalid_color_match.to_record().is_err());
    let mut invalid_color_match_record = record.clone();
    invalid_color_match_record.data[0..4].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_color_match_record.parse_data().is_err());
    let mut invalid_color_match_flags = color_match.clone();
    let EmfRecordData::ColorMatchToTargetW(value) = &mut invalid_color_match_flags else {
      unreachable!();
    };
    value.flags = 2;
    assert!(invalid_color_match_flags.to_record().is_err());
    let mut invalid_color_match_flags_record = record.clone();
    invalid_color_match_flags_record.data[4..8].copy_from_slice(&2_u32.to_le_bytes());
    assert!(invalid_color_match_flags_record.parse_data().is_err());
    let mut oversized_color_match_name = record.clone();
    oversized_color_match_name.data[8..12].copy_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(oversized_color_match_name.parse_data().is_err());
    let mut oversized_color_match_data = record.clone();
    oversized_color_match_data.data[12..16].copy_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(oversized_color_match_data.parse_data().is_err());
    let mut invalid_color_match_padding_record = record.clone();
    invalid_color_match_padding_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(invalid_color_match_padding_record.parse_data().is_err());
    let mut invalid_color_match_padding = color_match.clone();
    let EmfRecordData::ColorMatchToTargetW(value) = &mut invalid_color_match_padding else {
      unreachable!();
    };
    value.padding = vec![0];
    assert!(invalid_color_match_padding.to_record().is_err());
  }

  #[test]
  fn typed_fixed_drawing_records_roundtrip() {
    fn assert_fixed_roundtrip(value: EmfRecordData, record_type: EmfRecordType, data_len: usize) {
      let record = value.to_record().unwrap();
      assert_eq!(record.record_type, record_type.raw());
      assert_eq!(record.data.len(), data_len);
      assert_eq!(record.parse_data().unwrap(), value);

      let mut trailing_record = record;
      trailing_record.data.extend_from_slice(&0_u32.to_le_bytes());
      assert!(trailing_record.parse_data().is_err());
    }

    let arc = EmrArc {
      box_bounds: RectL {
        left: 1,
        top: 2,
        right: 30,
        bottom: 40,
      },
      start: PointL { x: 5, y: 6 },
      end: PointL { x: 7, y: 8 },
    };
    assert_fixed_roundtrip(EmfRecordData::Arc(arc.clone()), EmfRecordType::Arc, 32);
    assert_fixed_roundtrip(EmfRecordData::ArcTo(arc.clone()), EmfRecordType::ArcTo, 32);
    assert_fixed_roundtrip(EmfRecordData::Chord(arc.clone()), EmfRecordType::Chord, 32);
    assert_fixed_roundtrip(EmfRecordData::Pie(arc), EmfRecordType::Pie, 32);

    assert_fixed_roundtrip(
      EmfRecordData::AngleArc(EmrAngleArc {
        center: PointL { x: 10, y: 11 },
        radius: 12,
        start_angle: 45.0,
        sweep_angle: 90.0,
      }),
      EmfRecordType::AngleArc,
      20,
    );

    assert_fixed_roundtrip(
      EmfRecordData::RoundRect(EmrRoundRect {
        bounds: RectL {
          left: 0,
          top: 0,
          right: 100,
          bottom: 50,
        },
        corner: SizeL { cx: 8, cy: 10 },
      }),
      EmfRecordType::RoundRect,
      24,
    );

    assert_fixed_roundtrip(
      EmfRecordData::MoveToEx(EmrMoveToEx {
        point: PointL { x: 1, y: 2 },
      }),
      EmfRecordType::MoveToEx,
      8,
    );
    assert_fixed_roundtrip(
      EmfRecordData::LineTo(EmrLineTo {
        point: PointL { x: 3, y: 4 },
      }),
      EmfRecordType::LineTo,
      8,
    );

    let bounds = RectL {
      left: -10,
      top: -20,
      right: 30,
      bottom: 40,
    };
    assert_fixed_roundtrip(
      EmfRecordData::Rectangle(EmrRectangle { bounds }),
      EmfRecordType::Rectangle,
      16,
    );
    assert_fixed_roundtrip(
      EmfRecordData::Ellipse(EmrEllipse { bounds }),
      EmfRecordType::Ellipse,
      16,
    );

    let set_pixel = EmfRecordData::SetPixelV(EmrSetPixelV {
      pixel: PointL { x: 7, y: 8 },
      color: ColorRef {
        red: 10,
        green: 20,
        blue: 30,
        reserved: 0,
      },
    });
    let record = set_pixel.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SetPixelV.raw());
    assert_eq!(record.data.len(), 12);
    assert_eq!(record.parse_data().unwrap(), set_pixel);
    let mut invalid_color = record.clone();
    invalid_color.data[11] = 1;
    let parsed = invalid_color.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_color);
    let mut trailing_record = record;
    trailing_record.data.extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_record.parse_data().is_err());

    let invalid_set_pixel = EmfRecordData::SetPixelV(EmrSetPixelV {
      pixel: PointL { x: 7, y: 8 },
      color: ColorRef {
        red: 10,
        green: 20,
        blue: 30,
        reserved: 1,
      },
    });
    assert!(invalid_set_pixel.validate_strict().is_err());
    assert!(invalid_set_pixel.to_record().is_ok());
  }

  #[test]
  fn typed_path_bounds_and_flood_fill_records_roundtrip() {
    let bounds = RectL {
      left: -1,
      top: -2,
      right: 20,
      bottom: 30,
    };
    let values = [
      EmfRecordData::FillPath(EmrFillPath { bounds }),
      EmfRecordData::StrokeAndFillPath(EmrStrokeAndFillPath { bounds }),
      EmfRecordData::StrokePath(EmrStrokePath { bounds }),
    ];

    for value in values {
      let record = value.to_record().unwrap();
      assert_eq!(record.data.len(), 16);
      assert_eq!(record.parse_data().unwrap(), value);
      let mut trailing_record = record;
      trailing_record.data.extend_from_slice(&0_u32.to_le_bytes());
      assert!(trailing_record.parse_data().is_err());
    }

    let flood_fill = EmfRecordData::ExtFloodFill(EmrExtFloodFill {
      start: PointL { x: 3, y: 4 },
      color: ColorRef {
        red: 10,
        green: 20,
        blue: 30,
        reserved: 0,
      },
      flood_fill_mode: 1,
    });
    let record = flood_fill.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ExtFloodFill.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ExtFloodFill(value) = &parsed else {
      panic!("expected EMR_EXTFLOODFILL");
    };
    assert_eq!(
      value.flood_fill_mode_kind(),
      Some(EmrFloodFillMode::Surface)
    );
    assert_eq!(parsed, flood_fill);
    let mut invalid_flood_fill = flood_fill.clone();
    let EmfRecordData::ExtFloodFill(value) = &mut invalid_flood_fill else {
      unreachable!();
    };
    value.flood_fill_mode = 0xFFFF_FFFF;
    assert!(invalid_flood_fill.to_record().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[12..16].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());
    let mut trailing_record = record;
    trailing_record.data.extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_record.parse_data().is_err());

    let select_clip = EmfRecordData::SelectClipPath(EmrSelectClipPath {
      region_mode: EmrRegionMode::Or.raw(),
    });
    let record = select_clip.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SelectClipPath(value) = &parsed else {
      panic!("expected EMR_SELECTCLIPPATH");
    };
    assert_eq!(value.region_mode_kind(), Some(EmrRegionMode::Or));
    assert_eq!(parsed, select_clip);
    let mut invalid_select_clip = select_clip.clone();
    let EmfRecordData::SelectClipPath(value) = &mut invalid_select_clip else {
      unreachable!();
    };
    value.region_mode = 0xFFFF_FFFF;
    assert!(invalid_select_clip.to_record().is_err());
    let mut invalid_record = record.clone();
    invalid_record.data[0..4].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_record.parse_data().is_err());
  }

  #[test]
  fn typed_region_records_roundtrip() {
    let bounds = RectL {
      left: 0,
      top: 0,
      right: 10,
      bottom: 10,
    };
    let region_data = vec![
      32, 0, 0, 0, // HeaderSize
      1, 0, 0, 0, // Type
      1, 0, 0, 0, // CountRects
      16, 0, 0, 0, // RgnSize
      0, 0, 0, 0, // Bounds.left
      0, 0, 0, 0, // Bounds.top
      10, 0, 0, 0, // Bounds.right
      10, 0, 0, 0, // Bounds.bottom
      1, 0, 0, 0, // Rect.left
      2, 0, 0, 0, // Rect.top
      3, 0, 0, 0, // Rect.right
      4, 0, 0, 0, // Rect.bottom
    ];
    let mut header_reader = Reader::new(Cursor::new(region_data[..32].to_vec()));
    let header = RegionDataHeader::read_from(&mut header_reader).unwrap();
    assert_eq!(header.size, RegionDataHeader::SIZE);
    let mut invalid_header = header.clone();
    invalid_header.region_type = 2;
    let mut header_writer = Writer::new(Cursor::new(Vec::new()));
    assert!(invalid_header.write_to(&mut header_writer).is_err());
    let mut invalid_header_bytes = region_data[..32].to_vec();
    invalid_header_bytes[4..8].copy_from_slice(&2_u32.to_le_bytes());
    let mut header_reader = Reader::new(Cursor::new(invalid_header_bytes));
    assert!(RegionDataHeader::read_from(&mut header_reader).is_err());

    let values = [
      EmfRecordData::FillRgn(EmrFillRgn {
        bounds,
        brush_index: 3,
        region_data: region_data.clone(),
      }),
      EmfRecordData::FrameRgn(EmrFrameRgn {
        bounds,
        brush_index: 4,
        width: 5,
        height: 6,
        region_data: region_data.clone(),
      }),
      EmfRecordData::InvertRgn(EmrRgnDataRecord {
        bounds,
        region_data: region_data.clone(),
      }),
      EmfRecordData::PaintRgn(EmrRgnDataRecord {
        bounds,
        region_data: region_data.clone(),
      }),
      EmfRecordData::ExtSelectClipRgn(EmrExtSelectClipRgn {
        region_mode: 5,
        region_data: region_data.clone(),
      }),
    ];
    let expected_types = [
      EmfRecordType::FillRgn,
      EmfRecordType::FrameRgn,
      EmfRecordType::InvertRgn,
      EmfRecordType::PaintRgn,
      EmfRecordType::ExtSelectClipRgn,
    ];

    for (value, expected_type) in values.into_iter().zip(expected_types) {
      let record = value.to_record().unwrap();
      assert_eq!(record.record_type, expected_type.raw());
      assert_eq!(record.parse_data().unwrap(), value);
    }

    let zero_fill_brush = EmfRecordData::FillRgn(EmrFillRgn {
      bounds,
      brush_index: 0,
      region_data: region_data.clone(),
    });
    assert!(zero_fill_brush.to_record().is_err());
    let mut zero_fill_brush_record = EmfRecordData::FillRgn(EmrFillRgn {
      bounds,
      brush_index: 3,
      region_data: region_data.clone(),
    })
    .to_record()
    .unwrap();
    zero_fill_brush_record.data[20..24].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_fill_brush_record.parse_data().is_err());

    let zero_frame_brush = EmfRecordData::FrameRgn(EmrFrameRgn {
      bounds,
      brush_index: 0,
      width: 5,
      height: 6,
      region_data: region_data.clone(),
    });
    assert!(zero_frame_brush.to_record().is_err());
    let mut zero_frame_brush_record = EmfRecordData::FrameRgn(EmrFrameRgn {
      bounds,
      brush_index: 4,
      width: 5,
      height: 6,
      region_data: region_data.clone(),
    })
    .to_record()
    .unwrap();
    zero_frame_brush_record.data[20..24].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_frame_brush_record.parse_data().is_err());

    let invalid_fill_region = EmfRecordData::FillRgn(EmrFillRgn {
      bounds,
      brush_index: 3,
      region_data: vec![0; 32],
    });
    assert!(invalid_fill_region.to_record().is_err());
    let invalid_paint_region = EmfRecordData::PaintRgn(EmrRgnDataRecord {
      bounds,
      region_data: vec![0; 32],
    });
    assert!(invalid_paint_region.to_record().is_err());

    let ext_select = EmfRecordData::ExtSelectClipRgn(EmrExtSelectClipRgn {
      region_mode: EmrRegionMode::Copy.raw(),
      region_data: region_data.clone(),
    });
    let record = ext_select.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ExtSelectClipRgn(value) = &parsed else {
      panic!("expected EMR_EXTSELECTCLIPRGN");
    };
    assert_eq!(value.region_mode_kind(), Some(EmrRegionMode::Copy));
    let typed_region = value.typed_region_data().unwrap();
    assert_eq!(typed_region.header.size, RegionDataHeader::SIZE);
    assert_eq!(
      typed_region.header.region_type,
      RegionDataHeader::TYPE_RECTANGLES
    );
    assert_eq!(typed_region.rectangles.len(), 1);
    assert_eq!(
      typed_region.rectangles[0],
      RectL {
        left: 1,
        top: 2,
        right: 3,
        bottom: 4
      }
    );
    assert_eq!(typed_region.to_data().unwrap(), region_data);
    assert_eq!(parsed, ext_select);

    let copy_default_clip = EmfRecordData::ExtSelectClipRgn(EmrExtSelectClipRgn {
      region_mode: EmrRegionMode::Copy.raw(),
      region_data: Vec::new(),
    });
    let record = copy_default_clip.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), copy_default_clip);

    let invalid_empty_and = EmfRecordData::ExtSelectClipRgn(EmrExtSelectClipRgn {
      region_mode: EmrRegionMode::And.raw(),
      region_data: Vec::new(),
    });
    assert!(invalid_empty_and.to_record().is_err());
    let mut invalid_empty_and_record = record.clone();
    invalid_empty_and_record.data[4..8].copy_from_slice(&EmrRegionMode::And.raw().to_le_bytes());
    assert!(invalid_empty_and_record.parse_data().is_err());

    let mut invalid_mode = ext_select.clone();
    let EmfRecordData::ExtSelectClipRgn(value) = &mut invalid_mode else {
      unreachable!();
    };
    value.region_mode = 0xFFFF_FFFF;
    assert!(invalid_mode.to_record().is_err());
    let mut invalid_mode_record = ext_select.to_record().unwrap();
    invalid_mode_record.data[4..8].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_mode_record.parse_data().is_err());

    let invalid_region = EmfRecordData::ExtSelectClipRgn(EmrExtSelectClipRgn {
      region_mode: EmrRegionMode::Copy.raw(),
      region_data: vec![0; 32],
    });
    assert!(invalid_region.to_record().is_err());
  }

  #[test]
  fn typed_ext_text_out_w_record_roundtrips_without_decoding() {
    let text = SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le);
    let value = EmfRecordData::ExtTextOutW(EmrExtTextOut {
      bounds: RectL::default(),
      graphics_mode: 1,
      ex_scale: 1.0,
      ey_scale: 1.0,
      text: EmrText {
        reference: PointL { x: 12, y: 34 },
        options: ExtTextOutOptions::empty(),
        rectangle: None,
        text,
        undefined_space_before_string: Vec::new(),
        dx_buffer_present: false,
        undefined_space_before_dx: Vec::new(),
        dx: Vec::new(),
      },
      padding: Vec::new(),
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::ExtTextOutW.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::ExtTextOutW(parsed_value) = &parsed else {
      panic!("expected EMR_EXTTEXTOUTW");
    };
    assert_eq!(
      parsed_value.graphics_mode_kind(),
      Some(EmrGraphicsMode::Compatible)
    );
    assert_eq!(parsed, value);
    let mut invalid_options = record.clone();
    invalid_options.data[44..48].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
    assert!(invalid_options.parse_data().is_err());
    let mut invalid_graphics_mode = record.clone();
    invalid_graphics_mode.data[16..20].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_graphics_mode.parse_data().is_err());
    let mut misaligned_string = record.clone();
    misaligned_string.data[40..44].copy_from_slice(&59_u32.to_le_bytes());
    assert!(misaligned_string.parse_data().is_err());
    let rectangle_without_options = EmfRecordData::ExtTextOutW(EmrExtTextOut {
      bounds: RectL::default(),
      graphics_mode: EmrGraphicsMode::Compatible.raw(),
      ex_scale: 1.0,
      ey_scale: 1.0,
      text: EmrText {
        reference: PointL { x: 12, y: 34 },
        options: ExtTextOutOptions::empty(),
        rectangle: Some(RectL::default()),
        text: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
        undefined_space_before_string: Vec::new(),
        dx_buffer_present: false,
        undefined_space_before_dx: Vec::new(),
        dx: Vec::new(),
      },
      padding: Vec::new(),
    });
    let rectangle_record = rectangle_without_options.to_record().unwrap();
    assert_eq!(
      rectangle_record.parse_data().unwrap(),
      rectangle_without_options
    );
    let invalid_no_rect = EmfRecordData::ExtTextOutW(EmrExtTextOut {
      bounds: RectL::default(),
      graphics_mode: EmrGraphicsMode::Compatible.raw(),
      ex_scale: 1.0,
      ey_scale: 1.0,
      text: EmrText {
        reference: PointL { x: 12, y: 34 },
        options: ExtTextOutOptions::NO_RECT,
        rectangle: Some(RectL::default()),
        text: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
        undefined_space_before_string: Vec::new(),
        dx_buffer_present: false,
        undefined_space_before_dx: Vec::new(),
        dx: Vec::new(),
      },
      padding: Vec::new(),
    });
    let invalid_no_rect_record = invalid_no_rect.to_record().unwrap();
    assert_eq!(
      invalid_no_rect_record
        .parse_data()
        .unwrap()
        .to_record()
        .unwrap(),
      invalid_no_rect_record
    );
    assert!(invalid_no_rect.validate_strict().is_err());
    let invalid_dx = EmfRecordData::ExtTextOutW(EmrExtTextOut {
      bounds: RectL::default(),
      graphics_mode: EmrGraphicsMode::Compatible.raw(),
      ex_scale: 1.0,
      ey_scale: 1.0,
      text: EmrText {
        reference: PointL { x: 12, y: 34 },
        options: ExtTextOutOptions::PDY,
        rectangle: None,
        text: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
        undefined_space_before_string: Vec::new(),
        dx_buffer_present: true,
        undefined_space_before_dx: Vec::new(),
        dx: vec![1, 2],
      },
      padding: Vec::new(),
    });
    assert!(invalid_dx.to_record().is_err());
    let with_dx = EmfRecordData::ExtTextOutW(EmrExtTextOut {
      bounds: RectL::default(),
      graphics_mode: EmrGraphicsMode::Compatible.raw(),
      ex_scale: 1.0,
      ey_scale: 1.0,
      text: EmrText {
        reference: PointL { x: 12, y: 34 },
        options: ExtTextOutOptions::empty(),
        rectangle: None,
        text: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
        undefined_space_before_string: vec![0xAA, 0xBB, 0xCC, 0xDD],
        dx_buffer_present: true,
        undefined_space_before_dx: vec![0x11, 0x22, 0x33, 0x44],
        dx: vec![7, 8],
      },
      padding: Vec::new(),
    })
    .to_record()
    .unwrap();
    assert_eq!(with_dx.parse_data().unwrap().to_record().unwrap(), with_dx);
    let mut misaligned_dx = with_dx.clone();
    misaligned_dx.data[48..52].copy_from_slice(&62_u32.to_le_bytes());
    assert!(misaligned_dx.parse_data().is_err());
    let mut truncated_dx = record.clone();
    truncated_dx.data[36..40].copy_from_slice(&2_u32.to_le_bytes());
    truncated_dx.data[48..52].copy_from_slice(&60_u32.to_le_bytes());
    assert!(truncated_dx.parse_data().is_err());

    let padded_ansi = EmfRecordData::ExtTextOutA(EmrExtTextOut {
      bounds: RectL::default(),
      graphics_mode: EmrGraphicsMode::Compatible.raw(),
      ex_scale: 1.0,
      ey_scale: 1.0,
      text: EmrText {
        reference: PointL { x: 1, y: 2 },
        options: ExtTextOutOptions::empty(),
        rectangle: None,
        text: SdkString::raw(b"A".to_vec(), SdkEncoding::Windows1252),
        undefined_space_before_string: Vec::new(),
        dx_buffer_present: false,
        undefined_space_before_dx: Vec::new(),
        dx: Vec::new(),
      },
      padding: vec![0xAA, 0xBB, 0xCC],
    });
    let record = padded_ansi.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), padded_ansi);
    let mut trailing_ansi_record = record.clone();
    trailing_ansi_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_ansi_record.parse_data().is_err());
    let mut invalid_padding = padded_ansi.clone();
    let EmfRecordData::ExtTextOutA(value) = &mut invalid_padding else {
      unreachable!();
    };
    value.padding.push(0);
    assert!(invalid_padding.to_record().is_err());
  }

  #[test]
  fn typed_small_text_out_records_roundtrip() {
    let unicode = EmfRecordData::SmallTextOut(EmrSmallTextOut {
      reference: PointL { x: 12, y: 34 },
      options: ExtTextOutOptions::NO_RECT,
      graphics_mode: 1,
      ex_scale: 1.0,
      ey_scale: 1.0,
      bounds: None,
      text: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
      padding: Vec::new(),
    });
    let record = unicode.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SmallTextOut.raw());
    assert_eq!(record.parse_data().unwrap(), unicode);
    let mut trailing_unicode_record = record.clone();
    trailing_unicode_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_unicode_record.parse_data().is_err());

    let small_chars = EmfRecordData::SmallTextOut(EmrSmallTextOut {
      reference: PointL { x: -2, y: -4 },
      options: ExtTextOutOptions::SMALL_CHARS | ExtTextOutOptions::CLIPPED,
      graphics_mode: 2,
      ex_scale: 1.25,
      ey_scale: 0.75,
      bounds: Some(RectL {
        left: 1,
        top: 2,
        right: 30,
        bottom: 40,
      }),
      text: SdkString::raw(b"Ansi".to_vec(), SdkEncoding::UnicodeLowByte),
      padding: Vec::new(),
    });
    let record = small_chars.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::SmallTextOut.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::SmallTextOut(value) = &parsed else {
      panic!("expected EMR_SMALLTEXTOUT");
    };
    assert_eq!(value.graphics_mode_kind(), Some(EmrGraphicsMode::Advanced));
    assert_eq!(parsed, small_chars);

    let padded_small_chars = EmfRecordData::SmallTextOut(EmrSmallTextOut {
      reference: PointL { x: 1, y: 2 },
      options: ExtTextOutOptions::SMALL_CHARS | ExtTextOutOptions::NO_RECT,
      graphics_mode: EmrGraphicsMode::Compatible.raw(),
      ex_scale: 1.0,
      ey_scale: 1.0,
      bounds: None,
      text: SdkString::raw(b"Odd".to_vec(), SdkEncoding::UnicodeLowByte),
      padding: vec![0],
    });
    let record = padded_small_chars.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), padded_small_chars);
    let mut trailing_small_chars_record = record.clone();
    trailing_small_chars_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_small_chars_record.parse_data().is_err());

    let mut invalid_options = record.clone();
    invalid_options.data[12..16].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
    assert!(invalid_options.parse_data().is_err());
    let mut invalid_graphics_mode = record.clone();
    invalid_graphics_mode.data[16..20].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_graphics_mode.parse_data().is_err());
    let invalid_bounds = EmfRecordData::SmallTextOut(EmrSmallTextOut {
      reference: PointL { x: 12, y: 34 },
      options: ExtTextOutOptions::NO_RECT,
      graphics_mode: EmrGraphicsMode::Compatible.raw(),
      ex_scale: 1.0,
      ey_scale: 1.0,
      bounds: Some(RectL::default()),
      text: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
      padding: Vec::new(),
    });
    assert!(invalid_bounds.to_record().is_err());
  }

  #[test]
  fn typed_poly_text_out_records_roundtrip() {
    let bounds = RectL {
      left: 0,
      top: 0,
      right: 200,
      bottom: 100,
    };
    let ansi = EmfRecordData::PolyTextOutA(EmrPolyTextOut {
      bounds,
      graphics_mode: 1,
      ex_scale: 1.0,
      ey_scale: 1.0,
      texts: vec![
        EmrText {
          reference: PointL { x: 10, y: 20 },
          options: ExtTextOutOptions::CLIPPED,
          rectangle: Some(RectL {
            left: 5,
            top: 6,
            right: 70,
            bottom: 26,
          }),
          text: SdkString::raw(b"ABC".to_vec(), SdkEncoding::Windows1252),
          undefined_space_before_string: vec![0x11, 0x12, 0x13, 0x14],
          dx_buffer_present: true,
          undefined_space_before_dx: vec![0x21],
          dx: vec![8, 9, 10],
        },
        EmrText {
          reference: PointL { x: 80, y: 20 },
          options: ExtTextOutOptions::empty(),
          rectangle: None,
          text: SdkString::raw(b"Z".to_vec(), SdkEncoding::Windows1252),
          undefined_space_before_string: vec![0x31, 0x32, 0x33, 0x34],
          dx_buffer_present: false,
          undefined_space_before_dx: Vec::new(),
          dx: Vec::new(),
        },
      ],
      padding: vec![0; 3],
    });
    let record = ansi.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyTextOutA.raw());
    assert_eq!(record.parse_data().unwrap(), ansi);
    let mut trailing_ansi_record = record.clone();
    trailing_ansi_record
      .data
      .extend_from_slice(&0_u32.to_le_bytes());
    assert!(trailing_ansi_record.parse_data().is_err());
    let mut invalid_ansi_padding = ansi.clone();
    let EmfRecordData::PolyTextOutA(value) = &mut invalid_ansi_padding else {
      unreachable!();
    };
    value.padding.push(0);
    assert!(invalid_ansi_padding.to_record().is_err());

    let wide = EmfRecordData::PolyTextOutW(EmrPolyTextOut {
      bounds,
      graphics_mode: 2,
      ex_scale: 0.5,
      ey_scale: 1.5,
      texts: vec![EmrText {
        reference: PointL { x: -10, y: -20 },
        options: ExtTextOutOptions::PDY,
        rectangle: None,
        text: SdkString::raw(vec![b'H', 0, b'i', 0], SdkEncoding::Utf16Le),
        undefined_space_before_string: vec![0x41, 0x42],
        dx_buffer_present: true,
        undefined_space_before_dx: vec![0x51, 0x52],
        dx: vec![7, 1, 8, 2],
      }],
      padding: Vec::new(),
    });
    let record = wide.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PolyTextOutW.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::PolyTextOutW(value) = &parsed else {
      panic!("expected EMR_POLYTEXTOUTW");
    };
    assert_eq!(value.graphics_mode_kind(), Some(EmrGraphicsMode::Advanced));
    assert_eq!(parsed, wide);
    let mut misaligned_poly_string = record.clone();
    misaligned_poly_string.data[44..48].copy_from_slice(&57_u32.to_le_bytes());
    assert!(misaligned_poly_string.parse_data().is_err());

    let mut oversized_string_count_data = Vec::new();
    {
      let mut writer = Writer::new(&mut oversized_string_count_data);
      bounds.write_to(&mut writer).unwrap();
    }
    oversized_string_count_data.extend_from_slice(&1_u32.to_le_bytes());
    oversized_string_count_data.extend_from_slice(&1.0_f32.to_le_bytes());
    oversized_string_count_data.extend_from_slice(&1.0_f32.to_le_bytes());
    oversized_string_count_data.extend_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(
      EmfRecord::new(
        EmfRecordType::PolyTextOutW.raw(),
        oversized_string_count_data
      )
      .parse_data()
      .is_err()
    );
  }

  #[test]
  fn typed_stretch_dibits_record_roundtrips() {
    let value = EmfRecordData::StretchDiBits(EmrStretchDiBits {
      bounds: RectL::default(),
      dest: PointL { x: 1, y: 2 },
      source: BitmapSourceBounds {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
      },
      color_usage: 0,
      raster_operation: 0x00CC_0020,
      dest_size: SizeL { cx: 2, cy: 2 },
      bitmap: EmrBitmapBuffer {
        undefined_space_before_bitmap_info: vec![0xA1, 0xA2, 0xA3, 0xA4],
        bitmap_info: vec![
          40, 0, 0, 0, // HeaderSize
          2, 0, 0, 0, // Width
          0xFE, 0xFF, 0xFF, 0xFF, // Height = -2
          1, 0, // Planes
          24, 0, // BitCount
          0, 0, 0, 0, // BI_RGB
          0, 0, 0, 0, // ImageSize
          0, 0, 0, 0, // XPelsPerMeter
          0, 0, 0, 0, // YPelsPerMeter
          0, 0, 0, 0, // ColorUsed
          0, 0, 0, 0, // ColorImportant
        ],
        undefined_space_before_bitmap_bits: vec![0xB1, 0xB2, 0xB3, 0xB4],
        bitmap_bits: vec![1, 2, 3],
      },
      padding: vec![0xCC],
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::StretchDiBits.raw());
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, value);
    let EmfRecordData::StretchDiBits(parsed) = parsed else {
      unreachable!();
    };
    assert_eq!(
      parsed.color_usage_kind(),
      Some(crate::bitmap::DibColorUsage::RgbColors)
    );
    assert_eq!(
      parsed.raster_operation_code(),
      WmfTernaryRasterOperationCode::SRCCOPY
    );
    let mut invalid_stretch_dibits = value.clone();
    let EmfRecordData::StretchDiBits(value) = &mut invalid_stretch_dibits else {
      unreachable!();
    };
    value.color_usage = 0xFFFF_FFFF;
    assert!(invalid_stretch_dibits.to_record().is_err());
    let mut invalid_stretch_dibits_record = record.clone();
    invalid_stretch_dibits_record.data[56..60].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_stretch_dibits_record.parse_data().is_err());
    let info = parsed.bitmap.dib_info().unwrap();
    assert_eq!(
      info.compression_kind(),
      Some(crate::bitmap::BitmapCompression::Rgb)
    );
    assert!(info.header.is_top_down());
  }

  #[test]
  fn typed_bit_blt_records_roundtrip() {
    let xform_source = XForm {
      m11: 1.0,
      m12: 0.0,
      m21: 0.0,
      m22: 1.0,
      dx: 0.0,
      dy: 0.0,
    };
    let no_source = EmfRecordData::BitBlt(EmrBitBlt {
      bounds: RectL::default(),
      dest: PointL { x: 1, y: 2 },
      dest_size: SizeL { cx: 3, cy: 4 },
      raster_operation: 0x00F0_0021,
      source: PointL { x: 5, y: 6 },
      xform_source,
      background_color_source: ColorRef {
        red: 7,
        green: 8,
        blue: 9,
        reserved: 0,
      },
      color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      bitmap: None,
      padding: Vec::new(),
    });
    let record = no_source.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::BitBlt.raw());
    assert_eq!(record.data.len(), 92);
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, no_source);
    let EmfRecordData::BitBlt(parsed) = parsed else {
      unreachable!();
    };
    assert_eq!(
      parsed.raster_operation_code(),
      WmfTernaryRasterOperationCode::PATCOPY
    );
    assert_eq!(parsed.ternary_raster_operation().raw(), 0x00F0_0021);
    let mut invalid_source_rop = no_source.clone();
    let EmfRecordData::BitBlt(value) = &mut invalid_source_rop else {
      unreachable!();
    };
    value.raster_operation = 0x00CC_0020;
    assert!(invalid_source_rop.to_record().is_err());
    let mut invalid_source_rop_record = record.clone();
    invalid_source_rop_record.data[32..36].copy_from_slice(&0x00CC_0020_u32.to_le_bytes());
    assert!(invalid_source_rop_record.parse_data().is_err());
    let mut invalid_bit_blt = no_source.clone();
    let EmfRecordData::BitBlt(value) = &mut invalid_bit_blt else {
      unreachable!();
    };
    value.color_usage = 0xFFFF_FFFF;
    assert!(invalid_bit_blt.to_record().is_err());
    let mut invalid_bit_blt_record = record.clone();
    invalid_bit_blt_record.data[72..76].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_bit_blt_record.parse_data().is_err());
    let mut trailing_no_source_record = record.clone();
    trailing_no_source_record
      .data
      .extend_from_slice(&0xAA_BB_CC_DD_u32.to_le_bytes());
    assert!(trailing_no_source_record.parse_data().is_err());

    let bitmap_info = vec![
      40, 0, 0, 0, // HeaderSize
      2, 0, 0, 0, // Width
      2, 0, 0, 0, // Height
      1, 0, // Planes
      24, 0, // BitCount
      0, 0, 0, 0, // BI_RGB
      0, 0, 0, 0, // ImageSize
      0, 0, 0, 0, // XPelsPerMeter
      0, 0, 0, 0, // YPelsPerMeter
      0, 0, 0, 0, // ColorUsed
      0, 0, 0, 0, // ColorImportant
    ];
    let stretch = EmfRecordData::StretchBlt(EmrStretchBlt {
      bounds: RectL::default(),
      dest: PointL { x: 1, y: 2 },
      dest_size: SizeL { cx: 3, cy: 4 },
      raster_operation: 0x00CC_0020,
      source: PointL { x: 5, y: 6 },
      xform_source,
      background_color_source: ColorRef {
        red: 10,
        green: 20,
        blue: 30,
        reserved: 0,
      },
      color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      source_size: SizeL { cx: 2, cy: 2 },
      bitmap: Some(EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info,
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits: vec![1, 2, 3, 4],
      }),
      padding: Vec::new(),
    });
    let record = stretch.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::StretchBlt.raw());
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, stretch);
    let mut trailing_bitmap_record = record.clone();
    trailing_bitmap_record
      .data
      .extend_from_slice(&0xAA_BB_CC_DD_u32.to_le_bytes());
    assert!(trailing_bitmap_record.parse_data().is_err());
    let mut no_bitmap_header_record = record.clone();
    no_bitmap_header_record.data[76..80].copy_from_slice(&0_u32.to_le_bytes());
    no_bitmap_header_record.data[80..84].copy_from_slice(&0_u32.to_le_bytes());
    let EmfRecordData::StretchBlt(no_bitmap_header) = no_bitmap_header_record.parse_data().unwrap()
    else {
      unreachable!();
    };
    let bitmap = no_bitmap_header.bitmap.unwrap();
    assert_eq!(bitmap.bitmap_info, Vec::<u8>::new());
    assert_eq!(bitmap.bitmap_bits, vec![1, 2, 3, 4]);
    let EmfRecordData::StretchBlt(parsed) = parsed else {
      unreachable!();
    };
    assert_eq!(
      parsed.color_usage_kind(),
      Some(crate::bitmap::DibColorUsage::RgbColors)
    );
    assert_eq!(
      parsed.raster_operation_code(),
      WmfTernaryRasterOperationCode::SRCCOPY
    );
    assert_eq!(parsed.bitmap.unwrap().bitmap_bits, [1, 2, 3, 4]);
    let mut no_source_stretch = stretch.clone();
    let EmfRecordData::StretchBlt(value) = &mut no_source_stretch else {
      unreachable!();
    };
    value.raster_operation = 0x00F0_0021;
    value.bitmap = None;
    let no_source_stretch_record = no_source_stretch.to_record().unwrap();
    assert_eq!(no_source_stretch_record.data.len(), 100);
    let mut invalid_source_stretch = no_source_stretch.clone();
    let EmfRecordData::StretchBlt(value) = &mut invalid_source_stretch else {
      unreachable!();
    };
    value.raster_operation = 0x00CC_0020;
    assert!(invalid_source_stretch.to_record().is_err());
    let mut invalid_source_stretch_record = no_source_stretch_record.clone();
    invalid_source_stretch_record.data[32..36].copy_from_slice(&0x00CC_0020_u32.to_le_bytes());
    assert!(invalid_source_stretch_record.parse_data().is_err());
    let mut invalid_stretch_blt = stretch.clone();
    let EmfRecordData::StretchBlt(value) = &mut invalid_stretch_blt else {
      unreachable!();
    };
    value.color_usage = 0xFFFF_FFFF;
    assert!(invalid_stretch_blt.to_record().is_err());
    let mut invalid_stretch_blt_record = record.clone();
    invalid_stretch_blt_record.data[72..76].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_stretch_blt_record.parse_data().is_err());

    let alpha_bitmap = rgb_bitmap(
      2,
      2,
      32,
      vec![
        0x10, 0x20, 0x30, 0x80, 0x11, 0x21, 0x31, 0x80, 0x12, 0x22, 0x32, 0x80, 0x13, 0x23, 0x33,
        0x80,
      ],
    );
    let alpha = EmfRecordData::AlphaBlend(EmrAlphaBlend {
      bounds: RectL::default(),
      dest: PointL { x: 1, y: 2 },
      dest_size: SizeL { cx: 3, cy: 4 },
      blend_function: EmrBlendFunction {
        blend_operation: 0,
        blend_flags: 0,
        source_constant_alpha: 0x80,
        alpha_format: 1,
      },
      source: PointL { x: 5, y: 6 },
      xform_source,
      background_color_source: ColorRef {
        red: 1,
        green: 2,
        blue: 3,
        reserved: 0,
      },
      color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      source_size: SizeL { cx: 2, cy: 2 },
      bitmap: Some(alpha_bitmap),
      padding: Vec::new(),
    });
    let record = alpha.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::AlphaBlend.raw());
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, alpha);
    let EmfRecordData::AlphaBlend(parsed) = parsed else {
      unreachable!();
    };
    assert_eq!(
      parsed.blend_function.blend_operation_kind(),
      Some(EmrBlendOperation::SourceOver)
    );
    assert_eq!(
      parsed.blend_function.alpha_format_kind(),
      Some(EmrAlphaFormat::SourceAlpha)
    );
    let mut invalid_alpha_size = alpha.clone();
    let EmfRecordData::AlphaBlend(invalid_alpha) = &mut invalid_alpha_size else {
      unreachable!();
    };
    invalid_alpha.dest_size.cx = 0;
    assert!(invalid_alpha_size.to_record().is_err());
    let mut invalid_alpha_size_record = record.clone();
    invalid_alpha_size_record.data[24..28].copy_from_slice(&0_i32.to_le_bytes());
    assert!(invalid_alpha_size_record.parse_data().is_err());
    let mut invalid_alpha_source_size = alpha.clone();
    let EmfRecordData::AlphaBlend(invalid_alpha) = &mut invalid_alpha_source_size else {
      unreachable!();
    };
    invalid_alpha.source_size.cx = 0;
    assert!(invalid_alpha_source_size.to_record().is_err());
    let mut invalid_alpha_source_size_record = record.clone();
    invalid_alpha_source_size_record.data[92..96].copy_from_slice(&0_i32.to_le_bytes());
    assert!(invalid_alpha_source_size_record.parse_data().is_err());
    let mut invalid_alpha_operation = alpha.clone();
    let EmfRecordData::AlphaBlend(invalid_alpha) = &mut invalid_alpha_operation else {
      unreachable!();
    };
    invalid_alpha.blend_function.blend_operation = 0xFF;
    assert!(invalid_alpha_operation.to_record().is_err());
    let mut ignored_alpha_flags = alpha.clone();
    let EmfRecordData::AlphaBlend(ignored_alpha) = &mut ignored_alpha_flags else {
      unreachable!();
    };
    ignored_alpha.blend_function.blend_flags = 1;
    let ignored_alpha_flags_record = ignored_alpha_flags.to_record().unwrap();
    assert_eq!(ignored_alpha_flags_record.data[33], 1);
    assert_eq!(
      ignored_alpha_flags_record.parse_data().unwrap(),
      ignored_alpha_flags
    );
    let mut invalid_alpha_format_record = record.clone();
    invalid_alpha_format_record.data[35] = 0xFF;
    assert!(invalid_alpha_format_record.parse_data().is_err());
    let mut invalid_alpha_color_usage = alpha.clone();
    let EmfRecordData::AlphaBlend(invalid_alpha) = &mut invalid_alpha_color_usage else {
      unreachable!();
    };
    invalid_alpha.color_usage = 0xFFFF_FFFF;
    assert!(invalid_alpha_color_usage.to_record().is_err());
    let mut missing_alpha_bitmap = alpha.clone();
    let EmfRecordData::AlphaBlend(missing_alpha) = &mut missing_alpha_bitmap else {
      unreachable!();
    };
    missing_alpha.bitmap = None;
    assert!(missing_alpha_bitmap.to_record().is_err());
    let mut missing_alpha_bitmap_record = record.clone();
    missing_alpha_bitmap_record.data.truncate(100);
    missing_alpha_bitmap_record.data[76..92].fill(0);
    assert!(missing_alpha_bitmap_record.parse_data().is_err());

    let transparent = EmfRecordData::TransparentBlt(EmrTransparentBlt {
      bounds: RectL::default(),
      dest: PointL { x: 1, y: 2 },
      dest_size: SizeL { cx: 3, cy: 4 },
      transparent_color: ColorRef {
        red: 0x10,
        green: 0x20,
        blue: 0x30,
        reserved: 0,
      },
      source: PointL { x: 5, y: 6 },
      xform_source,
      background_color_source: ColorRef {
        red: 4,
        green: 5,
        blue: 6,
        reserved: 0,
      },
      color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      source_size: SizeL { cx: 2, cy: 2 },
      bitmap: Some(EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info: vec![
          40, 0, 0, 0, // HeaderSize
          1, 0, 0, 0, // Width
          1, 0, 0, 0, // Height
          1, 0, // Planes
          32, 0, // BitCount
          0, 0, 0, 0, // BI_RGB
          0, 0, 0, 0, // ImageSize
          0, 0, 0, 0, // XPelsPerMeter
          0, 0, 0, 0, // YPelsPerMeter
          0, 0, 0, 0, // ColorUsed
          0, 0, 0, 0, // ColorImportant
        ],
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits: vec![0xAA, 0xBB, 0xCC, 0xDD],
      }),
      padding: Vec::new(),
    });
    let record = transparent.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::TransparentBlt.raw());
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, transparent);
    let mut invalid_transparent_color_usage = transparent.clone();
    let EmfRecordData::TransparentBlt(invalid_transparent) = &mut invalid_transparent_color_usage
    else {
      unreachable!();
    };
    invalid_transparent.color_usage = 0xFFFF_FFFF;
    assert!(invalid_transparent_color_usage.to_record().is_err());
    let mut invalid_transparent_record = record.clone();
    invalid_transparent_record.data[72..76].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_transparent_record.parse_data().is_err());
    let mut invalid_transparent_reserved = transparent.clone();
    let EmfRecordData::TransparentBlt(invalid_transparent) = &mut invalid_transparent_reserved
    else {
      unreachable!();
    };
    invalid_transparent.transparent_color.reserved = 1;
    assert!(invalid_transparent_reserved.to_record().is_err());
    let mut invalid_transparent_reserved_record = record.clone();
    invalid_transparent_reserved_record.data[35] = 1;
    assert!(invalid_transparent_reserved_record.parse_data().is_err());
    let mut missing_transparent_bitmap = transparent.clone();
    let EmfRecordData::TransparentBlt(missing_transparent) = &mut missing_transparent_bitmap else {
      unreachable!();
    };
    missing_transparent.bitmap = None;
    assert!(missing_transparent_bitmap.to_record().is_err());
    let mut missing_transparent_bitmap_record = record.clone();
    missing_transparent_bitmap_record.data.truncate(100);
    missing_transparent_bitmap_record.data[76..92].fill(0);
    assert!(missing_transparent_bitmap_record.parse_data().is_err());
  }

  #[test]
  fn typed_mask_and_plg_blt_records_roundtrip() {
    let mut source_bitmap = rgb_bitmap(2, 2, 24, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    source_bitmap.undefined_space_before_bitmap_info = vec![0x11, 0x12, 0x13, 0x14];
    source_bitmap.undefined_space_before_bitmap_bits = vec![0x21, 0x22, 0x23, 0x24];
    let mut mask_bitmap = rgb_bitmap(2, 2, 1, vec![0x80, 0, 0, 0, 0x40, 0, 0, 0]);
    mask_bitmap.undefined_space_before_bitmap_info = vec![0x31, 0x32, 0x33, 0x34];
    mask_bitmap.undefined_space_before_bitmap_bits = vec![0x41, 0x42, 0x43, 0x44];
    let mask_blt = EmfRecordData::MaskBlt(EmrMaskBlt {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 20,
        bottom: 20,
      },
      dest: PointL { x: 1, y: 2 },
      dest_size: SizeL { cx: 3, cy: 4 },
      raster_operation: EmrRop4 {
        reserved: 0,
        background_rop3: 0xAA,
        foreground_rop3: 0xCC,
      },
      source: PointL { x: 5, y: 6 },
      xform_source: identity_xform(),
      background_color_source: ColorRef {
        red: 7,
        green: 8,
        blue: 9,
        reserved: 0,
      },
      source_color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      source_bitmap: Some(source_bitmap.clone()),
      mask: PointL { x: 0, y: 1 },
      mask_color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      mask_bitmap: Some(mask_bitmap.clone()),
      bitmap_order: EmrBitmapBufferOrder::SourceThenMask,
      padding: Vec::new(),
    });
    let record = mask_blt.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::MaskBlt.raw());
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, mask_blt);
    let EmfRecordData::MaskBlt(parsed) = parsed else {
      unreachable!();
    };
    assert_eq!(
      parsed.source_color_usage_kind(),
      Some(crate::bitmap::DibColorUsage::RgbColors)
    );
    assert_eq!(
      parsed.mask_color_usage_kind(),
      Some(crate::bitmap::DibColorUsage::RgbColors)
    );
    assert_eq!(
      parsed.raster_operation.background_rop3_code(),
      WmfTernaryRasterOperationCode::D
    );
    assert_eq!(
      parsed.raster_operation.foreground_rop3_code(),
      WmfTernaryRasterOperationCode::SRCCOPY
    );
    let mut invalid_mask_blt = mask_blt.clone();
    let EmfRecordData::MaskBlt(value) = &mut invalid_mask_blt else {
      unreachable!();
    };
    value.source_color_usage = 0xFFFF_FFFF;
    assert!(invalid_mask_blt.to_record().is_err());
    let mut invalid_mask_blt_record = record.clone();
    invalid_mask_blt_record.data[100..104].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_mask_blt_record.parse_data().is_err());
    let mut missing_mask_blt_source = mask_blt.clone();
    let EmfRecordData::MaskBlt(value) = &mut missing_mask_blt_source else {
      unreachable!();
    };
    value.source_bitmap = None;
    assert!(missing_mask_blt_source.to_record().is_err());
    let mut missing_mask_blt_source_record = record.clone();
    missing_mask_blt_source_record.data[76..92].fill(0);
    assert!(missing_mask_blt_source_record.parse_data().is_err());
    let mut missing_mask_blt_mask = mask_blt.clone();
    let EmfRecordData::MaskBlt(value) = &mut missing_mask_blt_mask else {
      unreachable!();
    };
    value.mask_bitmap = None;
    assert!(missing_mask_blt_mask.to_record().is_err());
    let mut missing_mask_blt_mask_record = record.clone();
    missing_mask_blt_mask_record.data[104..120].fill(0);
    assert!(missing_mask_blt_mask_record.parse_data().is_err());
    let mut invalid_mask_blt_mask = mask_blt.clone();
    let EmfRecordData::MaskBlt(value) = &mut invalid_mask_blt_mask else {
      unreachable!();
    };
    value.mask_bitmap.as_mut().unwrap().bitmap_info[14..16].copy_from_slice(&24u16.to_le_bytes());
    assert!(invalid_mask_blt_mask.to_record().is_err());
    let mut invalid_mask_blt_mask_record = record.clone();
    let mask_bmi_offset = u32::from_le_bytes(
      invalid_mask_blt_mask_record.data[104..108]
        .try_into()
        .unwrap(),
    ) as usize
      - 8;
    invalid_mask_blt_mask_record.data[mask_bmi_offset + 14..mask_bmi_offset + 16]
      .copy_from_slice(&24u16.to_le_bytes());
    assert!(invalid_mask_blt_mask_record.parse_data().is_err());

    let mut mask_first = mask_blt.clone();
    let EmfRecordData::MaskBlt(value) = &mut mask_first else {
      unreachable!();
    };
    value.bitmap_order = EmrBitmapBufferOrder::MaskThenSource;
    let mask_first_record = mask_first.to_record().unwrap();
    let source_offset = u32::from_le_bytes(mask_first_record.data[76..80].try_into().unwrap());
    let mask_offset = u32::from_le_bytes(mask_first_record.data[104..108].try_into().unwrap());
    assert!(mask_offset < source_offset);
    let parsed = mask_first_record.parse_data().unwrap();
    assert_eq!(parsed, mask_first);
    assert_eq!(parsed.to_record().unwrap(), mask_first_record);

    let plg_blt = EmfRecordData::PlgBlt(EmrPlgBlt {
      bounds: RectL {
        left: -10,
        top: -10,
        right: 30,
        bottom: 30,
      },
      dest: [
        PointL { x: 0, y: 0 },
        PointL { x: 20, y: 2 },
        PointL { x: 2, y: 20 },
      ],
      source: BitmapSourceBounds {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
      },
      xform_source: identity_xform(),
      background_color_source: ColorRef {
        red: 10,
        green: 11,
        blue: 12,
        reserved: 0,
      },
      source_color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      source_bitmap: Some(source_bitmap),
      mask: PointL { x: 1, y: 1 },
      mask_color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      mask_bitmap: Some(mask_bitmap),
      bitmap_order: EmrBitmapBufferOrder::SourceThenMask,
      padding: Vec::new(),
    });
    let record = plg_blt.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::PlgBlt.raw());
    assert_eq!(record.parse_data().unwrap(), plg_blt);
    let mut invalid_plg_blt = plg_blt.clone();
    let EmfRecordData::PlgBlt(value) = &mut invalid_plg_blt else {
      unreachable!();
    };
    value.mask_color_usage = 0xFFFF_FFFF;
    assert!(invalid_plg_blt.to_record().is_err());
    let mut invalid_plg_blt_record = record.clone();
    invalid_plg_blt_record.data[84..88].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_plg_blt_record.parse_data().is_err());
    let mut missing_plg_blt_source = plg_blt.clone();
    let EmfRecordData::PlgBlt(value) = &mut missing_plg_blt_source else {
      unreachable!();
    };
    value.source_bitmap = None;
    assert!(missing_plg_blt_source.to_record().is_err());
    let mut missing_plg_blt_source_record = record.clone();
    missing_plg_blt_source_record.data[88..104].fill(0);
    assert!(missing_plg_blt_source_record.parse_data().is_err());
    let mut missing_plg_blt_mask = plg_blt.clone();
    let EmfRecordData::PlgBlt(value) = &mut missing_plg_blt_mask else {
      unreachable!();
    };
    value.mask_bitmap = None;
    assert!(missing_plg_blt_mask.validate_strict().is_err());
    let missing_plg_blt_mask_record = missing_plg_blt_mask.to_record().unwrap();
    let parsed = missing_plg_blt_mask_record.parse_data().unwrap();
    assert_eq!(parsed.to_record().unwrap(), missing_plg_blt_mask_record);
    assert!(parsed.validate_strict().is_err());
    let mut invalid_plg_blt_mask = plg_blt.clone();
    let EmfRecordData::PlgBlt(value) = &mut invalid_plg_blt_mask else {
      unreachable!();
    };
    value.mask_bitmap.as_mut().unwrap().bitmap_info[14..16].copy_from_slice(&24u16.to_le_bytes());
    assert!(invalid_plg_blt_mask.to_record().is_ok());
    assert!(invalid_plg_blt_mask.validate_strict().is_err());
    let mut invalid_plg_blt_mask_record = record.clone();
    let mask_bmi_offset = u32::from_le_bytes(
      invalid_plg_blt_mask_record.data[116..120]
        .try_into()
        .unwrap(),
    ) as usize
      - 8;
    invalid_plg_blt_mask_record.data[mask_bmi_offset + 14..mask_bmi_offset + 16]
      .copy_from_slice(&24u16.to_le_bytes());
    let parsed = invalid_plg_blt_mask_record.parse_data().unwrap();
    assert_eq!(parsed.to_record().unwrap(), invalid_plg_blt_mask_record);
    assert!(parsed.validate_strict().is_err());

    let mut mask_first = plg_blt.clone();
    let EmfRecordData::PlgBlt(value) = &mut mask_first else {
      unreachable!();
    };
    value.bitmap_order = EmrBitmapBufferOrder::MaskThenSource;
    let mask_first_record = mask_first.to_record().unwrap();
    let source_offset = u32::from_le_bytes(mask_first_record.data[88..92].try_into().unwrap());
    let mask_offset = u32::from_le_bytes(mask_first_record.data[116..120].try_into().unwrap());
    assert!(mask_offset < source_offset);
    let parsed = mask_first_record.parse_data().unwrap();
    assert_eq!(parsed, mask_first);
    assert_eq!(parsed.to_record().unwrap(), mask_first_record);
  }

  #[test]
  fn typed_create_dib_pattern_brush_roundtrips() {
    let bitmap_info = vec![
      40, 0, 0, 0, // HeaderSize
      2, 0, 0, 0, // Width
      2, 0, 0, 0, // Height
      1, 0, // Planes
      0, 0, // BitCount
      5, 0, 0, 0, // BI_PNG
      4, 0, 0, 0, // ImageSize
      0, 0, 0, 0, // XPelsPerMeter
      0, 0, 0, 0, // YPelsPerMeter
      0, 0, 0, 0, // ColorUsed
      0, 0, 0, 0, // ColorImportant
    ];
    let value = EmfRecordData::CreateDibPatternBrushPt(EmrCreateDibPatternBrushPt {
      brush_index: 3,
      color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      bitmap: EmrBitmapBuffer {
        undefined_space_before_bitmap_info: vec![0xAA, 0xBB, 0xCC, 0xDD],
        bitmap_info,
        undefined_space_before_bitmap_bits: vec![0x11, 0x22, 0x33, 0x44],
        bitmap_bits: vec![0x89, b'P', b'N', b'G'],
      },
      padding: Vec::new(),
    });

    let record = value.to_record().unwrap();
    assert_eq!(
      record.record_type,
      EmfRecordType::CreateDibPatternBrushPt.raw()
    );
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, value);
    let EmfRecordData::CreateDibPatternBrushPt(parsed) = parsed else {
      unreachable!();
    };
    assert_eq!(
      parsed.color_usage_kind(),
      Some(crate::bitmap::DibColorUsage::RgbColors)
    );
    assert_eq!(
      parsed
        .bitmap
        .device_independent_bitmap()
        .unwrap()
        .embedded_format(),
      Some(crate::bitmap::EmbeddedBitmapFormat::Png)
    );
    let mut zero_pattern_brush_index = value.clone();
    let EmfRecordData::CreateDibPatternBrushPt(record_value) = &mut zero_pattern_brush_index else {
      unreachable!();
    };
    record_value.brush_index = 0;
    assert!(zero_pattern_brush_index.to_record().is_err());
    let mut zero_pattern_brush_index_record = record.clone();
    zero_pattern_brush_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_pattern_brush_index_record.parse_data().is_err());
    let mut invalid_pattern_brush = value.clone();
    let EmfRecordData::CreateDibPatternBrushPt(value) = &mut invalid_pattern_brush else {
      unreachable!();
    };
    value.color_usage = 0xFFFF_FFFF;
    assert!(invalid_pattern_brush.to_record().is_err());
    let mut invalid_pattern_record = record.clone();
    invalid_pattern_record.data[4..8].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    assert!(invalid_pattern_record.parse_data().is_err());
  }

  #[test]
  fn typed_create_mono_brush_roundtrips() {
    let value = EmfRecordData::CreateMonoBrush(EmrCreateMonoBrush {
      brush_index: 4,
      color_usage: crate::bitmap::DibColorUsage::RgbColors.raw(),
      bitmap: EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info: vec![
          40, 0, 0, 0, // HeaderSize
          2, 0, 0, 0, // Width
          2, 0, 0, 0, // Height
          1, 0, // Planes
          1, 0, // BitCount
          0, 0, 0, 0, // BI_RGB
          0, 0, 0, 0, // ImageSize
          0, 0, 0, 0, // XPelsPerMeter
          0, 0, 0, 0, // YPelsPerMeter
          0, 0, 0, 0, // ColorUsed
          0, 0, 0, 0, // ColorImportant
          0, 0, 0, 0, // RGBQuad black
          0xFF, 0xFF, 0xFF, 0, // RGBQuad white
        ],
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits: vec![0x80, 0, 0, 0, 0x40, 0, 0, 0],
      },
      padding: Vec::new(),
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::CreateMonoBrush.raw());
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, value);
    let EmfRecordData::CreateMonoBrush(parsed) = parsed else {
      unreachable!();
    };
    assert_eq!(
      parsed.bitmap.dib_info().unwrap().header.bit_count_kind(),
      Some(crate::bitmap::BitmapBitCount::One)
    );
    let mut zero_mono_brush_index = value.clone();
    let EmfRecordData::CreateMonoBrush(record_value) = &mut zero_mono_brush_index else {
      unreachable!();
    };
    record_value.brush_index = 0;
    assert!(zero_mono_brush_index.to_record().is_err());
    let mut zero_mono_brush_index_record = record.clone();
    zero_mono_brush_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_mono_brush_index_record.parse_data().is_err());
    let mut invalid_mono_brush = value.clone();
    let EmfRecordData::CreateMonoBrush(value) = &mut invalid_mono_brush else {
      unreachable!();
    };
    value.color_usage = 0xFFFF_FFFF;
    assert!(invalid_mono_brush.to_record().is_err());
  }

  #[test]
  fn typed_create_color_space_records_roundtrip() {
    fn fixed_bytes(bytes: &[u8], len: usize) -> Vec<u8> {
      let mut value = bytes.to_vec();
      value.resize(len, 0);
      value
    }

    fn log_color_space(filename: SdkString, size: u32) -> LogColorSpace {
      LogColorSpace {
        signature: EmrLogColorSpaceSignature::Psoc.raw(),
        version: 0x0000_0400,
        size,
        color_space_type: EmrLogicalColorSpace::SRgb.raw(),
        intent: EmrGamutMappingIntent::Graphics.raw(),
        endpoints: CieXyzTriple {
          red: CieXyz { x: 1, y: 2, z: 3 },
          green: CieXyz { x: 4, y: 5, z: 6 },
          blue: CieXyz { x: 7, y: 8, z: 9 },
        },
        gamma_red: 0x0001_0000,
        gamma_green: 0x0001_1000,
        gamma_blue: 0x0001_2000,
        filename,
      }
    }

    let ascii = EmfRecordData::CreateColorSpace(EmrCreateColorSpace {
      color_space_index: 3,
      log_color_space: log_color_space(
        SdkString::raw(fixed_bytes(b"sRGB.icc\0", 260), SdkEncoding::Windows1252),
        328,
      ),
      extension: Vec::new(),
    });
    let record = ascii.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::CreateColorSpace.raw());
    assert_eq!(record.data.len(), 332);
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::CreateColorSpace(value) = &parsed else {
      panic!("expected EMR_CREATECOLORSPACE");
    };
    assert_eq!(
      value.log_color_space.signature_kind(),
      Some(EmrLogColorSpaceSignature::Psoc)
    );
    assert_eq!(
      value.log_color_space.color_space_type_kind(),
      Some(EmrLogicalColorSpace::SRgb)
    );
    assert_eq!(
      value.log_color_space.intent_kind(),
      Some(EmrGamutMappingIntent::Graphics)
    );
    assert_eq!(
      value.log_color_space.gamma_red_value(),
      LogColorSpaceGamma::from_parts(1, 0)
    );
    assert_eq!(value.log_color_space.gamma_green_value().integer(), 1);
    assert_eq!(value.log_color_space.gamma_green_value().fraction(), 0x10);
    assert_eq!(
      value.log_color_space.gamma_green_value().real_value(),
      1.0625
    );
    assert_eq!(value.log_color_space.gamma_blue_value().reserved_bits(), 0);
    assert_eq!(parsed, ascii);
    let mut zero_ascii_color_space_index = ascii.clone();
    let EmfRecordData::CreateColorSpace(value) = &mut zero_ascii_color_space_index else {
      unreachable!();
    };
    value.color_space_index = 0;
    assert!(zero_ascii_color_space_index.to_record().is_err());
    let mut zero_ascii_color_space_index_record = record.clone();
    zero_ascii_color_space_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_ascii_color_space_index_record.parse_data().is_err());
    let mut invalid_signature = ascii.clone();
    let EmfRecordData::CreateColorSpace(value) = &mut invalid_signature else {
      unreachable!();
    };
    value.log_color_space.signature = 0xFFFF_FFFF;
    assert!(invalid_signature.validate_strict().is_err());
    assert!(invalid_signature.to_record().is_ok());
    let mut invalid_signature_record = record.clone();
    invalid_signature_record.data[4..8].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    let parsed = invalid_signature_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_signature_record);

    let mut invalid_version = ascii.clone();
    let EmfRecordData::CreateColorSpace(value) = &mut invalid_version else {
      unreachable!();
    };
    value.log_color_space.version = 0x0000_0300;
    assert!(invalid_version.validate_strict().is_err());
    assert!(invalid_version.to_record().is_ok());
    let mut invalid_version_record = record.clone();
    invalid_version_record.data[8..12].copy_from_slice(&0x0000_0300_u32.to_le_bytes());
    let parsed = invalid_version_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_version_record);

    let mut invalid_size = ascii.clone();
    let EmfRecordData::CreateColorSpace(value) = &mut invalid_size else {
      unreachable!();
    };
    value.log_color_space.size = 12;
    assert!(invalid_size.validate_strict().is_err());
    assert!(invalid_size.to_record().is_ok());
    let mut invalid_size_record = record.clone();
    invalid_size_record.data[12..16].copy_from_slice(&12_u32.to_le_bytes());
    let parsed = invalid_size_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_size_record);

    let mut invalid_color_space_type = ascii.clone();
    let EmfRecordData::CreateColorSpace(value) = &mut invalid_color_space_type else {
      unreachable!();
    };
    value.log_color_space.color_space_type = 0xFFFF_FFFF_u32 as i32;
    assert!(invalid_color_space_type.validate_strict().is_err());
    assert!(invalid_color_space_type.to_record().is_ok());
    let mut invalid_color_space_type_record = record.clone();
    invalid_color_space_type_record.data[16..20].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    let parsed = invalid_color_space_type_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_color_space_type_record);

    let mut invalid_intent = ascii.clone();
    let EmfRecordData::CreateColorSpace(value) = &mut invalid_intent else {
      unreachable!();
    };
    value.log_color_space.intent = 0xFFFF_FFFF_u32 as i32;
    assert!(invalid_intent.validate_strict().is_err());
    assert!(invalid_intent.to_record().is_ok());
    let mut invalid_intent_record = record.clone();
    invalid_intent_record.data[20..24].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    let parsed = invalid_intent_record.parse_data().unwrap();
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), invalid_intent_record);

    let mut truncated_record = record.clone();
    truncated_record.data.truncate(96);
    let truncated = truncated_record.parse_data().unwrap();
    assert!(truncated.validate_strict().is_err());
    assert_eq!(truncated.to_record().unwrap(), truncated_record);

    let wide = EmfRecordData::CreateColorSpaceW(EmrCreateColorSpaceW {
      color_space_index: 4,
      log_color_space: log_color_space(
        SdkString::raw(fixed_bytes(&[b'w', 0, 0, 0], 520), SdkEncoding::Utf16Le),
        588,
      ),
      flags: 1,
      data: vec![0xAA, 0xBB, 0xCC],
      padding: vec![0],
    });
    let record = wide.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::CreateColorSpaceW.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::CreateColorSpaceW(value) = &parsed else {
      panic!("expected EMR_CREATECOLORSPACEW");
    };
    assert!(value.contains_color_profile_data());
    assert!(
      value
        .flags()
        .contains(EmrCreateColorSpaceWFlags::COLOR_PROFILE_DATA)
    );
    assert_eq!(parsed, wide);
    let mut zero_wide_color_space_index = wide.clone();
    let EmfRecordData::CreateColorSpaceW(value) = &mut zero_wide_color_space_index else {
      unreachable!();
    };
    value.color_space_index = 0;
    assert!(zero_wide_color_space_index.to_record().is_err());
    let mut zero_wide_color_space_index_record = record.clone();
    zero_wide_color_space_index_record.data[0..4].copy_from_slice(&0_u32.to_le_bytes());
    assert!(zero_wide_color_space_index_record.parse_data().is_err());
    let mut invalid_flags = wide.clone();
    let EmfRecordData::CreateColorSpaceW(value) = &mut invalid_flags else {
      unreachable!();
    };
    value.flags = 2;
    assert!(invalid_flags.to_record().is_err());
    let mut invalid_flags_record = record.clone();
    let flags_offset = 4 + LogColorSpace::sdk_size(520);
    invalid_flags_record.data[flags_offset..flags_offset + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(invalid_flags_record.parse_data().is_err());
    let mut oversized_profile_data_record = record.clone();
    let data_size_offset = flags_offset + 4;
    oversized_profile_data_record.data[data_size_offset..data_size_offset + 4]
      .copy_from_slice(&1_000_000_u32.to_le_bytes());
    assert!(oversized_profile_data_record.parse_data().is_err());
  }

  #[test]
  fn raw_comment_records_validate_private_identifier_and_size() {
    let empty = EmfRecordData::Comment(EmrComment::PrivateData {
      data: Vec::new(),
      alignment_padding: Vec::new(),
    });
    let empty_record = empty.to_record().unwrap();
    assert_eq!(empty_record.data, vec![0, 0, 0, 0]);
    assert_eq!(empty_record.parse_data().unwrap(), empty);

    let short = EmfRecordData::Comment(EmrComment::PrivateData {
      data: vec![1, 2, 3],
      alignment_padding: vec![0xCD],
    });
    let short_record = short.to_record().unwrap();
    assert_eq!(short_record.parse_data().unwrap(), short);
    assert_eq!(
      short_record.parse_data().unwrap().to_record().unwrap(),
      short_record
    );

    assert!(
      EmfRecordData::Comment(EmrComment::PrivateData {
        data: vec![1, 2, 3, 4],
        alignment_padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );

    let value = EmfRecordData::Comment(EmrComment::Raw {
      data_size: 7,
      identifier: 0x1234_5678,
      data: vec![1, 2, 3],
      alignment_padding: vec![0xAB],
    });
    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::Comment.raw());
    assert_eq!(record.data.last(), Some(&0xAB));
    assert_eq!(record.parse_data().unwrap(), value);

    assert!(
      EmfRecordData::Comment(EmrComment::Raw {
        data_size: 4,
        identifier: EMR_COMMENT_EMFPLUS,
        data: Vec::new(),
        alignment_padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      EmfRecordData::Comment(EmrComment::Raw {
        data_size: 8,
        identifier: 0x1234_5678,
        data: vec![1, 2, 3],
        alignment_padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    let excessive_padding = EmfRecordData::Comment(EmrComment::Raw {
      data_size: 4,
      identifier: 0x1234_5678,
      data: Vec::new(),
      alignment_padding: vec![0, 0, 0, 0],
    });
    let excessive_padding_record = excessive_padding.to_record().unwrap();
    assert_eq!(
      excessive_padding_record.parse_data().unwrap(),
      excessive_padding
    );
    assert!(excessive_padding.validate_strict().is_err());

    let mut unaligned_record = record.clone();
    unaligned_record.data.push(0);
    assert!(unaligned_record.parse_data().is_err());

    let oversized_data_size_record = EmfRecord::new(EmfRecordType::Comment.raw(), {
      let mut data = Vec::new();
      data.extend_from_slice(&u32::MAX.to_le_bytes());
      data.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
      data
    });
    assert!(oversized_data_size_record.parse_data().is_err());
  }

  #[test]
  fn typed_emf_plus_comment_roundtrips() {
    let value = EmfRecordData::Comment(EmrComment::EmfPlus {
      records: vec![crate::emfplus::EmfPlusRecord {
        record_type: crate::emfplus::EmfPlusRecordType::Header.raw(),
        flags: 0,
        total_object_size: None,
        data: vec![
          0x02, 0x10, 0xC0, 0xDB, // GraphicsVersion
          0x00, 0x00, 0x00, 0x00, // EmfPlusFlags
          0x60, 0x00, 0x00, 0x00, // LogicalDpiX
          0x60, 0x00, 0x00, 0x00, // LogicalDpiY
        ],
        padding: Vec::new(),
      }],
      emf_plus_trailing_data: Vec::new(),
      alignment_padding: Vec::new(),
    });

    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::Comment.raw());
    assert_eq!(record.parse_data().unwrap(), value);

    let mut with_trailing_data = value.clone();
    let EmfRecordData::Comment(EmrComment::EmfPlus {
      emf_plus_trailing_data,
      ..
    }) = &mut with_trailing_data
    else {
      unreachable!()
    };
    *emf_plus_trailing_data = vec![0x46, 0, 0, 0];
    let record = with_trailing_data.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed, with_trailing_data);
    assert!(parsed.validate_strict().is_err());

    assert!(
      EmfRecordData::Comment(EmrComment::EmfPlus {
        records: Vec::new(),
        emf_plus_trailing_data: Vec::new(),
        alignment_padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );
  }

  #[test]
  fn typed_public_comment_records_roundtrip() {
    let begin_group = EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::BeginGroup(EmrCommentBeginGroup {
        rectangle: RectL {
          left: 1,
          top: 2,
          right: 30,
          bottom: 40,
        },
        description_chars: 3,
        description: SdkString::raw(vec![b'H', 0, b'i', 0, 0, 0], SdkEncoding::Utf16Le),
        padding: Vec::new(),
      }),
      alignment_padding: vec![0, 0],
    });
    let record = begin_group.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::Comment.raw());
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::Comment(EmrComment::Public { comment: value, .. }) = &parsed else {
      panic!("expected EMR_COMMENT_PUBLIC");
    };
    assert_eq!(
      value.identifier(),
      EmrPublicCommentIdentifier::BeginGroup.raw()
    );
    assert_eq!(parsed, begin_group);
    let mut unterminated_group = begin_group.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::BeginGroup(value),
      ..
    }) = &mut unterminated_group
    else {
      unreachable!();
    };
    value.description_chars = 2;
    value.description = SdkString::raw(vec![b'N', 0, b'o', 0], SdkEncoding::Utf16Le);
    assert!(unterminated_group.to_record().is_err());
    let mut trailing_group_data = begin_group.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::BeginGroup(value),
      ..
    }) = &mut trailing_group_data
    else {
      unreachable!();
    };
    value.padding = vec![0xA1, 0xA2, 0xA3, 0xA4];
    let record = trailing_group_data.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), trailing_group_data);

    let end_group = EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::EndGroup,
      alignment_padding: Vec::new(),
    });
    let record = end_group.to_record().unwrap();
    assert_eq!(record.parse_data().unwrap(), end_group);
    for identifier in [
      EmrPublicCommentIdentifier::UnicodeString,
      EmrPublicCommentIdentifier::UnicodeEnd,
    ] {
      let reserved = EmfRecordData::Comment(EmrComment::Public {
        comment: EmrPublicComment::Unknown {
          identifier: identifier.raw(),
          data: Vec::new(),
        },
        alignment_padding: Vec::new(),
      });
      assert!(reserved.to_record().is_err());

      let mut reserved_record = record.clone();
      reserved_record.data[8..12].copy_from_slice(&identifier.raw().to_le_bytes());
      assert!(reserved_record.parse_data().is_err());
    }

    let eps_data = EmrEpsData {
      size_data: 36,
      version: 1,
      points: [
        EmrPoint28_4 {
          x: EmrBitFix28_4 { raw: 0x0018 },
          y: EmrBitFix28_4 { raw: 0x0020 },
        },
        EmrPoint28_4 {
          x: EmrBitFix28_4 { raw: 0x1010 },
          y: EmrBitFix28_4 { raw: 0x0020 },
        },
        EmrPoint28_4 {
          x: EmrBitFix28_4 { raw: 0x0018 },
          y: EmrBitFix28_4 { raw: 0x0810 },
        },
      ],
      postscript_data: vec![0x25, b'P', b'S', 0],
    };
    let multi_formats = EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::MultiFormats(EmrCommentMultiFormats {
        output_rect: RectL {
          left: 0,
          top: 0,
          right: 100,
          bottom: 50,
        },
        formats: vec![EmrFormat {
          signature: EmrFormatSignature::Eps.raw(),
          version: 1,
          size_data: 36,
          data_offset: 44,
        }],
        format_data: eps_data.to_bytes().unwrap(),
        padding: Vec::new(),
      }),
      alignment_padding: Vec::new(),
    });
    let record = multi_formats.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::MultiFormats(value),
      ..
    }) = &parsed
    else {
      panic!("expected EMR_COMMENT_MULTIFORMATS");
    };
    assert_eq!(
      value.formats[0].signature_kind(),
      Some(EmrFormatSignature::Eps)
    );
    let parsed_eps = value.eps_data(0).unwrap().unwrap();
    assert_eq!(parsed_eps, eps_data);
    assert_eq!(parsed_eps.points[0].x.int_value(), 1);
    assert_eq!(parsed_eps.points[0].x.frac_value(), 8);
    assert_eq!(parsed_eps.points[0].x.real_value(), 1.5);
    assert_eq!(parsed, multi_formats);
    let mut oversized_format_count = record.clone();
    oversized_format_count.data[28..32].copy_from_slice(&1_000_000_u32.to_le_bytes());
    oversized_format_count.data.truncate(32);
    assert!(oversized_format_count.parse_data().is_err());
    let mut trailing_multi_data = multi_formats.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::MultiFormats(value),
      ..
    }) = &mut trailing_multi_data
    else {
      unreachable!();
    };
    value.padding = vec![0xB1, 0xB2, 0xB3, 0xB4];
    let trailing_multi_record = trailing_multi_data.to_record().unwrap();
    assert_eq!(
      trailing_multi_record.parse_data().unwrap(),
      trailing_multi_data
    );
    let mut invalid_multi_signature = multi_formats.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::MultiFormats(value),
      ..
    }) = &mut invalid_multi_signature
    else {
      unreachable!();
    };
    value.formats[0].signature = 0xFFFF_FFFF;
    let invalid_multi_signature_record = invalid_multi_signature.to_record().unwrap();
    assert!(invalid_multi_signature.validate_strict().is_err());
    let mut invalid_multi_record = record.clone();
    invalid_multi_record.data[32..36].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    let parsed_invalid_multi = invalid_multi_record.parse_data().unwrap();
    assert!(parsed_invalid_multi.validate_strict().is_err());
    assert_eq!(
      parsed_invalid_multi.to_record().unwrap(),
      invalid_multi_record
    );
    assert_eq!(invalid_multi_signature_record, invalid_multi_record);

    let mut invalid_eps_version = multi_formats.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::MultiFormats(value),
      ..
    }) = &mut invalid_eps_version
    else {
      unreachable!();
    };
    value.formats[0].version = 2;
    assert!(invalid_eps_version.to_record().is_err());
    let mut invalid_eps_version_record = record.clone();
    invalid_eps_version_record.data[36..40].copy_from_slice(&2_u32.to_le_bytes());
    assert!(invalid_eps_version_record.parse_data().is_err());

    let mut invalid_eps_payload_version = record.clone();
    invalid_eps_payload_version.data[52..56].copy_from_slice(&2_u32.to_le_bytes());
    assert!(invalid_eps_payload_version.parse_data().is_err());

    let mut invalid_data_offset = multi_formats.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::MultiFormats(value),
      ..
    }) = &mut invalid_data_offset
    else {
      unreachable!();
    };
    value.formats[0].data_offset = 45;
    assert!(invalid_data_offset.to_record().is_err());
    let mut invalid_data_offset_record = record.clone();
    invalid_data_offset_record.data[44..48].copy_from_slice(&45_u32.to_le_bytes());
    assert!(invalid_data_offset_record.parse_data().is_err());

    let windows_metafile = EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::WindowsMetafile(EmrCommentWindowsMetafile {
        version: WmfMetafileVersion::Version300.raw(),
        reserved: 0,
        checksum: 0xAABB_CCDD,
        flags: 0,
        metafile_size: 4,
        metafile: vec![1, 2, 3, 4],
        padding: Vec::new(),
      }),
      alignment_padding: Vec::new(),
    });
    let record = windows_metafile.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::WindowsMetafile(value),
      ..
    }) = &parsed
    else {
      panic!("expected EMR_COMMENT_WINDOWS_METAFILE");
    };
    assert_eq!(value.version_kind(), Some(WmfMetafileVersion::Version300));
    assert_eq!(value.metafile_len(), 4);
    assert!(value.metafile_size_matches_data());
    assert!(!value.has_padding());
    assert!(value.windows_metafile().is_err());
    assert_eq!(parsed, windows_metafile);
    let embedded_wmf = [
      1u16.to_le_bytes().as_slice(),
      9u16.to_le_bytes().as_slice(),
      WmfMetafileVersion::Version300
        .raw()
        .to_le_bytes()
        .as_slice(),
      12u32.to_le_bytes().as_slice(),
      0u16.to_le_bytes().as_slice(),
      3u32.to_le_bytes().as_slice(),
      0u16.to_le_bytes().as_slice(),
      3u32.to_le_bytes().as_slice(),
      0u16.to_le_bytes().as_slice(),
    ]
    .concat();
    let valid_windows_metafile = EmrCommentWindowsMetafile {
      version: WmfMetafileVersion::Version300.raw(),
      reserved: 0,
      checksum: 0,
      flags: 0,
      metafile_size: embedded_wmf.len() as u32,
      metafile: embedded_wmf,
      padding: vec![0, 0],
    };
    let parsed_wmf = valid_windows_metafile.windows_metafile().unwrap();
    assert_eq!(
      parsed_wmf.header.version_kind(),
      Some(WmfMetafileVersion::Version300)
    );
    assert_eq!(parsed_wmf.records.len(), 1);
    assert!(valid_windows_metafile.has_padding());
    assert!(valid_windows_metafile.metafile_size_matches_data());
    let mut trailing_wmf_data = windows_metafile.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::WindowsMetafile(value),
      ..
    }) = &mut trailing_wmf_data
    else {
      unreachable!();
    };
    value.padding = vec![0xC1, 0xC2, 0xC3, 0xC4];
    let trailing_wmf_record = trailing_wmf_data.to_record().unwrap();
    assert_eq!(trailing_wmf_record.parse_data().unwrap(), trailing_wmf_data);
    let mut invalid_wmf_version = windows_metafile.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::WindowsMetafile(value),
      ..
    }) = &mut invalid_wmf_version
    else {
      unreachable!();
    };
    value.version = 0xFFFF;
    assert!(invalid_wmf_version.to_record().is_err());
    let mut invalid_wmf_version_record = record.clone();
    invalid_wmf_version_record.data[12..14].copy_from_slice(&0xFFFF_u16.to_le_bytes());
    assert!(invalid_wmf_version_record.parse_data().is_err());

    let mut invalid_wmf_reserved = windows_metafile.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::WindowsMetafile(value),
      ..
    }) = &mut invalid_wmf_reserved
    else {
      unreachable!();
    };
    value.reserved = 1;
    assert!(invalid_wmf_reserved.to_record().is_err());
    let mut invalid_wmf_reserved_record = record.clone();
    invalid_wmf_reserved_record.data[14..16].copy_from_slice(&1_u16.to_le_bytes());
    assert!(invalid_wmf_reserved_record.parse_data().is_err());

    let mut invalid_wmf_flags = windows_metafile.clone();
    let EmfRecordData::Comment(EmrComment::Public {
      comment: EmrPublicComment::WindowsMetafile(value),
      ..
    }) = &mut invalid_wmf_flags
    else {
      unreachable!();
    };
    value.flags = 1;
    assert!(invalid_wmf_flags.to_record().is_err());
    let mut invalid_wmf_flags_record = record.clone();
    invalid_wmf_flags_record.data[20..24].copy_from_slice(&1_u32.to_le_bytes());
    assert!(invalid_wmf_flags_record.parse_data().is_err());
  }

  #[test]
  fn typed_emf_spool_comment_roundtrips() {
    let value = EmfRecordData::Comment(EmrComment::EmfSpool {
      spool_identifier: EMR_COMMENT_EMFSPOOL_FONT_DEFINITION,
      data: vec![1, 2, 3, 4, 5],
      alignment_padding: vec![0, 0, 0],
    });
    let record = value.to_record().unwrap();
    assert_eq!(record.record_type, EmfRecordType::Comment.raw());
    assert_eq!(record.parse_data().unwrap(), value);
    let invalid_identifier = EmfRecordData::Comment(EmrComment::EmfSpool {
      spool_identifier: 0x1234_5678,
      data: Vec::new(),
      alignment_padding: Vec::new(),
    });
    assert!(invalid_identifier.to_record().is_err());
    let empty_spool = EmfRecordData::Comment(EmrComment::EmfSpool {
      spool_identifier: EMR_COMMENT_EMFSPOOL_FONT_DEFINITION,
      data: Vec::new(),
      alignment_padding: Vec::new(),
    });
    assert!(empty_spool.to_record().is_err());
    let empty_spool_record = EmfRecord::new(
      EmfRecordType::Comment.raw(),
      [
        8_u32.to_le_bytes(),
        EMR_COMMENT_EMFSPOOL.to_le_bytes(),
        EMR_COMMENT_EMFSPOOL_FONT_DEFINITION.to_le_bytes(),
      ]
      .concat(),
    );
    assert!(empty_spool_record.parse_data().is_err());
  }
}
