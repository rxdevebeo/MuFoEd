use emfsdk_derive::{SdkEnum, SdkObject};

use crate::common::{Error, Reader, Result, SdkEnumValue, SdkRead, SdkSize, SdkWrite, Writer};

pub const BITMAP_CORE_HEADER_SIZE: u32 = 12;
pub const BITMAP_INFO_HEADER_SIZE: u32 = 40;
pub const BITMAP_V4_HEADER_SIZE: u32 = 108;
pub const BITMAP_V5_HEADER_SIZE: u32 = 124;

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum DibColorUsage {
  RgbColors = 0x0000,
  PalColors = 0x0001,
  PalIndices = 0x0002,
}

impl DibColorUsage {
  pub fn from_wmf_raw(value: u16) -> Option<Self> {
    Self::from_raw(u32::from(value))
  }

  pub fn wmf_raw(self) -> u16 {
    self.raw() as u16
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum BitmapCompression {
  Rgb = 0x0000,
  Rle8 = 0x0001,
  Rle4 = 0x0002,
  Bitfields = 0x0003,
  Jpeg = 0x0004,
  Png = 0x0005,
  Cmyk = 0x000B,
  CmykRle8 = 0x000C,
  CmykRle4 = 0x000D,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddedBitmapFormat {
  Jpeg,
  Png,
}

impl EmbeddedBitmapFormat {
  pub fn content_type(self) -> &'static str {
    match self {
      Self::Jpeg => "image/jpeg",
      Self::Png => "image/png",
    }
  }
}

impl BitmapCompression {
  pub fn embedded_format(self) -> Option<EmbeddedBitmapFormat> {
    match self {
      Self::Jpeg => Some(EmbeddedBitmapFormat::Jpeg),
      Self::Png => Some(EmbeddedBitmapFormat::Png),
      _ => None,
    }
  }

  pub fn is_top_down_allowed(self) -> bool {
    matches!(self, Self::Rgb | Self::Bitfields)
  }

  pub fn required_bit_count(self) -> Option<BitmapBitCount> {
    match self {
      Self::Rle4 | Self::CmykRle4 => Some(BitmapBitCount::Four),
      Self::Rle8 | Self::CmykRle8 => Some(BitmapBitCount::Eight),
      _ => None,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u16")]
pub enum BitmapBitCount {
  Undefined = 0x0000,
  One = 0x0001,
  Four = 0x0004,
  Eight = 0x0008,
  Sixteen = 0x0010,
  TwentyFour = 0x0018,
  ThirtyTwo = 0x0020,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum BitmapLogicalColorSpace {
  CalibratedRgb = 0x0000_0000,
  SRgb = 0x7352_4742,
  WindowsColorSpace = 0x5769_6E20,
}

impl BitmapLogicalColorSpace {
  pub fn uses_calibrated_fields(self) -> bool {
    matches!(self, Self::CalibratedRgb)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum BitmapLogicalColorSpaceV5 {
  ProfileLinked = 0x4C49_4E4B,
  ProfileEmbedded = 0x4D42_4544,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SdkEnum)]
#[sdk(repr = "u32")]
pub enum BitmapGamutMappingIntent {
  Business = 0x0000_0001,
  Graphics = 0x0000_0002,
  Images = 0x0000_0004,
  AbsoluteColorimetric = 0x0000_0008,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_rgb_quad")]
pub struct RgbQuad {
  pub blue: u8,
  pub green: u8,
  pub red: u8,
  pub reserved: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_bitmap_core_header")]
pub struct BitmapCoreHeader {
  pub header_size: u32,
  pub width: u16,
  pub height: u16,
  pub planes: u16,
  pub bit_count: u16,
}

impl BitmapCoreHeader {
  pub fn bit_count_kind(&self) -> Option<BitmapBitCount> {
    BitmapBitCount::from_raw(self.bit_count)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_bitmap_info_header")]
pub struct BitmapInfoHeader {
  pub header_size: u32,
  pub width: i32,
  pub height: i32,
  pub planes: u16,
  pub bit_count: u16,
  pub compression: u32,
  pub image_size: u32,
  pub x_pels_per_meter: i32,
  pub y_pels_per_meter: i32,
  pub color_used: u32,
  pub color_important: u32,
}

impl BitmapInfoHeader {
  pub fn compression_kind(&self) -> Option<BitmapCompression> {
    BitmapCompression::from_raw(self.compression)
  }

  pub fn bit_count_kind(&self) -> Option<BitmapBitCount> {
    BitmapBitCount::from_raw(self.bit_count)
  }

  pub fn is_top_down(&self) -> bool {
    self.height < 0
  }

  pub fn height_abs(&self) -> u32 {
    self.height.unsigned_abs()
  }
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct BitmapCieXyz {
  pub x: i32,
  pub y: i32,
  pub z: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(format = "wmf")]
pub struct BitmapCieXyzTriple {
  pub red: BitmapCieXyz,
  pub green: BitmapCieXyz,
  pub blue: BitmapCieXyz,
}

#[derive(Clone, Debug, PartialEq, Eq, SdkObject)]
#[sdk(validate = "validate_bitmap_v4_header")]
pub struct BitmapV4Header {
  pub base: BitmapInfoHeader,
  pub red_mask: u32,
  pub green_mask: u32,
  pub blue_mask: u32,
  pub alpha_mask: u32,
  pub color_space_type: u32,
  pub endpoints: BitmapCieXyzTriple,
  pub gamma_red: u32,
  pub gamma_green: u32,
  pub gamma_blue: u32,
}

impl BitmapV4Header {
  pub fn color_space_kind(&self) -> Option<BitmapLogicalColorSpace> {
    BitmapLogicalColorSpace::from_raw(self.color_space_type)
  }

  pub fn color_space_v5_kind(&self) -> Option<BitmapLogicalColorSpaceV5> {
    BitmapLogicalColorSpaceV5::from_raw(self.color_space_type)
  }

  pub fn compression_kind(&self) -> Option<BitmapCompression> {
    self.base.compression_kind()
  }

  pub fn bit_count_kind(&self) -> Option<BitmapBitCount> {
    self.base.bit_count_kind()
  }

  pub fn is_top_down(&self) -> bool {
    self.base.is_top_down()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitmapV5Header {
  pub v4: BitmapV4Header,
  pub intent: u32,
  pub profile_data: u32,
  pub profile_size: u32,
  pub reserved: u32,
}

impl BitmapV5Header {
  pub fn intent_kind(&self) -> Option<BitmapGamutMappingIntent> {
    BitmapGamutMappingIntent::from_raw(self.intent)
  }

  pub fn color_space_kind(&self) -> Option<BitmapLogicalColorSpace> {
    self.v4.color_space_kind()
  }

  pub fn color_space_v5_kind(&self) -> Option<BitmapLogicalColorSpaceV5> {
    self.v4.color_space_v5_kind()
  }

  pub fn compression_kind(&self) -> Option<BitmapCompression> {
    self.v4.compression_kind()
  }

  pub fn bit_count_kind(&self) -> Option<BitmapBitCount> {
    self.v4.bit_count_kind()
  }

  pub fn is_top_down(&self) -> bool {
    self.v4.is_top_down()
  }
}

impl SdkRead for BitmapV5Header {
  fn read_from<R: std::io::Read + std::io::Seek>(reader: &mut Reader<R>) -> Result<Self> {
    let v4 = BitmapV4Header {
      base: BitmapInfoHeader::read_from(reader)?,
      red_mask: reader.read_u32()?,
      green_mask: reader.read_u32()?,
      blue_mask: reader.read_u32()?,
      alpha_mask: reader.read_u32()?,
      color_space_type: reader.read_u32()?,
      endpoints: BitmapCieXyzTriple::read_from(reader)?,
      gamma_red: reader.read_u32()?,
      gamma_green: reader.read_u32()?,
      gamma_blue: reader.read_u32()?,
    };
    let value = Self {
      v4,
      intent: reader.read_u32()?,
      profile_data: reader.read_u32()?,
      profile_size: reader.read_u32()?,
      reserved: reader.read_u32()?,
    };
    validate_bitmap_v5_header(&value)?;
    Ok(value)
  }
}

impl SdkWrite for BitmapV5Header {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    validate_bitmap_v5_header(self)?;
    self.v4.base.write_to(writer)?;
    writer.write_u32(self.v4.red_mask)?;
    writer.write_u32(self.v4.green_mask)?;
    writer.write_u32(self.v4.blue_mask)?;
    writer.write_u32(self.v4.alpha_mask)?;
    writer.write_u32(self.v4.color_space_type)?;
    self.v4.endpoints.write_to(writer)?;
    writer.write_u32(self.v4.gamma_red)?;
    writer.write_u32(self.v4.gamma_green)?;
    writer.write_u32(self.v4.gamma_blue)?;
    writer.write_u32(self.intent)?;
    writer.write_u32(self.profile_data)?;
    writer.write_u32(self.profile_size)?;
    writer.write_u32(self.reserved)
  }
}

impl SdkSize for BitmapV5Header {
  fn sdk_size(&self) -> u64 {
    u64::from(BITMAP_V5_HEADER_SIZE)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DibHeader {
  Core(BitmapCoreHeader),
  Info {
    base: BitmapInfoHeader,
    extension: Vec<u8>,
  },
  V4(BitmapV4Header),
  V5(BitmapV5Header),
}

impl DibHeader {
  pub fn header_size(&self) -> u32 {
    match self {
      Self::Core(value) => value.header_size,
      Self::Info { base, .. } => base.header_size,
      Self::V4(value) => value.base.header_size,
      Self::V5(value) => value.v4.base.header_size,
    }
  }

  pub fn width(&self) -> i32 {
    match self {
      Self::Core(value) => i32::from(value.width),
      Self::Info { base, .. } => base.width,
      Self::V4(value) => value.base.width,
      Self::V5(value) => value.v4.base.width,
    }
  }

  pub fn height(&self) -> i32 {
    match self {
      Self::Core(value) => i32::from(value.height),
      Self::Info { base, .. } => base.height,
      Self::V4(value) => value.base.height,
      Self::V5(value) => value.v4.base.height,
    }
  }

  pub fn bit_count(&self) -> u16 {
    match self {
      Self::Core(value) => value.bit_count,
      Self::Info { base, .. } => base.bit_count,
      Self::V4(value) => value.base.bit_count,
      Self::V5(value) => value.v4.base.bit_count,
    }
  }

  pub fn bit_count_kind(&self) -> Option<BitmapBitCount> {
    BitmapBitCount::from_raw(self.bit_count())
  }

  pub fn color_used(&self) -> u32 {
    match self {
      Self::Core(_) => 0,
      Self::Info { base, .. } => base.color_used,
      Self::V4(value) => value.base.color_used,
      Self::V5(value) => value.v4.base.color_used,
    }
  }

  pub fn image_size(&self) -> u32 {
    match self {
      Self::Core(_) => 0,
      Self::Info { base, .. } => base.image_size,
      Self::V4(value) => value.base.image_size,
      Self::V5(value) => value.v4.base.image_size,
    }
  }

  pub fn planes(&self) -> u16 {
    match self {
      Self::Core(value) => value.planes,
      Self::Info { base, .. } => base.planes,
      Self::V4(value) => value.base.planes,
      Self::V5(value) => value.v4.base.planes,
    }
  }

  pub fn compression_kind(&self) -> Option<BitmapCompression> {
    match self {
      Self::Core(_) => Some(BitmapCompression::Rgb),
      Self::Info { base, .. } => base.compression_kind(),
      Self::V4(value) => value.compression_kind(),
      Self::V5(value) => value.compression_kind(),
    }
  }

  pub fn is_top_down(&self) -> bool {
    match self {
      Self::Core(_) => false,
      Self::Info { base, .. } => base.is_top_down(),
      Self::V4(value) => value.is_top_down(),
      Self::V5(value) => value.is_top_down(),
    }
  }

  pub fn height_abs(&self) -> u32 {
    match self {
      Self::Core(value) => u32::from(value.height),
      Self::Info { base, .. } => base.height_abs(),
      Self::V4(value) => value.base.height_abs(),
      Self::V5(value) => value.v4.base.height_abs(),
    }
  }

  pub fn scan_line_stride_bytes(&self) -> Result<u64> {
    dib_scan_line_stride_bytes(self)
  }

  pub fn calculated_bitmap_bits_size_bytes(&self) -> Result<u64> {
    let stride = self.scan_line_stride_bytes()?;
    stride
      .checked_mul(u64::from(self.height_abs()))
      .ok_or_else(|| Error::invalid(0, "DIB bitmap bits size overflows"))
  }

  pub fn expected_bitmap_bits_size_bytes(&self) -> Result<u64> {
    match self.compression_kind() {
      Some(BitmapCompression::Rgb | BitmapCompression::Bitfields | BitmapCompression::Cmyk) => {
        self.calculated_bitmap_bits_size_bytes()
      }
      Some(_) => Ok(u64::from(self.image_size())),
      None => Err(Error::invalid(0, "DIB Compression is invalid")),
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Core(value) => value.write_to(writer),
      Self::Info { base, extension } => {
        base.write_to(writer)?;
        writer.write_all(extension)
      }
      Self::V4(value) => value.write_to(writer),
      Self::V5(value) => value.write_to(writer),
    }
  }
}

impl SdkSize for DibHeader {
  fn sdk_size(&self) -> u64 {
    match self {
      Self::Core(value) => value.sdk_size(),
      Self::Info { base, extension } => base.sdk_size() + extension.len() as u64,
      Self::V4(value) => value.sdk_size(),
      Self::V5(value) => value.sdk_size(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DibBitmapInfo {
  pub header: DibHeader,
  pub color_table: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DibColorTable {
  RgbQuads {
    entries: Vec<RgbQuad>,
    trailing_data: Vec<u8>,
  },
  PaletteIndices {
    entries: Vec<u16>,
    trailing_data: Vec<u8>,
  },
  None {
    trailing_data: Vec<u8>,
  },
}

impl DibColorTable {
  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let capacity = match self {
      Self::RgbQuads {
        entries,
        trailing_data,
      } => entries
        .len()
        .checked_mul(4)
        .and_then(|size| size.checked_add(trailing_data.len())),
      Self::PaletteIndices {
        entries,
        trailing_data,
      } => entries
        .len()
        .checked_mul(2)
        .and_then(|size| size.checked_add(trailing_data.len())),
      Self::None { trailing_data } => Some(trailing_data.len()),
    }
    .ok_or_else(|| Error::invalid(0, "DIB color table size overflows usize"))?;
    let mut writer = Writer::new(std::io::Cursor::new(Vec::with_capacity(capacity)));
    match self {
      Self::RgbQuads {
        entries,
        trailing_data,
      } => {
        for entry in entries {
          validate_rgb_quad(entry)?;
          entry.write_to(&mut writer)?;
        }
        writer.write_all(trailing_data)?;
      }
      Self::PaletteIndices {
        entries,
        trailing_data,
      } => {
        for entry in entries {
          writer.write_u16(*entry)?;
        }
        writer.write_all(trailing_data)?;
      }
      Self::None { trailing_data } => writer.write_all(trailing_data)?,
    }
    Ok(writer.into_inner().into_inner())
  }
}

impl DibBitmapInfo {
  pub fn read_from_slice(bytes: &[u8]) -> Result<Self> {
    let header = read_dib_header_from_slice(bytes)?;
    let header_size = header.header_size() as usize;
    let value = Self {
      header,
      color_table: bytes[header_size..].to_vec(),
    };
    validate_dib_bitmap_info(&value)?;
    Ok(value)
  }

  pub fn read_packed_prefix_from_slice(
    bytes: &[u8],
    color_usage: DibColorUsage,
  ) -> Result<(Self, usize)> {
    let header = read_dib_header_from_slice(bytes)?;
    let prefix_len = packed_dib_info_len(&header, color_usage)?;
    if bytes.len() < prefix_len {
      return Err(Error::invalid(0, "packed DIB bitmap info is truncated"));
    }
    let header_size = header.header_size() as usize;
    let value = Self {
      header,
      color_table: bytes[header_size..prefix_len].to_vec(),
    };
    validate_dib_bitmap_info(&value)?;
    Ok((value, prefix_len))
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut writer = Writer::new(std::io::Cursor::new(Vec::with_capacity(
      self.header.sdk_size() as usize + self.color_table.len(),
    )));
    validate_dib_bitmap_info(self)?;
    self.header.write_to(&mut writer)?;
    writer.write_all(&self.color_table)?;
    Ok(writer.into_inner().into_inner())
  }

  pub fn compression_kind(&self) -> Option<BitmapCompression> {
    self.header.compression_kind()
  }

  pub fn embedded_format(&self) -> Option<EmbeddedBitmapFormat> {
    self.compression_kind()?.embedded_format()
  }

  pub fn validate_strict(&self) -> Result<()> {
    validate_dib_bitmap_info(self)?;
    validate_dib_header_strict(&self.header)
  }

  pub fn bitfield_masks(&self) -> Result<Option<[u32; 3]>> {
    dib_info_bitfield_masks(self)
  }

  pub fn parse_color_table(&self, color_usage: DibColorUsage) -> Result<DibColorTable> {
    let entry_count = dib_color_table_entry_count(&self.header, color_usage)?;
    let color_table = dib_info_color_table_payload(self)?;
    match color_usage {
      DibColorUsage::RgbColors => {
        let (table, trailing_data) = split_color_table_bytes(color_table, entry_count, 4)?;
        let mut entries = Vec::with_capacity(entry_count);
        for chunk in table.chunks_exact(4) {
          let entry = RgbQuad {
            blue: chunk[0],
            green: chunk[1],
            red: chunk[2],
            reserved: chunk[3],
          };
          validate_rgb_quad(&entry)?;
          entries.push(entry);
        }
        Ok(DibColorTable::RgbQuads {
          entries,
          trailing_data: trailing_data.to_vec(),
        })
      }
      DibColorUsage::PalColors => {
        let (table, trailing_data) = split_color_table_bytes(color_table, entry_count, 2)?;
        let entries = table
          .chunks_exact(2)
          .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
          .collect();
        Ok(DibColorTable::PaletteIndices {
          entries,
          trailing_data: trailing_data.to_vec(),
        })
      }
      DibColorUsage::PalIndices => Ok(DibColorTable::None {
        trailing_data: color_table.to_vec(),
      }),
    }
  }
}

fn read_dib_header_from_slice(bytes: &[u8]) -> Result<DibHeader> {
  if bytes.len() < 4 {
    return Err(Error::invalid(
      0,
      "DIB bitmap info is smaller than HeaderSize",
    ));
  }

  let mut reader = Reader::new(std::io::Cursor::new(bytes));
  let header_size = reader.read_u32()?;
  reader.seek(0)?;

  let header = match header_size {
    BITMAP_CORE_HEADER_SIZE => {
      if bytes.len() < BITMAP_CORE_HEADER_SIZE as usize {
        return Err(Error::invalid(0, "BitmapCoreHeader is truncated"));
      }
      let value = BitmapCoreHeader::read_from(&mut reader)?;
      validate_bitmap_core_header(&value)?;
      DibHeader::Core(value)
    }
    BITMAP_V4_HEADER_SIZE => {
      if bytes.len() < BITMAP_V4_HEADER_SIZE as usize {
        return Err(Error::invalid(0, "BitmapV4Header is truncated"));
      }
      let value = BitmapV4Header::read_from(&mut reader)?;
      validate_bitmap_v4_header(&value)?;
      DibHeader::V4(value)
    }
    BITMAP_V5_HEADER_SIZE => {
      if bytes.len() < BITMAP_V5_HEADER_SIZE as usize {
        return Err(Error::invalid(0, "BitmapV5Header is truncated"));
      }
      let value = BitmapV5Header::read_from(&mut reader)?;
      validate_bitmap_v5_header(&value)?;
      DibHeader::V5(value)
    }
    size if size >= BITMAP_INFO_HEADER_SIZE => {
      if bytes.len() < size as usize {
        return Err(Error::invalid(0, "BitmapInfoHeader extension is truncated"));
      }
      let base = BitmapInfoHeader::read_from(&mut reader)?;
      validate_bitmap_info_header(&base)?;
      let extension_size = size as usize - BITMAP_INFO_HEADER_SIZE as usize;
      let extension = reader.read_vec(extension_size)?;
      DibHeader::Info { base, extension }
    }
    _ => {
      return Err(Error::invalid(
        0,
        format!("unsupported DIB header size {header_size}"),
      ));
    }
  };

  Ok(header)
}

fn packed_dib_info_len(header: &DibHeader, color_usage: DibColorUsage) -> Result<usize> {
  let header_size = usize::try_from(header.header_size())
    .map_err(|_| Error::invalid(0, "DIB header size overflows usize"))?;
  let bitfields_bytes = if header.header_size() == BITMAP_INFO_HEADER_SIZE
    && header.compression_kind() == Some(BitmapCompression::Bitfields)
  {
    12usize
  } else {
    0usize
  };
  let entry_count = dib_color_table_entry_count(header, color_usage)?;
  let entry_size = match color_usage {
    DibColorUsage::RgbColors => 4usize,
    DibColorUsage::PalColors => 2usize,
    DibColorUsage::PalIndices => 0usize,
  };
  let color_table_bytes = entry_count
    .checked_mul(entry_size)
    .ok_or_else(|| Error::invalid(0, "DIB color table size overflows"))?;
  header_size
    .checked_add(bitfields_bytes)
    .and_then(|value| value.checked_add(color_table_bytes))
    .ok_or_else(|| Error::invalid(0, "DIB bitmap info size overflows"))
}

fn dib_color_table_entry_count(header: &DibHeader, color_usage: DibColorUsage) -> Result<usize> {
  if color_usage == DibColorUsage::PalIndices {
    return Ok(0);
  }

  let bit_count = header.bit_count();
  match bit_count {
    1 | 4 | 8 => {
      let maximum = 1usize << bit_count;
      if header.color_used() == 0 {
        Ok(maximum)
      } else {
        let declared = usize::try_from(header.color_used())
          .map_err(|_| Error::invalid(0, "DIB color table count overflows usize"))?;
        Ok(declared.min(maximum))
      }
    }
    _ if header.color_used() != 0 => usize::try_from(header.color_used())
      .map_err(|_| Error::invalid(0, "DIB color table count overflows usize")),
    _ => Ok(0),
  }
}

fn dib_scan_line_stride_bytes(header: &DibHeader) -> Result<u64> {
  let width = u64::try_from(header.width())
    .map_err(|_| Error::invalid(0, "DIB Width must be nonnegative"))?;
  let bits_per_scan_line = width
    .checked_mul(u64::from(header.planes()))
    .and_then(|value| value.checked_mul(u64::from(header.bit_count())))
    .ok_or_else(|| Error::invalid(0, "DIB scan line size overflows"))?;
  bits_per_scan_line
    .checked_add(31)
    .map(|value| (value & !31) / 8)
    .ok_or_else(|| Error::invalid(0, "DIB scan line stride overflows"))
}

fn split_color_table_bytes(
  bytes: &[u8],
  entry_count: usize,
  entry_size: usize,
) -> Result<(&[u8], &[u8])> {
  let color_table_len = entry_count
    .checked_mul(entry_size)
    .ok_or_else(|| Error::invalid(0, "DIB color table size overflows"))?;
  if bytes.len() < color_table_len {
    return Err(Error::invalid(0, "DIB color table is truncated"));
  }
  Ok(bytes.split_at(color_table_len))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceIndependentBitmap {
  pub info: DibBitmapInfo,
  pub bits: Vec<u8>,
}

impl DeviceIndependentBitmap {
  pub fn from_parts(bitmap_info: &[u8], bitmap_bits: &[u8]) -> Result<Self> {
    Ok(Self {
      info: DibBitmapInfo::read_from_slice(bitmap_info)?,
      bits: bitmap_bits.to_vec(),
    })
  }

  pub fn from_packed_slice(bytes: &[u8], color_usage: DibColorUsage) -> Result<Self> {
    let (info, bits_offset) = DibBitmapInfo::read_packed_prefix_from_slice(bytes, color_usage)?;
    Ok(Self {
      info,
      bits: bytes[bits_offset..].to_vec(),
    })
  }

  pub fn to_packed_bytes(&self) -> Result<Vec<u8>> {
    let info = self.info.to_bytes()?;
    let mut bytes = Vec::with_capacity(info.len() + self.bits.len());
    bytes.extend_from_slice(&info);
    bytes.extend_from_slice(&self.bits);
    Ok(bytes)
  }

  pub fn embedded_format(&self) -> Option<EmbeddedBitmapFormat> {
    self.info.embedded_format()
  }

  pub fn validate_strict(&self) -> Result<()> {
    self.info.validate_strict()?;
    validate_device_independent_bitmap_strict(self)
  }
}

fn validate_device_independent_bitmap_strict(value: &DeviceIndependentBitmap) -> Result<()> {
  if value.info.embedded_format().is_some() {
    let image_size = usize::try_from(value.info.header.image_size())
      .map_err(|_| Error::invalid(0, "DIB ImageSize overflows usize"))?;
    if image_size != value.bits.len() {
      return Err(Error::invalid(
        0,
        "DIB JPEG/PNG ImageSize must match bitmap buffer size",
      ));
    }
  }
  Ok(())
}

fn validate_dib_bitmap_info(value: &DibBitmapInfo) -> Result<()> {
  validate_dib_header(&value.header)?;
  if let Some(masks) = dib_info_bitfield_masks(value)? {
    validate_bitfield_masks(&masks, "BitmapInfoHeader")?;
  }
  validate_bitmap_v5_profile_payload(value)?;
  Ok(())
}

fn validate_bitmap_v5_profile_payload(value: &DibBitmapInfo) -> Result<()> {
  let DibHeader::V5(header) = &value.header else {
    return Ok(());
  };
  if header.color_space_v5_kind().is_none() {
    return Ok(());
  }
  if header.profile_data < BITMAP_V5_HEADER_SIZE {
    return Err(Error::invalid(
      0,
      "BitmapV5Header ProfileData must point after the fixed header",
    ));
  }
  let profile_offset = usize::try_from(header.profile_data - BITMAP_V5_HEADER_SIZE)
    .map_err(|_| Error::invalid(0, "BitmapV5Header ProfileData overflows usize"))?;
  let profile_size = usize::try_from(header.profile_size)
    .map_err(|_| Error::invalid(0, "BitmapV5Header ProfileSize overflows usize"))?;
  let profile_end = profile_offset
    .checked_add(profile_size)
    .ok_or_else(|| Error::invalid(0, "BitmapV5Header profile range overflows"))?;
  if profile_end > value.color_table.len() {
    return Err(Error::invalid(
      0,
      "BitmapV5Header profile range exceeds bitmap info payload",
    ));
  }
  Ok(())
}

fn dib_info_bitfield_masks(value: &DibBitmapInfo) -> Result<Option<[u32; 3]>> {
  if value.header.header_size() == BITMAP_INFO_HEADER_SIZE
    && value.header.compression_kind() == Some(BitmapCompression::Bitfields)
  {
    let masks = value.color_table.get(..12).ok_or_else(|| {
      Error::invalid(
        0,
        "BitmapInfoHeader BI_BITFIELDS requires three color masks",
      )
    })?;
    return Ok(Some([
      u32::from_le_bytes(masks[0..4].try_into().unwrap()),
      u32::from_le_bytes(masks[4..8].try_into().unwrap()),
      u32::from_le_bytes(masks[8..12].try_into().unwrap()),
    ]));
  }
  Ok(None)
}

fn dib_info_color_table_payload(value: &DibBitmapInfo) -> Result<&[u8]> {
  if value.bitfield_masks()?.is_some() {
    Ok(&value.color_table[12..])
  } else {
    Ok(&value.color_table)
  }
}

fn validate_dib_header(value: &DibHeader) -> Result<()> {
  match value {
    DibHeader::Core(value) => validate_bitmap_core_header(value),
    DibHeader::Info { base, .. } => validate_bitmap_info_header(base),
    DibHeader::V4(value) => validate_bitmap_v4_header(value),
    DibHeader::V5(value) => validate_bitmap_v5_header(value),
  }
}

fn validate_dib_header_strict(value: &DibHeader) -> Result<()> {
  validate_dib_header(value)?;
  match value {
    DibHeader::Core(_) => Ok(()),
    DibHeader::Info { base, .. } => validate_bitmap_info_header_strict(base),
    DibHeader::V4(value) => validate_bitmap_info_header_strict(&value.base),
    DibHeader::V5(value) => validate_bitmap_info_header_strict(&value.v4.base),
  }
}

fn validate_bitmap_core_header(value: &BitmapCoreHeader) -> Result<()> {
  if value.header_size != BITMAP_CORE_HEADER_SIZE {
    return Err(Error::invalid(0, "BitmapCoreHeader HeaderSize must be 12"));
  }
  if value.planes != 1 {
    return Err(Error::invalid(0, "BitmapCoreHeader Planes must be 1"));
  }
  if value.bit_count_kind().is_none() {
    return Err(Error::invalid(0, "BitmapCoreHeader BitCount is invalid"));
  }
  Ok(())
}

fn validate_rgb_quad(value: &RgbQuad) -> Result<()> {
  if value.reserved != 0 {
    return Err(Error::invalid(0, "RGBQuad Reserved must be 0"));
  }
  Ok(())
}

fn validate_bitmap_info_header(value: &BitmapInfoHeader) -> Result<()> {
  if value.header_size < BITMAP_INFO_HEADER_SIZE {
    return Err(Error::invalid(
      0,
      "BitmapInfoHeader HeaderSize must be at least 40",
    ));
  }
  if value.width <= 0 {
    return Err(Error::invalid(0, "BitmapInfoHeader Width must be positive"));
  }
  if value.height == 0 {
    return Err(Error::invalid(
      0,
      "BitmapInfoHeader Height must not be zero",
    ));
  }
  if value.planes != 1 {
    return Err(Error::invalid(0, "BitmapInfoHeader Planes must be 1"));
  }
  let bit_count = value.bit_count_kind();
  if bit_count.is_none() {
    return Err(Error::invalid(0, "BitmapInfoHeader BitCount is invalid"));
  }
  let compression = value.compression_kind();
  if compression.is_none() {
    return Err(Error::invalid(0, "BitmapInfoHeader Compression is invalid"));
  }
  if compression == Some(BitmapCompression::Bitfields)
    && !matches!(
      bit_count,
      Some(BitmapBitCount::Sixteen | BitmapBitCount::ThirtyTwo)
    )
  {
    return Err(Error::invalid(
      0,
      "BitmapInfoHeader BI_BITFIELDS is valid only for 16 or 32 bpp",
    ));
  }
  match compression.and_then(BitmapCompression::required_bit_count) {
    Some(required) if bit_count != Some(required) => {
      return Err(Error::invalid(
        0,
        "BitmapInfoHeader RLE compression does not match BitCount",
      ));
    }
    _ => {}
  }
  if compression.unwrap().embedded_format().is_some() && value.image_size == 0 {
    return Err(Error::invalid(
      0,
      "BitmapInfoHeader JPEG/PNG ImageSize must specify the image buffer size",
    ));
  }
  Ok(())
}

fn validate_bitmap_info_header_strict(value: &BitmapInfoHeader) -> Result<()> {
  validate_bitmap_info_header(value)?;
  if value.is_top_down() && !value.compression_kind().unwrap().is_top_down_allowed() {
    return Err(Error::invalid(
      0,
      "BitmapInfoHeader top-down DIB must not use a compressed format",
    ));
  }
  Ok(())
}

fn validate_bitmap_v4_header(value: &BitmapV4Header) -> Result<()> {
  if value.base.header_size != BITMAP_V4_HEADER_SIZE {
    return Err(Error::invalid(0, "BitmapV4Header HeaderSize must be 108"));
  }
  validate_bitmap_info_header(&value.base)?;
  let color_space = value.color_space_kind();
  if color_space.is_none() {
    return Err(Error::invalid(
      0,
      "BitmapV4Header ColorSpaceType is invalid",
    ));
  }
  if color_space.unwrap().uses_calibrated_fields() {
    validate_bitmap_gamma(value.gamma_red, "BitmapV4Header GammaRed")?;
    validate_bitmap_gamma(value.gamma_green, "BitmapV4Header GammaGreen")?;
    validate_bitmap_gamma(value.gamma_blue, "BitmapV4Header GammaBlue")?;
  }
  if value.compression_kind() == Some(BitmapCompression::Bitfields) {
    validate_bitfield_masks(
      &[
        value.red_mask,
        value.green_mask,
        value.blue_mask,
        value.alpha_mask,
      ],
      "BitmapV4Header",
    )?;
  }
  Ok(())
}

fn validate_bitmap_v5_header(value: &BitmapV5Header) -> Result<()> {
  if value.v4.base.header_size != BITMAP_V5_HEADER_SIZE {
    return Err(Error::invalid(0, "BitmapV5Header HeaderSize must be 124"));
  }
  validate_bitmap_info_header(&value.v4.base)?;
  let color_space = value.color_space_kind();
  if color_space.is_none() && value.color_space_v5_kind().is_none() {
    return Err(Error::invalid(
      0,
      "BitmapV5Header ColorSpaceType is invalid",
    ));
  }
  if matches!(color_space, Some(space) if space.uses_calibrated_fields()) {
    validate_bitmap_gamma(value.v4.gamma_red, "BitmapV5Header GammaRed")?;
    validate_bitmap_gamma(value.v4.gamma_green, "BitmapV5Header GammaGreen")?;
    validate_bitmap_gamma(value.v4.gamma_blue, "BitmapV5Header GammaBlue")?;
  }
  if value.intent_kind().is_none() {
    return Err(Error::invalid(0, "BitmapV5Header Intent is invalid"));
  }
  if value.compression_kind() == Some(BitmapCompression::Bitfields) {
    validate_bitfield_masks(
      &[
        value.v4.red_mask,
        value.v4.green_mask,
        value.v4.blue_mask,
        value.v4.alpha_mask,
      ],
      "BitmapV5Header",
    )?;
  }
  Ok(())
}

fn validate_bitmap_gamma(value: u32, name: &str) -> Result<()> {
  if value & 0xFF00_00FF != 0 {
    return Err(Error::invalid(
      0,
      format!("{name} must use the 00nnnnnnffffffff00 fixed-point layout"),
    ));
  }
  Ok(())
}

fn validate_bitfield_masks(masks: &[u32], name: &str) -> Result<()> {
  let mut used_bits = 0u32;
  for mask in masks.iter().copied().filter(|mask| *mask != 0) {
    if !is_contiguous_bit_mask(mask) {
      return Err(Error::invalid(
        0,
        format!("{name} BI_BITFIELDS mask bits must be contiguous"),
      ));
    }
    if used_bits & mask != 0 {
      return Err(Error::invalid(
        0,
        format!("{name} BI_BITFIELDS masks must not overlap"),
      ));
    }
    used_bits |= mask;
  }
  Ok(())
}

fn is_contiguous_bit_mask(mask: u32) -> bool {
  let shifted = mask >> mask.trailing_zeros();
  shifted & (shifted + 1) == 0
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn bitmap_info_header_parses_typed_fields() {
    let bytes = [
      40, 0, 0, 0, // HeaderSize
      3, 0, 0, 0, // Width
      0xFC, 0xFF, 0xFF, 0xFF, // Height = -4
      1, 0, // Planes
      32, 0, // BitCount
      0, 0, 0, 0, // BI_RGB
      0, 0, 0, 0, // ImageSize
      0, 0, 0, 0, // XPelsPerMeter
      0, 0, 0, 0, // YPelsPerMeter
      0, 0, 0, 0, // ColorUsed
      0, 0, 0, 0, // ColorImportant
    ];

    let info = DibBitmapInfo::read_from_slice(&bytes).unwrap();
    let DibHeader::Info { base, extension } = &info.header else {
      unreachable!();
    };
    assert_eq!(base.width, 3);
    assert_eq!(base.height, -4);
    assert_eq!(base.bit_count_kind(), Some(BitmapBitCount::ThirtyTwo));
    assert_eq!(base.compression_kind(), Some(BitmapCompression::Rgb));
    assert!(base.is_top_down());
    assert!(extension.is_empty());
    assert_eq!(info.header.planes(), 1);
    assert_eq!(info.header.height_abs(), 4);
    assert_eq!(info.header.scan_line_stride_bytes().unwrap(), 12);
    assert_eq!(info.header.calculated_bitmap_bits_size_bytes().unwrap(), 48);
    assert_eq!(info.header.expected_bitmap_bits_size_bytes().unwrap(), 48);
    assert_eq!(info.to_bytes().unwrap(), bytes);

    let mut jpeg_bytes = bytes;
    jpeg_bytes[8..12].copy_from_slice(&4i32.to_le_bytes());
    jpeg_bytes[16..20].copy_from_slice(&BitmapCompression::Jpeg.raw().to_le_bytes());
    jpeg_bytes[20..24].copy_from_slice(&123u32.to_le_bytes());
    let jpeg_info = DibBitmapInfo::read_from_slice(&jpeg_bytes).unwrap();
    assert_eq!(
      jpeg_info.header.compression_kind(),
      Some(BitmapCompression::Jpeg)
    );
    assert_eq!(
      jpeg_info.header.expected_bitmap_bits_size_bytes().unwrap(),
      123
    );
  }

  #[test]
  fn dib_info_preserves_header_extension_and_color_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&44u32.to_le_bytes());
    bytes.extend_from_slice(&1i32.to_le_bytes());
    bytes.extend_from_slice(&2i32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(&(BitmapCompression::Bitfields.raw()).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0xAB, 4));
    bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

    let info = DibBitmapInfo::read_from_slice(&bytes).unwrap();
    let DibHeader::Info { base, extension } = &info.header else {
      unreachable!();
    };
    assert_eq!(base.header_size, 44);
    assert_eq!(extension, &[0xAB; 4]);
    assert_eq!(info.color_table, [0x01, 0x02, 0x03, 0x04]);
    assert_eq!(info.to_bytes().unwrap(), bytes);
  }

  #[test]
  fn dib_color_table_parses_rgbquads_and_palette_indices() {
    let mut rgb_bytes = Vec::new();
    rgb_bytes.extend_from_slice(&BITMAP_INFO_HEADER_SIZE.to_le_bytes());
    rgb_bytes.extend_from_slice(&2i32.to_le_bytes());
    rgb_bytes.extend_from_slice(&2i32.to_le_bytes());
    rgb_bytes.extend_from_slice(&1u16.to_le_bytes());
    rgb_bytes.extend_from_slice(&8u16.to_le_bytes());
    rgb_bytes.extend_from_slice(&(BitmapCompression::Rgb.raw()).to_le_bytes());
    rgb_bytes.extend_from_slice(&0u32.to_le_bytes());
    rgb_bytes.extend_from_slice(&0i32.to_le_bytes());
    rgb_bytes.extend_from_slice(&0i32.to_le_bytes());
    rgb_bytes.extend_from_slice(&2u32.to_le_bytes());
    rgb_bytes.extend_from_slice(&0u32.to_le_bytes());
    rgb_bytes.extend_from_slice(&[0x10, 0x20, 0x30, 0x00]);
    rgb_bytes.extend_from_slice(&[0x40, 0x50, 0x60, 0x00]);
    rgb_bytes.extend_from_slice(&[0xAA, 0xBB]);

    let info = DibBitmapInfo::read_from_slice(&rgb_bytes).unwrap();
    let table = info.parse_color_table(DibColorUsage::RgbColors).unwrap();
    let DibColorTable::RgbQuads {
      entries,
      trailing_data,
    } = &table
    else {
      panic!("expected RGBQuad color table");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].red, 0x30);
    assert_eq!(entries[1].blue, 0x40);
    assert_eq!(trailing_data, &[0xAA, 0xBB]);
    assert_eq!(table.to_bytes().unwrap(), info.color_table);

    let mut invalid_reserved = rgb_bytes;
    invalid_reserved[43] = 0x7F;
    let info = DibBitmapInfo::read_from_slice(&invalid_reserved).unwrap();
    assert!(info.parse_color_table(DibColorUsage::RgbColors).is_err());
    assert!(
      DibColorTable::RgbQuads {
        entries: vec![RgbQuad {
          blue: 1,
          green: 2,
          red: 3,
          reserved: 4,
        }],
        trailing_data: Vec::new(),
      }
      .to_bytes()
      .is_err()
    );

    let pal_info = DibBitmapInfo {
      header: DibHeader::Info {
        base: BitmapInfoHeader {
          header_size: BITMAP_INFO_HEADER_SIZE,
          width: 2,
          height: 2,
          planes: 1,
          bit_count: BitmapBitCount::Eight.raw(),
          compression: BitmapCompression::Rgb.raw(),
          image_size: 0,
          x_pels_per_meter: 0,
          y_pels_per_meter: 0,
          color_used: 2,
          color_important: 0,
        },
        extension: Vec::new(),
      },
      color_table: vec![0x34, 0x12, 0x78, 0x56, 0xFE],
    };
    let table = pal_info
      .parse_color_table(DibColorUsage::PalColors)
      .unwrap();
    let DibColorTable::PaletteIndices {
      entries,
      trailing_data,
    } = &table
    else {
      panic!("expected palette index color table");
    };
    assert_eq!(entries, &[0x1234, 0x5678]);
    assert_eq!(trailing_data, &[0xFE]);
    assert_eq!(table.to_bytes().unwrap(), pal_info.color_table);
  }

  #[test]
  fn dib_pal_indices_has_no_color_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&BITMAP_INFO_HEADER_SIZE.to_le_bytes());
    bytes.extend_from_slice(&1i32.to_le_bytes());
    bytes.extend_from_slice(&1i32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(BitmapCompression::Rgb.raw()).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0x80]);

    let (info, bits_offset) =
      DibBitmapInfo::read_packed_prefix_from_slice(&bytes, DibColorUsage::PalIndices).unwrap();
    assert_eq!(bits_offset, BITMAP_INFO_HEADER_SIZE as usize);
    assert!(info.color_table.is_empty());

    let raw_info = DibBitmapInfo::read_from_slice(&bytes).unwrap();
    let table = raw_info
      .parse_color_table(DibColorUsage::PalIndices)
      .unwrap();
    assert_eq!(
      table,
      DibColorTable::None {
        trailing_data: vec![0x80],
      }
    );
    assert_eq!(table.to_bytes().unwrap(), [0x80]);
  }

  #[test]
  fn bitmap_headers_validate_spec_fields() {
    let base_info = [
      40, 0, 0, 0, // HeaderSize
      3, 0, 0, 0, // Width
      4, 0, 0, 0, // Height
      1, 0, // Planes
      32, 0, // BitCount
      0, 0, 0, 0, // BI_RGB
      0, 0, 0, 0, // ImageSize
      0, 0, 0, 0, // XPelsPerMeter
      0, 0, 0, 0, // YPelsPerMeter
      0, 0, 0, 0, // ColorUsed
      0, 0, 0, 0, // ColorImportant
    ];

    let mut invalid_width = base_info;
    invalid_width[4..8].copy_from_slice(&0i32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_width).is_err());

    let mut invalid_height = base_info;
    invalid_height[8..12].copy_from_slice(&0i32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_height).is_err());

    let mut invalid_planes = base_info;
    invalid_planes[12..14].copy_from_slice(&2u16.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_planes).is_err());

    let mut invalid_bit_count = base_info;
    invalid_bit_count[14..16].copy_from_slice(&3u16.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_bit_count).is_err());

    let mut invalid_compression = base_info;
    invalid_compression[16..20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_compression).is_err());

    let mut invalid_top_down_compressed = base_info;
    invalid_top_down_compressed[8..12].copy_from_slice(&(-4i32).to_le_bytes());
    invalid_top_down_compressed[16..20]
      .copy_from_slice(&(BitmapCompression::Png.raw()).to_le_bytes());
    invalid_top_down_compressed[20..24].copy_from_slice(&4u32.to_le_bytes());
    let top_down_compressed = DibBitmapInfo::read_from_slice(&invalid_top_down_compressed).unwrap();
    assert!(top_down_compressed.validate_strict().is_err());
    assert_eq!(
      top_down_compressed.to_bytes().unwrap(),
      invalid_top_down_compressed
    );

    let mut valid_rle4 = base_info;
    valid_rle4[14..16].copy_from_slice(&(BitmapBitCount::Four.raw()).to_le_bytes());
    valid_rle4[16..20].copy_from_slice(&(BitmapCompression::Rle4.raw()).to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&valid_rle4).is_ok());

    let mut invalid_rle4 = base_info;
    invalid_rle4[14..16].copy_from_slice(&(BitmapBitCount::Eight.raw()).to_le_bytes());
    invalid_rle4[16..20].copy_from_slice(&(BitmapCompression::Rle4.raw()).to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_rle4).is_err());

    let mut valid_cmyk_rle8 = base_info;
    valid_cmyk_rle8[14..16].copy_from_slice(&(BitmapBitCount::Eight.raw()).to_le_bytes());
    valid_cmyk_rle8[16..20].copy_from_slice(&(BitmapCompression::CmykRle8.raw()).to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&valid_cmyk_rle8).is_ok());

    let mut invalid_cmyk_rle8 = base_info;
    invalid_cmyk_rle8[14..16].copy_from_slice(&(BitmapBitCount::Four.raw()).to_le_bytes());
    invalid_cmyk_rle8[16..20].copy_from_slice(&(BitmapCompression::CmykRle8.raw()).to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_cmyk_rle8).is_err());

    let mut invalid_bitfields_bit_count = base_info.to_vec();
    invalid_bitfields_bit_count[14..16].copy_from_slice(&24u16.to_le_bytes());
    invalid_bitfields_bit_count[16..20]
      .copy_from_slice(&(BitmapCompression::Bitfields.raw()).to_le_bytes());
    invalid_bitfields_bit_count.extend_from_slice(&0x00FF_0000u32.to_le_bytes());
    invalid_bitfields_bit_count.extend_from_slice(&0x0000_FF00u32.to_le_bytes());
    invalid_bitfields_bit_count.extend_from_slice(&0x0000_00FFu32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&invalid_bitfields_bit_count).is_err());

    let mut bitfields_info = base_info.to_vec();
    bitfields_info[16..20].copy_from_slice(&(BitmapCompression::Bitfields.raw()).to_le_bytes());
    bitfields_info.extend_from_slice(&0x00FF_0000u32.to_le_bytes());
    bitfields_info.extend_from_slice(&0x0000_FF00u32.to_le_bytes());
    bitfields_info.extend_from_slice(&0x0000_00FFu32.to_le_bytes());
    let bitfields = DibBitmapInfo::read_from_slice(&bitfields_info).unwrap();
    assert_eq!(
      bitfields.bitfield_masks().unwrap(),
      Some([0x00FF_0000, 0x0000_FF00, 0x0000_00FF])
    );
    assert_eq!(bitfields.to_bytes().unwrap(), bitfields_info);
    let mut bitfields_with_palette = bitfields_info.clone();
    bitfields_with_palette[32..36].copy_from_slice(&2u32.to_le_bytes());
    bitfields_with_palette.extend_from_slice(&[1, 2, 3, 0]);
    bitfields_with_palette.extend_from_slice(&[4, 5, 6, 0]);
    let table = DibBitmapInfo::read_from_slice(&bitfields_with_palette)
      .unwrap()
      .parse_color_table(DibColorUsage::RgbColors)
      .unwrap();
    let DibColorTable::RgbQuads { entries, .. } = table else {
      panic!("expected RGB color table");
    };
    assert_eq!(
      entries,
      vec![
        RgbQuad {
          blue: 1,
          green: 2,
          red: 3,
          reserved: 0,
        },
        RgbQuad {
          blue: 4,
          green: 5,
          red: 6,
          reserved: 0,
        },
      ]
    );

    let mut missing_bitfield_masks = base_info.to_vec();
    missing_bitfield_masks[16..20]
      .copy_from_slice(&(BitmapCompression::Bitfields.raw()).to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&missing_bitfield_masks).is_err());

    let mut non_contiguous_info_masks = bitfields_info.clone();
    non_contiguous_info_masks[40..44].copy_from_slice(&0x00F0_00F0u32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&non_contiguous_info_masks).is_err());

    let mut overlapping_info_masks = bitfields_info.clone();
    overlapping_info_masks[44..48].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&overlapping_info_masks).is_err());

    let info = DibBitmapInfo::read_from_slice(&base_info).unwrap();
    let DibHeader::Info {
      mut base,
      extension,
    } = info.header
    else {
      panic!("expected BitmapInfoHeader");
    };
    let mut invalid_rle4_write = base.clone();
    base.planes = 2;
    assert!(
      DibBitmapInfo {
        header: DibHeader::Info { base, extension },
        color_table: Vec::new(),
      }
      .to_bytes()
      .is_err()
    );
    invalid_rle4_write.bit_count = BitmapBitCount::Eight.raw();
    invalid_rle4_write.compression = BitmapCompression::Rle4.raw();
    let invalid = DibBitmapInfo {
      header: DibHeader::Info {
        base: invalid_rle4_write.clone(),
        extension: Vec::new(),
      },
      color_table: Vec::new(),
    };
    assert!(invalid.to_bytes().is_err());
    let mut invalid_bitfields = invalid_rle4_write.clone();
    invalid_bitfields.bit_count = 24;
    invalid_bitfields.compression = BitmapCompression::Bitfields.raw();
    let invalid = DibBitmapInfo {
      header: DibHeader::Info {
        base: invalid_bitfields,
        extension: [
          0x00FF_0000u32.to_le_bytes(),
          0x0000_FF00u32.to_le_bytes(),
          0x0000_00FFu32.to_le_bytes(),
        ]
        .concat(),
      },
      color_table: Vec::new(),
    };
    assert!(invalid.to_bytes().is_err());
    let mut invalid_masks = bitfields.clone();
    invalid_masks.color_table[0..4].copy_from_slice(&0x00F0_00F0u32.to_le_bytes());
    assert!(invalid_masks.to_bytes().is_err());
  }

