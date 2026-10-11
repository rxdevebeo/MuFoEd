use std::io::{Read, Seek, SeekFrom, Write};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),
  #[error("invalid metafile data at offset {offset}: {message}")]
  InvalidData { offset: u64, message: String },
  #[error("unsupported metafile format")]
  UnsupportedFormat,
  #[error("string encoding error for {encoding}: {message}")]
  Encoding { encoding: String, message: String },
}

impl Error {
  pub fn invalid(offset: u64, message: impl Into<String>) -> Self {
    Self::InvalidData {
      offset,
      message: message.into(),
    }
  }

  pub fn encoding(encoding: impl Into<String>, message: impl Into<String>) -> Self {
    Self::Encoding {
      encoding: encoding.into(),
      message: message.into(),
    }
  }

  pub const fn offset(&self) -> Option<u64> {
    match self {
      Self::InvalidData { offset, .. } => Some(*offset),
      Self::Io(_) | Self::UnsupportedFormat | Self::Encoding { .. } => None,
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
  Emf,
  Wmf,
}

pub trait SdkRead: Sized {
  fn read_from<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Self>;
}

pub trait SdkWrite {
  fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> Result<()>;
}

pub trait SdkSize {
  fn sdk_size(&self) -> u64;
}

pub trait SdkEnumValue: Copy + Sized {
  type Repr: Copy + PartialEq;

  fn from_raw(value: Self::Repr) -> Option<Self>;

  fn raw(self) -> Self::Repr;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownRecord {
  pub record_type: u32,
  pub data: Vec<u8>,
}

pub struct Reader<R> {
  inner: R,
}

impl<R: Read + Seek> Reader<R> {
  pub fn new(inner: R) -> Self {
    Self { inner }
  }

  pub fn into_inner(self) -> R {
    self.inner
  }

  pub fn position(&mut self) -> Result<u64> {
    Ok(self.inner.stream_position()?)
  }

  pub fn seek(&mut self, position: u64) -> Result<()> {
    self.inner.seek(SeekFrom::Start(position))?;
    Ok(())
  }

  pub fn skip(&mut self, len: u64) -> Result<()> {
    let current = self.position()?;
    let position = current
      .checked_add(len)
      .ok_or_else(|| Error::invalid(current, "reader position overflows"))?;
    self.seek(position)
  }

  pub fn read_u8(&mut self) -> Result<u8> {
    let mut buf = [0; 1];
    self.inner.read_exact(&mut buf)?;
    Ok(buf[0])
  }

  pub fn read_i8(&mut self) -> Result<i8> {
    Ok(self.read_u8()? as i8)
  }

  pub fn read_u16(&mut self) -> Result<u16> {
    let mut buf = [0; 2];
    self.inner.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
  }

  pub fn read_i16(&mut self) -> Result<i16> {
    Ok(self.read_u16()? as i16)
  }

  pub fn read_u32(&mut self) -> Result<u32> {
    let mut buf = [0; 4];
    self.inner.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
  }

  pub fn read_i32(&mut self) -> Result<i32> {
    Ok(self.read_u32()? as i32)
  }

  pub fn read_u64(&mut self) -> Result<u64> {
    let mut buf = [0; 8];
    self.inner.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
  }

  pub fn read_i64(&mut self) -> Result<i64> {
    Ok(self.read_u64()? as i64)
  }

  pub fn read_f32(&mut self) -> Result<f32> {
    Ok(f32::from_bits(self.read_u32()?))
  }

  pub fn read_f64(&mut self) -> Result<f64> {
    Ok(f64::from_bits(self.read_u64()?))
  }

  pub fn read_vec(&mut self, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0; len];
    self.inner.read_exact(&mut buf)?;
    Ok(buf)
  }

  pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
    let mut buf = [0; N];
    self.inner.read_exact(&mut buf)?;
    Ok(buf)
  }
}

pub struct Writer<W> {
  inner: W,
  position: u64,
}

impl<W: Write> Writer<W> {
  /// Wraps a sink at logical position zero.
  pub fn new(inner: W) -> Self {
    Self { inner, position: 0 }
  }

