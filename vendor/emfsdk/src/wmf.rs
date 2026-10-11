use std::io::Cursor;

use bitflags::bitflags;
use emfsdk_derive::{SdkEnum, SdkObject};

use crate::bitmap::{
  BitmapBitCount, BitmapCieXyz, BitmapCieXyzTriple, BitmapCompression, BitmapCoreHeader,
  BitmapGamutMappingIntent, BitmapInfoHeader, BitmapLogicalColorSpace, BitmapLogicalColorSpaceV5,
  BitmapV4Header, BitmapV5Header, DeviceIndependentBitmap, DibBitmapInfo, DibColorTable,
  DibColorUsage, RgbQuad,
};
use crate::common::{Error, Reader, Result, SdkEnumValue, SdkRead, SdkSize, SdkWrite, Writer};
use crate::emf::{EmrLogColorSpaceSignature, LogColorSpace};
use crate::string::SdkEncoding;
use crate::types::{ColorRef, PointL, PointS, RectL, SizeL};

pub const META_EOF: u16 = 0x0000;
pub const PLACEABLE_KEY: u32 = 0x9AC6_CDD7;
pub const WMF_EMF_COMMENT_IDENTIFIER: u32 = 0x4346_4D57;
pub const WMF_EMF_COMMENT_TYPE: u32 = 0x0000_0001;
pub const WMF_EMF_INTEROP_VERSION: u32 = 0x0001_0000;
pub const WMF_EMF_ESCAPE_HEADER_SIZE: usize = 34;
pub const WMF_EMF_ESCAPE_MAX_RECORD_SIZE: u32 = 8_192;
pub const PLACEABLE_HEADER_SIZE: usize = 22;
pub const WMF_HEADER_SIZE: usize = 18;
pub const WMF_LOG_COLOR_SPACE_SIZE: usize = 328;
pub const WMF_LOG_COLOR_SPACE_W_SIZE: usize = 588;

pub type WmfBitCount = BitmapBitCount;
pub type WmfColorUsage = DibColorUsage;
pub type WmfCompression = BitmapCompression;
pub type WmfGamutMappingIntent = BitmapGamutMappingIntent;
pub type WmfLogicalColorSpace = BitmapLogicalColorSpace;
pub type WmfLogicalColorSpaceV5 = BitmapLogicalColorSpaceV5;
pub type WmfLogColorSpace = LogColorSpace;
pub type WmfLogColorSpaceW = LogColorSpace;
pub type WmfLogColorSpaceSignature = EmrLogColorSpaceSignature;
pub type WmfFloodFill = WmfFloodFillMode;
pub type WmfLayout = WmfLayoutFlags;
pub type WmfPaletteEntryFlag = WmfPaletteEntryFlags;
pub type WmfPenStyle = WmfPenStyleFlags;
pub type WmfBitmapCoreHeader = BitmapCoreHeader;
pub type WmfBitmapInfoHeader = BitmapInfoHeader;
pub type WmfBitmapV4Header = BitmapV4Header;
pub type WmfBitmapV5Header = BitmapV5Header;
pub type WmfCieXyz = BitmapCieXyz;
pub type WmfCieXyzTriple = BitmapCieXyzTriple;
pub type WmfColorRef = ColorRef;
pub type WmfDeviceIndependentBitmap = DeviceIndependentBitmap;
pub type WmfDibColorTable = DibColorTable;
pub type WmfPointL = PointL;
pub type WmfPointS = PointS;
pub type WmfRectL = RectL;
pub type WmfRgbQuad = RgbQuad;
pub type WmfSizeL = SizeL;

pub fn read_wmf_log_color_space<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
) -> Result<WmfLogColorSpace> {
  WmfLogColorSpace::read_from(reader, SdkEncoding::Windows1252, 260)
}

pub fn write_wmf_log_color_space<W: std::io::Write>(
  value: &WmfLogColorSpace,
  writer: &mut Writer<W>,
) -> Result<()> {
  value.write_to(writer, 260)
}

pub fn read_wmf_log_color_space_w<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
) -> Result<WmfLogColorSpaceW> {
  WmfLogColorSpaceW::read_from(reader, SdkEncoding::Utf16Le, 520)
}