  #[test]
  fn bitmap_v4_and_v5_headers_parse_typed_fields() {
    let mut v4 = Vec::new();
    v4.extend_from_slice(&BITMAP_V4_HEADER_SIZE.to_le_bytes());
    v4.extend_from_slice(&1i32.to_le_bytes());
    v4.extend_from_slice(&(-2i32).to_le_bytes());
    v4.extend_from_slice(&1u16.to_le_bytes());
    v4.extend_from_slice(&32u16.to_le_bytes());
    v4.extend_from_slice(&(BitmapCompression::Bitfields.raw()).to_le_bytes());
    v4.extend_from_slice(&0u32.to_le_bytes());
    v4.extend_from_slice(&0i32.to_le_bytes());
    v4.extend_from_slice(&0i32.to_le_bytes());
    v4.extend_from_slice(&0u32.to_le_bytes());
    v4.extend_from_slice(&0u32.to_le_bytes());
    v4.extend_from_slice(&0x00FF_0000u32.to_le_bytes());
    v4.extend_from_slice(&0x0000_FF00u32.to_le_bytes());
    v4.extend_from_slice(&0x0000_00FFu32.to_le_bytes());
    v4.extend_from_slice(&0xFF00_0000u32.to_le_bytes());
    v4.extend_from_slice(&(BitmapLogicalColorSpace::SRgb.raw()).to_le_bytes());
    for value in 1i32..=9 {
      v4.extend_from_slice(&value.to_le_bytes());
    }
    v4.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    v4.extend_from_slice(&0x0002_0000u32.to_le_bytes());
    v4.extend_from_slice(&0x0003_0000u32.to_le_bytes());
    v4.extend_from_slice(&[0xAA, 0xBB]);

    let info = DibBitmapInfo::read_from_slice(&v4).unwrap();
    let DibHeader::V4(header) = &info.header else {
      panic!("expected BitmapV4Header");
    };
    assert_eq!(header.bit_count_kind(), Some(BitmapBitCount::ThirtyTwo));
    assert_eq!(
      header.compression_kind(),
      Some(BitmapCompression::Bitfields)
    );
    assert_eq!(
      header.color_space_kind(),
      Some(BitmapLogicalColorSpace::SRgb)
    );
    assert!(header.is_top_down());
    assert_eq!(header.red_mask, 0x00FF_0000);
    assert_eq!(header.endpoints.red.x, 1);
    assert_eq!(info.color_table, [0xAA, 0xBB]);
    assert_eq!(info.to_bytes().unwrap(), v4);

    let mut non_contiguous_mask = v4.clone();
    non_contiguous_mask[40..44].copy_from_slice(&0x00F0_00F0u32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&non_contiguous_mask).is_err());