  /// Wraps a sink whose next byte has the supplied logical position.
  pub fn with_position(inner: W, position: u64) -> Self {
    Self { inner, position }
  }

  pub fn into_inner(self) -> W {
    self.inner
  }

  pub const fn position(&self) -> Result<u64> {
    Ok(self.position)
  }

  pub fn write_u8(&mut self, value: u8) -> Result<()> {
    self.write_all(&[value])
  }

  pub fn write_i8(&mut self, value: i8) -> Result<()> {
    self.write_u8(value as u8)
  }

  pub fn write_u16(&mut self, value: u16) -> Result<()> {
    self.write_all(&value.to_le_bytes())
  }

  pub fn write_i16(&mut self, value: i16) -> Result<()> {
    self.write_all(&value.to_le_bytes())
  }

  pub fn write_u32(&mut self, value: u32) -> Result<()> {
    self.write_all(&value.to_le_bytes())
  }

  pub fn write_i32(&mut self, value: i32) -> Result<()> {
    self.write_all(&value.to_le_bytes())
  }

  pub fn write_u64(&mut self, value: u64) -> Result<()> {
    self.write_all(&value.to_le_bytes())
  }

  pub fn write_i64(&mut self, value: i64) -> Result<()> {
    self.write_all(&value.to_le_bytes())
  }

  pub fn write_f32(&mut self, value: f32) -> Result<()> {
    self.write_u32(value.to_bits())
  }

  pub fn write_f64(&mut self, value: f64) -> Result<()> {
    self.write_u64(value.to_bits())
  }

  pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
    Write::write_all(self, bytes)?;
    Ok(())
  }
}

impl<W: Write> Write for Writer<W> {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    let requested = u64::try_from(bytes.len())
      .map_err(|_| std::io::Error::other("writer request length does not fit u64"))?;
    self
      .position
      .checked_add(requested)
      .ok_or_else(|| std::io::Error::other("writer position overflows u64"))?;
    let written = self.inner.write(bytes)?;
    self.position += written as u64;
    Ok(written)
  }

  fn flush(&mut self) -> std::io::Result<()> {
    self.inner.flush()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Default)]
  struct ChunkWriter {
    bytes: Vec<u8>,
    max_chunk: usize,
    fail_after: Option<usize>,
  }

  impl Write for ChunkWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
      if self
        .fail_after
        .is_some_and(|limit| self.bytes.len() >= limit)
      {
        return Err(std::io::Error::other("injected write failure"));
      }
      let remaining = self
        .fail_after
        .map_or(bytes.len(), |limit| limit.saturating_sub(self.bytes.len()));
      let written = bytes.len().min(self.max_chunk).min(remaining);
      if written == 0 {
        return Err(std::io::ErrorKind::WriteZero.into());
      }
      self.bytes.extend_from_slice(&bytes[..written]);
      Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
      Ok(())
    }
  }

  #[test]
  fn writer_tracks_position_without_a_seekable_sink() {
    let mut writer = Writer::new(Vec::new());
    writer.write_u16(0x1234).unwrap();
    writer.write_all(&[5, 6, 7]).unwrap();
    assert_eq!(writer.position().unwrap(), 5);
    assert_eq!(writer.into_inner(), [0x34, 0x12, 5, 6, 7]);
  }

  #[test]
  fn writer_tracks_partial_writes_and_nonzero_start() {
    let sink = ChunkWriter {
      max_chunk: 2,
      ..ChunkWriter::default()
    };
    let mut writer = Writer::with_position(sink, 11);
    writer.write_all(&[1, 2, 3, 4, 5]).unwrap();
    assert_eq!(writer.position().unwrap(), 16);
    assert_eq!(writer.into_inner().bytes, [1, 2, 3, 4, 5]);
  }

  #[test]
  fn writer_position_stops_at_the_last_successful_partial_write() {
    let sink = ChunkWriter {
      max_chunk: 2,
      fail_after: Some(3),
      ..ChunkWriter::default()
    };
    let mut writer = Writer::new(sink);
    assert!(writer.write_all(&[1, 2, 3, 4, 5]).is_err());
    assert_eq!(writer.position().unwrap(), 3);
    assert_eq!(writer.into_inner().bytes, [1, 2, 3]);
  }
}