pub fn write_wmf_log_color_space_w<W: std::io::Write>(
  value: &WmfLogColorSpaceW,
  writer: &mut Writer<W>,
) -> Result<()> {
  value.write_to(writer, 520)
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WmfExtTextOutOptions: u16 {
        const OPAQUE = 0x0002;
        const CLIPPED = 0x0004;
        const GLYPH_INDEX = 0x0010;
        const RTL_READING = 0x0080;
        const NUMERICS_LOCAL = 0x0400;
        const NUMERICS_LATIN = 0x0800;
        const PDY = 0x2000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WmfClipPrecisionFlags: u8 {
        const CHARACTER = 0x01;
        const STROKE = 0x02;
        const LH_ANGLES = 0x10;
        const TT_ALWAYS = 0x20;
        const DFA_DISABLE = 0x40;
        const EMBEDDED = 0x80;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WmfTextAlignmentModeFlags: u16 {
        const UPDATE_CP = 0x0001;
        const RIGHT = 0x0002;
        const CENTER = 0x0006;
        const BOTTOM = 0x0008;
        const BASELINE = 0x0018;
        const RTL_READING = 0x0100;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WmfVerticalTextAlignmentModeFlags: u16 {
        const BOTTOM = 0x0002;
        const CENTER = 0x0006;
        const LEFT = 0x0008;
        const BASELINE = 0x0018;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WmfLayoutFlags: u16 {
        const RTL = 0x0001;
        const BITMAP_ORIENTATION_PRESERVED = 0x0008;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WmfPaletteEntryFlags: u8 {
        const RESERVED = 0x01;
        const EXPLICIT = 0x02;
        const NO_COLLAPSE = 0x04;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WmfPenStyleFlags: u16 {
        const DASH = 0x0001;
        const DOT = 0x0002;
        const DASH_DOT = 0x0003;
        const DASH_DOT_DOT = 0x0004;
        const NULL = 0x0005;
        const INSIDE_FRAME = 0x0006;
        const USER_STYLE = 0x0007;
        const ALTERNATE = 0x0008;
        const END_CAP_SQUARE = 0x0100;
        const END_CAP_FLAT = 0x0200;
        const JOIN_BEVEL = 0x1000;
        const JOIN_MITER = 0x2000;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfRecordFunction {
  Eof = 0x0000,
  SaveDc = 0x001E,
  RealizePalette = 0x0035,
  SetPalEntries = 0x0037,
  CreatePalette = 0x00F7,
  SetBkMode = 0x0102,
  SetMapMode = 0x0103,
  SetRop2 = 0x0104,
  SetRelabs = 0x0105,
  SetPolyFillMode = 0x0106,
  SetStretchBltMode = 0x0107,
  SetTextCharExtra = 0x0108,
  RestoreDc = 0x0127,
  InvertRegion = 0x012A,
  PaintRegion = 0x012B,
  SelectClipRegion = 0x012C,
  SelectObject = 0x012D,
  SetTextAlign = 0x012E,
  ResizePalette = 0x0139,
  DibCreatePatternBrush = 0x0142,
  SetLayout = 0x0149,
  DeleteObject = 0x01F0,
  CreatePatternBrush = 0x01F9,
  CreatePenIndirect = 0x02FA,
  CreateFontIndirect = 0x02FB,
  CreateBrushIndirect = 0x02FC,
  SetBkColor = 0x0201,
  SetTextColor = 0x0209,
  SetTextJustification = 0x020A,
  SetWindowOrg = 0x020B,
  SetWindowExt = 0x020C,
  SetViewportOrg = 0x020D,
  SetViewportExt = 0x020E,
  OffsetWindowOrg = 0x020F,
  OffsetViewportOrg = 0x0211,
  LineTo = 0x0213,
  MoveTo = 0x0214,
  OffsetClipRgn = 0x0220,
  FillRegion = 0x0228,
  SetMapperFlags = 0x0231,
  SelectPalette = 0x0234,
  Polygon = 0x0324,
  Polyline = 0x0325,
  AnimatePalette = 0x0436,
  SetPixel = 0x041F,
  ExcludeClipRect = 0x0415,
  IntersectClipRect = 0x0416,
  Ellipse = 0x0418,
  FloodFill = 0x0419,
  Rectangle = 0x041B,
  ScaleWindowExt = 0x0410,
  ScaleViewportExt = 0x0412,
  FrameRegion = 0x0429,
  TextOut = 0x0521,
  PolyPolygon = 0x0538,
  ExtFloodFill = 0x0548,
  RoundRect = 0x061C,
  PatBlt = 0x061D,
  Escape = 0x0626,
  CreateRegion = 0x06FF,
  Arc = 0x0817,
  Pie = 0x081A,
  Chord = 0x0830,
  BitBlt = 0x0922,
  DibBitBlt = 0x0940,
  ExtTextOut = 0x0A32,
  StretchBlt = 0x0B23,
  DibStretchBlt = 0x0B41,
  SetDibToDev = 0x0D33,
  StretchDib = 0x0F43,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum WmfCharacterSet {
  Ansi = 0x00,
  Default = 0x01,
  Symbol = 0x02,
  Mac = 0x4D,
  ShiftJis = 0x80,
  Hangul = 0x81,
  Johab = 0x82,
  Gb2312 = 0x86,
  ChineseBig5 = 0x88,
  Greek = 0xA1,
  Turkish = 0xA2,
  Vietnamese = 0xA3,
  Hebrew = 0xB1,
  Arabic = 0xB2,
  Baltic = 0xBA,
  Russian = 0xCC,
  Thai = 0xDE,
  EastEurope = 0xEE,
  Oem = 0xFF,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum WmfOutPrecision {
  Default = 0x00,
  String = 0x01,
  Stroke = 0x03,
  TrueType = 0x04,
  Device = 0x05,
  Raster = 0x06,
  TrueTypeOnly = 0x07,
  Outline = 0x08,
  ScreenOutline = 0x09,
  PostScriptOnly = 0x0A,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum WmfFontQuality {
  Default = 0x00,
  Draft = 0x01,
  Proof = 0x02,
  NonAntialiased = 0x03,
  Antialiased = 0x04,
  ClearType = 0x05,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum WmfFamilyFont {
  DontCare = 0x00,
  Roman = 0x01,
  Swiss = 0x02,
  Modern = 0x03,
  Script = 0x04,
  Decorative = 0x05,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u8")]
pub enum WmfPitchFont {
  Default = 0x00,
  Fixed = 0x01,
  Variable = 0x02,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfBinaryRasterOperation {
  Black = 0x0001,
  NotMergePen = 0x0002,
  MaskNotPen = 0x0003,
  NotCopyPen = 0x0004,
  MaskPenNot = 0x0005,
  Not = 0x0006,
  XorPen = 0x0007,
  NotMaskPen = 0x0008,
  MaskPen = 0x0009,
  NotXorPen = 0x000A,
  Nop = 0x000B,
  MergeNotPen = 0x000C,
  CopyPen = 0x000D,
  MergePenNot = 0x000E,
  MergePen = 0x000F,
  White = 0x0010,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WmfTernaryRasterOperationCode(u8);

impl WmfTernaryRasterOperationCode {
  pub const BLACKNESS: Self = Self(0x00);
  pub const DPSOON: Self = Self(0x01);
  pub const DPSONA: Self = Self(0x02);
  pub const PSON: Self = Self(0x03);
  pub const SDPONA: Self = Self(0x04);
  pub const DPON: Self = Self(0x05);
  pub const PDSXNON: Self = Self(0x06);
  pub const PDSAON: Self = Self(0x07);
  pub const SDPNAA: Self = Self(0x08);
  pub const PDSXON: Self = Self(0x09);
  pub const DPNA: Self = Self(0x0A);
  pub const PSDNAON: Self = Self(0x0B);
  pub const SPNA: Self = Self(0x0C);
  pub const PDSNAON: Self = Self(0x0D);
  pub const PDSONON: Self = Self(0x0E);
  pub const PN: Self = Self(0x0F);
  pub const PDSONA: Self = Self(0x10);
  pub const NOTSRCERASE: Self = Self(0x11);
  pub const SDPXNON: Self = Self(0x12);
  pub const SDPAON: Self = Self(0x13);
  pub const DPSXNON: Self = Self(0x14);
  pub const DPSAON: Self = Self(0x15);
  pub const PSDPSANAXX: Self = Self(0x16);
  pub const SSPXDSXAXN: Self = Self(0x17);
  pub const SPXPDXA: Self = Self(0x18);
  pub const SDPSANAXN: Self = Self(0x19);
  pub const PDSPAOX: Self = Self(0x1A);
  pub const SDPSXAXN: Self = Self(0x1B);
  pub const PSDPAOX: Self = Self(0x1C);
  pub const DSPDXAXN: Self = Self(0x1D);
  pub const PDSOX: Self = Self(0x1E);
  pub const PDSOAN: Self = Self(0x1F);
  pub const DPSNAA: Self = Self(0x20);
  pub const SDPXON: Self = Self(0x21);
  pub const DSNA: Self = Self(0x22);
  pub const SPDNAON: Self = Self(0x23);
  pub const SPXDSXA: Self = Self(0x24);
  pub const PDSPANAXN: Self = Self(0x25);
  pub const SDPSAOX: Self = Self(0x26);
  pub const SDPSXNOX: Self = Self(0x27);
  pub const DPSXA: Self = Self(0x28);
  pub const PSDPSAOXXN: Self = Self(0x29);
  pub const DPSANA: Self = Self(0x2A);
  pub const SSPXPDXAXN: Self = Self(0x2B);
  pub const SPDSOAX: Self = Self(0x2C);
  pub const PSDNOX: Self = Self(0x2D);
  pub const PSDPXOX: Self = Self(0x2E);
  pub const PSDNOAN: Self = Self(0x2F);
  pub const PSNA: Self = Self(0x30);
  pub const SDPNAON: Self = Self(0x31);
  pub const SDPSOOX: Self = Self(0x32);
  pub const NOTSRCCOPY: Self = Self(0x33);
  pub const SPDSAOX: Self = Self(0x34);
  pub const SPDSXNOX: Self = Self(0x35);
  pub const SDPOX: Self = Self(0x36);
  pub const SDPOAN: Self = Self(0x37);
  pub const PSDPOAX: Self = Self(0x38);
  pub const SPDNOX: Self = Self(0x39);
  pub const SPDSXOX: Self = Self(0x3A);
  pub const SPDNOAN: Self = Self(0x3B);
  pub const PSX: Self = Self(0x3C);
  pub const SPDSONOX: Self = Self(0x3D);
  pub const SPDSNAOX: Self = Self(0x3E);
  pub const PSAN: Self = Self(0x3F);
  pub const PSDNAA: Self = Self(0x40);
  pub const DPSXON: Self = Self(0x41);
  pub const SDXPDXA: Self = Self(0x42);
  pub const SPDSANAXN: Self = Self(0x43);
  pub const SRCERASE: Self = Self(0x44);
  pub const DPSNAON: Self = Self(0x45);
  pub const DSPDAOX: Self = Self(0x46);
  pub const PSDPXAXN: Self = Self(0x47);
  pub const SDPXA: Self = Self(0x48);
  pub const PDSPDAOXXN: Self = Self(0x49);
  pub const DPSDOAX: Self = Self(0x4A);
  pub const PDSNOX: Self = Self(0x4B);
  pub const SDPANA: Self = Self(0x4C);
  pub const SSPXDSXOXN: Self = Self(0x4D);
  pub const PDSPXOX: Self = Self(0x4E);
  pub const PDSNOAN: Self = Self(0x4F);
  pub const PDNA: Self = Self(0x50);
  pub const DSPNAON: Self = Self(0x51);
  pub const DPSDAOX: Self = Self(0x52);
  pub const SPDSXAXN: Self = Self(0x53);
  pub const DPSONON: Self = Self(0x54);
  pub const DSTINVERT: Self = Self(0x55);
  pub const DPSOX: Self = Self(0x56);
  pub const DPSOAN: Self = Self(0x57);
  pub const PDSPOAX: Self = Self(0x58);
  pub const DPSNOX: Self = Self(0x59);
  pub const PATINVERT: Self = Self(0x5A);
  pub const DPSDONOX: Self = Self(0x5B);
  pub const DPSDXOX: Self = Self(0x5C);
  pub const DPSNOAN: Self = Self(0x5D);
  pub const DPSDNAOX: Self = Self(0x5E);
  pub const DPAN: Self = Self(0x5F);
  pub const PDSXA: Self = Self(0x60);
  pub const DSPDSAOXXN: Self = Self(0x61);
  pub const DSPDOAX: Self = Self(0x62);
  pub const SDPNOX: Self = Self(0x63);
  pub const SDPSOAX: Self = Self(0x64);
  pub const DSPNOX: Self = Self(0x65);
  pub const SRCINVERT: Self = Self(0x66);
  pub const SDPSONOX: Self = Self(0x67);
  pub const DSPDSONOXXN: Self = Self(0x68);
  pub const PDSXXN: Self = Self(0x69);
  pub const DPSAX: Self = Self(0x6A);
  pub const PSDPSOAXXN: Self = Self(0x6B);
  pub const SDPAX: Self = Self(0x6C);
  pub const PDSPDOAXXN: Self = Self(0x6D);
  pub const SDPSNOAX: Self = Self(0x6E);
  pub const PDXNAN: Self = Self(0x6F);
  pub const PDSANA: Self = Self(0x70);
  pub const SSDXPDXAXN: Self = Self(0x71);
  pub const SDPSXOX: Self = Self(0x72);
  pub const SDPNOAN: Self = Self(0x73);
  pub const DSPDXOX: Self = Self(0x74);
  pub const DSPNOAN: Self = Self(0x75);
  pub const SDPSNAOX: Self = Self(0x76);
  pub const DSAN: Self = Self(0x77);
  pub const PDSAX: Self = Self(0x78);
  pub const DSPDSOAXXN: Self = Self(0x79);
  pub const DPSDNOAX: Self = Self(0x7A);
  pub const SDPXNAN: Self = Self(0x7B);
  pub const SPDSNOAX: Self = Self(0x7C);
  pub const DPSXNAN: Self = Self(0x7D);
  pub const SPXDSXO: Self = Self(0x7E);
  pub const DPSAAN: Self = Self(0x7F);
  pub const DPSAA: Self = Self(0x80);
  pub const SPXDSXON: Self = Self(0x81);
  pub const DPSXNA: Self = Self(0x82);
  pub const SPDSNOAXN: Self = Self(0x83);
  pub const SDPXNA: Self = Self(0x84);
  pub const PDSPNOAXN: Self = Self(0x85);
  pub const DSPDSOAXX: Self = Self(0x86);
  pub const PDSAXN: Self = Self(0x87);
  pub const SRCAND: Self = Self(0x88);
  pub const SDPSNAOXN: Self = Self(0x89);
  pub const DSPNOA: Self = Self(0x8A);
  pub const DSPDXOXN: Self = Self(0x8B);
  pub const SDPNOA: Self = Self(0x8C);
  pub const SDPSXOXN: Self = Self(0x8D);
  pub const SSDXPDXAX: Self = Self(0x8E);
  pub const PDSANAN: Self = Self(0x8F);
  pub const PDSXNA: Self = Self(0x90);
  pub const SDPSNOAXN: Self = Self(0x91);
  pub const DPSDPOAXX: Self = Self(0x92);
  pub const SPDAXN: Self = Self(0x93);
  pub const PSDPSOAXX: Self = Self(0x94);
  pub const DPSAXN: Self = Self(0x95);
  pub const DPSXX: Self = Self(0x96);
  pub const PSDPSONOXX: Self = Self(0x97);
  pub const SDPSONOXN: Self = Self(0x98);
  pub const DSXN: Self = Self(0x99);
  pub const DPSNAX: Self = Self(0x9A);
  pub const SDPSOAXN: Self = Self(0x9B);
  pub const SPDNAX: Self = Self(0x9C);
  pub const DSPDOAXN: Self = Self(0x9D);
  pub const DSPDSAOXX: Self = Self(0x9E);
  pub const PDSXAN: Self = Self(0x9F);
  pub const DPA: Self = Self(0xA0);
  pub const PDSPNAOXN: Self = Self(0xA1);
  pub const DPSNOA: Self = Self(0xA2);
  pub const DPSDXOXN: Self = Self(0xA3);
  pub const PDSPONOXN: Self = Self(0xA4);
  pub const PDXN: Self = Self(0xA5);
  pub const DSPNAX: Self = Self(0xA6);
  pub const PDSPOAXN: Self = Self(0xA7);
  pub const DPSOA: Self = Self(0xA8);
  pub const DPSOXN: Self = Self(0xA9);
  pub const D: Self = Self(0xAA);
  pub const DPSONO: Self = Self(0xAB);
  pub const SPDSXAX: Self = Self(0xAC);
  pub const DPSDAOXN: Self = Self(0xAD);
  pub const DSPNAO: Self = Self(0xAE);
  pub const DPNO: Self = Self(0xAF);
  pub const PDSNOA: Self = Self(0xB0);
  pub const PDSPXOXN: Self = Self(0xB1);
  pub const SSPXDSXOX: Self = Self(0xB2);
  pub const SDPANAN: Self = Self(0xB3);
  pub const PSDNAX: Self = Self(0xB4);
  pub const DPSDOAXN: Self = Self(0xB5);
  pub const DPSDPAOXX: Self = Self(0xB6);
  pub const SDPXAN: Self = Self(0xB7);
  pub const PSDPXAX: Self = Self(0xB8);
  pub const DSPDAOXN: Self = Self(0xB9);
  pub const DPSNAO: Self = Self(0xBA);
  pub const MERGEPAINT: Self = Self(0xBB);
  pub const SPDSANAX: Self = Self(0xBC);
  pub const SDXPDXAN: Self = Self(0xBD);
  pub const DPSXO: Self = Self(0xBE);
  pub const DPSANO: Self = Self(0xBF);
  pub const MERGECOPY: Self = Self(0xC0);
  pub const SPDSNAOXN: Self = Self(0xC1);
  pub const SPDSONOXN: Self = Self(0xC2);
  pub const PSXN: Self = Self(0xC3);
  pub const SPDNOA: Self = Self(0xC4);
  pub const SPDSXOXN: Self = Self(0xC5);
  pub const SDPNAX: Self = Self(0xC6);
  pub const PSDPOAXN: Self = Self(0xC7);
  pub const SDPOA: Self = Self(0xC8);
  pub const SPDOXN: Self = Self(0xC9);
  pub const DPSDXAX: Self = Self(0xCA);
  pub const SPDSAOXN: Self = Self(0xCB);
  pub const SRCCOPY: Self = Self(0xCC);
  pub const SDPONO: Self = Self(0xCD);
  pub const SDPNAO: Self = Self(0xCE);
  pub const SPNO: Self = Self(0xCF);
  pub const PSDNOA: Self = Self(0xD0);
  pub const PSDPXOXN: Self = Self(0xD1);
  pub const PDSNAX: Self = Self(0xD2);
  pub const SPDSOAXN: Self = Self(0xD3);
  pub const SSPXPDXAX: Self = Self(0xD4);
  pub const DPSANAN: Self = Self(0xD5);
  pub const PSDPSAOXX: Self = Self(0xD6);
  pub const DPSXAN: Self = Self(0xD7);
  pub const PDSPXAX: Self = Self(0xD8);
  pub const SDPSAOXN: Self = Self(0xD9);
  pub const DPSDANAX: Self = Self(0xDA);
  pub const SPXDSXAN: Self = Self(0xDB);
  pub const SPDNAO: Self = Self(0xDC);
  pub const SDNO: Self = Self(0xDD);
  pub const SDPXO: Self = Self(0xDE);
  pub const SDPANO: Self = Self(0xDF);
  pub const PDSOA: Self = Self(0xE0);
  pub const PDSOXN: Self = Self(0xE1);
  pub const DSPDXAX: Self = Self(0xE2);
  pub const PSDPAOXN: Self = Self(0xE3);
  pub const SDPSXAX: Self = Self(0xE4);
  pub const PDSPAOXN: Self = Self(0xE5);
  pub const SDPSANAX: Self = Self(0xE6);
  pub const SPXPDXAN: Self = Self(0xE7);
  pub const SSPXDSXAX: Self = Self(0xE8);
  pub const DSPDSANAXXN: Self = Self(0xE9);
  pub const DPSAO: Self = Self(0xEA);
  pub const DPSXNO: Self = Self(0xEB);
  pub const SDPAO: Self = Self(0xEC);
  pub const SDPXNO: Self = Self(0xED);
  pub const SRCPAINT: Self = Self(0xEE);
  pub const SDPNOO: Self = Self(0xEF);
  pub const PATCOPY: Self = Self(0xF0);
  pub const PDSONO: Self = Self(0xF1);
  pub const PDSNAO: Self = Self(0xF2);
  pub const PSNO: Self = Self(0xF3);
  pub const PSDNAO: Self = Self(0xF4);
  pub const PDNO: Self = Self(0xF5);
  pub const PDSXO: Self = Self(0xF6);
  pub const PDSANO: Self = Self(0xF7);
  pub const PDSAO: Self = Self(0xF8);
  pub const PDSXNO: Self = Self(0xF9);
  pub const DPO: Self = Self(0xFA);
  pub const PATPAINT: Self = Self(0xFB);
  pub const PSO: Self = Self(0xFC);
  pub const PSDNOO: Self = Self(0xFD);
  pub const DPSOO: Self = Self(0xFE);
  pub const WHITENESS: Self = Self(0xFF);

  pub const fn from_raw(raw: u8) -> Self {
    Self(raw)
  }

  pub const fn raw(self) -> u8 {
    self.0
  }

  pub const fn uses_source(self) -> bool {
    ((self.0 ^ (self.0 >> 2)) & 0x33) != 0
  }

  pub const fn canonical_raw(self) -> u32 {
    WMF_TERNARY_RASTER_OPERATION_VALUES[self.0 as usize]
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WmfTernaryRasterOperation {
  raw: u32,
}

impl WmfTernaryRasterOperation {
  pub const fn new(raw: u32) -> Self {
    Self { raw }
  }

  pub const fn from_operation_code(code: WmfTernaryRasterOperationCode, low_word: u16) -> Self {
    Self {
      raw: ((code.raw() as u32) << 16) | low_word as u32,
    }
  }

  pub const fn raw(self) -> u32 {
    self.raw
  }

  pub const fn operation_code_raw(self) -> u8 {
    ((self.raw >> 16) & 0xFF) as u8
  }

  pub const fn operation_code(self) -> WmfTernaryRasterOperationCode {
    WmfTernaryRasterOperationCode::from_raw(self.operation_code_raw())
  }

  pub const fn uses_source(self) -> bool {
    self.operation_code().uses_source()
  }

  pub const fn canonical_raw(self) -> u32 {
    self.operation_code().canonical_raw()
  }

  pub const fn is_valid(self) -> bool {
    self.raw == self.canonical_raw()
  }
}

const WMF_TERNARY_RASTER_OPERATION_VALUES: [u32; 256] = [
  0x00000042, 0x00010289, 0x00020C89, 0x000300AA, 0x00040C88, 0x000500A9, 0x00060865, 0x000702C5,
  0x00080F08, 0x00090245, 0x000A0329, 0x000B0B2A, 0x000C0324, 0x000D0B25, 0x000E08A5, 0x000F0001,
  0x00100C85, 0x001100A6, 0x00120868, 0x001302C8, 0x00140869, 0x001502C9, 0x00165CCA, 0x00171D54,
  0x00180D59, 0x00191CC8, 0x001A06C5, 0x001B0768, 0x001C06CA, 0x001D0766, 0x001E01A5, 0x001F0385,
  0x00200F09, 0x00210248, 0x00220326, 0x00230B24, 0x00240D55, 0x00251CC5, 0x002606C8, 0x00271868,
  0x00280369, 0x002916CA, 0x002A0CC9, 0x002B1D58, 0x002C0784, 0x002D060A, 0x002E064A, 0x002F0E2A,
  0x0030032A, 0x00310B28, 0x00320688, 0x00330008, 0x003406C4, 0x00351864, 0x003601A8, 0x00370388,
  0x0038078A, 0x00390604, 0x003A0644, 0x003B0E24, 0x003C004A, 0x003D18A4, 0x003E1B24, 0x003F00EA,
  0x00400F0A, 0x00410249, 0x00420D5D, 0x00431CC4, 0x00440328, 0x00450B29, 0x004606C6, 0x0047076A,
  0x00480368, 0x004916C5, 0x004A0789, 0x004B0605, 0x004C0CC8, 0x004D1954, 0x004E0645, 0x004F0E25,
  0x00500325, 0x00510B26, 0x005206C9, 0x00530764, 0x005408A9, 0x00550009, 0x005601A9, 0x00570389,
  0x00580785, 0x00590609, 0x005A0049, 0x005B18A9, 0x005C0649, 0x005D0E29, 0x005E1B29, 0x005F00E9,
  0x00600365, 0x006116C6, 0x00620786, 0x00630608, 0x00640788, 0x00650606, 0x00660046, 0x006718A8,
  0x006858A6, 0x00690145, 0x006A01E9, 0x006B178A, 0x006C01E8, 0x006D1785, 0x006E1E28, 0x006F0C65,
  0x00700CC5, 0x00711D5C, 0x00720648, 0x00730E28, 0x00740646, 0x00750E26, 0x00761B28, 0x007700E6,
  0x007801E5, 0x00791786, 0x007A1E29, 0x007B0C68, 0x007C1E24, 0x007D0C69, 0x007E0955, 0x007F03C9,
  0x008003E9, 0x00810975, 0x00820C49, 0x00831E04, 0x00840C48, 0x00851E05, 0x008617A6, 0x008701C5,
  0x008800C6, 0x00891B08, 0x008A0E06, 0x008B0666, 0x008C0E08, 0x008D0668, 0x008E1D7C, 0x008F0CE5,
  0x00900C45, 0x00911E08, 0x009217A9, 0x009301C4, 0x009417AA, 0x009501C9, 0x00960169, 0x0097588A,
  0x00981888, 0x00990066, 0x009A0709, 0x009B07A8, 0x009C0704, 0x009D07A6, 0x009E16E6, 0x009F0345,
  0x00A000C9, 0x00A11B05, 0x00A20E09, 0x00A30669, 0x00A41885, 0x00A50065, 0x00A60706, 0x00A707A5,
  0x00A803A9, 0x00A90189, 0x00AA0029, 0x00AB0889, 0x00AC0744, 0x00AD06E9, 0x00AE0B06, 0x00AF0229,
  0x00B00E05, 0x00B10665, 0x00B21974, 0x00B30CE8, 0x00B4070A, 0x00B507A9, 0x00B616E9, 0x00B70348,
  0x00B8074A, 0x00B906E6, 0x00BA0B09, 0x00BB0226, 0x00BC1CE4, 0x00BD0D7D, 0x00BE0269, 0x00BF08C9,
  0x00C000CA, 0x00C11B04, 0x00C21884, 0x00C3006A, 0x00C40E04, 0x00C50664, 0x00C60708, 0x00C707AA,
  0x00C803A8, 0x00C90184, 0x00CA0749, 0x00CB06E4, 0x00CC0020, 0x00CD0888, 0x00CE0B08, 0x00CF0224,
  0x00D00E0A, 0x00D1066A, 0x00D20705, 0x00D307A4, 0x00D41D78, 0x00D50CE9, 0x00D616EA, 0x00D70349,
  0x00D80745, 0x00D906E8, 0x00DA1CE9, 0x00DB0D75, 0x00DC0B04, 0x00DD0228, 0x00DE0268, 0x00DF08C8,
  0x00E003A5, 0x00E10185, 0x00E20746, 0x00E306EA, 0x00E40748, 0x00E506E5, 0x00E61CE8, 0x00E70D79,
  0x00E81D74, 0x00E95CE6, 0x00EA02E9, 0x00EB0849, 0x00EC02E8, 0x00ED0848, 0x00EE0086, 0x00EF0A08,
  0x00F00021, 0x00F10885, 0x00F20B05, 0x00F3022A, 0x00F40B0A, 0x00F50225, 0x00F60265, 0x00F708C5,
  0x00F802E5, 0x00F90845, 0x00FA0089, 0x00FB0A09, 0x00FC008A, 0x00FD0A0A, 0x00FE02A9, 0x00FF0062,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfPenLineStyle {
  Solid = 0x0000,
  Dash = 0x0001,
  Dot = 0x0002,
  DashDot = 0x0003,
  DashDotDot = 0x0004,
  Null = 0x0005,
  InsideFrame = 0x0006,
  UserStyle = 0x0007,
  Alternate = 0x0008,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfPenEndCap {
  Round = 0x0000,
  Square = 0x0100,
  Flat = 0x0200,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfPenJoin {
  Round = 0x0000,
  Bevel = 0x1000,
  Miter = 0x2000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfPenType {
  Cosmetic = 0x0000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfBrushStyle {
  Solid = 0x0000,
  Null = 0x0001,
  Hatched = 0x0002,
  Pattern = 0x0003,
  Indexed = 0x0004,
  DibPattern = 0x0005,
  DibPatternPt = 0x0006,
  Pattern8x8 = 0x0007,
  DibPattern8x8 = 0x0008,
  MonoPattern = 0x0009,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfFloodFillMode {
  Border = 0x0000,
  Surface = 0x0001,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfHatchStyle {
  Horizontal = 0x0000,
  Vertical = 0x0001,
  ForwardDiagonal = 0x0002,
  BackwardDiagonal = 0x0003,
  Cross = 0x0004,
  DiagonalCross = 0x0005,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfMapMode {
  Text = 0x0001,
  LoMetric = 0x0002,
  HiMetric = 0x0003,
  LoEnglish = 0x0004,
  HiEnglish = 0x0005,
  Twips = 0x0006,
  Isotropic = 0x0007,
  Anisotropic = 0x0008,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfMetafileEscape {
  NewFrame = 0x0001,
  AbortDoc = 0x0002,
  NextBand = 0x0003,
  SetColorTable = 0x0004,
  GetColorTable = 0x0005,
  FlushOut = 0x0006,
  DraftMode = 0x0007,
  QueryEscSupport = 0x0008,
  SetAbortProc = 0x0009,
  StartDoc = 0x000A,
  EndDoc = 0x000B,
  GetPhysPageSize = 0x000C,
  GetPrintingOffset = 0x000D,
  GetScalingFactor = 0x000E,
  MetaFile = 0x000F,
  SetPenWidth = 0x0010,
  SetCopyCount = 0x0011,
  SetPaperSource = 0x0012,
  PassThrough = 0x0013,
  GetTechnology = 0x0014,
  SetLineCap = 0x0015,
  SetLineJoin = 0x0016,
  SetMiterLimit = 0x0017,
  BandInfo = 0x0018,
  DrawPatternRect = 0x0019,
  GetVectorPenSize = 0x001A,
  GetVectorBrushSize = 0x001B,
  EnableDuplex = 0x001C,
  GetSetPaperBins = 0x001D,
  GetSetPrintOrient = 0x001E,
  EnumPaperBins = 0x001F,
  SetDibScaling = 0x0020,
  EpsPrinting = 0x0021,
  EnumPaperMetrics = 0x0022,
  GetSetPaperMetrics = 0x0023,
  PostScriptData = 0x0025,
  PostScriptIgnore = 0x0026,
  GetDeviceUnits = 0x002A,
  GetExtendedTextMetrics = 0x0100,
  GetPairKernTable = 0x0102,
  ExtTextOut = 0x0200,
  GetFaceName = 0x0201,
  DownloadFace = 0x0202,
  MetafileDriver = 0x0801,
  QueryDibSupport = 0x0C01,
  BeginPath = 0x1000,
  ClipToPath = 0x1001,
  EndPath = 0x1002,
  OpenChannel = 0x100E,
  DownloadHeader = 0x100F,
  CloseChannel = 0x1010,
  PostScriptPassThrough = 0x1013,
  EncapsulatedPostScript = 0x1014,
  PostScriptIdentify = 0x1015,
  PostScriptInjection = 0x1016,
  CheckJpegFormat = 0x1017,
  CheckPngFormat = 0x1018,
  GetPsFeatureSetting = 0x1019,
  MxdcEscape = 0x101A,
  SpclPassThrough2 = 0x11D8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum WmfPostScriptCap {
  NotSet = -2,
  Flat = 0,
  Round = 1,
  Square = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfPostScriptClipping {
  Save = 0x0000,
  Restore = 0x0001,
  Inclusive = 0x0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum WmfPostScriptFeatureSetting {
  NUp = 0x0000_0000,
  Output = 0x0000_0001,
  PsLevel = 0x0000_0002,
  CustomPaper = 0x0000_0003,
  Mirror = 0x0000_0004,
  Negative = 0x0000_0005,
  Protocol = 0x0000_0006,
  PrivateBegin = 0x0000_1000,
  PrivateEnd = 0x0000_1FFF,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "i32")]
pub enum WmfPostScriptJoin {
  NotSet = -2,
  Miter = 0,
  Round = 1,
  Bevel = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfMetafileType {
  Memory = 0x0001,
  Disk = 0x0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfMetafileVersion {
  Version100 = 0x0100,
  Version300 = 0x0300,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfMixMode {
  Transparent = 0x0001,
  Opaque = 0x0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfPolyFillMode {
  Alternate = 0x0001,
  Winding = 0x0002,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum WmfStretchMode {
  BlackOnWhite = 0x0001,
  WhiteOnBlack = 0x0002,
  ColorOnColor = 0x0003,
  Halftone = 0x0004,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmfRecordRef<'a> {
  pub function: u16,
  pub data: &'a [u8],
}

impl<'a> WmfRecordRef<'a> {
  pub fn function_kind(&self) -> Option<WmfRecordFunction> {
    WmfRecordFunction::from_raw(self.function)
  }

  pub fn normalized_function_kind(&self) -> Option<WmfRecordFunction> {
    normalized_wmf_record_function(self.function)
  }

  pub fn size_words(&self) -> Result<u32> {
    record_size_words_parts(self.data.len())
  }

  pub fn into_owned(self) -> WmfRecord {
    WmfRecord::new(self.function, self.data.to_vec())
  }

  pub fn embedded_source_present(&self) -> Result<Option<bool>> {
    match self.normalized_function_kind() {
      Some(
        WmfRecordFunction::BitBlt
        | WmfRecordFunction::DibBitBlt
        | WmfRecordFunction::DibStretchBlt
        | WmfRecordFunction::StretchBlt,
      ) => Ok(Some(has_bitmap_source_parts(
        self.function,
        self.data.len(),
      )?)),
      _ => Ok(None),
    }
  }

  pub fn parse_data(self) -> Result<WmfRecordData<'a>> {
    WmfRecordData::from_record_ref(self)
  }

  pub fn rebuild_typed(self) -> Result<WmfRecord> {
    self.parse_data()?.to_record_with_function(self.function)
  }
}

impl SdkSize for WmfRecordRef<'_> {
  fn sdk_size(&self) -> u64 {
    6 + self.data.len() as u64
  }
}

#[derive(Clone, Debug)]
pub struct WmfRecords<'a> {
  bytes: &'a [u8],
  offset: usize,
  remaining: usize,
}

impl<'a> Iterator for WmfRecords<'a> {
  type Item = WmfRecordRef<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.remaining == 0 {
      return None;
    }
    let size_words = u32::from_le_bytes(
      self.bytes[self.offset..self.offset + 4]
        .try_into()
        .expect("validated WMF record header"),
    ) as usize;
    let function = u16::from_le_bytes(
      self.bytes[self.offset + 4..self.offset + 6]
        .try_into()
        .expect("validated WMF record header"),
    );
    let size = size_words * 2;
    let data_start = self.offset + 6;
    let end = self.offset + size;
    self.offset = end;
    self.remaining -= 1;
    Some(WmfRecordRef {
      function,
      data: &self.bytes[data_start..end],
    })
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.remaining, Some(self.remaining))
  }
}

impl ExactSizeIterator for WmfRecords<'_> {}
impl std::iter::FusedIterator for WmfRecords<'_> {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmfMetafileRef<'a> {
  pub placeable_header: Option<WmfPlaceableHeader>,
  pub header: WmfHeader,
  records_bytes: &'a [u8],
  trailing_data: &'a [u8],
  record_count: usize,
}

impl<'a> WmfMetafileRef<'a> {
  pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(bytes));
    let placeable_header = if has_placeable_header(bytes) {
      Some(WmfPlaceableHeader::read_from(&mut reader)?)
    } else {
      None
    };
    let header = WmfHeader::read_from(&mut reader)?;
    let records_start = reader.position()? as usize;
    let (records_end, record_count) = scan_wmf_records(bytes, records_start)?;
    Ok(Self {
      placeable_header,
      header,
      records_bytes: &bytes[records_start..records_end],
      trailing_data: &bytes[records_end..],
      record_count,
    })
  }

  pub fn records(&self) -> WmfRecords<'a> {
    WmfRecords {
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

  pub fn into_owned(self) -> WmfMetafile {
    let records = self.records().map(WmfRecordRef::into_owned).collect();
    let trailing_data = self.trailing_data.to_vec();
    WmfMetafile {
      placeable_header: self.placeable_header,
      header: self.header,
      records,
      trailing_data,
    }
  }
}

fn scan_wmf_records(bytes: &[u8], mut offset: usize) -> Result<(usize, usize)> {
  let mut record_count = 0usize;
  loop {
    let header = bytes
      .get(offset..offset.saturating_add(6))
      .ok_or_else(|| Error::invalid(offset as u64, "WMF record header is truncated"))?;
    let size_words =
      u32::from_le_bytes(header[..4].try_into().expect("slice length checked")) as usize;
    let function = u16::from_le_bytes(header[4..].try_into().expect("slice length checked"));
    let size = size_words
      .checked_mul(2)
      .ok_or_else(|| Error::invalid(offset as u64, "WMF record size overflows"))?;
    if size < 6 {
      return Err(Error::invalid(
        offset as u64,
        "WMF record size is smaller than its header",
      ));
    }
    let end = offset
      .checked_add(size)
      .ok_or_else(|| Error::invalid(offset as u64, "WMF record size overflows"))?;
    if end > bytes.len() {
      return Err(Error::invalid(
        offset as u64,
        "WMF record extends past end of file",
      ));
    }
    record_count += 1;
    offset = end;
    if function == META_EOF {
      return Ok((offset, record_count));
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfMetafile {
  pub placeable_header: Option<WmfPlaceableHeader>,
  pub header: WmfHeader,
  pub records: Vec<WmfRecord>,
  pub trailing_data: Vec<u8>,
}

impl WmfMetafile {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    Ok(WmfMetafileRef::from_bytes(bytes)?.into_owned())
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut capacity = WMF_HEADER_SIZE as u64
      + self
        .placeable_header
        .as_ref()
        .map_or(0, |_| PLACEABLE_HEADER_SIZE as u64)
      + self.trailing_data.len() as u64;
    for record in &self.records {
      capacity = capacity
        .checked_add(u64::from(record_size_words(record)?) * 2)
        .ok_or_else(|| Error::invalid(0, "WMF serialized size overflows"))?;
    }
    let capacity = usize::try_from(capacity)
      .map_err(|_| Error::invalid(0, "WMF serialized size overflows usize"))?;
    let mut writer = Writer::new(Vec::with_capacity(capacity));
    self.write_to_writer(&mut writer)?;
    Ok(writer.into_inner())
  }

  pub fn write_to<W: std::io::Write>(&self, writer: W) -> Result<()> {
    self.write_to_writer(&mut Writer::new(writer))
  }

  fn write_to_writer<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    if let Some(header) = &self.placeable_header {
      header.write_to(writer)?;
    }
    self.header.write_to(writer)?;
    for record in &self.records {
      record.write_to(writer)?;
    }
    writer.write_all(&self.trailing_data)
  }

  pub fn computed_file_size_words(&self) -> Result<u32> {
    let mut total = u64::from(self.header.header_size_words);
    for record in &self.records {
      total = total
        .checked_add(u64::from(record_size_words(record)?))
        .ok_or_else(|| Error::invalid(0, "WMF FileSize overflows"))?;
    }
    if total > u64::from(u32::MAX) {
      return Err(Error::invalid(0, "WMF FileSize exceeds u32::MAX WORDs"));
    }
    Ok(total as u32)
  }

  pub fn computed_max_record_words(&self) -> Result<u32> {
    let mut max_record_words = 0;
    for record in &self.records {
      max_record_words = max_record_words.max(record_size_words(record)?);
    }
    Ok(max_record_words)
  }

  pub fn computed_number_of_objects(&self) -> Result<u16> {
    count_wmf_object_creation_records(&self.records)
  }

  pub fn validate_header_metrics(&self) -> Result<()> {
    let file_size_words = self.computed_file_size_words()?;
    if self.header.file_size_words != file_size_words {
      return Err(Error::invalid(
        0,
        "WMF header FileSize does not match records",
      ));
    }
    let max_record_words = self.computed_max_record_words()?;
    if self.header.max_record_words != max_record_words {
      return Err(Error::invalid(
        0,
        "WMF header MaxRecord does not match records",
      ));
    }
    let number_of_objects = self.computed_number_of_objects()?;
    if self.header.number_of_objects != number_of_objects {
      return Err(Error::invalid(
        0,
        "WMF header NumberOfObjects does not match object records",
      ));
    }
    validate_wmf_object_table_references(self.header.number_of_objects, &self.records)?;
    Ok(())
  }
}

impl SdkWrite for WmfMetafile {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    self.write_to_writer(writer)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_placeable_header_lossless")]
pub struct WmfPlaceableHeader {
  pub key: u32,
  pub handle: u16,
  pub left: i16,
  pub top: i16,
  pub right: i16,
  pub bottom: i16,
  pub inch: u16,
  pub reserved: u32,
  pub checksum: u16,
}

impl WmfPlaceableHeader {
  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    <Self as SdkRead>::read_from(reader)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    <Self as SdkWrite>::write_to(self, writer)
  }

  pub fn computed_checksum(&self) -> u16 {
    let mut checksum = 0u16;
    checksum ^= (self.key & 0xFFFF) as u16;
    checksum ^= (self.key >> 16) as u16;
    checksum ^= self.handle;
    checksum ^= self.left as u16;
    checksum ^= self.top as u16;
    checksum ^= self.right as u16;
    checksum ^= self.bottom as u16;
    checksum ^= self.inch;
    checksum ^= (self.reserved & 0xFFFF) as u16;
    checksum ^= (self.reserved >> 16) as u16;
    checksum
  }

  pub fn refresh_checksum(&mut self) {
    self.checksum = self.computed_checksum();
  }

  pub fn with_computed_checksum(mut self) -> Self {
    self.refresh_checksum();
    self
  }

  pub fn bounding_box_width(&self) -> i32 {
    i32::from(self.right) - i32::from(self.left)
  }

  pub fn bounding_box_height(&self) -> i32 {
    i32::from(self.bottom) - i32::from(self.top)
  }

  pub fn uses_twips(&self) -> bool {
    self.inch == 1440
  }

  pub fn validate(&self) -> Result<()> {
    validate_wmf_placeable_header(self)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_header")]
pub struct WmfHeader {
  pub metafile_type: u16,
  pub header_size_words: u16,
  pub version: u16,
  pub file_size_words: u32,
  pub number_of_objects: u16,
  pub max_record_words: u32,
  pub number_of_parameters: u16,
}

impl WmfHeader {
  pub fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    <Self as SdkRead>::read_from(reader)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    <Self as SdkWrite>::write_to(self, writer)
  }

  pub fn metafile_type_kind(&self) -> Option<WmfMetafileType> {
    WmfMetafileType::from_raw(self.metafile_type)
  }

  pub fn version_kind(&self) -> Option<WmfMetafileVersion> {
    WmfMetafileVersion::from_raw(self.version)
  }

  pub fn header_size_bytes(&self) -> u32 {
    u32::from(self.header_size_words) * 2
  }

  pub fn file_size_bytes(&self) -> u64 {
    u64::from(self.file_size_words) * 2
  }

  pub fn max_record_bytes(&self) -> u64 {
    u64::from(self.max_record_words) * 2
  }

  pub fn number_of_members_is_zero(&self) -> bool {
    self.number_of_parameters == 0
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfRecord {
  pub function: u16,
  pub data: Vec<u8>,
}

impl WmfRecord {
  pub fn new(function: u16, data: Vec<u8>) -> Self {
    Self { function, data }
  }

  pub fn function_kind(&self) -> Option<WmfRecordFunction> {
    WmfRecordFunction::from_raw(self.function)
  }

  pub fn normalized_function_kind(&self) -> Option<WmfRecordFunction> {
    normalized_wmf_record_function(self.function)
  }

  pub fn as_ref(&self) -> WmfRecordRef<'_> {
    WmfRecordRef {
      function: self.function,
      data: &self.data,
    }
  }

  pub fn size_words(&self) -> Result<u32> {
    record_size_words(self)
  }

  pub fn embedded_source_present(&self) -> Result<Option<bool>> {
    match self.normalized_function_kind() {
      Some(
        WmfRecordFunction::BitBlt
        | WmfRecordFunction::DibBitBlt
        | WmfRecordFunction::DibStretchBlt
        | WmfRecordFunction::StretchBlt,
      ) => Ok(Some(has_bitmap_source(self)?)),
      _ => Ok(None),
    }
  }

  pub fn parse_data(&self) -> Result<WmfRecordData<'_>> {
    WmfRecordData::from_record(self)
  }

  pub fn rebuild_typed(&self) -> Result<Self> {
    self.as_ref().rebuild_typed()
  }

  pub fn read_from<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    file_len: u64,
  ) -> Result<Self> {
    let offset = reader.position()?;
    let size_words = reader.read_u32()?;
    let function = reader.read_u16()?;
    let size_bytes = size_words
      .checked_mul(2)
      .ok_or_else(|| Error::invalid(offset, "WMF record size overflows"))?;
    if size_bytes < 6 {
      return Err(Error::invalid(
        offset,
        "WMF record size is smaller than its header",
      ));
    }
    let end = offset
      .checked_add(size_bytes as u64)
      .ok_or_else(|| Error::invalid(offset, "WMF record size overflows"))?;
    if end > file_len {
      return Err(Error::invalid(
        offset,
        "WMF record extends past end of file",
      ));
    }
    let data = reader.read_vec(size_bytes as usize - 6)?;
    Ok(Self { function, data })
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    let size_bytes = self
      .data
      .len()
      .checked_add(6)
      .ok_or_else(|| Error::invalid(writer.position().unwrap_or(0), "WMF record is too large"))?;
    if size_bytes % 2 != 0 {
      return Err(Error::invalid(
        writer.position()?,
        "WMF record data must include WORD alignment padding",
      ));
    }
    let size_words = size_bytes / 2;
    if size_words > u32::MAX as usize {
      return Err(Error::invalid(
        writer.position()?,
        "WMF record size exceeds u32::MAX WORDs",
      ));
    }
    writer.write_u32(size_words as u32)?;
    writer.write_u16(self.function)?;
    writer.write_all(&self.data)
  }
}

impl SdkWrite for WmfRecord {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    WmfRecord::write_to(self, writer)
  }
}

impl SdkSize for WmfRecord {
  fn sdk_size(&self) -> u64 {
    6 + self.data.len() as u64
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WmfRecordData<'a> {
  Eof(WmfEofRecord),
  RealizePalette,
  SaveDc,
  SetRelabs,
  SetBkMode(WmfU16Record),
  SetMapMode(WmfU16Record),
  SetRop2(WmfU16Record),
  SetPolyFillMode(WmfU16Record),
  SetStretchBltMode(WmfU16Record),
  SetTextAlign(WmfU16Record),
  SetTextCharExtra(WmfU16Record),
  SetLayout(WmfU16Record),
  ResizePalette(WmfU16Record),
  RestoreDc(WmfI16Record),
  SetMapperFlags(WmfU32Record),
  SetTextJustification(WmfTextJustificationRecord),
  SetBkColor(WmfColorRecord),
  SetTextColor(WmfColorRecord),
  SetWindowOrg(WmfPointRecord),
  SetWindowExt(WmfPointRecord),
  SetViewportOrg(WmfPointRecord),
  SetViewportExt(WmfPointRecord),
  OffsetWindowOrg(WmfPointRecord),
  OffsetViewportOrg(WmfPointRecord),
  OffsetClipRgn(WmfPointRecord),
  MoveTo(WmfPointRecord),
  LineTo(WmfPointRecord),
  ScaleWindowExt(WmfScaleExtRecord),
  ScaleViewportExt(WmfScaleExtRecord),
  ExcludeClipRect(WmfRectRecord),
  IntersectClipRect(WmfRectRecord),
  Ellipse(WmfRectRecord),
  Rectangle(WmfRectRecord),
  RoundRect(WmfRoundRectRecord),
  Arc(WmfArcRecord),
  Chord(WmfArcRecord),
  Pie(WmfArcRecord),
  BitBlt(WmfBitBltRecord),
  DibBitBlt(WmfDibBitBltRecord),
  DibStretchBlt(WmfDibStretchBltRecord),
  FloodFill(WmfFloodFillRecord),
  ExtFloodFill(WmfExtFloodFillRecord),
  SetDibToDev(WmfSetDibToDevRecord),
  SetPixel(WmfSetPixelRecord),
  StretchBlt(WmfStretchBltRecord),
  StretchDib(WmfStretchDibRecord),
  PatBlt(WmfPatBltRecord),
  Polygon(WmfPolyPointsRecord),
  Polyline(WmfPolyPointsRecord),
  PolyPolygon(WmfPolyPolygonRecord),
  FillRegion(WmfRegionBrushRecord),
  FrameRegion(WmfFrameRegionRecord),
  InvertRegion(WmfObjectIndexRecord),
  PaintRegion(WmfObjectIndexRecord),
  SelectClipRegion(WmfObjectIndexRecord),
  SelectObject(WmfObjectIndexRecord),
  SelectPalette(WmfObjectIndexRecord),
  DeleteObject(WmfObjectIndexRecord),
  CreateBrushIndirect(WmfLogBrushObject),
  CreateFontIndirect(WmfFontObject),
  CreatePalette(WmfPaletteObject),
  CreatePatternBrush(WmfCreatePatternBrushRecord),
  CreatePenIndirect(WmfCreatePenIndirectRecord),
  CreateRegion(WmfRegionObject),
  DibCreatePatternBrush(WmfDibCreatePatternBrushRecord),
  SetPalEntries(WmfPaletteObject),
  AnimatePalette(WmfPaletteObject),
  TextOut(WmfTextOutRecord),
  ExtTextOut(WmfExtTextOutRecord),
  Escape(WmfEscapeRecord),
  Unknown(WmfRecordRef<'a>),
}

impl<'a> WmfRecordData<'a> {
  pub fn validate_strict(&self) -> Result<()> {
    match self {
      Self::Eof(value) => ensure_empty_trailing_data(&value.trailing_data, "META_EOF"),
      Self::SetBkColor(value) | Self::SetTextColor(value) => value.color.validate_strict(),
      Self::SetTextAlign(value) => validate_wmf_set_text_align_strict(value),
      Self::FloodFill(value) => {
        value.color.validate_strict()?;
        ensure_empty_trailing_data(&value.trailing_data, "META_FLOODFILL")
      }
      Self::ExtFloodFill(value) => value.color.validate_strict(),
      Self::SetPixel(value) => value.color.validate_strict(),
      Self::CreateBrushIndirect(value) => {
        validate_wmf_log_brush_object_strict(value)?;
        value.color_ref.validate_strict()
      }
      Self::CreateFontIndirect(value) => validate_wmf_font_object_strict(value),
      Self::CreatePenIndirect(value) => {
        value.pen.color_ref.validate_strict()?;
        ensure_empty_trailing_data(&value.trailing_data, "META_CREATEPENINDIRECT")
      }
      Self::BitBlt(value) => validate_wmf_bitmap16_transfer_source_strict(value, "META_BITBLT"),
      Self::DibBitBlt(value) => validate_wmf_dib_transfer_source_strict(value, "META_DIBBITBLT"),
      Self::StretchBlt(value) => {
        validate_wmf_bitmap16_transfer_source_strict(value, "META_STRETCHBLT")
      }
      Self::DibStretchBlt(value) => {
        validate_wmf_dib_stretch_transfer_source_strict(value, "META_DIBSTRETCHBLT")
      }
      Self::StretchDib(value) => validate_wmf_stretch_dib_record_strict(value),
      Self::Polygon(value) => validate_wmf_poly_points_strict(value, "META_POLYGON", 2),
      _ => Ok(()),
    }
  }

  pub fn from_record(record: &'a WmfRecord) -> Result<Self> {
    Self::from_record_ref(record.as_ref())
  }

  pub fn from_record_ref(record: WmfRecordRef<'a>) -> Result<Self> {
    let data = record.data;
    Ok(match record.normalized_function_kind() {
      Some(WmfRecordFunction::Eof) => Self::Eof(WmfEofRecord {
        trailing_data: data.to_vec(),
      }),
      Some(WmfRecordFunction::RealizePalette) => {
        ensure_no_data(data, "META_REALIZEPALETTE")?;
        Self::RealizePalette
      }
      Some(WmfRecordFunction::SaveDc) => {
        ensure_no_data(data, "META_SAVEDC")?;
        Self::SaveDc
      }
      Some(WmfRecordFunction::SetRelabs) => {
        ensure_no_data(data, "META_SETRELABS")?;
        Self::SetRelabs
      }
      Some(WmfRecordFunction::SetBkMode) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_bk_mode(&value)?;
        Self::SetBkMode(value)
      }
      Some(WmfRecordFunction::SetMapMode) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_map_mode(&value)?;
        Self::SetMapMode(value)
      }
      Some(WmfRecordFunction::SetRop2) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_rop2(&value)?;
        Self::SetRop2(value)
      }
      Some(WmfRecordFunction::SetPolyFillMode) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_poly_fill_mode(&value)?;
        Self::SetPolyFillMode(value)
      }
      Some(WmfRecordFunction::SetStretchBltMode) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_stretch_blt_mode(&value)?;
        Self::SetStretchBltMode(value)
      }
      Some(WmfRecordFunction::SetTextAlign) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_text_align(&value)?;
        Self::SetTextAlign(value)
      }
      Some(WmfRecordFunction::SetTextCharExtra) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_text_char_extra(&value)?;
        Self::SetTextCharExtra(value)
      }
      Some(WmfRecordFunction::SetLayout) => {
        let value = WmfU16Record::read_data(data)?;
        validate_wmf_set_layout(&value)?;
        Self::SetLayout(value)
      }
      Some(WmfRecordFunction::ResizePalette) => {
        Self::ResizePalette(read_object(data, "META_RESIZEPALETTE")?)
      }
      Some(WmfRecordFunction::RestoreDc) => Self::RestoreDc(read_object(data, "META_RESTOREDC")?),
      Some(WmfRecordFunction::SetMapperFlags) => {
        Self::SetMapperFlags(read_object(data, "META_SETMAPPERFLAGS")?)
      }
      Some(WmfRecordFunction::SetTextJustification) => {
        Self::SetTextJustification(read_object(data, "META_SETTEXTJUSTIFICATION")?)
      }
      Some(WmfRecordFunction::SetBkColor) => {
        Self::SetBkColor(read_object(data, "META_SETBKCOLOR")?)
      }
      Some(WmfRecordFunction::SetTextColor) => {
        Self::SetTextColor(read_object(data, "META_SETTEXTCOLOR")?)
      }
      Some(WmfRecordFunction::SetWindowOrg) => {
        Self::SetWindowOrg(read_object(data, "META_SETWINDOWORG")?)
      }
      Some(WmfRecordFunction::SetWindowExt) => {
        Self::SetWindowExt(read_object(data, "META_SETWINDOWEXT")?)
      }
      Some(WmfRecordFunction::SetViewportOrg) => {
        Self::SetViewportOrg(read_object(data, "META_SETVIEWPORTORG")?)
      }
      Some(WmfRecordFunction::SetViewportExt) => {
        Self::SetViewportExt(read_object(data, "META_SETVIEWPORTEXT")?)
      }
      Some(WmfRecordFunction::OffsetWindowOrg) => {
        Self::OffsetWindowOrg(read_object(data, "META_OFFSETWINDOWORG")?)
      }
      Some(WmfRecordFunction::OffsetViewportOrg) => {
        Self::OffsetViewportOrg(read_object(data, "META_OFFSETVIEWPORTORG")?)
      }
      Some(WmfRecordFunction::OffsetClipRgn) => {
        Self::OffsetClipRgn(read_object(data, "META_OFFSETCLIPRGN")?)
      }
      Some(WmfRecordFunction::MoveTo) => Self::MoveTo(read_object(data, "META_MOVETO")?),
      Some(WmfRecordFunction::LineTo) => Self::LineTo(read_object(data, "META_LINETO")?),
      Some(WmfRecordFunction::ScaleWindowExt) => {
        Self::ScaleWindowExt(read_object(data, "META_SCALEWINDOWEXT")?)
      }
      Some(WmfRecordFunction::ScaleViewportExt) => {
        Self::ScaleViewportExt(read_object(data, "META_SCALEVIEWPORTEXT")?)
      }
      Some(WmfRecordFunction::ExcludeClipRect) => {
        Self::ExcludeClipRect(read_object(data, "META_EXCLUDECLIPRECT")?)
      }
      Some(WmfRecordFunction::IntersectClipRect) => {
        Self::IntersectClipRect(read_object(data, "META_INTERSECTCLIPRECT")?)
      }
      Some(WmfRecordFunction::Ellipse) => Self::Ellipse(read_object(data, "META_ELLIPSE")?),
      Some(WmfRecordFunction::Rectangle) => Self::Rectangle(read_object(data, "META_RECTANGLE")?),
      Some(WmfRecordFunction::RoundRect) => Self::RoundRect(read_object(data, "META_ROUNDRECT")?),
      Some(WmfRecordFunction::Arc) => Self::Arc(read_object(data, "META_ARC")?),
      Some(WmfRecordFunction::Chord) => Self::Chord(read_object(data, "META_CHORD")?),
      Some(WmfRecordFunction::Pie) => Self::Pie(read_object(data, "META_PIE")?),
      Some(WmfRecordFunction::BitBlt) => Self::BitBlt(WmfBitBltRecord::read_data(
        data,
        has_bitmap_source_parts(record.function, data.len())?,
        "META_BITBLT",
      )?),
      Some(WmfRecordFunction::DibBitBlt) => Self::DibBitBlt(WmfDibBitBltRecord::read_data(
        data,
        has_bitmap_source_parts(record.function, data.len())?,
      )?),
      Some(WmfRecordFunction::DibStretchBlt) => {
        Self::DibStretchBlt(WmfDibStretchBltRecord::read_data(
          data,
          has_bitmap_source_parts(record.function, data.len())?,
        )?)
      }
      Some(WmfRecordFunction::FloodFill) => Self::FloodFill(WmfFloodFillRecord::read_data(data)?),
      Some(WmfRecordFunction::ExtFloodFill) => {
        let value = read_object(data, "META_EXTFLOODFILL")?;
        validate_wmf_ext_flood_fill(&value)?;
        Self::ExtFloodFill(value)
      }
      Some(WmfRecordFunction::SetDibToDev) => {
        Self::SetDibToDev(WmfSetDibToDevRecord::read_data(data)?)
      }
      Some(WmfRecordFunction::SetPixel) => Self::SetPixel(read_object(data, "META_SETPIXEL")?),
      Some(WmfRecordFunction::StretchBlt) => Self::StretchBlt(WmfStretchBltRecord::read_data(
        data,
        has_bitmap_source_parts(record.function, data.len())?,
        "META_STRETCHBLT",
      )?),
      Some(WmfRecordFunction::StretchDib) => {
        Self::StretchDib(WmfStretchDibRecord::read_data(data)?)
      }
      Some(WmfRecordFunction::PatBlt) => Self::PatBlt(WmfPatBltRecord::read_data(data)?),
      Some(WmfRecordFunction::Polygon) => {
        Self::Polygon(WmfPolyPointsRecord::read_data(data, "META_POLYGON", 0)?)
      }
      Some(WmfRecordFunction::Polyline) => {
        Self::Polyline(WmfPolyPointsRecord::read_data(data, "META_POLYLINE", 0)?)
      }
      Some(WmfRecordFunction::PolyPolygon) => {
        Self::PolyPolygon(WmfPolyPolygonRecord::read_data(data)?)
      }
      Some(WmfRecordFunction::FillRegion) => {
        Self::FillRegion(read_object(data, "META_FILLREGION")?)
      }
      Some(WmfRecordFunction::FrameRegion) => {
        Self::FrameRegion(read_object(data, "META_FRAMEREGION")?)
      }
      Some(WmfRecordFunction::InvertRegion) => {
        Self::InvertRegion(read_object(data, "META_INVERTREGION")?)
      }
      Some(WmfRecordFunction::PaintRegion) => {
        Self::PaintRegion(read_object(data, "META_PAINTREGION")?)
      }
      Some(WmfRecordFunction::SelectClipRegion) => {
        Self::SelectClipRegion(read_object(data, "META_SELECTCLIPREGION")?)
      }
      Some(WmfRecordFunction::SelectObject) => {
        Self::SelectObject(read_object(data, "META_SELECTOBJECT")?)
      }
      Some(WmfRecordFunction::SelectPalette) => {
        Self::SelectPalette(read_object(data, "META_SELECTPALETTE")?)
      }
      Some(WmfRecordFunction::DeleteObject) => {
        Self::DeleteObject(read_object(data, "META_DELETEOBJECT")?)
      }
      Some(WmfRecordFunction::CreateBrushIndirect) => {
        let value = read_object(data, "META_CREATEBRUSHINDIRECT")?;
        validate_wmf_log_brush_object(&value)?;
        Self::CreateBrushIndirect(value)
      }
      Some(WmfRecordFunction::CreateFontIndirect) => {
        let value = WmfFontObject::read_data(data)?;
        Self::CreateFontIndirect(value)
      }
      Some(WmfRecordFunction::CreatePalette) => {
        let value = WmfPaletteObject::read_data(data, "META_CREATEPALETTE")?;
        validate_wmf_create_palette_record(&value)?;
        Self::CreatePalette(value)
      }
      Some(WmfRecordFunction::CreatePatternBrush) => {
        Self::CreatePatternBrush(WmfCreatePatternBrushRecord::read_data(data)?)
      }
      Some(WmfRecordFunction::CreatePenIndirect) => {
        Self::CreatePenIndirect(WmfCreatePenIndirectRecord::read_data(data)?)
      }
      Some(WmfRecordFunction::CreateRegion) => {
        Self::CreateRegion(WmfRegionObject::read_data(data)?)
      }
      Some(WmfRecordFunction::DibCreatePatternBrush) => {
        Self::DibCreatePatternBrush(WmfDibCreatePatternBrushRecord::read_data(data)?)
      }
      Some(WmfRecordFunction::SetPalEntries) => {
        Self::SetPalEntries(WmfPaletteObject::read_data(data, "META_SETPALENTRIES")?)
      }
      Some(WmfRecordFunction::AnimatePalette) => {
        Self::AnimatePalette(WmfPaletteObject::read_data(data, "META_ANIMATEPALETTE")?)
      }
      Some(WmfRecordFunction::TextOut) => Self::TextOut(WmfTextOutRecord::read_data(data)?),
      Some(WmfRecordFunction::ExtTextOut) => {
        Self::ExtTextOut(WmfExtTextOutRecord::read_data(data)?)
      }
      Some(WmfRecordFunction::Escape) => Self::Escape(WmfEscapeRecord::read_data(data)?),
      _ => Self::Unknown(record),
    })
  }

  pub fn to_record(&self) -> Result<WmfRecord> {
    Ok(match self {
      Self::Eof(value) => WmfRecord::new(WmfRecordFunction::Eof.raw(), value.trailing_data.clone()),
      Self::RealizePalette => no_data_record(WmfRecordFunction::RealizePalette),
      Self::SaveDc => no_data_record(WmfRecordFunction::SaveDc),
      Self::SetRelabs => no_data_record(WmfRecordFunction::SetRelabs),
      Self::SetBkMode(value) => {
        validate_wmf_set_bk_mode(value)?;
        u16_record(WmfRecordFunction::SetBkMode, value)?
      }
      Self::SetMapMode(value) => {
        validate_wmf_set_map_mode(value)?;
        u16_record(WmfRecordFunction::SetMapMode, value)?
      }
      Self::SetRop2(value) => {
        validate_wmf_set_rop2(value)?;
        u16_record(WmfRecordFunction::SetRop2, value)?
      }
      Self::SetPolyFillMode(value) => {
        validate_wmf_set_poly_fill_mode(value)?;
        u16_record(WmfRecordFunction::SetPolyFillMode, value)?
      }
      Self::SetStretchBltMode(value) => {
        validate_wmf_set_stretch_blt_mode(value)?;
        u16_record(WmfRecordFunction::SetStretchBltMode, value)?
      }
      Self::SetTextAlign(value) => {
        validate_wmf_set_text_align(value)?;
        u16_record(WmfRecordFunction::SetTextAlign, value)?
      }
      Self::SetTextCharExtra(value) => {
        validate_wmf_set_text_char_extra(value)?;
        object_record(WmfRecordFunction::SetTextCharExtra, value)?
      }
      Self::SetLayout(value) => {
        validate_wmf_set_layout(value)?;
        u16_record(WmfRecordFunction::SetLayout, value)?
      }
      Self::ResizePalette(value) => object_record(WmfRecordFunction::ResizePalette, value)?,
      Self::RestoreDc(value) => object_record(WmfRecordFunction::RestoreDc, value)?,
      Self::SetMapperFlags(value) => object_record(WmfRecordFunction::SetMapperFlags, value)?,
      Self::SetTextJustification(value) => {
        object_record(WmfRecordFunction::SetTextJustification, value)?
      }
      Self::SetBkColor(value) => object_record(WmfRecordFunction::SetBkColor, value)?,
      Self::SetTextColor(value) => object_record(WmfRecordFunction::SetTextColor, value)?,
      Self::SetWindowOrg(value) => object_record(WmfRecordFunction::SetWindowOrg, value)?,
      Self::SetWindowExt(value) => object_record(WmfRecordFunction::SetWindowExt, value)?,
      Self::SetViewportOrg(value) => object_record(WmfRecordFunction::SetViewportOrg, value)?,
      Self::SetViewportExt(value) => object_record(WmfRecordFunction::SetViewportExt, value)?,
      Self::OffsetWindowOrg(value) => object_record(WmfRecordFunction::OffsetWindowOrg, value)?,
      Self::OffsetViewportOrg(value) => object_record(WmfRecordFunction::OffsetViewportOrg, value)?,
      Self::OffsetClipRgn(value) => object_record(WmfRecordFunction::OffsetClipRgn, value)?,
      Self::MoveTo(value) => object_record(WmfRecordFunction::MoveTo, value)?,
      Self::LineTo(value) => object_record(WmfRecordFunction::LineTo, value)?,
      Self::ScaleWindowExt(value) => object_record(WmfRecordFunction::ScaleWindowExt, value)?,
      Self::ScaleViewportExt(value) => object_record(WmfRecordFunction::ScaleViewportExt, value)?,
      Self::ExcludeClipRect(value) => object_record(WmfRecordFunction::ExcludeClipRect, value)?,
      Self::IntersectClipRect(value) => object_record(WmfRecordFunction::IntersectClipRect, value)?,
      Self::Ellipse(value) => object_record(WmfRecordFunction::Ellipse, value)?,
      Self::Rectangle(value) => object_record(WmfRecordFunction::Rectangle, value)?,
      Self::RoundRect(value) => object_record(WmfRecordFunction::RoundRect, value)?,
      Self::Arc(value) => object_record(WmfRecordFunction::Arc, value)?,
      Self::Chord(value) => object_record(WmfRecordFunction::Chord, value)?,
      Self::Pie(value) => object_record(WmfRecordFunction::Pie, value)?,
      Self::BitBlt(value) => WmfRecord::new(WmfRecordFunction::BitBlt.raw(), value.write_data()?),
      Self::DibBitBlt(value) => {
        WmfRecord::new(WmfRecordFunction::DibBitBlt.raw(), value.write_data()?)
      }
      Self::DibStretchBlt(value) => {
        WmfRecord::new(WmfRecordFunction::DibStretchBlt.raw(), value.write_data()?)
      }
      Self::FloodFill(value) => {
        WmfRecord::new(WmfRecordFunction::FloodFill.raw(), value.write_data()?)
      }
      Self::ExtFloodFill(value) => {
        validate_wmf_ext_flood_fill(value)?;
        object_record(WmfRecordFunction::ExtFloodFill, value)?
      }
      Self::SetDibToDev(value) => {
        WmfRecord::new(WmfRecordFunction::SetDibToDev.raw(), value.write_data()?)
      }
      Self::SetPixel(value) => object_record(WmfRecordFunction::SetPixel, value)?,
      Self::StretchBlt(value) => {
        WmfRecord::new(WmfRecordFunction::StretchBlt.raw(), value.write_data()?)
      }
      Self::StretchDib(value) => {
        WmfRecord::new(WmfRecordFunction::StretchDib.raw(), value.write_data()?)
      }
      Self::PatBlt(value) => {
        validate_wmf_ternary_raster_operation(value.raster_operation, "META_PATBLT")?;
        object_record(WmfRecordFunction::PatBlt, value)?
      }
      Self::Polygon(value) => WmfRecord::new(
        WmfRecordFunction::Polygon.raw(),
        value.write_data("META_POLYGON", 0)?,
      ),
      Self::Polyline(value) => WmfRecord::new(
        WmfRecordFunction::Polyline.raw(),
        value.write_data("META_POLYLINE", 0)?,
      ),
      Self::PolyPolygon(value) => {
        WmfRecord::new(WmfRecordFunction::PolyPolygon.raw(), value.write_data()?)
      }
      Self::FillRegion(value) => object_record(WmfRecordFunction::FillRegion, value)?,
      Self::FrameRegion(value) => object_record(WmfRecordFunction::FrameRegion, value)?,
      Self::InvertRegion(value) => object_record(WmfRecordFunction::InvertRegion, value)?,
      Self::PaintRegion(value) => object_record(WmfRecordFunction::PaintRegion, value)?,
      Self::SelectClipRegion(value) => object_record(WmfRecordFunction::SelectClipRegion, value)?,
      Self::SelectObject(value) => object_record(WmfRecordFunction::SelectObject, value)?,
      Self::SelectPalette(value) => object_record(WmfRecordFunction::SelectPalette, value)?,
      Self::DeleteObject(value) => object_record(WmfRecordFunction::DeleteObject, value)?,
      Self::CreateBrushIndirect(value) => {
        validate_wmf_log_brush_object(value)?;
        object_record(WmfRecordFunction::CreateBrushIndirect, value)?
      }
      Self::CreateFontIndirect(value) => {
        validate_wmf_font_object(value)?;
        WmfRecord::new(
          WmfRecordFunction::CreateFontIndirect.raw(),
          value.write_data()?,
        )
      }
      Self::CreatePalette(value) => {
        validate_wmf_create_palette_record(value)?;
        WmfRecord::new(
          WmfRecordFunction::CreatePalette.raw(),
          value.write_data("META_CREATEPALETTE")?,
        )
      }
      Self::CreatePatternBrush(value) => WmfRecord::new(
        WmfRecordFunction::CreatePatternBrush.raw(),
        value.write_data()?,
      ),
      Self::CreatePenIndirect(value) => WmfRecord::new(
        WmfRecordFunction::CreatePenIndirect.raw(),
        value.write_data()?,
      ),
      Self::CreateRegion(value) => object_record(WmfRecordFunction::CreateRegion, value)?,
      Self::DibCreatePatternBrush(value) => WmfRecord::new(
        WmfRecordFunction::DibCreatePatternBrush.raw(),
        value.write_data()?,
      ),
      Self::SetPalEntries(value) => WmfRecord::new(
        WmfRecordFunction::SetPalEntries.raw(),
        value.write_data("META_SETPALENTRIES")?,
      ),
      Self::AnimatePalette(value) => WmfRecord::new(
        WmfRecordFunction::AnimatePalette.raw(),
        value.write_data("META_ANIMATEPALETTE")?,
      ),
      Self::TextOut(value) => WmfRecord::new(WmfRecordFunction::TextOut.raw(), value.write_data()?),
      Self::ExtTextOut(value) => {
        WmfRecord::new(WmfRecordFunction::ExtTextOut.raw(), value.write_data()?)
      }
      Self::Escape(value) => WmfRecord::new(WmfRecordFunction::Escape.raw(), value.write_data()?),
      Self::Unknown(record) => {
        validate_unknown_wmf_record(record.function)?;
        WmfRecordRef::into_owned(*record)
      }
    })
  }

  pub fn to_record_with_function(&self, function: u16) -> Result<WmfRecord> {
    let mut record = self.to_record()?;
    let canonical_function = record.function;
    if function != canonical_function {
      let canonical_kind = normalized_wmf_record_function(canonical_function);
      let requested_kind = normalized_wmf_record_function(function);
      let bitmap_function_has_meaningful_high_byte = matches!(
        canonical_kind,
        Some(
          WmfRecordFunction::BitBlt
            | WmfRecordFunction::DibBitBlt
            | WmfRecordFunction::DibStretchBlt
            | WmfRecordFunction::StretchBlt
        )
      );
      if matches!(self, Self::Unknown(_))
        || requested_kind != canonical_kind
        || bitmap_function_has_meaningful_high_byte
      {
        return Err(Error::invalid(
          0,
          "WMF RecordFunction does not identify the supplied typed record data",
        ));
      }
    }
    record.function = function;
    Ok(record)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfU16Record {
  pub value: u16,
  pub reserved: Vec<u8>,
}

impl WmfU16Record {
  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 2 {
      return Err(Error::invalid(0, "WMF u16 record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let value = reader.read_u16()?;
    let reserved = reader.read_vec(data.len() - 2)?;
    Ok(Self { value, reserved })
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u16(self.value)?;
    writer.write_all(&self.reserved)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn text_alignment_flags(&self) -> WmfTextAlignmentModeFlags {
    WmfTextAlignmentModeFlags::from_bits_retain(self.value)
  }

  pub fn vertical_text_alignment_flags(&self) -> WmfVerticalTextAlignmentModeFlags {
    WmfVerticalTextAlignmentModeFlags::from_bits_retain(self.value)
  }

  pub fn invalid_text_alignment_bits(&self) -> u16 {
    let allowed =
      WmfTextAlignmentModeFlags::all().bits() | WmfVerticalTextAlignmentModeFlags::all().bits();
    self.value & !allowed
  }

  pub fn mix_mode_kind(&self) -> Option<WmfMixMode> {
    WmfMixMode::from_raw(self.value)
  }

  pub fn map_mode_kind(&self) -> Option<WmfMapMode> {
    WmfMapMode::from_raw(self.value)
  }

  pub fn binary_raster_operation_kind(&self) -> Option<WmfBinaryRasterOperation> {
    WmfBinaryRasterOperation::from_raw(self.value)
  }

  pub fn poly_fill_mode_kind(&self) -> Option<WmfPolyFillMode> {
    WmfPolyFillMode::from_raw(self.value)
  }

  pub fn stretch_mode_kind(&self) -> Option<WmfStretchMode> {
    WmfStretchMode::from_raw(self.value)
  }

  pub fn layout_flags(&self) -> WmfLayoutFlags {
    WmfLayoutFlags::from_bits_retain(self.value)
  }

  pub fn invalid_layout_bits(&self) -> u16 {
    self.value & !WmfLayoutFlags::all().bits()
  }
}

impl SdkRead for WmfU16Record {
  fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Ok(Self {
      value: reader.read_u16()?,
      reserved: Vec::new(),
    })
  }
}

impl SdkWrite for WmfU16Record {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    writer.write_u16(self.value)?;
    writer.write_all(&self.reserved)
  }
}

impl SdkSize for WmfU16Record {
  fn sdk_size(&self) -> u64 {
    2 + self.reserved.len() as u64
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfI16Record {
  pub value: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfU32Record {
  pub value: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfColorRecord {
  pub color: ColorRef,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfPointRecord {
  pub y: i16,
  pub x: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfRectRecord {
  pub bottom: i16,
  pub right: i16,
  pub top: i16,
  pub left: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_scale_ext_record")]
pub struct WmfScaleExtRecord {
  pub y_denom: i16,
  pub y_num: i16,
  pub x_denom: i16,
  pub x_num: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfTextJustificationRecord {
  pub break_count: u16,
  pub break_extra: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WmfEofRecord {
  pub trailing_data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfRoundRectRecord {
  pub height: i16,
  pub width: i16,
  pub bottom: i16,
  pub right: i16,
  pub top: i16,
  pub left: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfArcRecord {
  pub y_radial_2: i16,
  pub x_radial_2: i16,
  pub y_radial_1: i16,
  pub x_radial_1: i16,
  pub bottom: i16,
  pub right: i16,
  pub top: i16,
  pub left: i16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WmfFloodFillRecord {
  pub color: ColorRef,
  pub y_start: i16,
  pub x_start: i16,
  pub trailing_data: Vec<u8>,
}

impl WmfFloodFillRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let color = ColorRef::read_from(&mut reader)?;
    let y_start = reader.read_i16()?;
    let x_start = reader.read_i16()?;
    let trailing_data = reader.read_vec(data.len() - 8)?;
    Ok(Self {
      color,
      y_start,
      x_start,
      trailing_data,
    })
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      8 + self.trailing_data.len(),
    )));
    self.color.write_to(&mut writer)?;
    writer.write_i16(self.y_start)?;
    writer.write_i16(self.x_start)?;
    writer.write_all(&self.trailing_data)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_ext_flood_fill")]
pub struct WmfExtFloodFillRecord {
  pub mode: u16,
  pub color: ColorRef,
  pub y: i16,
  pub x: i16,
}

impl WmfExtFloodFillRecord {
  pub fn mode_kind(&self) -> Option<WmfFloodFillMode> {
    WmfFloodFillMode::from_raw(self.mode)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfSetPixelRecord {
  pub color: ColorRef,
  pub y: i16,
  pub x: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_pat_blt_record")]
pub struct WmfPatBltRecord {
  pub raster_operation: u32,
  pub height: i16,
  pub width: i16,
  pub y_left: i16,
  pub x_left: i16,
}

impl WmfPatBltRecord {
  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  fn read_data(data: &[u8]) -> Result<Self> {
    read_object(data, "META_PATBLT")
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WmfBitmap16Target {
  Source(Vec<u8>),
  NoSource { reserved: u16 },
}

impl WmfBitmap16Target {
  pub fn is_source_present(&self) -> bool {
    matches!(self, Self::Source(_))
  }

  pub fn source_bytes(&self) -> Option<&[u8]> {
    match self {
      Self::Source(value) => Some(value),
      Self::NoSource { .. } => None,
    }
  }

  pub fn bitmap16(&self) -> Result<Option<WmfBitmap16>> {
    self
      .source_bytes()
      .map(WmfBitmap16::read_from_slice)
      .transpose()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WmfDibTarget {
  Source(Vec<u8>),
  NoSource { reserved: u16 },
}

impl WmfDibTarget {
  pub fn is_source_present(&self) -> bool {
    matches!(self, Self::Source(_))
  }

  pub fn source_bytes(&self) -> Option<&[u8]> {
    match self {
      Self::Source(value) => Some(value),
      Self::NoSource { .. } => None,
    }
  }

  pub fn device_independent_bitmap(
    &self,
    color_usage: DibColorUsage,
  ) -> Result<Option<DeviceIndependentBitmap>> {
    self
      .source_bytes()
      .map(|bytes| DeviceIndependentBitmap::from_packed_slice(bytes, color_usage))
      .transpose()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfBitBltRecord {
  pub raster_operation: u32,
  pub y_src: i16,
  pub x_src: i16,
  pub height: i16,
  pub width: i16,
  pub y_dest: i16,
  pub x_dest: i16,
  pub target: WmfBitmap16Target,
}

impl WmfBitBltRecord {
  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  fn read_data(data: &[u8], has_source: bool, name: &str) -> Result<Self> {
    if data.len() < if has_source { 16 } else { 18 } {
      return Err(Error::invalid(0, "META_BITBLT record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let raster_operation = reader.read_u32()?;
    let y_src = reader.read_i16()?;
    let x_src = reader.read_i16()?;
    let target = if has_source {
      None
    } else {
      Some(reader.read_u16()?)
    };
    let height = reader.read_i16()?;
    let width = reader.read_i16()?;
    let y_dest = reader.read_i16()?;
    let x_dest = reader.read_i16()?;
    let target = match target {
      Some(reserved) => WmfBitmap16Target::NoSource { reserved },
      None => WmfBitmap16Target::Source(read_remaining(&mut reader, data.len())?),
    };
    let value = Self {
      raster_operation,
      y_src,
      x_src,
      height,
      width,
      y_dest,
      x_dest,
      target,
    };
    validate_wmf_bitmap16_transfer_source(&value, name)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_bitmap16_transfer_source(self, "META_BITBLT")?;
    let capacity = match &self.target {
      WmfBitmap16Target::Source(target) => 16usize.checked_add(target.len()),
      WmfBitmap16Target::NoSource { .. } => Some(18),
    }
    .ok_or_else(|| Error::invalid(0, "META_BITBLT serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    writer.write_u32(self.raster_operation)?;
    writer.write_i16(self.y_src)?;
    writer.write_i16(self.x_src)?;
    if let WmfBitmap16Target::NoSource { reserved } = self.target {
      writer.write_u16(reserved)?;
    }
    writer.write_i16(self.height)?;
    writer.write_i16(self.width)?;
    writer.write_i16(self.y_dest)?;
    writer.write_i16(self.x_dest)?;
    if let WmfBitmap16Target::Source(target) = &self.target {
      writer.write_all(target)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfDibBitBltRecord {
  pub raster_operation: u32,
  pub y_src: i16,
  pub x_src: i16,
  pub height: i16,
  pub width: i16,
  pub y_dest: i16,
  pub x_dest: i16,
  pub target: WmfDibTarget,
}

impl WmfDibBitBltRecord {
  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  fn read_data(data: &[u8], has_source: bool) -> Result<Self> {
    if data.len() < if has_source { 16 } else { 18 } {
      return Err(Error::invalid(0, "META_DIBBITBLT record is too short"));
    }
    let bit_blt = WmfBitBltRecord::read_data(data, has_source, "META_DIBBITBLT")?;
    Ok(Self {
      raster_operation: bit_blt.raster_operation,
      y_src: bit_blt.y_src,
      x_src: bit_blt.x_src,
      height: bit_blt.height,
      width: bit_blt.width,
      y_dest: bit_blt.y_dest,
      x_dest: bit_blt.x_dest,
      target: match bit_blt.target {
        WmfBitmap16Target::Source(target) => WmfDibTarget::Source(target),
        WmfBitmap16Target::NoSource { reserved } => WmfDibTarget::NoSource { reserved },
      },
    })
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_dib_transfer_source(self, "META_DIBBITBLT")?;
    let capacity = match &self.target {
      WmfDibTarget::Source(target) => 16usize.checked_add(target.len()),
      WmfDibTarget::NoSource { .. } => Some(18),
    }
    .ok_or_else(|| Error::invalid(0, "META_DIBBITBLT serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    writer.write_u32(self.raster_operation)?;
    writer.write_i16(self.y_src)?;
    writer.write_i16(self.x_src)?;
    if let WmfDibTarget::NoSource { reserved } = self.target {
      writer.write_u16(reserved)?;
    }
    writer.write_i16(self.height)?;
    writer.write_i16(self.width)?;
    writer.write_i16(self.y_dest)?;
    writer.write_i16(self.x_dest)?;
    if let WmfDibTarget::Source(target) = &self.target {
      writer.write_all(target)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfStretchBltRecord {
  pub raster_operation: u32,
  pub src_height: i16,
  pub src_width: i16,
  pub y_src: i16,
  pub x_src: i16,
  pub dest_height: i16,
  pub dest_width: i16,
  pub y_dest: i16,
  pub x_dest: i16,
  pub target: WmfBitmap16Target,
}

impl WmfStretchBltRecord {
  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  fn read_data(data: &[u8], has_source: bool, name: &str) -> Result<Self> {
    if data.len() < if has_source { 20 } else { 22 } {
      return Err(Error::invalid(0, "META_STRETCHBLT record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let raster_operation = reader.read_u32()?;
    let src_height = reader.read_i16()?;
    let src_width = reader.read_i16()?;
    let y_src = reader.read_i16()?;
    let x_src = reader.read_i16()?;
    let target = if has_source {
      None
    } else {
      Some(reader.read_u16()?)
    };
    let dest_height = reader.read_i16()?;
    let dest_width = reader.read_i16()?;
    let y_dest = reader.read_i16()?;
    let x_dest = reader.read_i16()?;
    let target = match target {
      Some(reserved) => WmfBitmap16Target::NoSource { reserved },
      None => WmfBitmap16Target::Source(read_remaining(&mut reader, data.len())?),
    };
    let value = Self {
      raster_operation,
      src_height,
      src_width,
      y_src,
      x_src,
      dest_height,
      dest_width,
      y_dest,
      x_dest,
      target,
    };
    validate_wmf_bitmap16_transfer_source(&value, name)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_bitmap16_transfer_source(self, "META_STRETCHBLT")?;
    let capacity = match &self.target {
      WmfBitmap16Target::Source(target) => 20usize.checked_add(target.len()),
      WmfBitmap16Target::NoSource { .. } => Some(22),
    }
    .ok_or_else(|| Error::invalid(0, "META_STRETCHBLT serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    writer.write_u32(self.raster_operation)?;
    writer.write_i16(self.src_height)?;
    writer.write_i16(self.src_width)?;
    writer.write_i16(self.y_src)?;
    writer.write_i16(self.x_src)?;
    if let WmfBitmap16Target::NoSource { reserved } = self.target {
      writer.write_u16(reserved)?;
    }
    writer.write_i16(self.dest_height)?;
    writer.write_i16(self.dest_width)?;
    writer.write_i16(self.y_dest)?;
    writer.write_i16(self.x_dest)?;
    if let WmfBitmap16Target::Source(target) = &self.target {
      writer.write_all(target)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfDibStretchBltRecord {
  pub raster_operation: u32,
  pub src_height: i16,
  pub src_width: i16,
  pub y_src: i16,
  pub x_src: i16,
  pub dest_height: i16,
  pub dest_width: i16,
  pub y_dest: i16,
  pub x_dest: i16,
  pub target: WmfDibTarget,
}

impl WmfDibStretchBltRecord {
  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  fn read_data(data: &[u8], has_source: bool) -> Result<Self> {
    if data.len() < if has_source { 20 } else { 22 } {
      return Err(Error::invalid(0, "META_DIBSTRETCHBLT record is too short"));
    }
    let stretch_blt = WmfStretchBltRecord::read_data(data, has_source, "META_DIBSTRETCHBLT")?;
    Ok(Self {
      raster_operation: stretch_blt.raster_operation,
      src_height: stretch_blt.src_height,
      src_width: stretch_blt.src_width,
      y_src: stretch_blt.y_src,
      x_src: stretch_blt.x_src,
      dest_height: stretch_blt.dest_height,
      dest_width: stretch_blt.dest_width,
      y_dest: stretch_blt.y_dest,
      x_dest: stretch_blt.x_dest,
      target: match stretch_blt.target {
        WmfBitmap16Target::Source(target) => WmfDibTarget::Source(target),
        WmfBitmap16Target::NoSource { reserved } => WmfDibTarget::NoSource { reserved },
      },
    })
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_dib_stretch_transfer_source(self, "META_DIBSTRETCHBLT")?;
    let capacity = match &self.target {
      WmfDibTarget::Source(target) => 20usize.checked_add(target.len()),
      WmfDibTarget::NoSource { .. } => Some(22),
    }
    .ok_or_else(|| Error::invalid(0, "META_DIBSTRETCHBLT serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    writer.write_u32(self.raster_operation)?;
    writer.write_i16(self.src_height)?;
    writer.write_i16(self.src_width)?;
    writer.write_i16(self.y_src)?;
    writer.write_i16(self.x_src)?;
    if let WmfDibTarget::NoSource { reserved } = self.target {
      writer.write_u16(reserved)?;
    }
    writer.write_i16(self.dest_height)?;
    writer.write_i16(self.dest_width)?;
    writer.write_i16(self.y_dest)?;
    writer.write_i16(self.x_dest)?;
    if let WmfDibTarget::Source(target) = &self.target {
      writer.write_all(target)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfSetDibToDevRecord {
  pub color_usage: u16,
  pub scan_count: u16,
  pub start_scan: u16,
  pub y_dib: u16,
  pub x_dib: u16,
  pub height: u16,
  pub width: u16,
  pub y_dest: u16,
  pub x_dest: u16,
  pub dib: Vec<u8>,
}

impl WmfSetDibToDevRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 18 {
      return Err(Error::invalid(0, "META_SETDIBTODEV record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let value = Self {
      color_usage: reader.read_u16()?,
      scan_count: reader.read_u16()?,
      start_scan: reader.read_u16()?,
      y_dib: reader.read_u16()?,
      x_dib: reader.read_u16()?,
      height: reader.read_u16()?,
      width: reader.read_u16()?,
      y_dest: reader.read_u16()?,
      x_dest: reader.read_u16()?,
      dib: read_remaining(&mut reader, data.len())?,
    };
    validate_wmf_set_dib_to_dev_record(&value)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_set_dib_to_dev_record(self)?;
    let capacity = 18usize
      .checked_add(self.dib.len())
      .ok_or_else(|| Error::invalid(0, "META_SETDIBTODEV serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    writer.write_u16(self.color_usage)?;
    writer.write_u16(self.scan_count)?;
    writer.write_u16(self.start_scan)?;
    writer.write_u16(self.y_dib)?;
    writer.write_u16(self.x_dib)?;
    writer.write_u16(self.height)?;
    writer.write_u16(self.width)?;
    writer.write_u16(self.y_dest)?;
    writer.write_u16(self.x_dest)?;
    writer.write_all(&self.dib)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_wmf_raw(self.color_usage)
  }

  pub fn dib_info(&self) -> Result<DibBitmapInfo> {
    let (info, _) = DibBitmapInfo::read_packed_prefix_from_slice(
      &self.dib,
      require_wmf_color_usage(self.color_usage)?,
    )?;
    Ok(info)
  }

  pub fn device_independent_bitmap(&self) -> Result<DeviceIndependentBitmap> {
    DeviceIndependentBitmap::from_packed_slice(
      &self.dib,
      require_wmf_color_usage(self.color_usage)?,
    )
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfStretchDibRecord {
  pub raster_operation: u32,
  pub color_usage: u16,
  pub src_height: i16,
  pub src_width: i16,
  pub y_src: i16,
  pub x_src: i16,
  pub dest_height: i16,
  pub dest_width: i16,
  pub y_dest: i16,
  pub x_dest: i16,
  pub dib: Vec<u8>,
}

impl WmfStretchDibRecord {
  pub const fn ternary_raster_operation(&self) -> WmfTernaryRasterOperation {
    WmfTernaryRasterOperation::new(self.raster_operation)
  }

  pub const fn raster_operation_code(&self) -> WmfTernaryRasterOperationCode {
    self.ternary_raster_operation().operation_code()
  }

  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 22 {
      return Err(Error::invalid(0, "META_STRETCHDIB record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let value = Self {
      raster_operation: reader.read_u32()?,
      color_usage: reader.read_u16()?,
      src_height: reader.read_i16()?,
      src_width: reader.read_i16()?,
      y_src: reader.read_i16()?,
      x_src: reader.read_i16()?,
      dest_height: reader.read_i16()?,
      dest_width: reader.read_i16()?,
      y_dest: reader.read_i16()?,
      x_dest: reader.read_i16()?,
      dib: read_remaining(&mut reader, data.len())?,
    };
    validate_wmf_stretch_dib_record(&value)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_stretch_dib_record(self)?;
    let capacity = 22usize
      .checked_add(self.dib.len())
      .ok_or_else(|| Error::invalid(0, "META_STRETCHDIB serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    writer.write_u32(self.raster_operation)?;
    writer.write_u16(self.color_usage)?;
    writer.write_i16(self.src_height)?;
    writer.write_i16(self.src_width)?;
    writer.write_i16(self.y_src)?;
    writer.write_i16(self.x_src)?;
    writer.write_i16(self.dest_height)?;
    writer.write_i16(self.dest_width)?;
    writer.write_i16(self.y_dest)?;
    writer.write_i16(self.x_dest)?;
    writer.write_all(&self.dib)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_wmf_raw(self.color_usage)
  }

  pub fn dib_info(&self) -> Result<DibBitmapInfo> {
    let (info, _) = DibBitmapInfo::read_packed_prefix_from_slice(
      &self.dib,
      require_wmf_color_usage(self.color_usage)?,
    )?;
    Ok(info)
  }

  pub fn device_independent_bitmap(&self) -> Result<DeviceIndependentBitmap> {
    DeviceIndependentBitmap::from_packed_slice(
      &self.dib,
      require_wmf_color_usage(self.color_usage)?,
    )
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfPolyPointsRecord {
  pub points: Vec<PointS>,
}

impl WmfPolyPointsRecord {
  fn read_data(data: &[u8], name: &str, min_points: usize) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let count = reader.read_i16()?;
    if count < 0 {
      return Err(Error::invalid(
        0,
        format!("{name} has negative point count"),
      ));
    }
    if (count as usize) < min_points {
      return Err(Error::invalid(
        0,
        format!("{name} point count must be at least {min_points}"),
      ));
    }
    let point_bytes = checked_record_array_bytes(count as usize, 4, name)?;
    ensure_record_remaining(&mut reader, data.len() as u64, point_bytes, name)?;
    let mut points = Vec::with_capacity(count as usize);
    for _ in 0..count {
      points.push(PointS::read_from(&mut reader)?);
    }
    ensure_reader_end(&mut reader, data.len() as u64, name)?;
    Ok(Self { points })
  }

  fn write_data(&self, name: &str, min_points: usize) -> Result<Vec<u8>> {
    if self.points.len() < min_points {
      return Err(Error::invalid(
        0,
        format!("{name} point count must be at least {min_points}"),
      ));
    }
    if self.points.len() > i16::MAX as usize {
      return Err(Error::invalid(0, format!("{name} has too many points")));
    }
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_i16(self.points.len() as i16)?;
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfPolyPolygonRecord {
  pub points_per_polygon: Vec<u16>,
  pub points: Vec<PointS>,
}

impl WmfPolyPolygonRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let number_of_polygons = reader.read_u16()? as usize;
    let count_bytes = checked_record_array_bytes(number_of_polygons, 2, "META_POLYPOLYGON counts")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      count_bytes,
      "META_POLYPOLYGON counts",
    )?;
    let mut points_per_polygon = Vec::with_capacity(number_of_polygons);
    let mut total_points = 0usize;
    for _ in 0..number_of_polygons {
      let count = reader.read_u16()?;
      total_points = total_points
        .checked_add(count as usize)
        .ok_or_else(|| Error::invalid(0, "META_POLYPOLYGON point count overflows"))?;
      points_per_polygon.push(count);
    }
    let point_bytes = checked_record_array_bytes(total_points, 4, "META_POLYPOLYGON points")?;
    ensure_record_remaining(
      &mut reader,
      data.len() as u64,
      point_bytes,
      "META_POLYPOLYGON points",
    )?;
    let mut points = Vec::with_capacity(total_points);
    for _ in 0..total_points {
      points.push(PointS::read_from(&mut reader)?);
    }
    ensure_reader_end(&mut reader, data.len() as u64, "META_POLYPOLYGON")?;
    Ok(Self {
      points_per_polygon,
      points,
    })
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    if self.points_per_polygon.len() > u16::MAX as usize {
      return Err(Error::invalid(0, "META_POLYPOLYGON has too many polygons"));
    }
    let expected_points = self
      .points_per_polygon
      .iter()
      .try_fold(0usize, |sum, count| {
        sum
          .checked_add(*count as usize)
          .ok_or_else(|| Error::invalid(0, "META_POLYPOLYGON point count overflows"))
      })?;
    if expected_points != self.points.len() {
      return Err(Error::invalid(
        0,
        "META_POLYPOLYGON point count does not match points",
      ));
    }
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u16(self.points_per_polygon.len() as u16)?;
    for count in &self.points_per_polygon {
      writer.write_u16(*count)?;
    }
    for point in &self.points {
      point.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfRegionBrushRecord {
  pub region: u16,
  pub brush: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfFrameRegionRecord {
  pub region: u16,
  pub brush: u16,
  pub height: i16,
  pub width: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfObjectIndexRecord {
  pub index: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_log_brush_object")]
pub struct WmfLogBrushObject {
  pub brush_style: u16,
  pub color_ref: ColorRef,
  pub brush_hatch: u16,
}

impl WmfLogBrushObject {
  pub fn brush_style_kind(&self) -> Option<WmfBrushStyle> {
    WmfBrushStyle::from_raw(self.brush_style)
  }

  pub fn hatch_style_kind(&self) -> Option<WmfHatchStyle> {
    WmfHatchStyle::from_raw(self.brush_hatch)
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_wmf_log_brush_object(self)?;
    writer.write_u16(self.brush_style)?;
    self.color_ref.write_to(writer)?;
    writer.write_u16(self.brush_hatch)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_pen_object")]
pub struct WmfPenObject {
  pub pen_style: u16,
  pub width: PointS,
  pub color_ref: ColorRef,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WmfCreatePenIndirectRecord {
  pub pen: WmfPenObject,
  pub trailing_data: Vec<u8>,
}

impl WmfCreatePenIndirectRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let pen = WmfPenObject::read_from(&mut reader)?;
    validate_wmf_pen_object(&pen)?;
    let trailing_data = reader.read_vec(data.len() - 10)?;
    Ok(Self { pen, trailing_data })
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_pen_object(&self.pen)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      10 + self.trailing_data.len(),
    )));
    self.pen.write_to(&mut writer)?;
    writer.write_all(&self.trailing_data)?;
    Ok(writer.into_inner().into_inner())
  }
}

impl WmfPenObject {
  pub fn pen_style_flags(&self) -> WmfPenStyleFlags {
    WmfPenStyleFlags::from_bits_retain(self.pen_style)
  }

  pub const fn pen_line_style_raw(&self) -> u16 {
    self.pen_style & 0x000F
  }

  pub fn pen_line_style_kind(&self) -> Option<WmfPenLineStyle> {
    WmfPenLineStyle::from_raw(self.pen_line_style_raw())
  }

  pub const fn pen_end_cap_raw(&self) -> u16 {
    self.pen_style & 0x0F00
  }

  pub fn pen_end_cap_kind(&self) -> Option<WmfPenEndCap> {
    WmfPenEndCap::from_raw(self.pen_end_cap_raw())
  }

  pub const fn pen_join_raw(&self) -> u16 {
    self.pen_style & 0xF000
  }

  pub fn pen_join_kind(&self) -> Option<WmfPenJoin> {
    WmfPenJoin::from_raw(self.pen_join_raw())
  }

  pub const fn pen_type_raw(&self) -> u16 {
    0x0000
  }

  pub fn pen_type_kind(&self) -> Option<WmfPenType> {
    WmfPenType::from_raw(self.pen_type_raw())
  }

  pub const fn pen_reserved_bits(&self) -> u16 {
    self.pen_style & 0x00F0
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_wmf_pen_object(self)?;
    writer.write_u16(self.pen_style)?;
    self.width.write_to(writer)?;
    self.color_ref.write_to(writer)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfPitchAndFamily {
  pub value: u8,
}

impl WmfPitchAndFamily {
  pub const fn pitch_raw(&self) -> u8 {
    self.value & 0x03
  }

  pub fn pitch_kind(&self) -> Option<WmfPitchFont> {
    WmfPitchFont::from_raw(self.pitch_raw())
  }

  pub const fn reserved_bits(&self) -> u8 {
    self.value & 0x0C
  }

  pub const fn family_raw(&self) -> u8 {
    (self.value >> 4) & 0x0F
  }

  pub fn family_kind(&self) -> Option<WmfFamilyFont> {
    WmfFamilyFont::from_raw(self.family_raw())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfFontObject {
  pub height: i16,
  pub width: i16,
  pub escapement: i16,
  pub orientation: i16,
  pub weight: i16,
  pub italic: u8,
  pub underline: u8,
  pub strike_out: u8,
  pub char_set: u8,
  pub out_precision: u8,
  pub clip_precision: u8,
  pub quality: u8,
  pub pitch_and_family: u8,
  pub face_name: [u8; 32],
  pub face_name_bytes: u8,
}

impl WmfFontObject {
  const FIXED_SIZE: usize = 18;

  fn read_data(data: &[u8]) -> Result<Self> {
    if !(Self::FIXED_SIZE..=Self::FIXED_SIZE + 32).contains(&data.len()) {
      return Err(Error::invalid(
        0,
        "META_CREATEFONTINDIRECT Font size is invalid",
      ));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let mut face_name = [0; 32];
    let face_name_bytes = data.len() - Self::FIXED_SIZE;
    face_name[..face_name_bytes].copy_from_slice(&data[Self::FIXED_SIZE..]);
    let value = Self {
      height: reader.read_i16()?,
      width: reader.read_i16()?,
      escapement: reader.read_i16()?,
      orientation: reader.read_i16()?,
      weight: reader.read_i16()?,
      italic: reader.read_u8()?,
      underline: reader.read_u8()?,
      strike_out: reader.read_u8()?,
      char_set: reader.read_u8()?,
      out_precision: reader.read_u8()?,
      clip_precision: reader.read_u8()?,
      quality: reader.read_u8()?,
      pitch_and_family: reader.read_u8()?,
      face_name,
      face_name_bytes: face_name_bytes as u8,
    };
    validate_wmf_font_object(&value)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_font_object(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(
      Self::FIXED_SIZE + usize::from(self.face_name_bytes),
    )));
    writer.write_i16(self.height)?;
    writer.write_i16(self.width)?;
    writer.write_i16(self.escapement)?;
    writer.write_i16(self.orientation)?;
    writer.write_i16(self.weight)?;
    writer.write_u8(self.italic)?;
    writer.write_u8(self.underline)?;
    writer.write_u8(self.strike_out)?;
    writer.write_u8(self.char_set)?;
    writer.write_u8(self.out_precision)?;
    writer.write_u8(self.clip_precision)?;
    writer.write_u8(self.quality)?;
    writer.write_u8(self.pitch_and_family)?;
    writer.write_all(&self.face_name[..usize::from(self.face_name_bytes)])?;
    Ok(writer.into_inner().into_inner())
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_palette_entry")]
pub struct WmfPaletteEntry {
  pub red: u8,
  pub green: u8,
  pub blue: u8,
  pub values: u8,
}

impl WmfPaletteEntry {
  pub fn flags(&self) -> WmfPaletteEntryFlags {
    WmfPaletteEntryFlags::from_bits_retain(self.values)
  }

  pub fn flag_kind(&self) -> Option<WmfPaletteEntryFlags> {
    match self.values {
      0 => Some(WmfPaletteEntryFlags::empty()),
      value
        if value == WmfPaletteEntryFlags::RESERVED.bits()
          || value == WmfPaletteEntryFlags::EXPLICIT.bits()
          || value == WmfPaletteEntryFlags::NO_COLLAPSE.bits() =>
      {
        WmfPaletteEntryFlags::from_bits(value)
      }
      _ => None,
    }
  }

  pub fn invalid_value_bits(&self) -> u8 {
    self.values & !WmfPaletteEntryFlags::all().bits()
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_wmf_palette_entry(self)?;
    writer.write_u8(self.red)?;
    writer.write_u8(self.green)?;
    writer.write_u8(self.blue)?;
    writer.write_u8(self.values)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfPaletteObject {
  pub start: u16,
  pub entries: Vec<WmfPaletteEntry>,
}

impl WmfPaletteObject {
  fn read_data(data: &[u8], name: &str) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let start = reader.read_u16()?;
    let number_of_entries = reader.read_u16()? as usize;
    let entry_bytes = checked_record_array_bytes(number_of_entries, 4, name)?;
    ensure_record_remaining(&mut reader, data.len() as u64, entry_bytes, name)?;
    let mut entries = Vec::with_capacity(number_of_entries);
    for _ in 0..number_of_entries {
      entries.push(WmfPaletteEntry::read_from(&mut reader)?);
    }
    ensure_reader_end(&mut reader, data.len() as u64, name)?;
    let value = Self { start, entries };
    validate_wmf_palette_object(&value)?;
    Ok(value)
  }

  fn write_data(&self, name: &str) -> Result<Vec<u8>> {
    validate_wmf_palette_object(self)?;
    if self.entries.len() > u16::MAX as usize {
      return Err(Error::invalid(0, format!("{name} has too many entries")));
    }
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u16(self.start)?;
    writer.write_u16(self.entries.len() as u16)?;
    for entry in &self.entries {
      entry.write_to(&mut writer)?;
    }
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_wmf_bitmap16_header")]
pub struct WmfBitmap16Header {
  pub bitmap_type: i16,
  pub width: i16,
  pub height: i16,
  pub width_bytes: i16,
  pub planes: u8,
  pub bits_pixel: u8,
}

impl WmfBitmap16Header {
  pub fn computed_width_bytes(&self) -> Result<usize> {
    if self.width < 0 || self.height < 0 {
      return Err(Error::invalid(
        0,
        "Bitmap16 dimensions must be non-negative",
      ));
    }
    let width =
      usize::try_from(self.width).map_err(|_| Error::invalid(0, "Bitmap16 width is invalid"))?;
    let bits_per_line = width
      .checked_mul(usize::from(self.bits_pixel))
      .ok_or_else(|| Error::invalid(0, "Bitmap16 scan line size overflows"))?;
    bits_per_line
      .checked_add(15)
      .map(|value| (value >> 4) << 1)
      .ok_or_else(|| Error::invalid(0, "Bitmap16 scan line size overflows"))
  }

  pub fn computed_bits_len(&self) -> Result<usize> {
    let height =
      usize::try_from(self.height).map_err(|_| Error::invalid(0, "Bitmap16 height is invalid"))?;
    self
      .computed_width_bytes()?
      .checked_mul(height)
      .ok_or_else(|| Error::invalid(0, "Bitmap16 bits size overflows"))
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfBitmap16 {
  pub header: WmfBitmap16Header,
  pub bits: Vec<u8>,
}

impl WmfBitmap16 {
  pub fn read_from_slice(bytes: &[u8]) -> Result<Self> {
    if bytes.len() < 10 {
      return Err(Error::invalid(0, "Bitmap16 object is too short"));
    }
    let mut reader = Reader::new(Cursor::new(bytes));
    let header = WmfBitmap16Header::read_from(&mut reader)?;
    let bits = reader.read_vec(bytes.len() - 10)?;
    let value = Self { header, bits };
    validate_wmf_bitmap16(&value)?;
    Ok(value)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    validate_wmf_bitmap16(self)?;
    let capacity = 10usize
      .checked_add(self.bits.len())
      .ok_or_else(|| Error::invalid(0, "Bitmap16 serialized size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    self.header.write_to(&mut writer)?;
    writer.write_all(&self.bits)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfCreatePatternBrushRecord {
  pub bitmap: WmfBitmap16Header,
  pub ignored_bits: u32,
  pub reserved: [u8; 18],
  pub pattern: Vec<u8>,
}

impl WmfCreatePatternBrushRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 32 {
      return Err(Error::invalid(
        0,
        "META_CREATEPATTERNBRUSH record is too short",
      ));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let bitmap = WmfBitmap16Header::read_from(&mut reader)?;
    let ignored_bits = reader.read_u32()?;
    let reserved = reader.read_array::<18>()?;
    let pattern = reader.read_vec(data.len() - 32)?;
    let value = Self {
      bitmap,
      ignored_bits,
      reserved,
      pattern,
    };
    validate_wmf_create_pattern_brush_record(&value)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_create_pattern_brush_record(self)?;
    let capacity = 32usize
      .checked_add(self.pattern.len())
      .ok_or_else(|| Error::invalid(0, "META_CREATEPATTERNBRUSH size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    self.bitmap.write_to(&mut writer)?;
    writer.write_u32(self.ignored_bits)?;
    writer.write_all(&self.reserved)?;
    writer.write_all(&self.pattern)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn bitmap16(&self) -> Result<WmfBitmap16> {
    validate_wmf_create_pattern_brush_record(self)?;
    Ok(WmfBitmap16 {
      header: self.bitmap,
      bits: self.pattern.clone(),
    })
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfDibCreatePatternBrushRecord {
  pub style: u16,
  pub color_usage: u16,
  pub target: Vec<u8>,
}

impl WmfDibCreatePatternBrushRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 4 {
      return Err(Error::invalid(
        0,
        "META_DIBCREATEPATTERNBRUSH record is too short",
      ));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let style = reader.read_u16()?;
    let color_usage = reader.read_u16()?;
    let target = reader.read_vec(data.len() - 4)?;
    let value = Self {
      style,
      color_usage,
      target,
    };
    validate_wmf_dib_create_pattern_brush_record(&value)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_dib_create_pattern_brush_record(self)?;
    let capacity = 4usize
      .checked_add(self.target.len())
      .ok_or_else(|| Error::invalid(0, "META_DIBCREATEPATTERNBRUSH size overflows"))?;
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
    writer.write_u16(self.style)?;
    writer.write_u16(self.color_usage)?;
    writer.write_all(&self.target)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn color_usage_kind(&self) -> Option<DibColorUsage> {
    DibColorUsage::from_wmf_raw(self.color_usage)
  }

  pub fn style_kind(&self) -> Option<WmfBrushStyle> {
    WmfBrushStyle::from_raw(self.style)
  }

  pub fn dib_info(&self) -> Result<DibBitmapInfo> {
    let (info, _) = DibBitmapInfo::read_packed_prefix_from_slice(
      &self.target,
      require_wmf_color_usage(self.color_usage)?,
    )?;
    Ok(info)
  }

  pub fn device_independent_bitmap(&self) -> Result<DeviceIndependentBitmap> {
    DeviceIndependentBitmap::from_packed_slice(
      &self.target,
      require_wmf_color_usage(self.color_usage)?,
    )
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfRectObject {
  pub left: i16,
  pub top: i16,
  pub right: i16,
  pub bottom: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct WmfScanLine {
  pub left: u16,
  pub right: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfScanObject {
  pub count: u16,
  pub top: u16,
  pub bottom: u16,
  pub scan_lines: Vec<WmfScanLine>,
  pub count2: u16,
}

impl SdkRead for WmfScanObject {
  fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Self::read_from_with_end(reader, None)
  }
}

impl WmfScanObject {
  fn read_from_with_end<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    end: Option<u64>,
  ) -> Result<Self> {
    let count = reader.read_u16()?;
    if !count.is_multiple_of(2) {
      return Err(Error::invalid(0, "WMF scan count must be even"));
    }
    let top = reader.read_u16()?;
    let bottom = reader.read_u16()?;
    if let Some(end) = end {
      let scan_line_bytes =
        checked_record_array_bytes(usize::from(count / 2), 4, "WMF ScanObject ScanLines")?;
      ensure_record_remaining(reader, end, scan_line_bytes + 2, "WMF ScanObject")?;
    }
    let mut scan_lines = Vec::with_capacity(count as usize / 2);
    for _ in 0..count / 2 {
      scan_lines.push(WmfScanLine::read_from(reader)?);
    }
    let count2 = reader.read_u16()?;
    let value = Self {
      count,
      top,
      bottom,
      scan_lines,
      count2,
    };
    validate_wmf_scan_object(&value)?;
    Ok(value)
  }
}

impl SdkWrite for WmfScanObject {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_wmf_scan_object(self)?;
    writer.write_u16(self.count)?;
    writer.write_u16(self.top)?;
    writer.write_u16(self.bottom)?;
    for scan_line in &self.scan_lines {
      scan_line.write_to(writer)?;
    }
    writer.write_u16(self.count2)
  }
}

impl SdkSize for WmfScanObject {
  fn sdk_size(&self) -> u64 {
    8 + self.scan_lines.len() as u64 * 4
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfRegionObject {
  pub next_in_chain: u16,
  pub object_type: i16,
  pub object_count: u32,
  pub region_size: i16,
  pub scan_count: i16,
  pub max_scan: i16,
  pub bounding_rectangle: WmfRectObject,
  pub scans: Vec<WmfScanObject>,
}

impl SdkRead for WmfRegionObject {
  fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    Self::read_from_with_end(reader, None)
  }
}

impl WmfRegionObject {
  fn read_data(data: &[u8]) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let value = Self::read_from_with_end(&mut reader, Some(data.len() as u64))?;
    ensure_reader_end(&mut reader, data.len() as u64, "META_CREATEREGION")?;
    Ok(value)
  }

  fn read_from_with_end<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    end: Option<u64>,
  ) -> Result<Self> {
    let next_in_chain = reader.read_u16()?;
    let object_type = reader.read_i16()?;
    let object_count = reader.read_u32()?;
    let region_size = reader.read_i16()?;
    let scan_count = reader.read_i16()?;
    if scan_count < 0 {
      return Err(Error::invalid(0, "WMF region scan count is negative"));
    }
    let max_scan = reader.read_i16()?;
    let bounding_rectangle = WmfRectObject::read_from(reader)?;
    if let Some(end) = end {
      let minimum_scan_bytes =
        checked_record_array_bytes(scan_count as usize, 8, "WMF Region scans")?;
      ensure_record_remaining(reader, end, minimum_scan_bytes, "WMF Region scans")?;
    }
    let mut scans = Vec::with_capacity(scan_count as usize);
    for _ in 0..scan_count {
      scans.push(WmfScanObject::read_from_with_end(reader, end)?);
    }
    let value = Self {
      next_in_chain,
      object_type,
      object_count,
      region_size,
      scan_count,
      max_scan,
      bounding_rectangle,
      scans,
    };
    validate_wmf_region_object(&value)?;
    Ok(value)
  }
}

impl SdkWrite for WmfRegionObject {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_wmf_region_object(self)?;
    writer.write_u16(self.next_in_chain)?;
    writer.write_i16(self.object_type)?;
    writer.write_u32(self.object_count)?;
    writer.write_i16(self.region_size)?;
    writer.write_i16(self.scan_count)?;
    writer.write_i16(self.max_scan)?;
    self.bounding_rectangle.write_to(writer)?;
    for scan in &self.scans {
      scan.write_to(writer)?;
    }
    Ok(())
  }
}

impl SdkSize for WmfRegionObject {
  fn sdk_size(&self) -> u64 {
    14 + self.bounding_rectangle.sdk_size() + self.scans.iter().map(SdkSize::sdk_size).sum::<u64>()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfTextOutRecord {
  pub string: Vec<u8>,
  pub string_padding: Vec<u8>,
  pub y_start: i16,
  pub x_start: i16,
}

impl WmfTextOutRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 6 {
      return Err(Error::invalid(0, "META_TEXTOUT record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let string_length = reader.read_i16()?;
    if string_length < 0 {
      return Err(Error::invalid(0, "META_TEXTOUT has negative string length"));
    }
    let string_length = string_length as usize;
    let string_field_len = data.len() - 2 - 4;
    if string_field_len < string_length || !string_field_len.is_multiple_of(2) {
      return Err(Error::invalid(
        0,
        "META_TEXTOUT string field has invalid length",
      ));
    }
    let string = reader.read_vec(string_length)?;
    let string_padding = reader.read_vec(string_field_len - string_length)?;
    let y_start = reader.read_i16()?;
    let x_start = reader.read_i16()?;
    ensure_reader_end(&mut reader, data.len() as u64, "META_TEXTOUT")?;
    Ok(Self {
      string,
      string_padding,
      y_start,
      x_start,
    })
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    if self.string.len() > i16::MAX as usize {
      return Err(Error::invalid(0, "META_TEXTOUT string is too long"));
    }
    if !(self.string.len() + self.string_padding.len()).is_multiple_of(2) {
      return Err(Error::invalid(
        0,
        "META_TEXTOUT string field must be WORD aligned",
      ));
    }
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_i16(self.string.len() as i16)?;
    writer.write_all(&self.string)?;
    writer.write_all(&self.string_padding)?;
    writer.write_i16(self.y_start)?;
    writer.write_i16(self.x_start)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfExtTextOutRecord {
  pub y: i16,
  pub x: i16,
  pub string_length: i16,
  pub options: WmfExtTextOutOptions,
  pub rectangle: Option<WmfRectObject>,
  pub string: Vec<u8>,
  pub string_padding: Vec<u8>,
  pub dx: Vec<i16>,
  pub trailing_data: Vec<u8>,
}

impl WmfExtTextOutRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 8 {
      return Err(Error::invalid(0, "META_EXTTEXTOUT record is too short"));
    }

    let mut reader = Reader::new(Cursor::new(data));
    let _y = reader.read_i16()?;
    let _x = reader.read_i16()?;
    let string_length = reader.read_i16()?;
    if string_length < 0 {
      return Err(Error::invalid(
        0,
        "META_EXTTEXTOUT has negative string length",
      ));
    }
    let options = WmfExtTextOutOptions::from_bits_retain(reader.read_u16()?);
    let string_len = string_length as usize;
    let padded_string_len = string_len + usize::from(!string_len.is_multiple_of(2));
    let needs_rectangle =
      options.intersects(WmfExtTextOutOptions::OPAQUE | WmfExtTextOutOptions::CLIPPED);

    if needs_rectangle {
      return Self::read_data_with_rectangle(data, true);
    }

    match Self::read_data_with_rectangle(data, false) {
      Ok(value) => Ok(value),
      Err(no_rectangle_error) if data.len() >= 16 + padded_string_len => {
        match Self::read_data_with_rectangle(data, true) {
          Ok(value) => Ok(value),
          Err(_) => Err(no_rectangle_error),
        }
      }
      Err(error) => Err(error),
    }
  }

  fn read_data_with_rectangle(data: &[u8], include_rectangle: bool) -> Result<Self> {
    let mut reader = Reader::new(Cursor::new(data));
    let y = reader.read_i16()?;
    let x = reader.read_i16()?;
    let string_length = reader.read_i16()?;
    if string_length < 0 {
      return Err(Error::invalid(
        0,
        "META_EXTTEXTOUT has negative string length",
      ));
    }
    let options = WmfExtTextOutOptions::from_bits_retain(reader.read_u16()?);
    let string_len = string_length as usize;
    let padded_string_len = string_len + usize::from(!string_len.is_multiple_of(2));
    let rectangle = if include_rectangle {
      if data.len() < 16 + padded_string_len {
        return Err(Error::invalid(0, "META_EXTTEXTOUT rectangle is truncated"));
      }
      Some(WmfRectObject::read_from(&mut reader)?)
    } else {
      None
    };
    if data.len() < reader.position()? as usize + padded_string_len {
      return Err(Error::invalid(0, "META_EXTTEXTOUT string is truncated"));
    }
    let string = reader.read_vec(string_len)?;
    let string_padding = reader.read_vec(padded_string_len - string_len)?;
    let remaining = data.len() - reader.position()? as usize;
    let dx_count = remaining / 2;
    let mut dx = Vec::with_capacity(dx_count);
    for _ in 0..dx_count {
      dx.push(reader.read_i16()?);
    }
    let trailing_data = read_remaining(&mut reader, data.len())?;
    let value = Self {
      y,
      x,
      string_length,
      options,
      rectangle,
      string,
      string_padding,
      dx,
      trailing_data,
    };
    validate_wmf_ext_text_out_record(&value)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_wmf_ext_text_out_record(self)?;
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_i16(self.y)?;
    writer.write_i16(self.x)?;
    writer.write_i16(self.string_length)?;
    writer.write_u16(self.options.bits())?;
    if let Some(rectangle) = &self.rectangle {
      rectangle.write_to(&mut writer)?;
    }
    writer.write_all(&self.string)?;
    writer.write_all(&self.string_padding)?;
    for dx in &self.dx {
      writer.write_i16(*dx)?;
    }
    writer.write_all(&self.trailing_data)?;
    Ok(writer.into_inner().into_inner())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfEscapeRecord {
  pub escape_function: u16,
  pub escape_data: Vec<u8>,
  pub padding: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WmfEscapeData<'a> {
  NoData {
    escape: WmfMetafileEscape,
  },
  EnhancedMetafile {
    comment_identifier: u32,
    comment_type: u32,
    version: u32,
    checksum: u16,
    flags: u32,
    comment_record_count: u32,
    current_record_size: u32,
    remaining_bytes: u32,
    enhanced_metafile_data_size: u32,
    enhanced_metafile_data: &'a [u8],
  },
  StartDoc {
    doc_name: &'a [u8],
  },
  SetColorTable {
    color_table: &'a [u8],
  },
  GetColorTable {
    start: u16,
    undefined_space: &'a [u8],
    color_table: &'a [u8],
  },
  DrawPatternRect {
    position: PointL,
    size: PointL,
    style: u16,
    pattern: u16,
  },
  EncapsulatedPostScript {
    size: u32,
    version: u32,
    points: [PointL; 3],
    data: &'a [u8],
    trailing_data: &'a [u8],
  },
  EpsPrinting {
    set_eps_printing: u16,
  },
  BinaryData {
    escape: WmfMetafileEscape,
    data: &'a [u8],
  },
  QueryEscSupport {
    query: u16,
  },
  SetCopyCount {
    copy_count: u16,
  },
  SetLineCap {
    cap: i32,
  },
  SetLineJoin {
    join: i32,
  },
  SetMiterLimit {
    miter_limit: i32,
  },
  ClipToPath {
    clip_function: u16,
    reserved: u16,
  },
  GetPsFeatureSetting {
    feature_setting: i32,
  },
  PostScriptInjection {
    data_size: u32,
    injection_point: u16,
    page_number: u16,
    raw_data: &'a [u8],
    trailing_data: &'a [u8],
  },
  SpclPassThrough2 {
    reserved: u32,
    raw_data: &'a [u8],
    trailing_data: &'a [u8],
  },
  Raw {
    escape: WmfMetafileEscape,
    data: &'a [u8],
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmfEnhancedMetafile {
  pub version: u32,
  pub checksum: u16,
  pub data: Vec<u8>,
}

impl WmfEnhancedMetafile {
  pub fn computed_checksum(&self) -> Result<u16> {
    compute_enhanced_metafile_checksum(&self.data)
  }

  pub fn parse_emf(&self) -> Result<crate::emf::EmfMetafile> {
    crate::emf::EmfMetafile::from_bytes(&self.data)
  }
}

#[derive(Clone, Debug, Default)]
pub struct WmfEnhancedMetafileAssembler {
  state: Option<WmfEnhancedMetafileAssembly>,
}

#[derive(Clone, Debug)]
struct WmfEnhancedMetafileAssembly {
  version: u32,
  checksum: u16,
  record_count: u32,
  records_seen: u32,
  total_size: u32,
  data: Vec<u8>,
}

impl WmfEnhancedMetafileAssembler {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn push(&mut self, record: &WmfEscapeRecord) -> Result<Option<WmfEnhancedMetafile>> {
    let WmfEscapeData::EnhancedMetafile {
      version,
      checksum,
      comment_record_count,
      current_record_size,
      remaining_bytes,
      enhanced_metafile_data_size,
      enhanced_metafile_data,
      ..
    } = record.typed_data()?
    else {
      return Err(Error::invalid(
        0,
        "WMF enhanced-metafile assembler requires META_ESCAPE_ENHANCED_METAFILE",
      ));
    };

    let state = self
      .state
      .get_or_insert_with(|| WmfEnhancedMetafileAssembly {
        version,
        checksum,
        record_count: comment_record_count,
        records_seen: 0,
        total_size: enhanced_metafile_data_size,
        data: Vec::with_capacity(enhanced_metafile_data_size as usize),
      });
    if state.version != version
      || state.checksum != checksum
      || state.record_count != comment_record_count
      || state.total_size != enhanced_metafile_data_size
    {
      return Err(Error::invalid(
        0,
        "META_ESCAPE_ENHANCED_METAFILE sequence metadata changed between chunks",
      ));
    }
    if state.records_seen >= state.record_count {
      return Err(Error::invalid(
        0,
        "META_ESCAPE_ENHANCED_METAFILE has more chunks than CommentRecordCount",
      ));
    }
    if current_record_size as usize != enhanced_metafile_data.len() {
      return Err(Error::invalid(
        0,
        "META_ESCAPE_ENHANCED_METAFILE CurrentRecordSize does not match chunk data",
      ));
    }
    let accumulated = state
      .data
      .len()
      .checked_add(enhanced_metafile_data.len())
      .ok_or_else(|| Error::invalid(0, "embedded EMF size overflows"))?;
    let expected_remaining = (state.total_size as usize)
      .checked_sub(accumulated)
      .ok_or_else(|| Error::invalid(0, "embedded EMF chunks exceed total size"))?;
    if remaining_bytes as usize != expected_remaining {
      return Err(Error::invalid(
        0,
        "META_ESCAPE_ENHANCED_METAFILE RemainingBytes is inconsistent",
      ));
    }
    state.data.extend_from_slice(enhanced_metafile_data);
    state.records_seen += 1;

    if state.records_seen == state.record_count {
      if remaining_bytes != 0 || state.data.len() != state.total_size as usize {
        return Err(Error::invalid(
          0,
          "META_ESCAPE_ENHANCED_METAFILE final chunk does not complete the stream",
        ));
      }
      let state = self.state.take().expect("assembly state was initialized");
      let value = WmfEnhancedMetafile {
        version: state.version,
        checksum: state.checksum,
        data: state.data,
      };
      if value.checksum != value.computed_checksum()? {
        return Err(Error::invalid(
          0,
          "META_ESCAPE_ENHANCED_METAFILE Checksum is invalid",
        ));
      }
      Ok(Some(value))
    } else if remaining_bytes == 0 {
      Err(Error::invalid(
        0,
        "META_ESCAPE_ENHANCED_METAFILE ended before CommentRecordCount chunks",
      ))
    } else {
      Ok(None)
    }
  }

  pub fn finish(&self) -> Result<()> {
    if self.state.is_none() {
      Ok(())
    } else {
      Err(Error::invalid(
        0,
        "META_ESCAPE_ENHANCED_METAFILE sequence is incomplete",
      ))
    }
  }
}

pub fn compute_enhanced_metafile_checksum(data: &[u8]) -> Result<u16> {
  if !data.len().is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      "embedded EMF stream must contain complete WORDs",
    ));
  }
  let xor = data.chunks_exact(2).fold(0u16, |value, word| {
    value ^ u16::from_le_bytes([word[0], word[1]])
  });
  Ok(!xor)
}

impl WmfEscapeData<'_> {
  pub const fn escape_kind(&self) -> WmfMetafileEscape {
    match self {
      Self::NoData { escape } | Self::Raw { escape, .. } => *escape,
      Self::EnhancedMetafile { .. } => WmfMetafileEscape::MetaFile,
      Self::StartDoc { .. } => WmfMetafileEscape::StartDoc,
      Self::SetColorTable { .. } => WmfMetafileEscape::SetColorTable,
      Self::GetColorTable { .. } => WmfMetafileEscape::GetColorTable,
      Self::DrawPatternRect { .. } => WmfMetafileEscape::DrawPatternRect,
      Self::EncapsulatedPostScript { .. } => WmfMetafileEscape::EncapsulatedPostScript,
      Self::EpsPrinting { .. } => WmfMetafileEscape::EpsPrinting,
      Self::BinaryData { escape, .. } => *escape,
      Self::QueryEscSupport { .. } => WmfMetafileEscape::QueryEscSupport,
      Self::SetCopyCount { .. } => WmfMetafileEscape::SetCopyCount,
      Self::SetLineCap { .. } => WmfMetafileEscape::SetLineCap,
      Self::SetLineJoin { .. } => WmfMetafileEscape::SetLineJoin,
      Self::SetMiterLimit { .. } => WmfMetafileEscape::SetMiterLimit,
      Self::ClipToPath { .. } => WmfMetafileEscape::ClipToPath,
      Self::GetPsFeatureSetting { .. } => WmfMetafileEscape::GetPsFeatureSetting,
      Self::PostScriptInjection { .. } => WmfMetafileEscape::PostScriptInjection,
      Self::SpclPassThrough2 { .. } => WmfMetafileEscape::SpclPassThrough2,
    }
  }

  pub fn query_escape_kind(&self) -> Option<WmfMetafileEscape> {
    let Self::QueryEscSupport { query } = self else {
      return None;
    };
    WmfMetafileEscape::from_raw(*query)
  }

  pub fn post_script_cap_kind(&self) -> Option<WmfPostScriptCap> {
    let Self::SetLineCap { cap } = self else {
      return None;
    };
    WmfPostScriptCap::from_raw(*cap)
  }

  pub fn post_script_join_kind(&self) -> Option<WmfPostScriptJoin> {
    let Self::SetLineJoin { join } = self else {
      return None;
    };
    WmfPostScriptJoin::from_raw(*join)
  }

  pub fn post_script_clipping_kind(&self) -> Option<WmfPostScriptClipping> {
    let Self::ClipToPath { clip_function, .. } = self else {
      return None;
    };
    WmfPostScriptClipping::from_raw(*clip_function)
  }

  pub fn post_script_feature_setting_kind(&self) -> Option<WmfPostScriptFeatureSetting> {
    let Self::GetPsFeatureSetting { feature_setting } = self else {
      return None;
    };
    WmfPostScriptFeatureSetting::from_raw(*feature_setting)
  }

  pub fn is_valid_post_script_feature_setting(&self) -> bool {
    let Self::GetPsFeatureSetting { feature_setting } = self else {
      return false;
    };
    is_valid_post_script_feature_setting(*feature_setting)
  }

  fn to_escape_data(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    match self {
      Self::NoData { .. } => {}
      Self::EnhancedMetafile {
        comment_identifier,
        comment_type,
        version,
        checksum,
        flags,
        comment_record_count,
        current_record_size,
        remaining_bytes,
        enhanced_metafile_data_size,
        enhanced_metafile_data,
      } => {
        validate_enhanced_metafile_escape_fields(EnhancedMetafileEscapeValidation {
          comment_identifier: *comment_identifier,
          comment_type: *comment_type,
          flags: *flags,
          comment_record_count: *comment_record_count,
          current_record_size: *current_record_size,
          remaining_bytes: *remaining_bytes,
          enhanced_metafile_data_size: *enhanced_metafile_data_size,
          data_len: enhanced_metafile_data.len(),
        })?;
        writer.write_u32(*comment_identifier)?;
        writer.write_u32(*comment_type)?;
        writer.write_u32(*version)?;
        writer.write_u16(*checksum)?;
        writer.write_u32(*flags)?;
        writer.write_u32(*comment_record_count)?;
        writer.write_u32(*current_record_size)?;
        writer.write_u32(*remaining_bytes)?;
        writer.write_u32(*enhanced_metafile_data_size)?;
        writer.write_all(enhanced_metafile_data)?;
      }
      Self::StartDoc { doc_name } => {
        validate_start_doc_escape(doc_name)?;
        writer.write_all(doc_name)?;
      }
      Self::SetColorTable { color_table } => writer.write_all(color_table)?,
      Self::GetColorTable {
        start,
        undefined_space,
        color_table,
      } => {
        validate_get_color_table_escape(*start, undefined_space, color_table)?;
        writer.write_u16(*start)?;
        writer.write_all(undefined_space)?;
        writer.write_all(color_table)?;
      }
      Self::DrawPatternRect {
        position,
        size,
        style,
        pattern,
      } => {
        position.write_to(&mut writer)?;
        size.write_to(&mut writer)?;
        writer.write_u16(*style)?;
        writer.write_u16(*pattern)?;
      }
      Self::EncapsulatedPostScript {
        size,
        version,
        points,
        data,
        trailing_data,
      } => {
        validate_encapsulated_postscript_escape(*size, data)?;
        writer.write_u32(*size)?;
        writer.write_u32(*version)?;
        for point in points {
          point.write_to(&mut writer)?;
        }
        writer.write_all(data)?;
        writer.write_all(trailing_data)?;
      }
      Self::EpsPrinting { set_eps_printing } => writer.write_u16(*set_eps_printing)?,
      Self::BinaryData { data, .. } => writer.write_all(data)?,
      Self::QueryEscSupport { query } => writer.write_u16(*query)?,
      Self::SetCopyCount { copy_count } => writer.write_u16(*copy_count)?,
      Self::SetLineCap { cap } => writer.write_i32(*cap)?,
      Self::SetLineJoin { join } => writer.write_i32(*join)?,
      Self::SetMiterLimit { miter_limit } => writer.write_i32(*miter_limit)?,
      Self::ClipToPath {
        clip_function,
        reserved,
      } => {
        writer.write_u16(*clip_function)?;
        writer.write_u16(*reserved)?;
      }
      Self::GetPsFeatureSetting { feature_setting } => {
        writer.write_i32(*feature_setting)?;
      }
      Self::PostScriptInjection {
        data_size,
        injection_point,
        page_number,
        raw_data,
        trailing_data,
      } => {
        if raw_data.len() > u32::MAX as usize {
          return Err(Error::invalid(
            0,
            "POSTSCRIPT_INJECTION RawData is too large",
          ));
        }
        if *data_size as usize != raw_data.len() {
          return Err(Error::invalid(
            0,
            "POSTSCRIPT_INJECTION DataSize does not match RawData",
          ));
        }
        writer.write_u32(*data_size)?;
        writer.write_u16(*injection_point)?;
        writer.write_u16(*page_number)?;
        writer.write_all(raw_data)?;
        writer.write_all(trailing_data)?;
      }
      Self::SpclPassThrough2 {
        reserved,
        raw_data,
        trailing_data,
      } => {
        if raw_data.len() > u16::MAX as usize {
          return Err(Error::invalid(0, "SPCLPASSTHROUGH2 RawData is too large"));
        }
        writer.write_u32(*reserved)?;
        writer.write_u16(raw_data.len() as u16)?;
        writer.write_all(raw_data)?;
        writer.write_all(trailing_data)?;
      }
      Self::Raw { data, .. } => writer.write_all(data)?,
    }
    Ok(writer.into_inner().into_inner())
  }
}

impl WmfEscapeRecord {
  fn read_data(data: &[u8]) -> Result<Self> {
    if data.len() < 4 {
      return Err(Error::invalid(0, "META_ESCAPE record is too short"));
    }
    let mut reader = Reader::new(Cursor::new(data));
    let escape_function = reader.read_u16()?;
    let byte_count = reader.read_u16()? as usize;
    if data.len() < 4 + byte_count {
      return Err(Error::invalid(
        0,
        "META_ESCAPE byte count exceeds record data",
      ));
    }
    let escape_data = reader.read_vec(byte_count)?;
    let padding = reader.read_vec(data.len() - 4 - byte_count)?;
    let value = Self {
      escape_function,
      escape_data,
      padding,
    };
    validate_escape_record(&value)?;
    Ok(value)
  }

  fn write_data(&self) -> Result<Vec<u8>> {
    validate_escape_record(self)?;
    if self.escape_data.len() > u16::MAX as usize {
      return Err(Error::invalid(0, "META_ESCAPE data is too large"));
    }
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer.write_u16(self.escape_function)?;
    writer.write_u16(self.escape_data.len() as u16)?;
    writer.write_all(&self.escape_data)?;
    writer.write_all(&self.padding)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn escape_kind(&self) -> Option<WmfMetafileEscape> {
    WmfMetafileEscape::from_raw(self.escape_function)
  }

  pub fn post_script_cap_kind(&self) -> Option<WmfPostScriptCap> {
    if self.escape_kind()? != WmfMetafileEscape::SetLineCap {
      return None;
    }
    WmfPostScriptCap::from_raw(read_escape_i32(&self.escape_data)?)
  }

  pub fn post_script_join_kind(&self) -> Option<WmfPostScriptJoin> {
    if self.escape_kind()? != WmfMetafileEscape::SetLineJoin {
      return None;
    }
    WmfPostScriptJoin::from_raw(read_escape_i32(&self.escape_data)?)
  }

  pub fn post_script_clipping_kind(&self) -> Option<WmfPostScriptClipping> {
    if self.escape_kind()? != WmfMetafileEscape::ClipToPath {
      return None;
    }
    WmfPostScriptClipping::from_raw(read_escape_u16(&self.escape_data)?)
  }

  pub fn post_script_feature_setting_kind(&self) -> Option<WmfPostScriptFeatureSetting> {
    if self.escape_kind()? != WmfMetafileEscape::GetPsFeatureSetting {
      return None;
    }
    WmfPostScriptFeatureSetting::from_raw(read_escape_i32(&self.escape_data)?)
  }

  pub fn typed_data(&self) -> Result<WmfEscapeData<'_>> {
    validate_escape_record(self)?;
    let escape = self.escape_kind().ok_or_else(|| {
      Error::invalid(
        0,
        "META_ESCAPE EscapeFunction is not a valid MetafileEscape",
      )
    })?;
    if is_no_data_escape(escape) {
      return Ok(WmfEscapeData::NoData { escape });
    }
    Ok(match escape {
      WmfMetafileEscape::MetaFile
        if read_escape_u32(&self.escape_data) == Some(WMF_EMF_COMMENT_IDENTIFIER) =>
      {
        let data = parse_enhanced_metafile_escape(&self.escape_data)?;
        WmfEscapeData::EnhancedMetafile {
          comment_identifier: data.comment_identifier,
          comment_type: data.comment_type,
          version: data.version,
          checksum: data.checksum,
          flags: data.flags,
          comment_record_count: data.comment_record_count,
          current_record_size: data.current_record_size,
          remaining_bytes: data.remaining_bytes,
          enhanced_metafile_data_size: data.enhanced_metafile_data_size,
          enhanced_metafile_data: data.enhanced_metafile_data,
        }
      }
      WmfMetafileEscape::StartDoc => WmfEscapeData::StartDoc {
        doc_name: &self.escape_data,
      },
      WmfMetafileEscape::SetColorTable => WmfEscapeData::SetColorTable {
        color_table: &self.escape_data,
      },
      WmfMetafileEscape::GetColorTable => {
        let data = parse_get_color_table_escape(&self.escape_data)?;
        WmfEscapeData::GetColorTable {
          start: data.start,
          undefined_space: data.undefined_space,
          color_table: data.color_table,
        }
      }
      WmfMetafileEscape::DrawPatternRect => {
        let data = parse_draw_pattern_rect_escape(&self.escape_data)?;
        WmfEscapeData::DrawPatternRect {
          position: data.position,
          size: data.size,
          style: data.style,
          pattern: data.pattern,
        }
      }
      WmfMetafileEscape::EncapsulatedPostScript => {
        let data = parse_encapsulated_postscript_escape(&self.escape_data)?;
        WmfEscapeData::EncapsulatedPostScript {
          size: data.size,
          version: data.version,
          points: data.points,
          data: data.data,
          trailing_data: data.trailing_data,
        }
      }
      WmfMetafileEscape::EpsPrinting => WmfEscapeData::EpsPrinting {
        set_eps_printing: read_escape_u16(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "EPSPRINTING SetEpsPrinting missing"))?,
      },
      WmfMetafileEscape::CheckJpegFormat
      | WmfMetafileEscape::CheckPngFormat
      | WmfMetafileEscape::PassThrough
      | WmfMetafileEscape::PostScriptData
      | WmfMetafileEscape::PostScriptIdentify
      | WmfMetafileEscape::PostScriptPassThrough => WmfEscapeData::BinaryData {
        escape,
        data: &self.escape_data,
      },
      WmfMetafileEscape::QueryEscSupport => WmfEscapeData::QueryEscSupport {
        query: read_escape_u16(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "QUERYESCSUPPORT Query missing"))?,
      },
      WmfMetafileEscape::SetCopyCount => WmfEscapeData::SetCopyCount {
        copy_count: read_escape_u16(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "SETCOPYCOUNT CopyCount missing"))?,
      },
      WmfMetafileEscape::SetLineCap => WmfEscapeData::SetLineCap {
        cap: read_escape_i32(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "SETLINECAP Cap missing"))?,
      },
      WmfMetafileEscape::SetLineJoin => WmfEscapeData::SetLineJoin {
        join: read_escape_i32(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "SETLINEJOIN Join missing"))?,
      },
      WmfMetafileEscape::SetMiterLimit => WmfEscapeData::SetMiterLimit {
        miter_limit: read_escape_i32(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "SETMITERLIMIT MiterLimit missing"))?,
      },
      WmfMetafileEscape::ClipToPath => {
        let clip_function = read_escape_u16(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "CLIP_TO_PATH ClipFunction missing"))?;
        let reserved = read_escape_u16(&self.escape_data[2..])
          .ok_or_else(|| Error::invalid(0, "CLIP_TO_PATH Reserved1 missing"))?;
        WmfEscapeData::ClipToPath {
          clip_function,
          reserved,
        }
      }
      WmfMetafileEscape::GetPsFeatureSetting => WmfEscapeData::GetPsFeatureSetting {
        feature_setting: read_escape_i32(&self.escape_data)
          .ok_or_else(|| Error::invalid(0, "GET_PS_FEATURESETTING setting missing"))?,
      },
      WmfMetafileEscape::PostScriptInjection => {
        let data = parse_postscript_injection(&self.escape_data)?;
        WmfEscapeData::PostScriptInjection {
          data_size: data.data_size,
          injection_point: data.injection_point,
          page_number: data.page_number,
          raw_data: data.raw_data,
          trailing_data: data.trailing_data,
        }
      }
      WmfMetafileEscape::SpclPassThrough2 => {
        let (reserved, raw_data, trailing_data) = parse_spcl_pass_through2(&self.escape_data)?;
        WmfEscapeData::SpclPassThrough2 {
          reserved,
          raw_data,
          trailing_data,
        }
      }
      _ => WmfEscapeData::Raw {
        escape,
        data: &self.escape_data,
      },
    })
  }

  pub fn from_typed_data(data: WmfEscapeData<'_>, padding: Vec<u8>) -> Result<Self> {
    let mut value = Self {
      escape_function: data.escape_kind().raw(),
      escape_data: data.to_escape_data()?,
      padding,
    };
    if value.padding.is_empty() && !value.escape_data.len().is_multiple_of(2) {
      value.padding.push(0);
    }
    validate_escape_record(&value)?;
    Ok(value)
  }
}

fn read_escape_i32(data: &[u8]) -> Option<i32> {
  let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
  Some(i32::from_le_bytes(bytes))
}

fn read_escape_u32(data: &[u8]) -> Option<u32> {
  let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
  Some(u32::from_le_bytes(bytes))
}

fn read_escape_u16(data: &[u8]) -> Option<u16> {
  let bytes: [u8; 2] = data.get(..2)?.try_into().ok()?;
  Some(u16::from_le_bytes(bytes))
}

fn validate_start_doc_escape(data: &[u8]) -> Result<()> {
  if data.len() < 260 {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      "STARTDOC DocName must be shorter than 260 bytes",
    ))
  }
}

fn validate_get_color_table_escape(
  start: u16,
  undefined_space: &[u8],
  color_table: &[u8],
) -> Result<()> {
  let expected_start = 2usize
    .checked_add(undefined_space.len())
    .ok_or_else(|| Error::invalid(0, "GETCOLORTABLE Start overflows"))?;
  if usize::from(start) != expected_start {
    return Err(Error::invalid(
      0,
      "GETCOLORTABLE Start does not match UndefinedSpace length",
    ));
  }
  let _ = color_table;
  Ok(())
}

struct ParsedGetColorTableEscape<'a> {
  start: u16,
  undefined_space: &'a [u8],
  color_table: &'a [u8],
}

fn parse_get_color_table_escape(data: &[u8]) -> Result<ParsedGetColorTableEscape<'_>> {
  let start =
    read_escape_u16(data).ok_or_else(|| Error::invalid(0, "GETCOLORTABLE Start missing"))?;
  let start = usize::from(start);
  if start < 2 || start > data.len() {
    return Err(Error::invalid(
      0,
      "GETCOLORTABLE Start points outside EscapeData",
    ));
  }
  Ok(ParsedGetColorTableEscape {
    start: start as u16,
    undefined_space: &data[2..start],
    color_table: &data[start..],
  })
}

struct ParsedDrawPatternRectEscape {
  position: PointL,
  size: PointL,
  style: u16,
  pattern: u16,
}

fn parse_draw_pattern_rect_escape(data: &[u8]) -> Result<ParsedDrawPatternRectEscape> {
  if data.len() != 20 {
    return Err(Error::invalid(0, "DRAWPATTERNRECT ByteCount must be 20"));
  }
  let mut reader = Reader::new(Cursor::new(data));
  Ok(ParsedDrawPatternRectEscape {
    position: PointL::read_from(&mut reader)?,
    size: PointL::read_from(&mut reader)?,
    style: reader.read_u16()?,
    pattern: reader.read_u16()?,
  })
}

struct ParsedEncapsulatedPostScriptEscape<'a> {
  size: u32,
  version: u32,
  points: [PointL; 3],
  data: &'a [u8],
  trailing_data: &'a [u8],
}

fn validate_encapsulated_postscript_escape(size: u32, data: &[u8]) -> Result<()> {
  let expected_size = 32usize
    .checked_add(data.len())
    .ok_or_else(|| Error::invalid(0, "ENCAPSULATED_POSTSCRIPT Size overflows"))?;
  if expected_size > u32::MAX as usize {
    return Err(Error::invalid(
      0,
      "ENCAPSULATED_POSTSCRIPT Data is too large",
    ));
  }
  if size as usize != expected_size {
    return Err(Error::invalid(
      0,
      "ENCAPSULATED_POSTSCRIPT Size does not match Data length",
    ));
  }
  Ok(())
}

fn parse_encapsulated_postscript_escape(
  data: &[u8],
) -> Result<ParsedEncapsulatedPostScriptEscape<'_>> {
  if data.len() < 32 {
    return Err(Error::invalid(
      0,
      "ENCAPSULATED_POSTSCRIPT data is shorter than Size, Version, and Points",
    ));
  }
  let size = u32::from_le_bytes(data[0..4].try_into().unwrap());
  if size < 32 {
    return Err(Error::invalid(
      0,
      "ENCAPSULATED_POSTSCRIPT Size must include Size, Version, and Points",
    ));
  }
  let end = size as usize;
  if end > data.len() {
    return Err(Error::invalid(
      0,
      "ENCAPSULATED_POSTSCRIPT Size exceeds EscapeData",
    ));
  }

  let mut reader = Reader::new(Cursor::new(&data[4..32]));
  let version = reader.read_u32()?;
  let points = [
    PointL::read_from(&mut reader)?,
    PointL::read_from(&mut reader)?,
    PointL::read_from(&mut reader)?,
  ];
  Ok(ParsedEncapsulatedPostScriptEscape {
    size,
    version,
    points,
    data: &data[32..end],
    trailing_data: &data[end..],
  })
}

fn validate_wmf_placeable_header(value: &WmfPlaceableHeader) -> Result<()> {
  validate_wmf_placeable_header_lossless(value)?;
  if value.reserved != 0 {
    return Err(Error::invalid(
      0,
      "WMF placeable header Reserved must be zero",
    ));
  }
  if value.checksum != value.computed_checksum() {
    return Err(Error::invalid(
      0,
      "WMF placeable header Checksum is invalid",
    ));
  }
  Ok(())
}

fn validate_wmf_placeable_header_lossless(value: &WmfPlaceableHeader) -> Result<()> {
  if value.key != PLACEABLE_KEY {
    return Err(Error::invalid(0, "WMF placeable header Key is invalid"));
  }
  if value.inch == 0 {
    return Err(Error::invalid(
      0,
      "WMF placeable header Inch must be nonzero",
    ));
  }
  Ok(())
}

fn validate_wmf_header(value: &WmfHeader) -> Result<()> {
  if value.metafile_type_kind().is_none() {
    return Err(Error::invalid(0, "WMF header Type is invalid"));
  }
  if value.header_size_words != 9 {
    return Err(Error::invalid(0, "WMF header size must be 9 WORDs"));
  }
  if value.version_kind().is_none() {
    return Err(Error::invalid(0, "WMF header Version is invalid"));
  }
  Ok(())
}

fn validate_unknown_wmf_record(function: u16) -> Result<()> {
  if normalized_wmf_record_function(function).is_some() {
    return Err(Error::invalid(
      0,
      "WMF Unknown record requires an unknown RecordFunction",
    ));
  }
  Ok(())
}

fn validate_wmf_set_bk_mode(value: &WmfU16Record) -> Result<()> {
  validate_wmf_optional_reserved_word(&value.reserved, "META_SETBKMODE")?;
  if value.mix_mode_kind().is_none() {
    return Err(Error::invalid(0, "META_SETBKMODE MixMode is invalid"));
  }
  Ok(())
}

fn validate_wmf_set_map_mode(value: &WmfU16Record) -> Result<()> {
  validate_wmf_no_reserved_words(&value.reserved, "META_SETMAPMODE")?;
  if value.map_mode_kind().is_none() {
    return Err(Error::invalid(0, "META_SETMAPMODE MapMode is invalid"));
  }
  Ok(())
}

fn validate_wmf_set_rop2(value: &WmfU16Record) -> Result<()> {
  validate_wmf_optional_reserved_word(&value.reserved, "META_SETROP2")?;
  if value.binary_raster_operation_kind().is_none() {
    return Err(Error::invalid(0, "META_SETROP2 ROP2Mode is invalid"));
  }
  Ok(())
}

fn validate_wmf_set_poly_fill_mode(value: &WmfU16Record) -> Result<()> {
  validate_wmf_optional_reserved_word(&value.reserved, "META_SETPOLYFILLMODE")?;
  if value.poly_fill_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "META_SETPOLYFILLMODE PolyFillMode is invalid",
    ));
  }
  Ok(())
}

fn validate_wmf_set_stretch_blt_mode(value: &WmfU16Record) -> Result<()> {
  validate_wmf_optional_reserved_word(&value.reserved, "META_SETSTRETCHBLTMODE")?;
  if value.stretch_mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "META_SETSTRETCHBLTMODE StretchMode is invalid",
    ));
  }
  Ok(())
}

fn validate_wmf_set_text_align(value: &WmfU16Record) -> Result<()> {
  validate_wmf_optional_reserved_word(&value.reserved, "META_SETTEXTALIGN")?;
  Ok(())
}

fn validate_wmf_set_text_align_strict(value: &WmfU16Record) -> Result<()> {
  validate_wmf_set_text_align(value)?;
  validate_wmf_text_alignment_value(value.value, "META_SETTEXTALIGN")
}

pub(crate) fn validate_wmf_text_alignment_value(value: u16, name: &str) -> Result<()> {
  let allowed =
    WmfTextAlignmentModeFlags::all().bits() | WmfVerticalTextAlignmentModeFlags::all().bits();
  if value & !allowed != 0 {
    return Err(Error::invalid(
      0,
      format!("{name} TextAlignmentMode contains invalid flags"),
    ));
  }
  let horizontal = value & 0x0006;
  if !matches!(horizontal, 0x0000 | 0x0002 | 0x0006) {
    return Err(Error::invalid(
      0,
      format!("{name} horizontal TextAlignmentMode is invalid"),
    ));
  }
  let vertical = value & 0x0018;
  if !matches!(vertical, 0x0000 | 0x0008 | 0x0018) {
    return Err(Error::invalid(
      0,
      format!("{name} vertical TextAlignmentMode is invalid"),
    ));
  }
  Ok(())
}

fn validate_wmf_set_layout(value: &WmfU16Record) -> Result<()> {
  validate_wmf_required_reserved_word(&value.reserved, "META_SETLAYOUT")?;
  if value.invalid_layout_bits() != 0 {
    return Err(Error::invalid(
      0,
      "META_SETLAYOUT Layout contains invalid flags",
    ));
  }
  Ok(())
}

fn validate_wmf_set_text_char_extra(value: &WmfU16Record) -> Result<()> {
  validate_wmf_no_reserved_words(&value.reserved, "META_SETTEXTCHAREXTRA")
}

fn validate_wmf_no_reserved_words(reserved: &[u8], name: &str) -> Result<()> {
  if !reserved.is_empty() {
    return Err(Error::invalid(
      0,
      format!("{name} must not contain trailing reserved data"),
    ));
  }
  Ok(())
}

fn validate_wmf_optional_reserved_word(reserved: &[u8], name: &str) -> Result<()> {
  if !matches!(reserved.len(), 0 | 2) {
    return Err(Error::invalid(
      0,
      format!("{name} optional Reserved field must be absent or one WORD"),
    ));
  }
  Ok(())
}

fn validate_wmf_required_reserved_word(reserved: &[u8], name: &str) -> Result<()> {
  if reserved.len() != 2 {
    return Err(Error::invalid(
      0,
      format!("{name} Reserved field must be one WORD"),
    ));
  }
  Ok(())
}

fn validate_wmf_scale_ext_record(value: &WmfScaleExtRecord) -> Result<()> {
  if value.x_num == 0 || value.x_denom == 0 || value.y_num == 0 || value.y_denom == 0 {
    return Err(Error::invalid(
      0,
      "WMF scale extension numerator and denominator fields must be nonzero",
    ));
  }
  Ok(())
}

fn validate_wmf_pat_blt_record(value: &WmfPatBltRecord) -> Result<()> {
  validate_wmf_ternary_raster_operation(value.raster_operation, "META_PATBLT")
}

fn validate_wmf_ext_flood_fill(value: &WmfExtFloodFillRecord) -> Result<()> {
  if value.mode_kind().is_none() {
    return Err(Error::invalid(
      0,
      "META_EXTFLOODFILL FloodFillMode is invalid",
    ));
  }
  Ok(())
}

fn validate_wmf_log_brush_object(value: &WmfLogBrushObject) -> Result<()> {
  let _ = value;
  Ok(())
}

fn validate_wmf_log_brush_object_strict(value: &WmfLogBrushObject) -> Result<()> {
  match value.brush_style_kind() {
    Some(WmfBrushStyle::Hatched) => {
      if value.hatch_style_kind().is_none() {
        return Err(Error::invalid(
          0,
          "WMF LogBrush BrushHatch is not a valid HatchStyle for BS_HATCHED",
        ));
      }
    }
    Some(_) => {}
    None => {
      return Err(Error::invalid(
        0,
        "WMF LogBrush BrushStyle is not a valid BrushStyle",
      ));
    }
  }
  Ok(())
}

fn validate_wmf_poly_points_strict(
  value: &WmfPolyPointsRecord,
  name: &str,
  min_points: usize,
) -> Result<()> {
  if value.points.len() < min_points {
    Err(Error::invalid(
      0,
      format!("{name} point count must be at least {min_points}"),
    ))
  } else {
    Ok(())
  }
}

fn validate_wmf_palette_entry(value: &WmfPaletteEntry) -> Result<()> {
  if value.flag_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF PaletteEntry Values is not a valid PaletteEntryFlag",
    ));
  }
  Ok(())
}

fn validate_wmf_palette_object(value: &WmfPaletteObject) -> Result<()> {
  for entry in &value.entries {
    validate_wmf_palette_entry(entry)?;
  }
  Ok(())
}

fn validate_wmf_create_palette_record(value: &WmfPaletteObject) -> Result<()> {
  if value.start != 0x0300 {
    return Err(Error::invalid(0, "META_CREATEPALETTE Start must be 0x0300"));
  }
  Ok(())
}

fn validate_wmf_pen_object(value: &WmfPenObject) -> Result<()> {
  if value.pen_line_style_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF Pen PenStyle line style is not a valid PenStyle",
    ));
  }
  if value.pen_end_cap_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF Pen PenStyle end cap is not a valid PenStyle",
    ));
  }
  if value.pen_join_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF Pen PenStyle join is not a valid PenStyle",
    ));
  }
  if value.pen_reserved_bits() != 0 {
    return Err(Error::invalid(
      0,
      "WMF Pen PenStyle reserved bits must be zero",
    ));
  }
  Ok(())
}

fn validate_wmf_dib_create_pattern_brush_record(
  value: &WmfDibCreatePatternBrushRecord,
) -> Result<()> {
  if value.style_kind() == Some(WmfBrushStyle::Pattern) {
    if value.color_usage_kind() != Some(DibColorUsage::RgbColors) {
      return Err(Error::invalid(
        0,
        "META_DIBCREATEPATTERNBRUSH BS_PATTERN requires DIB_RGB_COLORS",
      ));
    }
  } else if value.color_usage_kind().is_none() {
    return Err(Error::invalid(
      0,
      "META_DIBCREATEPATTERNBRUSH ColorUsage is not a valid ColorUsage",
    ));
  }
  Ok(())
}

fn validate_wmf_set_dib_to_dev_record(value: &WmfSetDibToDevRecord) -> Result<()> {
  let color_usage = require_wmf_color_usage(value.color_usage)?;
  DeviceIndependentBitmap::from_packed_slice(&value.dib, color_usage)?;
  Ok(())
}

trait WmfBitmap16TransferSource {
  fn raster_operation(&self) -> u32;
  fn has_embedded_source(&self) -> bool;
}

impl WmfBitmap16TransferSource for WmfBitBltRecord {
  fn raster_operation(&self) -> u32 {
    self.raster_operation
  }

  fn has_embedded_source(&self) -> bool {
    matches!(self.target, WmfBitmap16Target::Source(_))
  }
}

impl WmfBitmap16TransferSource for WmfStretchBltRecord {
  fn raster_operation(&self) -> u32 {
    self.raster_operation
  }

  fn has_embedded_source(&self) -> bool {
    matches!(self.target, WmfBitmap16Target::Source(_))
  }
}

fn validate_wmf_bitmap16_transfer_source<T: WmfBitmap16TransferSource>(
  value: &T,
  name: &str,
) -> Result<()> {
  validate_wmf_transfer_source(value.raster_operation(), value.has_embedded_source(), name)
}

fn validate_wmf_bitmap16_transfer_source_strict<T: WmfBitmap16TransferSource>(
  value: &T,
  name: &str,
) -> Result<()> {
  validate_wmf_transfer_source_strict(value.raster_operation(), value.has_embedded_source(), name)
}

fn validate_wmf_dib_transfer_source(value: &WmfDibBitBltRecord, name: &str) -> Result<()> {
  validate_wmf_transfer_source(
    value.raster_operation,
    matches!(value.target, WmfDibTarget::Source(_)),
    name,
  )
}

fn validate_wmf_dib_transfer_source_strict(value: &WmfDibBitBltRecord, name: &str) -> Result<()> {
  validate_wmf_transfer_source_strict(
    value.raster_operation,
    matches!(value.target, WmfDibTarget::Source(_)),
    name,
  )
}

fn validate_wmf_dib_stretch_transfer_source(
  value: &WmfDibStretchBltRecord,
  name: &str,
) -> Result<()> {
  validate_wmf_transfer_source(
    value.raster_operation,
    matches!(value.target, WmfDibTarget::Source(_)),
    name,
  )
}

fn validate_wmf_dib_stretch_transfer_source_strict(
  value: &WmfDibStretchBltRecord,
  name: &str,
) -> Result<()> {
  validate_wmf_transfer_source_strict(
    value.raster_operation,
    matches!(value.target, WmfDibTarget::Source(_)),
    name,
  )
}

fn validate_wmf_transfer_source(
  raster_operation: u32,
  _has_embedded_source: bool,
  name: &str,
) -> Result<()> {
  validate_wmf_ternary_raster_operation(raster_operation, name)
}

fn validate_wmf_transfer_source_strict(
  raster_operation: u32,
  has_embedded_source: bool,
  name: &str,
) -> Result<()> {
  validate_wmf_transfer_source(raster_operation, has_embedded_source, name)?;
  if !has_embedded_source && WmfTernaryRasterOperation::new(raster_operation).uses_source() {
    return Err(Error::invalid(
      0,
      format!("{name} without embedded source must not use a source-dependent ROP"),
    ));
  }
  Ok(())
}

fn validate_wmf_stretch_dib_record(value: &WmfStretchDibRecord) -> Result<()> {
  validate_wmf_ternary_raster_operation(value.raster_operation, "META_STRETCHDIB")?;
  require_wmf_color_usage(value.color_usage)?;
  Ok(())
}

fn validate_wmf_stretch_dib_record_strict(value: &WmfStretchDibRecord) -> Result<()> {
  validate_wmf_stretch_dib_record(value)?;
  let color_usage = require_wmf_color_usage(value.color_usage)?;
  let dib = DeviceIndependentBitmap::from_packed_slice(&value.dib, color_usage)?;
  dib.validate_strict()?;
  if dib.embedded_format().is_some() {
    if color_usage != DibColorUsage::RgbColors {
      return Err(Error::invalid(
        0,
        "META_STRETCHDIB JPEG/PNG requires DIB_RGB_COLORS",
      ));
    }
    if value.raster_operation_code() != WmfTernaryRasterOperationCode::SRCCOPY {
      return Err(Error::invalid(
        0,
        "META_STRETCHDIB JPEG/PNG requires SRCCOPY",
      ));
    }
  }
  Ok(())
}

fn validate_wmf_ternary_raster_operation(raster_operation: u32, name: &str) -> Result<()> {
  if !WmfTernaryRasterOperation::new(raster_operation).is_valid() {
    return Err(Error::invalid(
      0,
      format!("{name} RasterOperation is not a valid TernaryRasterOperation"),
    ));
  }
  Ok(())
}

fn validate_wmf_bitmap16_header(value: &WmfBitmap16Header) -> Result<()> {
  if value.planes != 1 {
    return Err(Error::invalid(0, "Bitmap16 Planes must be 1"));
  }
  if value.width_bytes < 0 {
    return Err(Error::invalid(
      0,
      "Bitmap16 WidthBytes must be non-negative",
    ));
  }
  if value.width_bytes as usize != value.computed_width_bytes()? {
    return Err(Error::invalid(
      0,
      "Bitmap16 WidthBytes does not match Width and BitsPixel",
    ));
  }
  value.computed_bits_len()?;
  Ok(())
}

fn validate_wmf_bitmap16(value: &WmfBitmap16) -> Result<()> {
  validate_wmf_bitmap16_header(&value.header)?;
  let expected = value.header.computed_bits_len()?;
  if value.bits.len() != expected {
    return Err(Error::invalid(
      0,
      "Bitmap16 Bits length does not match WidthBytes and Height",
    ));
  }
  Ok(())
}

fn validate_wmf_create_pattern_brush_record(value: &WmfCreatePatternBrushRecord) -> Result<()> {
  validate_wmf_bitmap16_header(&value.bitmap)?;
  let expected = value.bitmap.computed_bits_len()?;
  if value.pattern.len() != expected {
    return Err(Error::invalid(
      0,
      "META_CREATEPATTERNBRUSH Pattern length does not match Bitmap16 parameters",
    ));
  }
  Ok(())
}

fn validate_wmf_scan_object(value: &WmfScanObject) -> Result<()> {
  if value.scan_lines.len() > (u16::MAX as usize / 2) {
    return Err(Error::invalid(0, "WMF scan has too many endpoints"));
  }
  let expected_count = (value.scan_lines.len() * 2) as u16;
  if value.count != expected_count {
    return Err(Error::invalid(
      0,
      "WMF Scan Count does not match ScanLines length",
    ));
  }
  if value.count2 != value.count {
    return Err(Error::invalid(0, "WMF Scan Count2 must equal Count"));
  }
  Ok(())
}

fn validate_wmf_region_object(value: &WmfRegionObject) -> Result<()> {
  if value.object_type != 6 {
    return Err(Error::invalid(0, "WMF Region ObjectType must be 6"));
  }
  if value.scans.len() > i16::MAX as usize {
    return Err(Error::invalid(0, "WMF region has too many scans"));
  }
  if value.scan_count < 0 || value.scan_count as usize != value.scans.len() {
    return Err(Error::invalid(
      0,
      "WMF Region ScanCount does not match aScans length",
    ));
  }
  let max_scan = value.scans.iter().map(|scan| scan.count).max().unwrap_or(0);
  if value.max_scan < 0 || value.max_scan as u16 != max_scan {
    return Err(Error::invalid(
      0,
      "WMF Region maxScan does not match aScans",
    ));
  }
  let expected_size = wmf_region_object_size(value)?;
  if value.region_size < 0 || value.region_size as usize != expected_size {
    return Err(Error::invalid(
      0,
      "WMF Region RegionSize does not match object size",
    ));
  }
  for scan in &value.scans {
    validate_wmf_scan_object(scan)?;
  }
  Ok(())
}

fn wmf_scan_object_size(value: &WmfScanObject) -> Result<usize> {
  value
    .scan_lines
    .len()
    .checked_mul(4)
    .and_then(|scan_lines_size| 8usize.checked_add(scan_lines_size))
    .ok_or_else(|| Error::invalid(0, "WMF Scan size overflows"))
}

fn wmf_region_object_size(value: &WmfRegionObject) -> Result<usize> {
  value.scans.iter().try_fold(22usize, |size, scan| {
    size
      .checked_add(wmf_scan_object_size(scan)?)
      .ok_or_else(|| Error::invalid(0, "WMF Region size overflows"))
  })
}

fn require_wmf_color_usage(value: u16) -> Result<DibColorUsage> {
  DibColorUsage::from_wmf_raw(value).ok_or_else(|| Error::invalid(0, "WMF ColorUsage is invalid"))
}

fn validate_wmf_ext_text_out_record(value: &WmfExtTextOutRecord) -> Result<()> {
  if value.options.bits() & !WmfExtTextOutOptions::all().bits() != 0 {
    return Err(Error::invalid(
      0,
      "META_EXTTEXTOUT fwOpts contains invalid flags",
    ));
  }
  if value
    .options
    .intersects(WmfExtTextOutOptions::OPAQUE | WmfExtTextOutOptions::CLIPPED)
    && value.rectangle.is_none()
  {
    return Err(Error::invalid(
      0,
      "META_EXTTEXTOUT rectangle is required by ETO_OPAQUE or ETO_CLIPPED",
    ));
  }
  if value.string_length < 0 || value.string_length as usize != value.string.len() {
    return Err(Error::invalid(
      0,
      "META_EXTTEXTOUT string length does not match string data",
    ));
  }
  if !(value.string.len() + value.string_padding.len()).is_multiple_of(2) {
    return Err(Error::invalid(
      0,
      "META_EXTTEXTOUT string field must be WORD aligned",
    ));
  }
  let expected_dx_count = if value.options.contains(WmfExtTextOutOptions::PDY) {
    value
      .string
      .len()
      .checked_mul(2)
      .ok_or_else(|| Error::invalid(0, "META_EXTTEXTOUT ETO_PDY Dx count overflows"))?
  } else {
    value.string.len()
  };
  if !value.dx.is_empty() && value.dx.len() != expected_dx_count {
    return Err(Error::invalid(
      0,
      "META_EXTTEXTOUT Dx count must match StringLength",
    ));
  }
  if !value.trailing_data.is_empty() {
    return Err(Error::invalid(
      0,
      "META_EXTTEXTOUT must not contain trailing data",
    ));
  }
  Ok(())
}

fn validate_wmf_font_object(value: &WmfFontObject) -> Result<()> {
  if value.face_name_bytes > 32 {
    return Err(Error::invalid(0, "WMF Font FaceName exceeds 32 bytes"));
  }
  Ok(())
}

fn validate_wmf_font_object_strict(value: &WmfFontObject) -> Result<()> {
  validate_wmf_font_object(value)?;
  if value.face_name_bytes != 32 {
    return Err(Error::invalid(0, "WMF Font FaceName must occupy 32 bytes"));
  }
  if !(0..=1000).contains(&value.weight) {
    return Err(Error::invalid(0, "WMF Font Weight must be 0 through 1000"));
  }
  if value.italic > 1 {
    return Err(Error::invalid(0, "WMF Font Italic must be a Boolean"));
  }
  if value.underline > 1 {
    return Err(Error::invalid(0, "WMF Font Underline must be a Boolean"));
  }
  if value.strike_out > 1 {
    return Err(Error::invalid(0, "WMF Font StrikeOut must be a Boolean"));
  }
  if value.out_precision_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF Font OutPrecision is not a valid OutPrecision",
    ));
  }
  if value.invalid_clip_precision_bits() != 0 {
    return Err(Error::invalid(
      0,
      "WMF Font ClipPrecision contains invalid flags",
    ));
  }
  if value.quality_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF Font Quality is not a valid FontQuality",
    ));
  }
  if value.pitch_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF Font PitchAndFamily pitch is not a valid PitchFont",
    ));
  }
  if value.pitch_and_family_object().reserved_bits() != 0 {
    return Err(Error::invalid(
      0,
      "WMF Font PitchAndFamily reserved bits are nonzero",
    ));
  }
  if value.family_kind().is_none() {
    return Err(Error::invalid(
      0,
      "WMF Font PitchAndFamily family is not a valid FamilyFont",
    ));
  }
  Ok(())
}

fn validate_escape_record(value: &WmfEscapeRecord) -> Result<()> {
  let Some(escape) = value.escape_kind() else {
    return Err(Error::invalid(0, "META_ESCAPE EscapeFunction is invalid"));
  };
  validate_escape_record_padding(value)?;
  match escape {
    WmfMetafileEscape::AbortDoc
    | WmfMetafileEscape::BeginPath
    | WmfMetafileEscape::CloseChannel
    | WmfMetafileEscape::DownloadFace
    | WmfMetafileEscape::DownloadHeader
    | WmfMetafileEscape::EndDoc
    | WmfMetafileEscape::EndPath
    | WmfMetafileEscape::ExtTextOut
    | WmfMetafileEscape::FlushOut
    | WmfMetafileEscape::GetDeviceUnits
    | WmfMetafileEscape::GetExtendedTextMetrics
    | WmfMetafileEscape::GetFaceName
    | WmfMetafileEscape::GetPairKernTable
    | WmfMetafileEscape::GetPhysPageSize
    | WmfMetafileEscape::GetPrintingOffset
    | WmfMetafileEscape::GetScalingFactor
    | WmfMetafileEscape::MetafileDriver
    | WmfMetafileEscape::NewFrame
    | WmfMetafileEscape::NextBand
    | WmfMetafileEscape::OpenChannel
    | WmfMetafileEscape::PostScriptIgnore
    | WmfMetafileEscape::QueryDibSupport => {
      ensure_escape_data_len(value, 0, "META_ESCAPE no-data escape")?;
    }
    WmfMetafileEscape::ClipToPath => {
      ensure_escape_data_len(value, 4, "CLIP_TO_PATH")?;
      if value.post_script_clipping_kind().is_none() {
        return Err(Error::invalid(0, "CLIP_TO_PATH ClipFunction is invalid"));
      }
    }
    WmfMetafileEscape::GetPsFeatureSetting => {
      ensure_escape_data_len(value, 4, "GET_PS_FEATURESETTING")?;
      let setting = read_escape_i32(&value.escape_data)
        .ok_or_else(|| Error::invalid(0, "GET_PS_FEATURESETTING setting missing"))?;
      if !is_valid_post_script_feature_setting(setting) {
        return Err(Error::invalid(
          0,
          "GET_PS_FEATURESETTING FeatureSetting is invalid",
        ));
      }
    }
    WmfMetafileEscape::MetaFile
      if read_escape_u32(&value.escape_data) == Some(WMF_EMF_COMMENT_IDENTIFIER) =>
    {
      parse_enhanced_metafile_escape(&value.escape_data)?;
    }
    WmfMetafileEscape::StartDoc => {
      validate_start_doc_escape(&value.escape_data)?;
    }
    WmfMetafileEscape::GetColorTable => {
      parse_get_color_table_escape(&value.escape_data)?;
    }
    WmfMetafileEscape::DrawPatternRect => {
      parse_draw_pattern_rect_escape(&value.escape_data)?;
    }
    WmfMetafileEscape::EncapsulatedPostScript => {
      parse_encapsulated_postscript_escape(&value.escape_data)?;
    }
    WmfMetafileEscape::EpsPrinting => {
      ensure_escape_data_len(value, 2, "EPSPRINTING")?;
    }
    WmfMetafileEscape::PostScriptInjection => {
      parse_postscript_injection(&value.escape_data)?;
    }
    WmfMetafileEscape::QueryEscSupport => {
      ensure_escape_data_len(value, 2, "QUERYESCSUPPORT")?;
      let query = read_escape_u16(&value.escape_data)
        .ok_or_else(|| Error::invalid(0, "QUERYESCSUPPORT Query missing"))?;
      if WmfMetafileEscape::from_raw(query).is_none() {
        return Err(Error::invalid(0, "QUERYESCSUPPORT Query is invalid"));
      }
    }
    WmfMetafileEscape::SetCopyCount => {
      ensure_escape_data_len(value, 2, "SETCOPYCOUNT")?;
    }
    WmfMetafileEscape::SetLineCap => {
      ensure_escape_data_len(value, 4, "SETLINECAP")?;
      if value.post_script_cap_kind().is_none() {
        return Err(Error::invalid(0, "SETLINECAP Cap is invalid"));
      }
    }
    WmfMetafileEscape::SetLineJoin => {
      ensure_escape_data_len(value, 4, "SETLINEJOIN")?;
      if value.post_script_join_kind().is_none() {
        return Err(Error::invalid(0, "SETLINEJOIN Join is invalid"));
      }
    }
    WmfMetafileEscape::SetMiterLimit => {
      ensure_escape_data_len(value, 4, "SETMITERLIMIT")?;
    }
    WmfMetafileEscape::SpclPassThrough2 => {
      parse_spcl_pass_through2(&value.escape_data)?;
    }
    _ => {}
  }
  Ok(())
}

fn validate_escape_record_padding(value: &WmfEscapeRecord) -> Result<()> {
  let expected = value.escape_data.len() % 2;
  if value.padding.len() == expected {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      "META_ESCAPE padding must align EscapeData to a WORD",
    ))
  }
}

const fn is_no_data_escape(escape: WmfMetafileEscape) -> bool {
  matches!(
    escape,
    WmfMetafileEscape::AbortDoc
      | WmfMetafileEscape::BeginPath
      | WmfMetafileEscape::CloseChannel
      | WmfMetafileEscape::DownloadFace
      | WmfMetafileEscape::DownloadHeader
      | WmfMetafileEscape::EndDoc
      | WmfMetafileEscape::EndPath
      | WmfMetafileEscape::ExtTextOut
      | WmfMetafileEscape::FlushOut
      | WmfMetafileEscape::GetDeviceUnits
      | WmfMetafileEscape::GetExtendedTextMetrics
      | WmfMetafileEscape::GetFaceName
      | WmfMetafileEscape::GetPairKernTable
      | WmfMetafileEscape::GetPhysPageSize
      | WmfMetafileEscape::GetPrintingOffset
      | WmfMetafileEscape::GetScalingFactor
      | WmfMetafileEscape::MetafileDriver
      | WmfMetafileEscape::NewFrame
      | WmfMetafileEscape::NextBand
      | WmfMetafileEscape::OpenChannel
      | WmfMetafileEscape::PostScriptIgnore
      | WmfMetafileEscape::QueryDibSupport
  )
}

struct ParsedEnhancedMetafileEscape<'a> {
  comment_identifier: u32,
  comment_type: u32,
  version: u32,
  checksum: u16,
  flags: u32,
  comment_record_count: u32,
  current_record_size: u32,
  remaining_bytes: u32,
  enhanced_metafile_data_size: u32,
  enhanced_metafile_data: &'a [u8],
}

struct EnhancedMetafileEscapeValidation {
  comment_identifier: u32,
  comment_type: u32,
  flags: u32,
  comment_record_count: u32,
  current_record_size: u32,
  remaining_bytes: u32,
  enhanced_metafile_data_size: u32,
  data_len: usize,
}

fn parse_enhanced_metafile_escape(data: &[u8]) -> Result<ParsedEnhancedMetafileEscape<'_>> {
  if data.len() < WMF_EMF_ESCAPE_HEADER_SIZE {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE data is shorter than the fixed header",
    ));
  }
  let comment_identifier = u32::from_le_bytes(data[0..4].try_into().unwrap());
  let comment_type = u32::from_le_bytes(data[4..8].try_into().unwrap());
  let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
  let checksum = u16::from_le_bytes(data[12..14].try_into().unwrap());
  let flags = u32::from_le_bytes(data[14..18].try_into().unwrap());
  let comment_record_count = u32::from_le_bytes(data[18..22].try_into().unwrap());
  let current_record_size = u32::from_le_bytes(data[22..26].try_into().unwrap());
  let remaining_bytes = u32::from_le_bytes(data[26..30].try_into().unwrap());
  let enhanced_metafile_data_size = u32::from_le_bytes(data[30..34].try_into().unwrap());
  validate_enhanced_metafile_escape_fields(EnhancedMetafileEscapeValidation {
    comment_identifier,
    comment_type,
    flags,
    comment_record_count,
    current_record_size,
    remaining_bytes,
    enhanced_metafile_data_size,
    data_len: data.len().saturating_sub(WMF_EMF_ESCAPE_HEADER_SIZE),
  })?;
  Ok(ParsedEnhancedMetafileEscape {
    comment_identifier,
    comment_type,
    version,
    checksum,
    flags,
    comment_record_count,
    current_record_size,
    remaining_bytes,
    enhanced_metafile_data_size,
    enhanced_metafile_data: &data[WMF_EMF_ESCAPE_HEADER_SIZE..],
  })
}

fn validate_enhanced_metafile_escape_fields(value: EnhancedMetafileEscapeValidation) -> Result<()> {
  if value.comment_identifier != WMF_EMF_COMMENT_IDENTIFIER {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE CommentIdentifier must be 0x43464D57",
    ));
  }
  if value.comment_type != WMF_EMF_COMMENT_TYPE {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE CommentType must be 1",
    ));
  }
  if value.flags != 0 {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE Flags must be zero",
    ));
  }
  if value.comment_record_count == 0 {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE CommentRecordCount must be nonzero",
    ));
  }
  if value.current_record_size > WMF_EMF_ESCAPE_MAX_RECORD_SIZE {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE CurrentRecordSize must be <= 8192",
    ));
  }
  if value.current_record_size as usize != value.data_len {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE CurrentRecordSize does not match data length",
    ));
  }
  let remaining_after_current = value
    .current_record_size
    .checked_add(value.remaining_bytes)
    .ok_or_else(|| {
      Error::invalid(
        0,
        "META_ESCAPE_ENHANCED_METAFILE CurrentRecordSize plus RemainingBytes overflows",
      )
    })?;
  if remaining_after_current > value.enhanced_metafile_data_size {
    return Err(Error::invalid(
      0,
      "META_ESCAPE_ENHANCED_METAFILE chunk sizes exceed EnhancedMetafileDataSize",
    ));
  }
  Ok(())
}

struct ParsedPostScriptInjection<'a> {
  data_size: u32,
  injection_point: u16,
  page_number: u16,
  raw_data: &'a [u8],
  trailing_data: &'a [u8],
}

fn parse_postscript_injection(data: &[u8]) -> Result<ParsedPostScriptInjection<'_>> {
  if data.len() < 8 {
    return Err(Error::invalid(
      0,
      "POSTSCRIPT_INJECTION data is shorter than DataSize, InjectionPoint, and PageNumber",
    ));
  }
  let data_size = u32::from_le_bytes(data[0..4].try_into().unwrap());
  let injection_point = u16::from_le_bytes(data[4..6].try_into().unwrap());
  let page_number = u16::from_le_bytes(data[6..8].try_into().unwrap());
  let data_size = data_size as usize;
  let end = 8usize
    .checked_add(data_size)
    .ok_or_else(|| Error::invalid(0, "POSTSCRIPT_INJECTION DataSize overflows"))?;
  if data.len() < end {
    return Err(Error::invalid(
      0,
      "POSTSCRIPT_INJECTION DataSize exceeds EscapeData",
    ));
  }
  Ok(ParsedPostScriptInjection {
    data_size: data_size as u32,
    injection_point,
    page_number,
    raw_data: &data[8..end],
    trailing_data: &data[end..],
  })
}

fn parse_spcl_pass_through2(data: &[u8]) -> Result<(u32, &[u8], &[u8])> {
  if data.len() < 6 {
    return Err(Error::invalid(
      0,
      "SPCLPASSTHROUGH2 data is shorter than Reserved and Size",
    ));
  }
  let reserved = u32::from_le_bytes(data[0..4].try_into().unwrap());
  let size = u16::from_le_bytes(data[4..6].try_into().unwrap()) as usize;
  if data.len() < 6 + size {
    return Err(Error::invalid(
      0,
      "SPCLPASSTHROUGH2 Size exceeds EscapeData",
    ));
  }
  Ok((reserved, &data[6..6 + size], &data[6 + size..]))
}

fn ensure_escape_data_len(value: &WmfEscapeRecord, expected: usize, name: &str) -> Result<()> {
  if value.escape_data.len() == expected {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} ByteCount must be {expected}"),
    ))
  }
}

fn is_valid_post_script_feature_setting(value: i32) -> bool {
  WmfPostScriptFeatureSetting::from_raw(value).is_some()
    || (WmfPostScriptFeatureSetting::PrivateBegin.raw()
      ..=WmfPostScriptFeatureSetting::PrivateEnd.raw())
      .contains(&value)
}

fn ensure_no_data(data: &[u8], name: &str) -> Result<()> {
  if data.is_empty() {
    Ok(())
  } else {
    Err(Error::invalid(
      0,
      format!("{name} record has unexpected payload"),
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

fn read_object<T: SdkRead>(data: &[u8], name: &str) -> Result<T> {
  let mut reader = Reader::new(Cursor::new(data));
  let value = T::read_from(&mut reader)?;
  ensure_reader_end(&mut reader, data.len() as u64, name)?;
  Ok(value)
}

fn ensure_empty_trailing_data(data: &[u8], name: &str) -> Result<()> {
  if data.is_empty() {
    Ok(())
  } else {
    Err(Error::invalid(0, format!("{name} has trailing data")))
  }
}

fn object_record<T: SdkWrite + SdkSize>(
  function: WmfRecordFunction,
  value: &T,
) -> Result<WmfRecord> {
  let capacity = usize::try_from(value.sdk_size())
    .map_err(|_| Error::invalid(0, "WMF object size overflows usize"))?;
  let mut writer = Writer::new(Cursor::new(Vec::with_capacity(capacity)));
  value.write_to(&mut writer)?;
  Ok(WmfRecord::new(
    function.raw(),
    writer.into_inner().into_inner(),
  ))
}

fn u16_record(function: WmfRecordFunction, value: &WmfU16Record) -> Result<WmfRecord> {
  Ok(WmfRecord::new(function.raw(), value.write_data()?))
}

fn no_data_record(function: WmfRecordFunction) -> WmfRecord {
  WmfRecord::new(function.raw(), Vec::new())
}

fn record_size_words(record: &WmfRecord) -> Result<u32> {
  record_size_words_parts(record.data.len())
}

fn record_size_words_parts(data_len: usize) -> Result<u32> {
  let size_bytes = data_len
    .checked_add(6)
    .ok_or_else(|| Error::invalid(0, "WMF record size overflows"))?;
  if !size_bytes.is_multiple_of(2) {
    return Err(Error::invalid(0, "WMF record has odd byte size"));
  }
  if size_bytes / 2 > u32::MAX as usize {
    return Err(Error::invalid(0, "WMF record size exceeds u32::MAX WORDs"));
  }
  Ok((size_bytes / 2) as u32)
}

fn count_wmf_object_creation_records(records: &[WmfRecord]) -> Result<u16> {
  let mut count = 0u32;
  for record in records {
    if matches!(
      record.normalized_function_kind(),
      Some(
        WmfRecordFunction::CreateBrushIndirect
          | WmfRecordFunction::CreateFontIndirect
          | WmfRecordFunction::CreatePalette
          | WmfRecordFunction::CreatePatternBrush
          | WmfRecordFunction::CreatePenIndirect
          | WmfRecordFunction::CreateRegion
          | WmfRecordFunction::DibCreatePatternBrush
      )
    ) {
      count = count
        .checked_add(1)
        .ok_or_else(|| Error::invalid(0, "WMF NumberOfObjects overflows"))?;
    }
  }
  u16::try_from(count).map_err(|_| Error::invalid(0, "WMF NumberOfObjects exceeds u16::MAX"))
}

fn validate_wmf_object_table_references(
  number_of_objects: u16,
  records: &[WmfRecord],
) -> Result<()> {
  if let Some(index) = max_wmf_referenced_object_index(records)?
    && index >= number_of_objects
  {
    return Err(Error::invalid(
      0,
      "WMF object table reference exceeds NumberOfObjects",
    ));
  }
  Ok(())
}

fn max_wmf_referenced_object_index(records: &[WmfRecord]) -> Result<Option<u16>> {
  let mut max_index = None;
  for record in records {
    let mut push_index = |index| {
      max_index = Some(max_index.map_or(index, |current: u16| current.max(index)));
    };
    match record.normalized_function_kind() {
      Some(
        WmfRecordFunction::InvertRegion
        | WmfRecordFunction::PaintRegion
        | WmfRecordFunction::SelectClipRegion
        | WmfRecordFunction::SelectObject
        | WmfRecordFunction::SelectPalette
        | WmfRecordFunction::DeleteObject,
      ) => {
        let value: WmfObjectIndexRecord = read_object(&record.data, "WMF object table reference")?;
        push_index(value.index);
      }
      Some(WmfRecordFunction::FillRegion) => {
        let value: WmfRegionBrushRecord = read_object(&record.data, "META_FILLREGION")?;
        push_index(value.region);
        push_index(value.brush);
      }
      Some(WmfRecordFunction::FrameRegion) => {
        let value: WmfFrameRegionRecord = read_object(&record.data, "META_FRAMEREGION")?;
        push_index(value.region);
        push_index(value.brush);
      }
      _ => {}
    }
  }
  Ok(max_index)
}

fn has_bitmap_source(record: &WmfRecord) -> Result<bool> {
  has_bitmap_source_parts(record.function, record.data.len())
}

fn has_bitmap_source_parts(function: u16, data_len: usize) -> Result<bool> {
  let kind = normalized_wmf_record_function(function).ok_or_else(|| {
    Error::invalid(
      0,
      "WMF bitmap transfer record function byte is not recognized",
    )
  })?;
  let canonical = kind.raw();
  let size_words = record_size_words_parts(data_len)?;
  let fixed_words = u32::from(canonical >> 8);
  let expected_no_source_words = fixed_words
    .checked_add(3)
    .ok_or_else(|| Error::invalid(0, "WMF bitmap transfer size overflows"))?;
  if function != canonical {
    return Err(Error::invalid(
      0,
      "WMF bitmap transfer RecordFunction high byte is invalid",
    ));
  }
  match size_words.cmp(&expected_no_source_words) {
    std::cmp::Ordering::Equal => Ok(false),
    std::cmp::Ordering::Greater => Ok(true),
    std::cmp::Ordering::Less => Err(Error::invalid(
      0,
      "WMF bitmap transfer RecordSize is smaller than fixed fields",
    )),
  }
}

fn normalized_wmf_record_function(function: u16) -> Option<WmfRecordFunction> {
  WmfRecordFunction::from_raw(function).or(match function & 0x00FF {
    0x00 => Some(WmfRecordFunction::Eof),
    0x01 => Some(WmfRecordFunction::SetBkColor),
    0x02 => Some(WmfRecordFunction::SetBkMode),
    0x03 => Some(WmfRecordFunction::SetMapMode),
    0x04 => Some(WmfRecordFunction::SetRop2),
    0x05 => Some(WmfRecordFunction::SetRelabs),
    0x06 => Some(WmfRecordFunction::SetPolyFillMode),
    0x07 => Some(WmfRecordFunction::SetStretchBltMode),
    0x08 => Some(WmfRecordFunction::SetTextCharExtra),
    0x09 => Some(WmfRecordFunction::SetTextColor),
    0x0A => Some(WmfRecordFunction::SetTextJustification),
    0x0B => Some(WmfRecordFunction::SetWindowOrg),
    0x0C => Some(WmfRecordFunction::SetWindowExt),
    0x0D => Some(WmfRecordFunction::SetViewportOrg),
    0x0E => Some(WmfRecordFunction::SetViewportExt),
    0x0F => Some(WmfRecordFunction::OffsetWindowOrg),
    0x10 => Some(WmfRecordFunction::ScaleWindowExt),
    0x11 => Some(WmfRecordFunction::OffsetViewportOrg),
    0x12 => Some(WmfRecordFunction::ScaleViewportExt),
    0x13 => Some(WmfRecordFunction::LineTo),
    0x14 => Some(WmfRecordFunction::MoveTo),
    0x15 => Some(WmfRecordFunction::ExcludeClipRect),
    0x16 => Some(WmfRecordFunction::IntersectClipRect),
    0x17 => Some(WmfRecordFunction::Arc),
    0x18 => Some(WmfRecordFunction::Ellipse),
    0x19 => Some(WmfRecordFunction::FloodFill),
    0x1A => Some(WmfRecordFunction::Pie),
    0x1B => Some(WmfRecordFunction::Rectangle),
    0x1C => Some(WmfRecordFunction::RoundRect),
    0x1D => Some(WmfRecordFunction::PatBlt),
    0x1E => Some(WmfRecordFunction::SaveDc),
    0x20 => Some(WmfRecordFunction::OffsetClipRgn),
    0x21 => Some(WmfRecordFunction::TextOut),
    0x22 => Some(WmfRecordFunction::BitBlt),
    0x23 => Some(WmfRecordFunction::StretchBlt),
    0x24 => Some(WmfRecordFunction::Polygon),
    0x25 => Some(WmfRecordFunction::Polyline),
    0x26 => Some(WmfRecordFunction::Escape),
    0x27 => Some(WmfRecordFunction::RestoreDc),
    0x28 => Some(WmfRecordFunction::FillRegion),
    0x29 => Some(WmfRecordFunction::FrameRegion),
    0x2A => Some(WmfRecordFunction::InvertRegion),
    0x2B => Some(WmfRecordFunction::PaintRegion),
    0x2C => Some(WmfRecordFunction::SelectClipRegion),
    0x2D => Some(WmfRecordFunction::SelectObject),
    0x2E => Some(WmfRecordFunction::SetTextAlign),
    0x30 => Some(WmfRecordFunction::Chord),
    0x31 => Some(WmfRecordFunction::SetMapperFlags),
    0x32 => Some(WmfRecordFunction::ExtTextOut),
    0x33 => Some(WmfRecordFunction::SetDibToDev),
    0x34 => Some(WmfRecordFunction::SelectPalette),
    0x35 => Some(WmfRecordFunction::RealizePalette),
    0x36 => Some(WmfRecordFunction::AnimatePalette),
    0x37 => Some(WmfRecordFunction::SetPalEntries),
    0x38 => Some(WmfRecordFunction::PolyPolygon),
    0x39 => Some(WmfRecordFunction::ResizePalette),
    0x40 => Some(WmfRecordFunction::DibBitBlt),
    0x41 => Some(WmfRecordFunction::DibStretchBlt),
    0x42 => Some(WmfRecordFunction::DibCreatePatternBrush),
    0x43 => Some(WmfRecordFunction::StretchDib),
    0x48 => Some(WmfRecordFunction::ExtFloodFill),
    0x49 => Some(WmfRecordFunction::SetLayout),
    0xF0 => Some(WmfRecordFunction::DeleteObject),
    0xF7 => Some(WmfRecordFunction::CreatePalette),
    0xF9 => Some(WmfRecordFunction::CreatePatternBrush),
    0xFA => Some(WmfRecordFunction::CreatePenIndirect),
    0xFB => Some(WmfRecordFunction::CreateFontIndirect),
    0xFC => Some(WmfRecordFunction::CreateBrushIndirect),
    0xFF => Some(WmfRecordFunction::CreateRegion),
    _ => None,
  })
}

fn read_remaining<R: std::io::Read + std::io::Seek>(
  reader: &mut Reader<R>,
  end: usize,
) -> Result<Vec<u8>> {
  let position = reader.position()? as usize;
  if position > end {
    return Err(Error::invalid(position as u64, "reader passed end of data"));
  }
  reader.read_vec(end - position)
}

pub fn looks_like_wmf(bytes: &[u8]) -> bool {
  let offset = if has_placeable_header(bytes) {
    PLACEABLE_HEADER_SIZE
  } else {
    0
  };
  if bytes.len() < offset + WMF_HEADER_SIZE {
    return false;
  }
  let metafile_type = u16::from_le_bytes(
    bytes[offset..offset + 2]
      .try_into()
      .expect("slice length checked"),
  );
  let header_size = u16::from_le_bytes(
    bytes[offset + 2..offset + 4]
      .try_into()
      .expect("slice length checked"),
  );
  let version = u16::from_le_bytes(
    bytes[offset + 4..offset + 6]
      .try_into()
      .expect("slice length checked"),
  );
  matches!(metafile_type, 1 | 2) && header_size == 9 && matches!(version, 0x0100 | 0x0300)
}

fn has_placeable_header(bytes: &[u8]) -> bool {
  bytes.len() >= PLACEABLE_HEADER_SIZE
    && u32::from_le_bytes(bytes[0..4].try_into().expect("slice length checked")) == PLACEABLE_KEY
}

#[cfg(test)]
mod tests {
  use super::*;

  fn minimal_wmf() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&9u16.to_le_bytes());
    bytes.extend_from_slice(&0x0300u16.to_le_bytes());
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&META_EOF.to_le_bytes());
    bytes
  }

  fn log_color_space_bytes(size: usize, filename: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&WmfLogColorSpaceSignature::Psoc.raw().to_le_bytes());
    bytes.extend_from_slice(&0x0000_0400u32.to_le_bytes());
    bytes.extend_from_slice(&(size as u32).to_le_bytes());
    bytes.extend_from_slice(&WmfLogicalColorSpace::CalibratedRgb.raw().to_le_bytes());
    bytes.extend_from_slice(&WmfGamutMappingIntent::Business.raw().to_le_bytes());
    bytes.extend_from_slice(&[0; 36]);
    bytes.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    bytes.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    bytes.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    bytes.extend_from_slice(filename);
    bytes.resize(size, 0);
    bytes
  }

  fn core_1bpp_dib_bytes() -> Vec<u8> {
    [
      12u32.to_le_bytes().as_slice(),
      1u16.to_le_bytes().as_slice(),
      1u16.to_le_bytes().as_slice(),
      1u16.to_le_bytes().as_slice(),
      1u16.to_le_bytes().as_slice(),
      &[0x00, 0x00, 0x00, 0x00],
      &[0xFF, 0xFF, 0xFF, 0x00],
      &[0x80, 0x00],
    ]
    .concat()
  }

  fn png_dib_bytes() -> Vec<u8> {
    [
      crate::bitmap::BITMAP_INFO_HEADER_SIZE
        .to_le_bytes()
        .as_slice(),
      1i32.to_le_bytes().as_slice(),
      1i32.to_le_bytes().as_slice(),
      1u16.to_le_bytes().as_slice(),
      0u16.to_le_bytes().as_slice(),
      WmfCompression::Png.raw().to_le_bytes().as_slice(),
      4u32.to_le_bytes().as_slice(),
      0i32.to_le_bytes().as_slice(),
      0i32.to_le_bytes().as_slice(),
      0u32.to_le_bytes().as_slice(),
      0u32.to_le_bytes().as_slice(),
      &[0x89, b'P', b'N', b'G'],
    ]
    .concat()
  }

  fn test_placeable_header() -> WmfPlaceableHeader {
    WmfPlaceableHeader {
      key: PLACEABLE_KEY,
      handle: 0,
      left: 0,
      top: 0,
      right: 100,
      bottom: 100,
      inch: 1440,
      reserved: 0,
      checksum: 0,
    }
    .with_computed_checksum()
  }

  #[test]
  fn wmf_roundtrip_preserves_bytes() {
    let bytes = minimal_wmf();
    let metafile = WmfMetafile::from_bytes(&bytes).unwrap();
    assert_eq!(
      metafile.header.metafile_type_kind(),
      Some(WmfMetafileType::Memory)
    );
    assert_eq!(
      metafile.header.version_kind(),
      Some(WmfMetafileVersion::Version300)
    );
    assert_eq!(metafile.records.len(), 1);
    assert_eq!(metafile.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn wmf_borrowed_view_uses_input_storage_and_materializes_explicitly() {
    let bytes = minimal_wmf();
    let view = WmfMetafileRef::from_bytes(&bytes).unwrap();
    assert_eq!(view.record_count(), 1);
    assert_eq!(view.trailing_data(), &bytes[24..]);

    let mut records = view.records();
    assert_eq!(records.len(), 1);
    let eof = records.next().unwrap();
    assert_eq!(eof.data.as_ptr(), bytes[24..24].as_ptr());
    assert!(matches!(eof.parse_data().unwrap(), WmfRecordData::Eof(_)));
    assert_eq!(eof.rebuild_typed().unwrap().as_ref(), eof);
    assert!(records.next().is_none());

    let owned = view.into_owned();
    assert_eq!(owned.to_bytes().unwrap(), bytes);

    let mut invalid_late_record = minimal_wmf();
    invalid_late_record[22..24].copy_from_slice(&WmfRecordFunction::SaveDc.raw().to_le_bytes());
    invalid_late_record.extend_from_slice(&2u32.to_le_bytes());
    invalid_late_record.extend_from_slice(&META_EOF.to_le_bytes());
    assert!(WmfMetafileRef::from_bytes(&invalid_late_record).is_err());
  }

  #[test]
  fn wmf_headers_validate_spec_fields() {
    let placeable = test_placeable_header();
    assert_eq!(placeable.sdk_size(), PLACEABLE_HEADER_SIZE as u64);
    let mut bytes = Vec::new();
    placeable.write_to(&mut Writer::new(&mut bytes)).unwrap();
    bytes.extend_from_slice(&minimal_wmf());
    let metafile = WmfMetafile::from_bytes(&bytes).unwrap();
    assert_eq!(metafile.placeable_header, Some(placeable));
    assert_eq!(placeable.bounding_box_width(), 100);
    assert_eq!(placeable.bounding_box_height(), 100);
    assert!(placeable.uses_twips());
    assert_eq!(metafile.header.sdk_size(), WMF_HEADER_SIZE as u64);
    assert_eq!(metafile.header.header_size_bytes(), WMF_HEADER_SIZE as u32);
    assert_eq!(metafile.header.file_size_bytes(), 24);
    assert_eq!(metafile.header.max_record_bytes(), 6);
    assert!(metafile.header.number_of_members_is_zero());
    assert_eq!(metafile.computed_file_size_words().unwrap(), 12);
    assert_eq!(metafile.computed_max_record_words().unwrap(), 3);
    assert_eq!(metafile.computed_number_of_objects().unwrap(), 0);
    assert!(metafile.validate_header_metrics().is_ok());
    assert_eq!(metafile.to_bytes().unwrap(), bytes);

    let create_brush = WmfRecordData::CreateBrushIndirect(WmfLogBrushObject {
      brush_style: WmfBrushStyle::Solid.raw(),
      color_ref: ColorRef {
        red: 1,
        green: 2,
        blue: 3,
        reserved: 0,
      },
      brush_hatch: 0,
    })
    .to_record()
    .unwrap();
    assert_eq!(create_brush.size_words().unwrap(), 7);
    let object_metafile = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 19,
        number_of_objects: 1,
        max_record_words: 7,
        number_of_parameters: 0,
      },
      records: vec![
        create_brush,
        WmfRecordData::Eof(WmfEofRecord::default())
          .to_record()
          .unwrap(),
      ],
      trailing_data: Vec::new(),
    };
    assert_eq!(object_metafile.header.file_size_bytes(), 38);
    assert_eq!(object_metafile.header.max_record_bytes(), 14);
    assert_eq!(object_metafile.computed_number_of_objects().unwrap(), 1);
    assert!(object_metafile.validate_header_metrics().is_ok());
    let object_bytes = object_metafile.to_bytes().unwrap();
    assert_eq!(
      WmfMetafile::from_bytes(&object_bytes)
        .unwrap()
        .computed_number_of_objects()
        .unwrap(),
      1
    );
    let mut invalid_number_of_objects = object_bytes.clone();
    invalid_number_of_objects[10..12].copy_from_slice(&0_u16.to_le_bytes());
    let invalid_number_of_objects_metafile =
      WmfMetafile::from_bytes(&invalid_number_of_objects).unwrap();
    assert!(
      invalid_number_of_objects_metafile
        .validate_header_metrics()
        .is_err()
    );
    let mut invalid_object_metafile = object_metafile.clone();
    invalid_object_metafile.header.number_of_objects = 0;
    assert!(invalid_object_metafile.validate_header_metrics().is_err());
    assert!(invalid_object_metafile.to_bytes().is_ok());

    let selected_object_metafile = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 23,
        number_of_objects: 1,
        max_record_words: 7,
        number_of_parameters: 0,
      },
      records: vec![
        object_metafile.records[0].clone(),
        WmfRecordData::SelectObject(WmfObjectIndexRecord { index: 0 })
          .to_record()
          .unwrap(),
        WmfRecordData::Eof(WmfEofRecord::default())
          .to_record()
          .unwrap(),
      ],
      trailing_data: Vec::new(),
    };
    assert!(selected_object_metafile.validate_header_metrics().is_ok());
    let mut invalid_selected_object_metafile = selected_object_metafile.clone();
    invalid_selected_object_metafile.records[1] =
      WmfRecordData::SelectObject(WmfObjectIndexRecord { index: 1 })
        .to_record()
        .unwrap();
    assert!(
      invalid_selected_object_metafile
        .validate_header_metrics()
        .is_err()
    );

    let mut invalid_checksum = bytes.clone();
    invalid_checksum[PLACEABLE_HEADER_SIZE - 1] ^= 0xFF;
    let invalid_checksum_metafile = WmfMetafile::from_bytes(&invalid_checksum).unwrap();
    assert!(
      invalid_checksum_metafile
        .placeable_header
        .as_ref()
        .expect("placeable header")
        .validate()
        .is_err()
    );
    assert_eq!(
      invalid_checksum_metafile.to_bytes().unwrap(),
      invalid_checksum
    );

    let mut invalid_reserved = placeable;
    invalid_reserved.reserved = 1;
    invalid_reserved.checksum = invalid_reserved.computed_checksum();
    assert!(invalid_reserved.validate().is_err());
    assert!(
      invalid_reserved
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_ok()
    );

    let mut invalid_inch = placeable;
    invalid_inch.inch = 0;
    invalid_inch.checksum = invalid_inch.computed_checksum();
    assert!(
      invalid_inch
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );

    let mut invalid_type = minimal_wmf();
    invalid_type[0..2].copy_from_slice(&0_u16.to_le_bytes());
    assert!(WmfMetafile::from_bytes(&invalid_type).is_err());

    let mut invalid_version = minimal_wmf();
    invalid_version[4..6].copy_from_slice(&0_u16.to_le_bytes());
    assert!(WmfMetafile::from_bytes(&invalid_version).is_err());

    let mut invalid_file_size = minimal_wmf();
    invalid_file_size[6..10].copy_from_slice(&13_u32.to_le_bytes());
    let invalid_file_size_metafile = WmfMetafile::from_bytes(&invalid_file_size).unwrap();
    assert!(
      invalid_file_size_metafile
        .validate_header_metrics()
        .is_err()
    );

    let mut invalid_max_record = minimal_wmf();
    invalid_max_record[12..16].copy_from_slice(&4_u32.to_le_bytes());
    let invalid_max_record_metafile = WmfMetafile::from_bytes(&invalid_max_record).unwrap();
    assert!(
      invalid_max_record_metafile
        .validate_header_metrics()
        .is_err()
    );

    let mut trailing = minimal_wmf();
    trailing.extend_from_slice(&[0; 2]);
    assert_eq!(
      WmfMetafile::from_bytes(&trailing)
        .unwrap()
        .to_bytes()
        .unwrap(),
      trailing
    );

    let missing_eof = minimal_wmf()[..18].to_vec();
    assert!(WmfMetafile::from_bytes(&missing_eof).is_err());

    let mut invalid_header = metafile.header;
    invalid_header.version = 0;
    assert!(
      invalid_header
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );
  }

  #[test]
  fn detects_wmf_header() {
    assert!(looks_like_wmf(&minimal_wmf()));
  }

  #[test]
  fn maps_wmf_record_function_enum() {
    let record = WmfRecord::new(META_EOF, Vec::new());
    assert_eq!(record.function_kind(), Some(WmfRecordFunction::Eof));
    assert_eq!(
      record.normalized_function_kind(),
      Some(WmfRecordFunction::Eof)
    );
    let high_byte_variant = WmfRecord::new(0x9902, WmfMixMode::Opaque.raw().to_le_bytes().to_vec());
    assert_eq!(high_byte_variant.function_kind(), None);
    assert_eq!(
      high_byte_variant.normalized_function_kind(),
      Some(WmfRecordFunction::SetBkMode)
    );
    let WmfRecordData::SetBkMode(value) = high_byte_variant.parse_data().unwrap() else {
      panic!("expected normalized META_SETBKMODE");
    };
    assert_eq!(value.mix_mode_kind(), Some(WmfMixMode::Opaque));
    assert_eq!(
      high_byte_variant.rebuild_typed().unwrap(),
      high_byte_variant
    );
    assert!(
      high_byte_variant
        .parse_data()
        .unwrap()
        .to_record_with_function(WmfRecordFunction::SetMapMode.raw())
        .is_err()
    );
    let accepted_high_byte_variant = WmfRecord::new(0x9922, Vec::new());
    assert_eq!(
      accepted_high_byte_variant.normalized_function_kind(),
      Some(WmfRecordFunction::BitBlt)
    );
    assert_eq!(accepted_high_byte_variant.size_words().unwrap(), 3);
    assert!(
      accepted_high_byte_variant
        .embedded_source_present()
        .is_err()
    );
    assert!(accepted_high_byte_variant.parse_data().is_err());
    let unknown = WmfRecord::new(0x7777, vec![1, 2]);
    assert!(matches!(
      unknown.parse_data(),
      Ok(WmfRecordData::Unknown(_))
    ));
    assert!(
      unknown
        .parse_data()
        .unwrap()
        .to_record_with_function(0x8877)
        .is_err()
    );
    assert_eq!(high_byte_variant.size_words().unwrap(), 4);
    assert_eq!(high_byte_variant.embedded_source_present().unwrap(), None);
    assert_eq!(WmfRecordFunction::ExtTextOut.raw(), 0x0A32);
    assert_eq!(WmfRecordFunction::SaveDc.raw(), 0x001E);
    assert_eq!(WmfBitCount::TwentyFour.raw(), 0x0018);
    assert_eq!(WmfColorUsage::RgbColors.wmf_raw(), 0x0000);
    assert_eq!(WmfCompression::Png.raw(), 0x0005);
    assert_eq!(WmfLogicalColorSpace::SRgb.raw(), 0x7352_4742);
    assert_eq!(WmfLogicalColorSpaceV5::ProfileEmbedded.raw(), 0x4D42_4544);
    assert_eq!(
      WmfGamutMappingIntent::AbsoluteColorimetric.raw(),
      0x0000_0008
    );
    assert_eq!(WmfFloodFill::Surface.raw(), 0x0001);
    assert!(WmfLayout::RTL.contains(WmfLayout::RTL));
    assert!(WmfPaletteEntryFlag::RESERVED.contains(WmfPaletteEntryFlag::RESERVED));
    assert_eq!(WmfPenStyle::DASH.bits(), WmfPenLineStyle::Dash.raw());
    let rgb_quad = WmfRgbQuad {
      blue: 1,
      green: 2,
      red: 3,
      reserved: 0,
    };
    assert_eq!(rgb_quad.blue, 1);
    assert_eq!(std::mem::size_of::<WmfColorRef>(), 4);
    assert_eq!(std::mem::size_of::<WmfPointS>(), 4);
    assert_eq!(std::mem::size_of::<WmfPointL>(), 8);
    assert_eq!(std::mem::size_of::<WmfRectL>(), 16);
    assert_eq!(std::mem::size_of::<WmfSizeL>(), 8);
    assert_eq!(
      std::any::type_name::<WmfBitmapCoreHeader>(),
      std::any::type_name::<BitmapCoreHeader>()
    );
    assert_eq!(
      std::any::type_name::<WmfBitmapInfoHeader>(),
      std::any::type_name::<BitmapInfoHeader>()
    );
    assert_eq!(
      std::any::type_name::<WmfBitmapV4Header>(),
      std::any::type_name::<BitmapV4Header>()
    );
    assert_eq!(
      std::any::type_name::<WmfBitmapV5Header>(),
      std::any::type_name::<BitmapV5Header>()
    );
    assert_eq!(
      std::any::type_name::<WmfCieXyz>(),
      std::any::type_name::<BitmapCieXyz>()
    );
    assert_eq!(
      std::any::type_name::<WmfCieXyzTriple>(),
      std::any::type_name::<BitmapCieXyzTriple>()
    );
    assert_eq!(
      std::any::type_name::<WmfDeviceIndependentBitmap>(),
      std::any::type_name::<DeviceIndependentBitmap>()
    );
    assert_eq!(WmfLogColorSpace::sdk_size(260), WMF_LOG_COLOR_SPACE_SIZE);
    assert_eq!(WmfLogColorSpaceW::sdk_size(520), WMF_LOG_COLOR_SPACE_W_SIZE);
    assert_eq!(WmfLogColorSpaceSignature::Psoc.raw(), 0x5053_4F43);
    let ansi = log_color_space_bytes(WMF_LOG_COLOR_SPACE_SIZE, b"sRGB.icc\0");
    let mut reader = Reader::new(Cursor::new(ansi.as_slice()));
    let value = read_wmf_log_color_space(&mut reader).unwrap();
    assert_eq!(value.size, WMF_LOG_COLOR_SPACE_SIZE as u32);
    let mut out = Writer::new(Cursor::new(Vec::new()));
    write_wmf_log_color_space(&value, &mut out).unwrap();
    assert_eq!(out.into_inner().into_inner(), ansi);

    let unicode = log_color_space_bytes(WMF_LOG_COLOR_SPACE_W_SIZE, &[b'w', 0, 0, 0]);
    let mut reader = Reader::new(Cursor::new(unicode.as_slice()));
    let value = read_wmf_log_color_space_w(&mut reader).unwrap();
    assert_eq!(value.size, WMF_LOG_COLOR_SPACE_W_SIZE as u32);
    let mut out = Writer::new(Cursor::new(Vec::new()));
    write_wmf_log_color_space_w(&value, &mut out).unwrap();
    assert_eq!(out.into_inner().into_inner(), unicode);
  }

  fn assert_typed_roundtrip(record: WmfRecord) {
    let parsed = record.parse_data().unwrap();
    assert_eq!(parsed.to_record().unwrap(), record);
  }

  #[test]
  fn typed_wmf_no_data_records_roundtrip() {
    assert_eq!(
      WmfRecord::new(META_EOF, Vec::new()).parse_data().unwrap(),
      WmfRecordData::Eof(WmfEofRecord::default())
    );
    let eof_with_trailing_data = WmfRecord::new(META_EOF, vec![0xAA, 0xBB]);
    let parsed = eof_with_trailing_data.parse_data().unwrap();
    assert_eq!(parsed.to_record().unwrap(), eof_with_trailing_data);
    assert!(parsed.validate_strict().is_err());
    assert_typed_roundtrip(WmfRecord::new(WmfRecordFunction::SaveDc.raw(), Vec::new()));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::RealizePalette.raw(),
      Vec::new(),
    ));
    let set_relabs = WmfRecord::new(WmfRecordFunction::SetRelabs.raw(), Vec::new());
    let parsed = set_relabs.parse_data().unwrap();
    assert_eq!(parsed, WmfRecordData::SetRelabs);
    assert_eq!(parsed.to_record().unwrap(), set_relabs);
  }

  #[test]
  fn typed_wmf_state_records_roundtrip() {
    let set_bk_mode = WmfRecord::new(
      WmfRecordFunction::SetBkMode.raw(),
      vec![0x02, 0x00, 0xAA, 0xBB],
    );
    let parsed = set_bk_mode.parse_data().unwrap();
    let WmfRecordData::SetBkMode(value) = &parsed else {
      panic!("expected META_SETBKMODE");
    };
    assert_eq!(value.mix_mode_kind(), Some(WmfMixMode::Opaque));
    assert_eq!(parsed.to_record().unwrap(), set_bk_mode);
    let set_map_mode = WmfRecord::new(
      WmfRecordFunction::SetMapMode.raw(),
      WmfMapMode::HiMetric.raw().to_le_bytes().to_vec(),
    );
    let parsed = set_map_mode.parse_data().unwrap();
    let WmfRecordData::SetMapMode(value) = &parsed else {
      panic!("expected META_SETMAPMODE");
    };
    assert_eq!(value.map_mode_kind(), Some(WmfMapMode::HiMetric));
    assert_eq!(parsed.to_record().unwrap(), set_map_mode);
    let set_rop2 = WmfRecord::new(
      WmfRecordFunction::SetRop2.raw(),
      WmfBinaryRasterOperation::CopyPen
        .raw()
        .to_le_bytes()
        .to_vec(),
    );
    let parsed = set_rop2.parse_data().unwrap();
    let WmfRecordData::SetRop2(value) = &parsed else {
      panic!("expected META_SETROP2");
    };
    assert_eq!(
      value.binary_raster_operation_kind(),
      Some(WmfBinaryRasterOperation::CopyPen)
    );
    assert_eq!(parsed.to_record().unwrap(), set_rop2);
    let set_poly_fill_mode = WmfRecord::new(
      WmfRecordFunction::SetPolyFillMode.raw(),
      WmfPolyFillMode::Winding.raw().to_le_bytes().to_vec(),
    );
    let parsed = set_poly_fill_mode.parse_data().unwrap();
    let WmfRecordData::SetPolyFillMode(value) = &parsed else {
      panic!("expected META_SETPOLYFILLMODE");
    };
    assert_eq!(value.poly_fill_mode_kind(), Some(WmfPolyFillMode::Winding));
    assert_eq!(parsed.to_record().unwrap(), set_poly_fill_mode);
    let set_stretch_blt_mode = WmfRecord::new(
      WmfRecordFunction::SetStretchBltMode.raw(),
      WmfStretchMode::Halftone.raw().to_le_bytes().to_vec(),
    );
    let parsed = set_stretch_blt_mode.parse_data().unwrap();
    let WmfRecordData::SetStretchBltMode(value) = &parsed else {
      panic!("expected META_SETSTRETCHBLTMODE");
    };
    assert_eq!(value.stretch_mode_kind(), Some(WmfStretchMode::Halftone));
    assert_eq!(parsed.to_record().unwrap(), set_stretch_blt_mode);
    let set_layout = WmfRecord::new(
      WmfRecordFunction::SetLayout.raw(),
      [
        (WmfLayoutFlags::RTL | WmfLayoutFlags::BITMAP_ORIENTATION_PRESERVED)
          .bits()
          .to_le_bytes(),
        0xBEEFu16.to_le_bytes(),
      ]
      .concat(),
    );
    let parsed = set_layout.parse_data().unwrap();
    let WmfRecordData::SetLayout(value) = &parsed else {
      panic!("expected META_SETLAYOUT");
    };
    assert!(value.layout_flags().contains(WmfLayoutFlags::RTL));
    assert!(
      value
        .layout_flags()
        .contains(WmfLayoutFlags::BITMAP_ORIENTATION_PRESERVED)
    );
    assert_eq!(value.invalid_layout_bits(), 0);
    assert_eq!(parsed.to_record().unwrap(), set_layout);

    assert!(
      WmfRecordData::SetBkMode(WmfU16Record {
        value: 0xFFFF,
        reserved: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::SetBkMode.raw(),
        0xFFFF_u16.to_le_bytes().to_vec(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::SetMapMode(WmfU16Record {
        value: 0,
        reserved: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::SetMapMode.raw(),
        0_u16.to_le_bytes().to_vec(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::SetRop2(WmfU16Record {
        value: 0,
        reserved: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::SetRop2.raw(),
        0_u16.to_le_bytes().to_vec(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::SetPolyFillMode(WmfU16Record {
        value: 3,
        reserved: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::SetPolyFillMode.raw(),
        3_u16.to_le_bytes().to_vec(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::SetStretchBltMode(WmfU16Record {
        value: 5,
        reserved: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::SetStretchBltMode.raw(),
        5_u16.to_le_bytes().to_vec(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::SetLayout(WmfU16Record {
        value: 0x0002,
        reserved: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::SetLayout.raw(),
        0x0002_u16.to_le_bytes().to_vec(),
      )
      .parse_data()
      .is_err()
    );
    let set_text_align = WmfRecord::new(
      WmfRecordFunction::SetTextAlign.raw(),
      (WmfTextAlignmentModeFlags::UPDATE_CP
        | WmfTextAlignmentModeFlags::BASELINE
        | WmfTextAlignmentModeFlags::RTL_READING)
        .bits()
        .to_le_bytes()
        .to_vec(),
    );
    let parsed = set_text_align.parse_data().unwrap();
    let WmfRecordData::SetTextAlign(value) = &parsed else {
      panic!("expected META_SETTEXTALIGN");
    };
    assert!(
      value
        .text_alignment_flags()
        .contains(WmfTextAlignmentModeFlags::UPDATE_CP)
    );
    assert!(
      value
        .text_alignment_flags()
        .contains(WmfTextAlignmentModeFlags::BASELINE)
    );
    assert!(
      value
        .text_alignment_flags()
        .contains(WmfTextAlignmentModeFlags::RTL_READING)
    );
    assert!(
      value
        .vertical_text_alignment_flags()
        .contains(WmfVerticalTextAlignmentModeFlags::BASELINE)
    );
    assert_eq!(parsed.to_record().unwrap(), set_text_align);
    for invalid_value in [0x0200_u16, 0x0004, 0x0010] {
      let record = WmfRecord::new(
        WmfRecordFunction::SetTextAlign.raw(),
        invalid_value.to_le_bytes().to_vec(),
      );
      let parsed = record.parse_data().unwrap();
      assert_eq!(parsed.to_record().unwrap(), record);
      assert!(parsed.validate_strict().is_err());
    }
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::RestoreDc.raw(),
      (-2i16).to_le_bytes().to_vec(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::SetWindowOrg.raw(),
      [10i16.to_le_bytes(), (-3i16).to_le_bytes()].concat(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::ScaleWindowExt.raw(),
      [
        2i16.to_le_bytes(),
        3i16.to_le_bytes(),
        4i16.to_le_bytes(),
        5i16.to_le_bytes(),
      ]
      .concat(),
    ));
  }

  #[test]
  fn typed_wmf_fixed_drawing_records_roundtrip() {
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::Ellipse.raw(),
      [
        40i16.to_le_bytes(),
        30i16.to_le_bytes(),
        20i16.to_le_bytes(),
        10i16.to_le_bytes(),
      ]
      .concat(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::Arc.raw(),
      [
        8i16.to_le_bytes(),
        7i16.to_le_bytes(),
        6i16.to_le_bytes(),
        5i16.to_le_bytes(),
        4i16.to_le_bytes(),
        3i16.to_le_bytes(),
        2i16.to_le_bytes(),
        1i16.to_le_bytes(),
      ]
      .concat(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::SetPixel.raw(),
      vec![1, 2, 3, 0, 9, 0, 8, 0],
    ));
    let ext_flood_fill = WmfRecord::new(
      WmfRecordFunction::ExtFloodFill.raw(),
      [
        WmfFloodFillMode::Surface.raw().to_le_bytes().as_slice(),
        &[1, 2, 3, 0],
        9i16.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = ext_flood_fill.parse_data().unwrap();
    let WmfRecordData::ExtFloodFill(value) = &parsed else {
      panic!("expected META_EXTFLOODFILL");
    };
    assert_eq!(value.mode_kind(), Some(WmfFloodFillMode::Surface));
    assert_eq!(parsed.to_record().unwrap(), ext_flood_fill);
    assert!(
      WmfRecordData::ExtFloodFill(WmfExtFloodFillRecord {
        mode: 2,
        color: ColorRef {
          red: 1,
          green: 2,
          blue: 3,
          reserved: 0,
        },
        y: 9,
        x: 8,
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::ExtFloodFill.raw(),
        [
          2_u16.to_le_bytes().as_slice(),
          &[1, 2, 3, 0],
          9_i16.to_le_bytes().as_slice(),
          8_i16.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
  }

  #[test]
  fn typed_wmf_polygon_text_and_escape_records_roundtrip() {
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::Polygon.raw(),
      [
        2i16.to_le_bytes(),
        1i16.to_le_bytes(),
        2i16.to_le_bytes(),
        3i16.to_le_bytes(),
        4i16.to_le_bytes(),
      ]
      .concat(),
    ));
    let one_point_polygon = WmfRecord::new(
      WmfRecordFunction::Polygon.raw(),
      [1i16.to_le_bytes(), 1i16.to_le_bytes(), 2i16.to_le_bytes()].concat(),
    );
    let parsed = one_point_polygon.parse_data().unwrap();
    assert_eq!(parsed.to_record().unwrap(), one_point_polygon);
    assert!(parsed.validate_strict().is_err());
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Polyline.raw(),
        30_000i16.to_le_bytes().to_vec()
      )
      .parse_data()
      .is_err()
    );
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::PolyPolygon.raw(),
      [
        2u16.to_le_bytes().as_slice(),
        3u16.to_le_bytes().as_slice(),
        2u16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        7i16.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
        9i16.to_le_bytes().as_slice(),
        10i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    ));
    assert!(
      WmfRecord::new(
        WmfRecordFunction::PolyPolygon.raw(),
        [
          2u16.to_le_bytes().as_slice(),
          u16::MAX.to_le_bytes().as_slice(),
          u16::MAX.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::TextOut.raw(),
      [
        3i16.to_le_bytes().as_slice(),
        b"abc",
        &[0],
        9i16.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::ExtTextOut.raw(),
      [
        9i16.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        (WmfExtTextOutOptions::OPAQUE | WmfExtTextOutOptions::CLIPPED)
          .bits()
          .to_le_bytes()
          .as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        b"abc",
        &[0],
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        7i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::ExtTextOut.raw(),
      [
        9i16.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        WmfExtTextOutOptions::PDY.bits().to_le_bytes().as_slice(),
        b"ab",
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        7i16.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    ));
    assert!(
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 9,
        x: 8,
        string_length: 3,
        options: WmfExtTextOutOptions::empty(),
        rectangle: None,
        string: b"abc".to_vec(),
        string_padding: vec![0],
        dx: vec![5],
        trailing_data: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 9,
        x: 8,
        string_length: 3,
        options: WmfExtTextOutOptions::empty(),
        rectangle: None,
        string: b"abc".to_vec(),
        string_padding: vec![0],
        dx: Vec::new(),
        trailing_data: vec![0],
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::ExtTextOut.raw(),
        [
          9i16.to_le_bytes().as_slice(),
          8i16.to_le_bytes().as_slice(),
          3i16.to_le_bytes().as_slice(),
          0u16.to_le_bytes().as_slice(),
          b"abc",
          &[0],
          5i16.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::ExtTextOut.raw(),
        [
          9i16.to_le_bytes().as_slice(),
          8i16.to_le_bytes().as_slice(),
          3i16.to_le_bytes().as_slice(),
          WmfExtTextOutOptions::OPAQUE.bits().to_le_bytes().as_slice(),
          b"abc",
          &[0],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 9,
        x: 8,
        string_length: 3,
        options: WmfExtTextOutOptions::CLIPPED,
        rectangle: None,
        string: b"abc".to_vec(),
        string_padding: vec![0],
        dx: Vec::new(),
        trailing_data: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 9,
        x: 8,
        string_length: 2,
        options: WmfExtTextOutOptions::PDY,
        rectangle: None,
        string: b"ab".to_vec(),
        string_padding: Vec::new(),
        dx: vec![5, 6],
        trailing_data: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 9,
        x: 8,
        string_length: 3,
        options: WmfExtTextOutOptions::from_bits_retain(0x8000),
        rectangle: None,
        string: b"abc".to_vec(),
        string_padding: vec![0],
        dx: Vec::new(),
        trailing_data: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::ExtTextOut.raw(),
        [
          9i16.to_le_bytes().as_slice(),
          8i16.to_le_bytes().as_slice(),
          3i16.to_le_bytes().as_slice(),
          0x8000_u16.to_le_bytes().as_slice(),
          b"abc",
          &[0],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 9,
        x: 8,
        string_length: 2,
        options: WmfExtTextOutOptions::empty(),
        rectangle: None,
        string: b"abc".to_vec(),
        string_padding: vec![0],
        dx: Vec::new(),
        trailing_data: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::MetaFile.raw().to_le_bytes().as_slice(),
        3u16.to_le_bytes().as_slice(),
        &[1, 2, 3, 0],
      ]
      .concat(),
    ));
    let escape = WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::PostScriptData
          .raw()
          .to_le_bytes()
          .as_slice(),
        2u16.to_le_bytes().as_slice(),
        &[0xAA, 0xBB],
      ]
      .concat(),
    );
    let parsed = escape.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(value.escape_kind(), Some(WmfMetafileEscape::PostScriptData));
    assert_eq!(parsed.to_record().unwrap(), escape);

    let line_cap = WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::SetLineCap.raw().to_le_bytes().as_slice(),
        4u16.to_le_bytes().as_slice(),
        WmfPostScriptCap::Round.raw().to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = line_cap.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(value.post_script_cap_kind(), Some(WmfPostScriptCap::Round));
    let typed = value.typed_data().unwrap();
    assert_eq!(typed.post_script_cap_kind(), Some(WmfPostScriptCap::Round));
    assert_eq!(
      typed,
      WmfEscapeData::SetLineCap {
        cap: WmfPostScriptCap::Round.raw()
      }
    );
    assert_eq!(parsed.to_record().unwrap(), line_cap);
    assert_eq!(
      WmfRecordData::Escape(
        WmfEscapeRecord::from_typed_data(
          WmfEscapeData::SetLineCap {
            cap: WmfPostScriptCap::Round.raw()
          },
          Vec::new(),
        )
        .unwrap()
      )
      .to_record()
      .unwrap(),
      line_cap
    );

    let line_join = WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::SetLineJoin
          .raw()
          .to_le_bytes()
          .as_slice(),
        4u16.to_le_bytes().as_slice(),
        WmfPostScriptJoin::Bevel.raw().to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = line_join.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.post_script_join_kind(),
      Some(WmfPostScriptJoin::Bevel)
    );
    let typed = value.typed_data().unwrap();
    assert_eq!(
      typed.post_script_join_kind(),
      Some(WmfPostScriptJoin::Bevel)
    );
    assert_eq!(
      typed,
      WmfEscapeData::SetLineJoin {
        join: WmfPostScriptJoin::Bevel.raw()
      }
    );
    assert_eq!(parsed.to_record().unwrap(), line_join);

    let clip = WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::ClipToPath.raw().to_le_bytes().as_slice(),
        4u16.to_le_bytes().as_slice(),
        WmfPostScriptClipping::Inclusive
          .raw()
          .to_le_bytes()
          .as_slice(),
        0u16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = clip.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.post_script_clipping_kind(),
      Some(WmfPostScriptClipping::Inclusive)
    );
    let typed = value.typed_data().unwrap();
    assert_eq!(
      typed.post_script_clipping_kind(),
      Some(WmfPostScriptClipping::Inclusive)
    );
    assert_eq!(
      typed,
      WmfEscapeData::ClipToPath {
        clip_function: WmfPostScriptClipping::Inclusive.raw(),
        reserved: 0,
      }
    );
    assert_eq!(parsed.to_record().unwrap(), clip);

    let feature = WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::GetPsFeatureSetting
          .raw()
          .to_le_bytes()
          .as_slice(),
        4u16.to_le_bytes().as_slice(),
        WmfPostScriptFeatureSetting::Protocol
          .raw()
          .to_le_bytes()
          .as_slice(),
      ]
      .concat(),
    );
    let parsed = feature.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.post_script_feature_setting_kind(),
      Some(WmfPostScriptFeatureSetting::Protocol)
    );
    let typed = value.typed_data().unwrap();
    assert_eq!(
      typed.post_script_feature_setting_kind(),
      Some(WmfPostScriptFeatureSetting::Protocol)
    );
    assert!(typed.is_valid_post_script_feature_setting());
    assert_eq!(
      typed,
      WmfEscapeData::GetPsFeatureSetting {
        feature_setting: WmfPostScriptFeatureSetting::Protocol.raw()
      }
    );
    assert_eq!(parsed.to_record().unwrap(), feature);

    let private_feature = WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::GetPsFeatureSetting
          .raw()
          .to_le_bytes()
          .as_slice(),
        4u16.to_le_bytes().as_slice(),
        0x1001_i32.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = private_feature.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    let typed = value.typed_data().unwrap();
    assert_eq!(typed.post_script_feature_setting_kind(), None);
    assert!(typed.is_valid_post_script_feature_setting());
    assert_eq!(
      typed,
      WmfEscapeData::GetPsFeatureSetting {
        feature_setting: 0x1001
      }
    );
    assert_eq!(parsed.to_record().unwrap(), private_feature);

    let query = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::QueryEscSupport {
          query: WmfMetafileEscape::SetLineCap.raw(),
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = query.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    let typed = value.typed_data().unwrap();
    assert_eq!(
      typed.query_escape_kind(),
      Some(WmfMetafileEscape::SetLineCap)
    );
    assert_eq!(
      typed,
      WmfEscapeData::QueryEscSupport {
        query: WmfMetafileEscape::SetLineCap.raw()
      }
    );
    assert_eq!(parsed, query);

    let no_data = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::NoData {
          escape: WmfMetafileEscape::BeginPath,
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = no_data.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::NoData {
        escape: WmfMetafileEscape::BeginPath
      }
    );

    let spcl = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::SpclPassThrough2 {
          reserved: 0x1122_3344,
          raw_data: &[0xAA, 0xBB, 0xCC],
          trailing_data: &[0],
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = spcl.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::SpclPassThrough2 {
        reserved: 0x1122_3344,
        raw_data: &[0xAA, 0xBB, 0xCC],
        trailing_data: &[0],
      }
    );

    let enhanced = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::EnhancedMetafile {
          comment_identifier: WMF_EMF_COMMENT_IDENTIFIER,
          comment_type: WMF_EMF_COMMENT_TYPE,
          version: WMF_EMF_INTEROP_VERSION,
          checksum: 0x1234,
          flags: 0,
          comment_record_count: 1,
          current_record_size: 3,
          remaining_bytes: 0,
          enhanced_metafile_data_size: 3,
          enhanced_metafile_data: &[0xAA, 0xBB, 0xCC],
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = enhanced.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::EnhancedMetafile {
        comment_identifier: WMF_EMF_COMMENT_IDENTIFIER,
        comment_type: WMF_EMF_COMMENT_TYPE,
        version: WMF_EMF_INTEROP_VERSION,
        checksum: 0x1234,
        flags: 0,
        comment_record_count: 1,
        current_record_size: 3,
        remaining_bytes: 0,
        enhanced_metafile_data_size: 3,
        enhanced_metafile_data: &[0xAA, 0xBB, 0xCC],
      }
    );
    assert_eq!(parsed, enhanced);

    let embedded = [1, 0, 2, 0, 3, 0];
    let checksum = compute_enhanced_metafile_checksum(&embedded).unwrap();
    let first_chunk = WmfEscapeRecord::from_typed_data(
      WmfEscapeData::EnhancedMetafile {
        comment_identifier: WMF_EMF_COMMENT_IDENTIFIER,
        comment_type: WMF_EMF_COMMENT_TYPE,
        version: WMF_EMF_INTEROP_VERSION,
        checksum,
        flags: 0,
        comment_record_count: 2,
        current_record_size: 2,
        remaining_bytes: 4,
        enhanced_metafile_data_size: 6,
        enhanced_metafile_data: &embedded[..2],
      },
      Vec::new(),
    )
    .unwrap();
    let second_chunk = WmfEscapeRecord::from_typed_data(
      WmfEscapeData::EnhancedMetafile {
        comment_identifier: WMF_EMF_COMMENT_IDENTIFIER,
        comment_type: WMF_EMF_COMMENT_TYPE,
        version: WMF_EMF_INTEROP_VERSION,
        checksum,
        flags: 0,
        comment_record_count: 2,
        current_record_size: 4,
        remaining_bytes: 0,
        enhanced_metafile_data_size: 6,
        enhanced_metafile_data: &embedded[2..],
      },
      Vec::new(),
    )
    .unwrap();
    let mut assembler = WmfEnhancedMetafileAssembler::new();
    assert_eq!(assembler.push(&first_chunk).unwrap(), None);
    assert!(assembler.finish().is_err());
    let assembled = assembler.push(&second_chunk).unwrap().unwrap();
    assert_eq!(assembled.data, embedded);
    assert_eq!(assembled.computed_checksum().unwrap(), checksum);
    assert!(assembler.finish().is_ok());

    let mut invalid_checksum = first_chunk.clone();
    invalid_checksum.escape_data[12..14].copy_from_slice(&0_u16.to_le_bytes());
    let mut assembler = WmfEnhancedMetafileAssembler::new();
    assembler.push(&invalid_checksum).unwrap();
    let mut invalid_checksum = second_chunk.clone();
    invalid_checksum.escape_data[12..14].copy_from_slice(&0_u16.to_le_bytes());
    assert!(assembler.push(&invalid_checksum).is_err());

    let enhanced_non_wmfc = WmfRecord::new(
      WmfRecordFunction::Escape.raw(),
      [
        WmfMetafileEscape::MetaFile.raw().to_le_bytes().as_slice(),
        4_u16.to_le_bytes().as_slice(),
        0x1122_3344_u32.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = enhanced_non_wmfc.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::Raw {
        escape: WmfMetafileEscape::MetaFile,
        data: &[0x44, 0x33, 0x22, 0x11],
      }
    );

    let injection = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::PostScriptInjection {
          data_size: 3,
          injection_point: 2,
          page_number: 7,
          raw_data: &[0x10, 0x20, 0x30],
          trailing_data: &[0],
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = injection.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::PostScriptInjection {
        data_size: 3,
        injection_point: 2,
        page_number: 7,
        raw_data: &[0x10, 0x20, 0x30],
        trailing_data: &[0],
      }
    );
    assert_eq!(parsed, injection);

    let start_doc = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::StartDoc {
          doc_name: b"report.ps",
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = start_doc.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::StartDoc {
        doc_name: b"report.ps",
      }
    );
    assert_eq!(parsed, start_doc);

    let set_color_table = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::SetColorTable {
          color_table: &[0x01, 0x02, 0x03],
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = set_color_table.to_record().unwrap();
    record
      .write_to(&mut Writer::new(Cursor::new(Vec::new())))
      .unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::SetColorTable {
        color_table: &[0x01, 0x02, 0x03],
      }
    );
    assert_eq!(value.padding, [0]);
    assert_eq!(parsed, set_color_table);

    let get_color_table = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::GetColorTable {
          start: 4,
          undefined_space: &[0xAA, 0xBB],
          color_table: &[0x10, 0x20],
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = get_color_table.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::GetColorTable {
        start: 4,
        undefined_space: &[0xAA, 0xBB],
        color_table: &[0x10, 0x20],
      }
    );
    assert_eq!(parsed, get_color_table);

    let draw_pattern = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::DrawPatternRect {
          position: PointL { x: 1, y: 2 },
          size: PointL { x: 3, y: 4 },
          style: 5,
          pattern: 6,
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = draw_pattern.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::DrawPatternRect {
        position: PointL { x: 1, y: 2 },
        size: PointL { x: 3, y: 4 },
        style: 5,
        pattern: 6,
      }
    );
    assert_eq!(parsed, draw_pattern);

    let encapsulated_postscript = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::EncapsulatedPostScript {
          size: 35,
          version: 3,
          points: [
            PointL { x: 16, y: 32 },
            PointL { x: 48, y: 64 },
            PointL { x: 80, y: 96 },
          ],
          data: b"ps!",
          trailing_data: &[0xEE],
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = encapsulated_postscript.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::EncapsulatedPostScript {
        size: 35,
        version: 3,
        points: [
          PointL { x: 16, y: 32 },
          PointL { x: 48, y: 64 },
          PointL { x: 80, y: 96 },
        ],
        data: b"ps!",
        trailing_data: &[0xEE],
      }
    );
    assert_eq!(parsed, encapsulated_postscript);

    let eps_printing = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::EpsPrinting {
          set_eps_printing: 1,
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = eps_printing.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::EpsPrinting {
        set_eps_printing: 1,
      }
    );
    assert_eq!(parsed, eps_printing);

    let check_png = WmfRecordData::Escape(
      WmfEscapeRecord::from_typed_data(
        WmfEscapeData::BinaryData {
          escape: WmfMetafileEscape::CheckPngFormat,
          data: &[0x89, b'P', b'N', b'G'],
        },
        Vec::new(),
      )
      .unwrap(),
    );
    let record = check_png.to_record().unwrap();
    let parsed = record.parse_data().unwrap();
    let WmfRecordData::Escape(value) = &parsed else {
      panic!("expected META_ESCAPE");
    };
    assert_eq!(
      value.typed_data().unwrap(),
      WmfEscapeData::BinaryData {
        escape: WmfMetafileEscape::CheckPngFormat,
        data: &[0x89, b'P', b'N', b'G'],
      }
    );
    assert_eq!(parsed, check_png);

    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          0xFFFF_u16.to_le_bytes().as_slice(),
          0_u16.to_le_bytes().as_slice()
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::Escape(WmfEscapeRecord {
        escape_function: WmfMetafileEscape::StartDoc.raw(),
        escape_data: vec![0; 260],
        padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::GetColorTable
            .raw()
            .to_le_bytes()
            .as_slice(),
          4_u16.to_le_bytes().as_slice(),
          1_u16.to_le_bytes().as_slice(),
          &[0x10, 0x20],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::EpsPrinting
            .raw()
            .to_le_bytes()
            .as_slice(),
          4_u16.to_le_bytes().as_slice(),
          1_u16.to_le_bytes().as_slice(),
          0_u16.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::PostScriptIgnore
            .raw()
            .to_le_bytes()
            .as_slice(),
          1_u16.to_le_bytes().as_slice(),
          &[0],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::DrawPatternRect
            .raw()
            .to_le_bytes()
            .as_slice(),
          18_u16.to_le_bytes().as_slice(),
          &[0; 18],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::EncapsulatedPostScript
            .raw()
            .to_le_bytes()
            .as_slice(),
          32_u16.to_le_bytes().as_slice(),
          31_u32.to_le_bytes().as_slice(),
          &[0; 28],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::EncapsulatedPostScript
            .raw()
            .to_le_bytes()
            .as_slice(),
          32_u16.to_le_bytes().as_slice(),
          33_u32.to_le_bytes().as_slice(),
          &[0; 28],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::SetLineCap.raw().to_le_bytes().as_slice(),
          2_u16.to_le_bytes().as_slice(),
          0_u16.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::SetLineJoin
            .raw()
            .to_le_bytes()
            .as_slice(),
          4_u16.to_le_bytes().as_slice(),
          0x7FFF_FFFF_i32.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::QueryEscSupport
            .raw()
            .to_le_bytes()
            .as_slice(),
          2_u16.to_le_bytes().as_slice(),
          0xFFFF_u16.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::SpclPassThrough2
            .raw()
            .to_le_bytes()
            .as_slice(),
          8_u16.to_le_bytes().as_slice(),
          0_u32.to_le_bytes().as_slice(),
          4_u16.to_le_bytes().as_slice(),
          &[1, 2],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::MetaFile.raw().to_le_bytes().as_slice(),
          34_u16.to_le_bytes().as_slice(),
          WMF_EMF_COMMENT_IDENTIFIER.to_le_bytes().as_slice(),
          0_u32.to_le_bytes().as_slice(),
          WMF_EMF_INTEROP_VERSION.to_le_bytes().as_slice(),
          0_u16.to_le_bytes().as_slice(),
          0_u32.to_le_bytes().as_slice(),
          1_u32.to_le_bytes().as_slice(),
          0_u32.to_le_bytes().as_slice(),
          0_u32.to_le_bytes().as_slice(),
          0_u32.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::PostScriptInjection
            .raw()
            .to_le_bytes()
            .as_slice(),
          10_u16.to_le_bytes().as_slice(),
          4_u32.to_le_bytes().as_slice(),
          2_u16.to_le_bytes().as_slice(),
          7_u16.to_le_bytes().as_slice(),
          &[0x10, 0x20],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::Escape(WmfEscapeRecord {
        escape_function: WmfMetafileEscape::AbortDoc.raw(),
        escape_data: vec![0],
        padding: Vec::new(),
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecordData::Escape(WmfEscapeRecord {
        escape_function: WmfMetafileEscape::PostScriptData.raw(),
        escape_data: vec![0xAA, 0xBB],
        padding: vec![0],
      })
      .to_record()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::Escape.raw(),
        [
          WmfMetafileEscape::PostScriptData
            .raw()
            .to_le_bytes()
            .as_slice(),
          3_u16.to_le_bytes().as_slice(),
          &[0xAA, 0xBB, 0xCC],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
  }

  #[test]
  fn typed_wmf_object_records_roundtrip() {
    let create_brush = WmfRecord::new(
      WmfRecordFunction::CreateBrushIndirect.raw(),
      [
        WmfBrushStyle::Hatched.raw().to_le_bytes().as_slice(),
        &[0x10, 0x20, 0x30, 0x00],
        WmfHatchStyle::ForwardDiagonal
          .raw()
          .to_le_bytes()
          .as_slice(),
      ]
      .concat(),
    );
    let parsed = create_brush.parse_data().unwrap();
    let WmfRecordData::CreateBrushIndirect(value) = &parsed else {
      panic!("expected META_CREATEBRUSHINDIRECT");
    };
    assert_eq!(value.brush_style_kind(), Some(WmfBrushStyle::Hatched));
    assert_eq!(
      value.hatch_style_kind(),
      Some(WmfHatchStyle::ForwardDiagonal)
    );
    assert_eq!(parsed.to_record().unwrap(), create_brush);
    for invalid_brush in [
      WmfRecord::new(
        WmfRecordFunction::CreateBrushIndirect.raw(),
        [
          0xFFFF_u16.to_le_bytes().as_slice(),
          &[0x10, 0x20, 0x30, 0x00],
          WmfHatchStyle::ForwardDiagonal
            .raw()
            .to_le_bytes()
            .as_slice(),
        ]
        .concat(),
      ),
      WmfRecord::new(
        WmfRecordFunction::CreateBrushIndirect.raw(),
        [
          WmfBrushStyle::Hatched.raw().to_le_bytes().as_slice(),
          &[0x10, 0x20, 0x30, 0x00],
          0xFFFF_u16.to_le_bytes().as_slice(),
        ]
        .concat(),
      ),
    ] {
      let parsed = invalid_brush.parse_data().unwrap();
      assert_eq!(parsed.to_record().unwrap(), invalid_brush);
      assert!(parsed.validate_strict().is_err());
    }
    let create_pen = WmfRecord::new(
      WmfRecordFunction::CreatePenIndirect.raw(),
      [
        (WmfPenStyleFlags::DASH | WmfPenStyleFlags::END_CAP_SQUARE | WmfPenStyleFlags::JOIN_MITER)
          .bits()
          .to_le_bytes()
          .as_slice(),
        7i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        &[0x01, 0x02, 0x03, 0x00],
      ]
      .concat(),
    );
    let parsed = create_pen.parse_data().unwrap();
    let WmfRecordData::CreatePenIndirect(value) = &parsed else {
      panic!("expected META_CREATEPENINDIRECT");
    };
    assert!(value.pen.pen_style_flags().contains(WmfPenStyleFlags::DASH));
    assert!(
      value
        .pen
        .pen_style_flags()
        .contains(WmfPenStyleFlags::END_CAP_SQUARE)
    );
    assert!(
      value
        .pen
        .pen_style_flags()
        .contains(WmfPenStyleFlags::JOIN_MITER)
    );
    assert_eq!(value.pen.pen_line_style_kind(), Some(WmfPenLineStyle::Dash));
    assert_eq!(value.pen.pen_end_cap_kind(), Some(WmfPenEndCap::Square));
    assert_eq!(value.pen.pen_join_kind(), Some(WmfPenJoin::Miter));
    assert_eq!(value.pen.pen_type_kind(), Some(WmfPenType::Cosmetic));
    assert_eq!(value.pen.pen_reserved_bits(), 0);
    assert!(value.trailing_data.is_empty());
    assert_eq!(parsed.to_record().unwrap(), create_pen);
    let mut create_pen_with_trailing_data = create_pen.clone();
    create_pen_with_trailing_data
      .data
      .extend_from_slice(&[0xAA, 0xBB]);
    let parsed = create_pen_with_trailing_data.parse_data().unwrap();
    let WmfRecordData::CreatePenIndirect(value) = &parsed else {
      panic!("expected META_CREATEPENINDIRECT");
    };
    assert_eq!(value.trailing_data, [0xAA, 0xBB]);
    assert_eq!(parsed.to_record().unwrap(), create_pen_with_trailing_data);
    assert!(parsed.validate_strict().is_err());
    let mut invalid_line_style = create_pen.clone();
    invalid_line_style.data[0..2].copy_from_slice(&0x000F_u16.to_le_bytes());
    assert!(invalid_line_style.parse_data().is_err());
    let mut invalid_end_cap = create_pen.clone();
    invalid_end_cap.data[0..2].copy_from_slice(&0x0300_u16.to_le_bytes());
    assert!(invalid_end_cap.parse_data().is_err());
    let mut invalid_join = create_pen.clone();
    invalid_join.data[0..2].copy_from_slice(&0x3000_u16.to_le_bytes());
    assert!(invalid_join.parse_data().is_err());
    let mut invalid_reserved_bits = create_pen.clone();
    invalid_reserved_bits.data[0..2].copy_from_slice(&0x0010_u16.to_le_bytes());
    assert!(invalid_reserved_bits.parse_data().is_err());
    let mut invalid_pen = value.clone();
    invalid_pen.pen.pen_style = 0x0010;
    assert!(
      WmfRecordData::CreatePenIndirect(invalid_pen)
        .to_record()
        .is_err()
    );
    let create_palette = WmfRecord::new(
      WmfRecordFunction::CreatePalette.raw(),
      [
        0x0300u16.to_le_bytes().as_slice(),
        2u16.to_le_bytes().as_slice(),
        &[0x10, 0x20, 0x30, 0x00],
        &[0x40, 0x50, 0x60, WmfPaletteEntryFlags::RESERVED.bits()],
      ]
      .concat(),
    );
    let parsed = create_palette.parse_data().unwrap();
    let WmfRecordData::CreatePalette(value) = &parsed else {
      panic!("expected META_CREATEPALETTE");
    };
    assert!(
      value.entries[1]
        .flags()
        .contains(WmfPaletteEntryFlags::RESERVED)
    );
    assert_eq!(
      value.entries[1].flag_kind(),
      Some(WmfPaletteEntryFlags::RESERVED)
    );
    assert_eq!(value.entries[1].invalid_value_bits(), 0);
    assert_eq!(parsed.to_record().unwrap(), create_palette);
    let mut invalid_palette_start = create_palette.clone();
    invalid_palette_start.data[0..2].copy_from_slice(&0x0000_u16.to_le_bytes());
    assert!(invalid_palette_start.parse_data().is_err());
    let mut invalid_palette_combination = create_palette.clone();
    invalid_palette_combination.data[11] =
      (WmfPaletteEntryFlags::RESERVED | WmfPaletteEntryFlags::EXPLICIT).bits();
    assert!(invalid_palette_combination.parse_data().is_err());
    let mut invalid_palette_unknown = create_palette.clone();
    invalid_palette_unknown.data[11] = 0x08;
    assert!(invalid_palette_unknown.parse_data().is_err());
    assert!(
      WmfRecord::new(
        WmfRecordFunction::CreatePalette.raw(),
        [
          0x0300u16.to_le_bytes().as_slice(),
          100u16.to_le_bytes().as_slice(),
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    let mut invalid_palette = value.clone();
    invalid_palette.entries[1].values =
      (WmfPaletteEntryFlags::RESERVED | WmfPaletteEntryFlags::EXPLICIT).bits();
    assert!(
      WmfRecordData::CreatePalette(invalid_palette)
        .to_record()
        .is_err()
    );
    let mut invalid_palette = value.clone();
    invalid_palette.start = 0;
    assert!(
      WmfRecordData::CreatePalette(invalid_palette)
        .to_record()
        .is_err()
    );
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::SetPalEntries.raw(),
      [
        1u16.to_le_bytes().as_slice(),
        1u16.to_le_bytes().as_slice(),
        &[0x70, 0x80, 0x90, 0x02],
      ]
      .concat(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::AnimatePalette.raw(),
      [
        2u16.to_le_bytes().as_slice(),
        1u16.to_le_bytes().as_slice(),
        &[0xAA, 0xBB, 0xCC, 0x04],
      ]
      .concat(),
    ));

    let mut face_name = [0u8; 32];
    face_name[..5].copy_from_slice(b"Arial");
    let create_font = WmfRecord::new(
      WmfRecordFunction::CreateFontIndirect.raw(),
      [
        (-12i16).to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        900i16.to_le_bytes().as_slice(),
        900i16.to_le_bytes().as_slice(),
        700i16.to_le_bytes().as_slice(),
        &[
          1,
          0,
          0,
          WmfCharacterSet::Ansi.raw(),
          WmfOutPrecision::Stroke.raw(),
          (WmfClipPrecisionFlags::STROKE | WmfClipPrecisionFlags::TT_ALWAYS).bits(),
          WmfFontQuality::Draft.raw(),
          (WmfFamilyFont::Swiss.raw() << 4) | WmfPitchFont::Variable.raw(),
        ],
        face_name.as_slice(),
      ]
      .concat(),
    );
    let parsed = create_font.parse_data().unwrap();
    let WmfRecordData::CreateFontIndirect(value) = &parsed else {
      panic!("expected META_CREATEFONTINDIRECT");
    };
    assert_eq!(value.char_set_kind(), Some(WmfCharacterSet::Ansi));
    assert_eq!(value.out_precision_kind(), Some(WmfOutPrecision::Stroke));
    assert!(
      value
        .clip_precision_flags()
        .contains(WmfClipPrecisionFlags::STROKE)
    );
    assert!(
      value
        .clip_precision_flags()
        .contains(WmfClipPrecisionFlags::TT_ALWAYS)
    );
    assert_eq!(value.invalid_clip_precision_bits(), 0);
    assert_eq!(value.quality_kind(), Some(WmfFontQuality::Draft));
    assert_eq!(value.pitch_kind(), Some(WmfPitchFont::Variable));
    assert_eq!(value.family_kind(), Some(WmfFamilyFont::Swiss));
    let pitch_and_family = value.pitch_and_family_object();
    assert_eq!(pitch_and_family.pitch_kind(), Some(WmfPitchFont::Variable));
    assert_eq!(pitch_and_family.family_kind(), Some(WmfFamilyFont::Swiss));
    assert_eq!(pitch_and_family.reserved_bits(), 0);
    assert_eq!(parsed.to_record().unwrap(), create_font);
    let mut short_face_name = create_font.clone();
    short_face_name.data.truncate(30);
    let parsed = short_face_name.parse_data().unwrap();
    let WmfRecordData::CreateFontIndirect(font) = &parsed else {
      unreachable!();
    };
    assert_eq!(font.face_name_bytes, 12);
    assert!(parsed.validate_strict().is_err());
    assert_eq!(parsed.to_record().unwrap(), short_face_name);
    let mut vendor_char_set = create_font.clone();
    vendor_char_set.data[13] = 0xFE;
    let WmfRecordData::CreateFontIndirect(vendor_font) = vendor_char_set.parse_data().unwrap()
    else {
      panic!("expected META_CREATEFONTINDIRECT");
    };
    assert_eq!(vendor_font.char_set, 0xFE);
    assert_eq!(vendor_font.char_set_kind(), None);
    assert_eq!(
      WmfRecordData::CreateFontIndirect(vendor_font)
        .to_record()
        .unwrap(),
      vendor_char_set
    );
    let assert_compatible_font = |record: &WmfRecord| {
      let parsed = record.parse_data().unwrap();
      assert!(parsed.validate_strict().is_err());
      assert_eq!(parsed.to_record().unwrap(), *record);
    };
    let mut invalid_weight = create_font.clone();
    invalid_weight.data[8..10].copy_from_slice(&1001_i16.to_le_bytes());
    assert_compatible_font(&invalid_weight);
    let mut invalid_italic = create_font.clone();
    invalid_italic.data[10] = 2;
    assert_compatible_font(&invalid_italic);
    let mut invalid_underline = create_font.clone();
    invalid_underline.data[11] = 2;
    assert_compatible_font(&invalid_underline);
    let mut invalid_strike_out = create_font.clone();
    invalid_strike_out.data[12] = 2;
    assert_compatible_font(&invalid_strike_out);
    let mut invalid_out_precision = create_font.clone();
    invalid_out_precision.data[14] = 0xFF;
    assert_compatible_font(&invalid_out_precision);
    let mut invalid_clip_precision = create_font.clone();
    invalid_clip_precision.data[15] = 0x08;
    assert_compatible_font(&invalid_clip_precision);
    let mut invalid_quality = create_font.clone();
    invalid_quality.data[16] = 0xFF;
    assert_compatible_font(&invalid_quality);
    let mut invalid_pitch = create_font.clone();
    invalid_pitch.data[17] = (WmfFamilyFont::Swiss.raw() << 4) | 0x03;
    assert_compatible_font(&invalid_pitch);
    let mut invalid_pitch_reserved = create_font.clone();
    invalid_pitch_reserved.data[17] =
      (WmfFamilyFont::Swiss.raw() << 4) | 0x04 | WmfPitchFont::Variable.raw();
    assert_compatible_font(&invalid_pitch_reserved);
    let mut invalid_family = create_font.clone();
    invalid_family.data[17] = (0x0F << 4) | WmfPitchFont::Variable.raw();
    assert_compatible_font(&invalid_family);
    let mut invalid_font = value.clone();
    invalid_font.quality = 0xFF;
    assert!(
      WmfRecordData::CreateFontIndirect(invalid_font)
        .validate_strict()
        .is_err()
    );
    let mut invalid_font = value.clone();
    invalid_font.weight = -1;
    assert!(
      WmfRecordData::CreateFontIndirect(invalid_font)
        .validate_strict()
        .is_err()
    );
    let mut invalid_font = value.clone();
    invalid_font.italic = 2;
    assert!(
      WmfRecordData::CreateFontIndirect(invalid_font)
        .validate_strict()
        .is_err()
    );
    let mut invalid_font = value.clone();
    invalid_font.clip_precision = 0x08;
    assert!(
      WmfRecordData::CreateFontIndirect(invalid_font)
        .validate_strict()
        .is_err()
    );
    let mut invalid_font = value.clone();
    invalid_font.pitch_and_family =
      (WmfFamilyFont::Swiss.raw() << 4) | 0x04 | WmfPitchFont::Variable.raw();
    assert!(
      WmfRecordData::CreateFontIndirect(invalid_font)
        .validate_strict()
        .is_err()
    );
  }

  #[test]
  fn typed_wmf_bitmap_and_region_object_records_roundtrip() {
    let core_1bpp_dib = core_1bpp_dib_bytes();
    let create_pattern = WmfRecord::new(
      WmfRecordFunction::CreatePatternBrush.raw(),
      [
        0i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        &[1, 1],
        0x1122_3344u32.to_le_bytes().as_slice(),
        &[0xEE; 18],
        &[0xAA, 0xBB, 0xCC, 0xDD],
      ]
      .concat(),
    );
    let parsed = create_pattern.parse_data().unwrap();
    let WmfRecordData::CreatePatternBrush(value) = &parsed else {
      panic!("expected META_CREATEPATTERNBRUSH");
    };
    let bitmap16 = value.bitmap16().unwrap();
    assert_eq!(bitmap16.header.width, 2);
    assert_eq!(bitmap16.header.computed_width_bytes().unwrap(), 2);
    assert_eq!(bitmap16.header.computed_bits_len().unwrap(), 4);
    assert_eq!(bitmap16.bits, [0xAA, 0xBB, 0xCC, 0xDD]);
    assert_eq!(
      WmfBitmap16::read_from_slice(&bitmap16.to_bytes().unwrap()).unwrap(),
      bitmap16
    );
    let mut truncated_bitmap16 = bitmap16.to_bytes().unwrap();
    truncated_bitmap16.pop();
    assert!(WmfBitmap16::read_from_slice(&truncated_bitmap16).is_err());
    assert_eq!(parsed.to_record().unwrap(), create_pattern);
    let mut invalid_bitmap16_header = create_pattern.data[..10].to_vec();
    invalid_bitmap16_header[8] = 2;
    assert!(
      WmfBitmap16Header::read_from(&mut Reader::new(Cursor::new(invalid_bitmap16_header))).is_err()
    );
    let mut invalid_bitmap16_header = bitmap16.header;
    invalid_bitmap16_header.planes = 2;
    assert!(
      invalid_bitmap16_header
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );
    let mut invalid_bitmap16_width_bytes = create_pattern.data[..10].to_vec();
    invalid_bitmap16_width_bytes[6..8].copy_from_slice(&4i16.to_le_bytes());
    assert!(
      WmfBitmap16Header::read_from(&mut Reader::new(Cursor::new(invalid_bitmap16_width_bytes)))
        .is_err()
    );
    let mut invalid_bitmap16_header = bitmap16.header;
    invalid_bitmap16_header.width_bytes = -1;
    assert!(
      invalid_bitmap16_header
        .write_to(&mut Writer::new(Cursor::new(Vec::new())))
        .is_err()
    );

    assert!(
      WmfRecordData::CreatePatternBrush(WmfCreatePatternBrushRecord {
        bitmap: WmfBitmap16Header {
          bitmap_type: 0,
          width: 2,
          height: 2,
          width_bytes: 2,
          planes: 1,
          bits_pixel: 1,
        },
        ignored_bits: 0,
        reserved: [0; 18],
        pattern: vec![0xAA],
      })
      .to_record()
      .is_err()
    );

    let dib_create = WmfRecord::new(
      WmfRecordFunction::DibCreatePatternBrush.raw(),
      [
        0x0006u16.to_le_bytes().as_slice(),
        DibColorUsage::RgbColors.wmf_raw().to_le_bytes().as_slice(),
        core_1bpp_dib.as_slice(),
      ]
      .concat(),
    );
    let parsed = dib_create.parse_data().unwrap();
    let WmfRecordData::DibCreatePatternBrush(value) = &parsed else {
      panic!("expected META_DIBCREATEPATTERNBRUSH");
    };
    assert_eq!(value.style_kind(), Some(WmfBrushStyle::DibPatternPt));
    assert_eq!(value.color_usage_kind(), Some(DibColorUsage::RgbColors));
    let dib_info = value.dib_info().unwrap();
    assert_eq!(
      dib_info.header.header_size(),
      crate::bitmap::BITMAP_CORE_HEADER_SIZE
    );
    assert_eq!(dib_info.header.bit_count_kind(), Some(BitmapBitCount::One));
    assert_eq!(dib_info.color_table.len(), 8);
    let dib = value.device_independent_bitmap().unwrap();
    assert_eq!(dib.info, dib_info);
    assert_eq!(dib.bits, [0x80, 0x00]);
    assert_eq!(parsed.to_record().unwrap(), dib_create);
    assert!(
      WmfRecord::new(
        WmfRecordFunction::DibCreatePatternBrush.raw(),
        [
          WmfBrushStyle::Pattern.raw().to_le_bytes().as_slice(),
          DibColorUsage::PalColors.wmf_raw().to_le_bytes().as_slice(),
          &[0xAA, 0xBB],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecord::new(
        WmfRecordFunction::DibCreatePatternBrush.raw(),
        [
          WmfBrushStyle::DibPatternPt.raw().to_le_bytes().as_slice(),
          0xFFFF_u16.to_le_bytes().as_slice(),
          &[0xAA, 0xBB],
        ]
        .concat(),
      )
      .parse_data()
      .is_err()
    );
    assert!(
      WmfRecordData::DibCreatePatternBrush(WmfDibCreatePatternBrushRecord {
        style: WmfBrushStyle::Pattern.raw(),
        color_usage: DibColorUsage::PalColors.wmf_raw(),
        target: Vec::new(),
      })
      .to_record()
      .is_err()
    );

    let create_region = WmfRecord::new(
      WmfRecordFunction::CreateRegion.raw(),
      [
        0u16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        1u32.to_le_bytes().as_slice(),
        34i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        10i16.to_le_bytes().as_slice(),
        11i16.to_le_bytes().as_slice(),
        2u16.to_le_bytes().as_slice(),
        1u16.to_le_bytes().as_slice(),
        2u16.to_le_bytes().as_slice(),
        3u16.to_le_bytes().as_slice(),
        4u16.to_le_bytes().as_slice(),
        2u16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = create_region.parse_data().unwrap();
    let WmfRecordData::CreateRegion(value) = &parsed else {
      panic!("expected META_CREATEREGION");
    };
    assert_eq!(value.object_type, 6);
    assert_eq!(value.scan_count as usize, value.scans.len());
    assert_eq!(value.max_scan, 2);
    assert_eq!(value.sdk_size() as usize, create_region.data.len());
    assert_eq!(parsed.to_record().unwrap(), create_region);

    let mut invalid_region = create_region.clone();
    invalid_region.data[2..4].copy_from_slice(&5i16.to_le_bytes());
    assert!(invalid_region.parse_data().is_err());

    let mut invalid_count2 = create_region.clone();
    let count2_offset = invalid_count2.data.len() - 2;
    invalid_count2.data[count2_offset..].copy_from_slice(&4u16.to_le_bytes());
    assert!(invalid_count2.parse_data().is_err());

    let mut oversized_scan_count = create_region.clone();
    oversized_scan_count.data.truncate(22);
    oversized_scan_count.data[10..12].copy_from_slice(&1_000i16.to_le_bytes());
    assert!(oversized_scan_count.parse_data().is_err());

    let mut oversized_scan_lines = create_region.clone();
    oversized_scan_lines.data.truncate(28);
    oversized_scan_lines.data[22..24].copy_from_slice(&100u16.to_le_bytes());
    assert!(oversized_scan_lines.parse_data().is_err());
  }

  #[test]
  fn typed_wmf_bitmap_transfer_records_roundtrip() {
    let core_1bpp_dib = core_1bpp_dib_bytes();
    assert!(WmfTernaryRasterOperationCode::SRCCOPY.uses_source());
    assert!(!WmfTernaryRasterOperationCode::PATCOPY.uses_source());
    assert_eq!(
      WmfTernaryRasterOperationCode::SRCCOPY.canonical_raw(),
      0x00CC_0020
    );
    assert_eq!(
      WmfTernaryRasterOperationCode::PATCOPY.canonical_raw(),
      0x00F0_0021
    );
    for (index, value) in WMF_TERNARY_RASTER_OPERATION_VALUES.iter().enumerate() {
      assert_eq!((value >> 16) as usize, index);
      assert!(WmfTernaryRasterOperation::new(*value).is_valid());
    }
    let bit_blt_no_source = WmfRecord::new(
      WmfRecordFunction::BitBlt.raw(),
      [
        0x00F0_0021u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        0xCAFEu16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = bit_blt_no_source.parse_data().unwrap();
    let WmfRecordData::BitBlt(value) = &parsed else {
      panic!("expected META_BITBLT");
    };
    assert_eq!(
      value.raster_operation_code(),
      WmfTernaryRasterOperationCode::PATCOPY
    );
    assert_eq!(value.ternary_raster_operation().raw(), 0x00F0_0021);
    assert!(!value.target.is_source_present());
    assert!(value.target.source_bytes().is_none());
    assert_eq!(value.target.bitmap16().unwrap(), None);
    assert_eq!(parsed.to_record().unwrap(), bit_blt_no_source);
    assert!(parsed.to_record_with_function(0x0822).is_err());
    let invalid_bit_blt_function_size = WmfRecord::new(
      0x0822,
      [
        0x00F0_0021u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        0xCAFEu16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    assert!(
      invalid_bit_blt_function_size
        .embedded_source_present()
        .is_err()
    );
    assert!(invalid_bit_blt_function_size.parse_data().is_err());
    let source_dependent_no_source = WmfRecord::new(
      WmfRecordFunction::BitBlt.raw(),
      [
        0x00CC_0020u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        0xCAFEu16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let source_dependent_no_source_data = source_dependent_no_source.parse_data().unwrap();
    assert_eq!(
      source_dependent_no_source_data.to_record().unwrap(),
      source_dependent_no_source
    );
    assert!(source_dependent_no_source_data.validate_strict().is_err());
    let invalid_bit_blt_rop = WmfRecord::new(
      WmfRecordFunction::BitBlt.raw(),
      [
        0x00F0_0020u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        0xCAFEu16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    assert!(invalid_bit_blt_rop.parse_data().is_err());
    let bit_blt_without_source = WmfRecordData::BitBlt(WmfBitBltRecord {
      raster_operation: 0x00CC_0020,
      y_src: 1,
      x_src: 2,
      height: 3,
      width: 4,
      y_dest: 5,
      x_dest: 6,
      target: WmfBitmap16Target::NoSource { reserved: 0xCAFE },
    });
    assert!(bit_blt_without_source.to_record().is_ok());
    assert!(bit_blt_without_source.validate_strict().is_err());
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::BitBlt.raw(),
      [
        0x00CC_0020u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        &[0, 0, 2, 0, 2, 0, 2, 0, 1, 1, 0xAA, 0xBB],
      ]
      .concat(),
    ));
    let valid_bitmap16_source = WmfBitmap16 {
      header: WmfBitmap16Header {
        bitmap_type: 0,
        width: 2,
        height: 1,
        width_bytes: 2,
        planes: 1,
        bits_pixel: 1,
      },
      bits: vec![0xAA, 0xBB],
    }
    .to_bytes()
    .unwrap();
    let bit_blt_source = WmfRecord::new(
      WmfRecordFunction::BitBlt.raw(),
      [
        0x00CC_0020u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        valid_bitmap16_source.as_slice(),
      ]
      .concat(),
    );
    let WmfRecordData::BitBlt(value) = bit_blt_source.parse_data().unwrap() else {
      panic!("expected META_BITBLT");
    };
    assert!(value.target.is_source_present());
    assert_eq!(
      value.target.source_bytes(),
      Some(valid_bitmap16_source.as_slice())
    );
    let parsed_bitmap16 = value.target.bitmap16().unwrap().unwrap();
    assert_eq!(parsed_bitmap16.header.height, 1);
    assert_eq!(parsed_bitmap16.bits, [0xAA, 0xBB]);
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::DibBitBlt.raw(),
      [
        0x00F0_0021u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        0x1234u16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    ));
    let dib_bit_blt_source = WmfRecord::new(
      WmfRecordFunction::DibBitBlt.raw(),
      [
        0x00CC_0020u32.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        3i16.to_le_bytes().as_slice(),
        4i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        core_1bpp_dib.as_slice(),
      ]
      .concat(),
    );
    let WmfRecordData::DibBitBlt(value) = dib_bit_blt_source.parse_data().unwrap() else {
      panic!("expected META_DIBBITBLT");
    };
    assert!(value.target.is_source_present());
    assert_eq!(value.target.source_bytes(), Some(core_1bpp_dib.as_slice()));
    let dib = value
      .target
      .device_independent_bitmap(DibColorUsage::RgbColors)
      .unwrap()
      .unwrap();
    assert_eq!(dib.info.header.width(), 1);
    assert_eq!(dib.bits, [0x80, 0x00]);
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::StretchBlt.raw(),
      [
        0x00F0_0021u32.to_le_bytes().as_slice(),
        10i16.to_le_bytes().as_slice(),
        20i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        0xBEEFu16.to_le_bytes().as_slice(),
        30i16.to_le_bytes().as_slice(),
        40i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    ));
    assert_typed_roundtrip(WmfRecord::new(
      WmfRecordFunction::DibStretchBlt.raw(),
      [
        0x00CC_0020u32.to_le_bytes().as_slice(),
        10i16.to_le_bytes().as_slice(),
        20i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        30i16.to_le_bytes().as_slice(),
        40i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        core_1bpp_dib.as_slice(),
      ]
      .concat(),
    ));
    let source_dependent_without_source = WmfRecordData::DibStretchBlt(WmfDibStretchBltRecord {
      raster_operation: 0x00CC_0020,
      src_height: 10,
      src_width: 20,
      y_src: 1,
      x_src: 2,
      dest_height: 30,
      dest_width: 40,
      y_dest: 5,
      x_dest: 6,
      target: WmfDibTarget::NoSource { reserved: 0xBEEF },
    });
    let source_dependent_record = source_dependent_without_source.to_record().unwrap();
    assert_eq!(
      source_dependent_record.parse_data().unwrap(),
      source_dependent_without_source
    );
    assert!(source_dependent_without_source.validate_strict().is_err());

    let pat_blt = WmfRecord::new(
      WmfRecordFunction::PatBlt.raw(),
      [
        0x00F0_0021u32.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
        7i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    let parsed = pat_blt.parse_data().unwrap();
    let WmfRecordData::PatBlt(value) = &parsed else {
      panic!("expected META_PATBLT");
    };
    assert_eq!(
      value.raster_operation_code(),
      WmfTernaryRasterOperationCode::PATCOPY
    );
    assert_eq!(parsed.to_record().unwrap(), pat_blt);

    let invalid_pat_blt_rop = WmfRecord::new(
      WmfRecordFunction::PatBlt.raw(),
      [
        0x00F0_0020u32.to_le_bytes().as_slice(),
        8i16.to_le_bytes().as_slice(),
        7i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
      ]
      .concat(),
    );
    assert!(invalid_pat_blt_rop.parse_data().is_err());
    assert!(
      WmfRecordData::PatBlt(WmfPatBltRecord {
        raster_operation: 0x00F0_0020,
        height: 8,
        width: 7,
        y_left: 6,
        x_left: 5,
      })
      .to_record()
      .is_err()
    );

    let set_dib_to_dev = WmfRecord::new(
      WmfRecordFunction::SetDibToDev.raw(),
      [
        DibColorUsage::RgbColors.wmf_raw().to_le_bytes().as_slice(),
        1u16.to_le_bytes().as_slice(),
        0u16.to_le_bytes().as_slice(),
        0u16.to_le_bytes().as_slice(),
        0u16.to_le_bytes().as_slice(),
        1u16.to_le_bytes().as_slice(),
        1u16.to_le_bytes().as_slice(),
        5u16.to_le_bytes().as_slice(),
        6u16.to_le_bytes().as_slice(),
        core_1bpp_dib.as_slice(),
      ]
      .concat(),
    );
    let parsed = set_dib_to_dev.parse_data().unwrap();
    let WmfRecordData::SetDibToDev(value) = &parsed else {
      panic!("expected META_SETDIBTODEV");
    };
    assert_eq!(value.color_usage_kind(), Some(DibColorUsage::RgbColors));
    let dib_info = value.dib_info().unwrap();
    assert_eq!(
      dib_info.header.header_size(),
      crate::bitmap::BITMAP_CORE_HEADER_SIZE
    );
    assert_eq!(dib_info.header.bit_count_kind(), Some(BitmapBitCount::One));
    assert_eq!(dib_info.color_table.len(), 8);
    let dib = value.device_independent_bitmap().unwrap();
    assert_eq!(dib.info, dib_info);
    assert_eq!(dib.bits, [0x80, 0x00]);
    assert_eq!(parsed.to_record().unwrap(), set_dib_to_dev);

    let stretch_dib = WmfRecord::new(
      WmfRecordFunction::StretchDib.raw(),
      [
        0x00CC_0020u32.to_le_bytes().as_slice(),
        DibColorUsage::RgbColors.wmf_raw().to_le_bytes().as_slice(),
        10i16.to_le_bytes().as_slice(),
        20i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        2i16.to_le_bytes().as_slice(),
        30i16.to_le_bytes().as_slice(),
        40i16.to_le_bytes().as_slice(),
        5i16.to_le_bytes().as_slice(),
        6i16.to_le_bytes().as_slice(),
        core_1bpp_dib.as_slice(),
      ]
      .concat(),
    );
    let parsed = stretch_dib.parse_data().unwrap();
    let WmfRecordData::StretchDib(value) = &parsed else {
      panic!("expected META_STRETCHDIB");
    };
    assert_eq!(value.color_usage_kind(), Some(DibColorUsage::RgbColors));
    assert_eq!(
      value.raster_operation_code(),
      WmfTernaryRasterOperationCode::SRCCOPY
    );
    let dib_info = value.dib_info().unwrap();
    assert_eq!(
      dib_info.header.header_size(),
      crate::bitmap::BITMAP_CORE_HEADER_SIZE
    );
    assert_eq!(dib_info.header.bit_count_kind(), Some(BitmapBitCount::One));
    assert_eq!(dib_info.color_table.len(), 8);
    let dib = value.device_independent_bitmap().unwrap();
    assert_eq!(dib.info, dib_info);
    assert_eq!(dib.bits, [0x80, 0x00]);
    assert_eq!(parsed.to_record().unwrap(), stretch_dib);

    let png_dib = png_dib_bytes();
    let stretch_png = WmfRecord::new(
      WmfRecordFunction::StretchDib.raw(),
      [
        WmfTernaryRasterOperation::from_operation_code(
          WmfTernaryRasterOperationCode::SRCCOPY,
          0x0020,
        )
        .raw()
        .to_le_bytes()
        .as_slice(),
        DibColorUsage::RgbColors.wmf_raw().to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        png_dib.as_slice(),
      ]
      .concat(),
    );
    let parsed = stretch_png.parse_data().unwrap();
    let WmfRecordData::StretchDib(value) = &parsed else {
      panic!("expected META_STRETCHDIB");
    };
    assert_eq!(
      value.device_independent_bitmap().unwrap().embedded_format(),
      Some(crate::bitmap::EmbeddedBitmapFormat::Png)
    );
    assert_eq!(parsed.to_record().unwrap(), stretch_png);

    let invalid_png_color_usage = WmfRecord::new(
      WmfRecordFunction::StretchDib.raw(),
      [
        WmfTernaryRasterOperation::from_operation_code(
          WmfTernaryRasterOperationCode::SRCCOPY,
          0x0020,
        )
        .raw()
        .to_le_bytes()
        .as_slice(),
        DibColorUsage::PalColors.wmf_raw().to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        png_dib.as_slice(),
      ]
      .concat(),
    );
    let invalid_png_color_usage_data = invalid_png_color_usage.parse_data().unwrap();
    assert_eq!(
      invalid_png_color_usage_data.to_record().unwrap(),
      invalid_png_color_usage
    );
    assert!(invalid_png_color_usage_data.validate_strict().is_err());

    let invalid_png_rop = WmfRecord::new(
      WmfRecordFunction::StretchDib.raw(),
      [
        WmfTernaryRasterOperation::from_operation_code(
          WmfTernaryRasterOperationCode::NOTSRCCOPY,
          0x0020,
        )
        .raw()
        .to_le_bytes()
        .as_slice(),
        DibColorUsage::RgbColors.wmf_raw().to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        png_dib.as_slice(),
      ]
      .concat(),
    );
    assert!(invalid_png_rop.parse_data().is_err());

    let mut invalid_png_size = png_dib.clone();
    invalid_png_size[20..24].copy_from_slice(&5u32.to_le_bytes());
    let invalid_png_size = WmfRecord::new(
      WmfRecordFunction::StretchDib.raw(),
      [
        WmfTernaryRasterOperation::from_operation_code(
          WmfTernaryRasterOperationCode::SRCCOPY,
          0x0020,
        )
        .raw()
        .to_le_bytes()
        .as_slice(),
        DibColorUsage::RgbColors.wmf_raw().to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        1i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        0i16.to_le_bytes().as_slice(),
        invalid_png_size.as_slice(),
      ]
      .concat(),
    );
    let invalid_png_size_data = invalid_png_size.parse_data().unwrap();
    assert_eq!(invalid_png_size_data.to_record().unwrap(), invalid_png_size);
    assert!(invalid_png_size_data.validate_strict().is_err());

    let mut invalid_write = parsed.clone();
    let WmfRecordData::StretchDib(value) = &mut invalid_write else {
      panic!("expected META_STRETCHDIB");
    };
    value.raster_operation = WmfTernaryRasterOperation::from_operation_code(
      WmfTernaryRasterOperationCode::NOTSRCCOPY,
      0x0020,
    )
    .raw();
    assert!(invalid_write.to_record().is_err());
  }
}