    let mut overlapping_mask = v4.clone();
    overlapping_mask[44..48].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
    assert!(DibBitmapInfo::read_from_slice(&overlapping_mask).is_err());

    let mut v5 = v4[..BITMAP_V4_HEADER_SIZE as usize].to_vec();
    v5[0..4].copy_from_slice(&BITMAP_V5_HEADER_SIZE.to_le_bytes());
    let color_space_offset = 40 + 16;
    v5[color_space_offset..color_space_offset + 4].copy_from_slice(
      &BitmapLogicalColorSpaceV5::ProfileEmbedded
        .raw()
        .to_le_bytes(),
    );
    v5.extend_from_slice(&(BitmapGamutMappingIntent::Images.raw()).to_le_bytes());
    v5.extend_from_slice(&124u32.to_le_bytes());
    v5.extend_from_slice(&4u32.to_le_bytes());
    v5.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    v5.extend_from_slice(&[1, 2, 3, 4]);

    let info = DibBitmapInfo::read_from_slice(&v5).unwrap();
    let DibHeader::V5(header) = &info.header else {
      panic!("expected BitmapV5Header");
    };
    assert_eq!(
      header.color_space_v5_kind(),
      Some(BitmapLogicalColorSpaceV5::ProfileEmbedded)
    );
    assert_eq!(header.intent_kind(), Some(BitmapGamutMappingIntent::Images));
    assert_eq!(header.profile_data, 124);
    assert_eq!(header.profile_size, 4);
    assert_eq!(info.color_table, [1, 2, 3, 4]);
    assert_eq!(info.to_bytes().unwrap(), v5);
  }

  #[test]
  fn device_independent_bitmap_detects_embedded_png() {
    let bitmap_info = [
      40, 0, 0, 0, // HeaderSize
      5, 0, 0, 0, // Width
      6, 0, 0, 0, // Height
      1, 0, // Planes
      0, 0, // BitCount
      5, 0, 0, 0, // BI_PNG
      4, 0, 0, 0, // ImageSize
      0, 0, 0, 0, // XPelsPerMeter
      0, 0, 0, 0, // YPelsPerMeter
      0, 0, 0, 0, // ColorUsed
      0, 0, 0, 0, // ColorImportant
    ];
    let bitmap_bits = [0x89, b'P', b'N', b'G'];

    let dib = DeviceIndependentBitmap::from_parts(&bitmap_info, &bitmap_bits).unwrap();
    assert_eq!(dib.embedded_format(), Some(EmbeddedBitmapFormat::Png));
    assert_eq!(dib.embedded_format().unwrap().content_type(), "image/png");
    let packed = dib.to_packed_bytes().unwrap();
    assert_eq!(&packed[..bitmap_info.len()], bitmap_info);
    assert_eq!(&packed[bitmap_info.len()..], bitmap_bits);

    let mut invalid_size = bitmap_info;
    invalid_size[20..24].copy_from_slice(&5u32.to_le_bytes());
    let mismatched_size = DeviceIndependentBitmap::from_parts(&invalid_size, &bitmap_bits).unwrap();
    assert!(mismatched_size.validate_strict().is_err());
    assert_eq!(
      mismatched_size.to_packed_bytes().unwrap(),
      [invalid_size.as_slice(), &bitmap_bits].concat()
    );

    let mut invalid_dib = dib;
    let DibHeader::Info { base, .. } = &mut invalid_dib.info.header else {
      panic!("expected BitmapInfoHeader");
    };
    base.image_size = 5;
    assert!(invalid_dib.to_packed_bytes().is_ok());
    assert!(invalid_dib.validate_strict().is_err());
  }
}
