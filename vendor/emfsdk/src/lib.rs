//! Typed, byte-preserving EMF, EMF+, and WMF parsing and writing.

#![forbid(unsafe_code)]

extern crate self as emfsdk;

pub mod bitmap;
pub mod common;
pub mod emf;
pub mod emfplus;
#[cfg(feature = "render")]
pub mod render;
pub mod string;
pub mod types;
pub mod wmf;

pub use emfsdk_derive::{SdkEnum, SdkObject};

pub use crate::bitmap::{
  BITMAP_CORE_HEADER_SIZE, BITMAP_INFO_HEADER_SIZE, BITMAP_V4_HEADER_SIZE, BITMAP_V5_HEADER_SIZE,
  BitmapBitCount, BitmapCompression, BitmapCoreHeader, BitmapInfoHeader, DeviceIndependentBitmap,
  DibBitmapInfo, DibColorTable, DibColorUsage, DibHeader, EmbeddedBitmapFormat, RgbQuad,
};
pub use crate::common::{
  Error, Format, Reader, Result, SdkEnumValue, SdkRead, SdkSize, SdkWrite, UnknownRecord, Writer,
};
pub use crate::emf::{
  BitmapSourceBounds, EMR_EOF, EMR_HEADER, EmfHeader, EmfMetafile, EmfMetafileRef, EmfRecord,
  EmfRecordData, EmfRecordRef, EmfRecordType, EmfRecords, EmrBitmapBuffer, EmrComment,
  EmrCreateBrushIndirect, EmrCreateDibPatternBrushPt, EmrCreateMonoBrush, EmrCreatePen,
  EmrDeleteObject, EmrEllipse, EmrExcludeClipRect, EmrExtCreateFontIndirectW, EmrExtCreatePen,
  EmrExtTextOut, EmrIntersectClipRect, EmrLineTo, EmrModifyWorldTransform, EmrMoveToEx,
  EmrPolyPointsL, EmrPolyPointsS, EmrPolyPolygonL, EmrPolyPolygonS, EmrRectangle, EmrSelectObject,
  EmrSetBkColor, EmrSetBrushOrgEx, EmrSetDiBitsToDevice, EmrSetTextColor, EmrSetViewportExtEx,
  EmrSetViewportOrgEx, EmrSetWindowExtEx, EmrSetWindowOrgEx, EmrSetWorldTransform,
  EmrStretchDiBits, EmrText, ExtTextOutOptions, LogFontW,
};
pub use crate::emfplus::{
  EmfPlusBrushRef, EmfPlusDrawRectsData, EmfPlusFillRectsData, EmfPlusGraphicsVersion,
  EmfPlusGraphicsVersionValue, EmfPlusHeaderData, EmfPlusRecord, EmfPlusRecordData,
  EmfPlusRecordFlags, EmfPlusRecordRef, EmfPlusRecordType, EmfPlusRecords, EmfPlusRect,
  EmfPlusRectS, EmfPlusScaleWorldTransformData, EmfPlusStream, EmfPlusStreamRef,
  EmfPlusTranslateWorldTransformData,
};
pub use crate::string::{SdkEncoding, SdkString};
pub use crate::types::{
  ColorRef, EmfPlusArgb, PointF, PointL, PointS, RectF, RectL, RectS, SizeF, SizeL, SizeS,
  TriVertex, XForm,
};
pub use crate::wmf::{
  META_EOF, WmfHeader, WmfMetafile, WmfMetafileRef, WmfPlaceableHeader, WmfRecord, WmfRecordData,
  WmfRecordFunction, WmfRecordRef, WmfRecords,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetafileRef<'a> {
  Emf(EmfMetafileRef<'a>),
  Wmf(WmfMetafileRef<'a>),
}

impl<'a> MetafileRef<'a> {
  pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
    match detect_format(bytes) {
      Some(Format::Emf) => Ok(Self::Emf(EmfMetafileRef::from_bytes(bytes)?)),
      Some(Format::Wmf) => Ok(Self::Wmf(WmfMetafileRef::from_bytes(bytes)?)),
      None => Err(Error::UnsupportedFormat),
    }
  }

  pub fn format(&self) -> Format {
    match self {
      Self::Emf(_) => Format::Emf,
      Self::Wmf(_) => Format::Wmf,
    }
  }

  pub fn into_owned(self) -> Metafile {
    match self {
      Self::Emf(value) => Metafile::Emf(value.into_owned()),
      Self::Wmf(value) => Metafile::Wmf(value.into_owned()),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Metafile {
  Emf(EmfMetafile),
  Wmf(WmfMetafile),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityDiagnostic {
  pub format: Format,
  pub record_index: Option<usize>,
  pub nested_record_index: Option<usize>,
  pub message: String,
}

impl Metafile {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    match detect_format(bytes) {
      Some(Format::Emf) => Ok(Self::Emf(EmfMetafile::from_bytes(bytes)?)),
      Some(Format::Wmf) => Ok(Self::Wmf(WmfMetafile::from_bytes(bytes)?)),
      None => Err(Error::UnsupportedFormat),
    }
  }

  pub fn from_bytes_strict(bytes: &[u8]) -> Result<Self> {
    let metafile = Self::from_bytes(bytes)?;
    metafile.validate_strict()?;
    Ok(metafile)
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    match self {
      Self::Emf(value) => value.to_bytes(),
      Self::Wmf(value) => value.to_bytes(),
    }
  }

  pub fn write_to<W: std::io::Write>(&self, writer: W) -> Result<()> {
    match self {
      Self::Emf(value) => value.write_to(writer),
      Self::Wmf(value) => value.write_to(writer),
    }
  }

  pub fn format(&self) -> Format {
    match self {
      Self::Emf(_) => Format::Emf,
      Self::Wmf(_) => Format::Wmf,
    }
  }

  pub fn validate_strict(&self) -> Result<()> {
    match self {
      Self::Emf(value) => validate_emf_strict(value),
      Self::Wmf(value) => validate_wmf_strict(value),
    }
  }

  pub fn compatibility_diagnostics(&self) -> Vec<CompatibilityDiagnostic> {
    match self {
      Self::Emf(value) => emf_compatibility_diagnostics(value),
      Self::Wmf(value) => wmf_compatibility_diagnostics(value),
    }
  }
}

impl SdkWrite for Metafile {
  fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
    match self {
      Self::Emf(value) => SdkWrite::write_to(value, writer),
      Self::Wmf(value) => SdkWrite::write_to(value, writer),
    }
  }
}

fn validate_emf_strict(value: &EmfMetafile) -> Result<()> {
  for (record_index, record) in value.records.iter().enumerate() {
    let data = record.parse_data().map_err(|error| {
      Error::invalid(
        0,
        format!(
          "EMF record {record_index} is nonconforming (type {}): {error}",
          record.record_type
        ),
      )
    })?;
    data.validate_strict().map_err(|error| {
      Error::invalid(
        0,
        format!(
          "EMF record {record_index} is nonconforming (type {}): {error}",
          record.record_type
        ),
      )
    })?;
    if let EmfRecordData::Comment(EmrComment::EmfPlus { records, .. }) = &data {
      for (nested_record_index, record) in records.iter().enumerate() {
        let data = record.parse_data().map_err(|error| {
          Error::invalid(
            0,
            format!(
              "EMF+ record {nested_record_index} in EMF record {record_index} is nonconforming: {error}"
            ),
          )
        })?;
        data.validate_strict().map_err(|error| {
          Error::invalid(
            0,
            format!(
              "EMF+ record {nested_record_index} in EMF record {record_index} is nonconforming: {error}"
            ),
          )
        })?;
        let flags = EmfPlusRecordFlags::from_bits_retain(record.flags);
        let mut rebuilt = EmfPlusRecord::from_data(&data, flags).map_err(|error| {
          Error::invalid(
            0,
            format!(
              "EMF+ record {nested_record_index} in EMF record {record_index} cannot be rebuilt: {error}"
            ),
          )
        })?;
        rebuilt.padding = record.padding.clone();
        if rebuilt != *record {
          return Err(Error::invalid(
            0,
            format!(
              "EMF+ record {nested_record_index} in EMF record {record_index} differs after typed parse/write"
            ),
          ));
        }
      }
    }
    match data.to_record() {
      Ok(rebuilt) if rebuilt == *record => {}
      Ok(_) => {
        return Err(Error::invalid(
          0,
          format!(
            "EMF record {record_index} differs after typed parse/write (type {})",
            record.record_type
          ),
        ));
      }
      Err(error) => {
        return Err(Error::invalid(
          0,
          format!(
            "EMF record {record_index} cannot be rebuilt (type {}): {error}",
            record.record_type
          ),
        ));
      }
    }
  }
  value
    .validate_header_metrics()
    .map_err(|error| Error::invalid(0, error.to_string()))?;
  if !value.trailing_data.is_empty() {
    return Err(Error::invalid(
      0,
      format!(
        "EMF contains {} bytes after EMR_EOF",
        value.trailing_data.len()
      ),
    ));
  }
  Ok(())
}

fn validate_wmf_strict(value: &WmfMetafile) -> Result<()> {
  if let Some(header) = &value.placeable_header {
    header
      .validate()
      .map_err(|error| Error::invalid(0, error.to_string()))?;
  }
  for (record_index, record) in value.records.iter().enumerate() {
    let data = record.parse_data().map_err(|error| {
      Error::invalid(
        0,
        format!(
          "WMF record {record_index} is nonconforming (function {}): {error}",
          record.function
        ),
      )
    })?;
    data.validate_strict().map_err(|error| {
      Error::invalid(
        0,
        format!(
          "WMF record {record_index} is nonconforming (function {}): {error}",
          record.function
        ),
      )
    })?;
    match data.to_record_with_function(record.function) {
      Ok(rebuilt) if rebuilt == *record => {}
      Ok(_) => {
        return Err(Error::invalid(
          0,
          format!(
            "WMF record {record_index} differs after typed parse/write (function {})",
            record.function
          ),
        ));
      }
      Err(error) => {
        return Err(Error::invalid(
          0,
          format!(
            "WMF record {record_index} cannot be rebuilt (function {}): {error}",
            record.function
          ),
        ));
      }
    }
  }
  value
    .validate_header_metrics()
    .map_err(|error| Error::invalid(0, error.to_string()))?;
  if !value.trailing_data.is_empty() {
    return Err(Error::invalid(
      0,
      format!(
        "WMF contains {} bytes after META_EOF",
        value.trailing_data.len()
      ),
    ));
  }
  Ok(())
}

fn emf_compatibility_diagnostics(value: &EmfMetafile) -> Vec<CompatibilityDiagnostic> {
  let mut diagnostics = Vec::new();
  for (record_index, record) in value.records.iter().enumerate() {
    match record.parse_data() {
      Ok(data) => {
        if let Err(error) = data.validate_strict() {
          diagnostics.push(CompatibilityDiagnostic {
            format: Format::Emf,
            record_index: Some(record_index),
            nested_record_index: None,
            message: format!(
              "EMF record {record_index} is nonconforming (type {}): {error}",
              record.record_type
            ),
          });
        }
        if let EmfRecordData::Comment(EmrComment::EmfPlus { records, .. }) = &data {
          for (nested_record_index, record) in records.iter().enumerate() {
            match record.parse_data() {
              Ok(data) => {
                if let Err(error) = data.validate_strict() {
                  diagnostics.push(CompatibilityDiagnostic {
                    format: Format::Emf,
                    record_index: Some(record_index),
                    nested_record_index: Some(nested_record_index),
                    message: format!(
                      "EMF+ record {nested_record_index} in EMF record {record_index} is nonconforming: {error}"
                    ),
                  });
                }
                let flags = EmfPlusRecordFlags::from_bits_retain(record.flags);
                match EmfPlusRecord::from_data(&data, flags) {
                  Ok(mut rebuilt) => {
                    rebuilt.padding = record.padding.clone();
                    if rebuilt != *record {
                      diagnostics.push(CompatibilityDiagnostic {
                        format: Format::Emf,
                        record_index: Some(record_index),
                        nested_record_index: Some(nested_record_index),
                        message: format!(
                          "EMF+ record {nested_record_index} in EMF record {record_index} differs after typed parse/write"
                        ),
                      });
                    }
                  }
                  Err(error) => diagnostics.push(CompatibilityDiagnostic {
                    format: Format::Emf,
                    record_index: Some(record_index),
                    nested_record_index: Some(nested_record_index),
                    message: format!(
                      "EMF+ record {nested_record_index} in EMF record {record_index} cannot be rebuilt: {error}"
                    ),
                  }),
                }
              }
              Err(error) => diagnostics.push(CompatibilityDiagnostic {
                format: Format::Emf,
                record_index: Some(record_index),
                nested_record_index: Some(nested_record_index),
                message: format!(
                  "EMF+ record {nested_record_index} in EMF record {record_index} is nonconforming: {error}"
                ),
              }),
            }
          }
        }
        match data.to_record() {
          Ok(rebuilt) if rebuilt == *record => {}
          Ok(_) => diagnostics.push(CompatibilityDiagnostic {
            format: Format::Emf,
            record_index: Some(record_index),
            nested_record_index: None,
            message: format!(
              "EMF record {record_index} differs after typed parse/write (type {})",
              record.record_type
            ),
          }),
          Err(error) => diagnostics.push(CompatibilityDiagnostic {
            format: Format::Emf,
            record_index: Some(record_index),
            nested_record_index: None,
            message: format!(
              "EMF record {record_index} cannot be rebuilt (type {}): {error}",
              record.record_type
            ),
          }),
        }
      }
      Err(error) => diagnostics.push(CompatibilityDiagnostic {
        format: Format::Emf,
        record_index: Some(record_index),
        nested_record_index: None,
        message: format!(
          "EMF record {record_index} is nonconforming (type {}): {error}",
          record.record_type
        ),
      }),
    }
  }
  push_unique_metafile_diagnostic(
    &mut diagnostics,
    Format::Emf,
    value.validate_header_metrics(),
  );
  if !value.trailing_data.is_empty() {
    diagnostics.push(CompatibilityDiagnostic {
      format: Format::Emf,
      record_index: None,
      nested_record_index: None,
      message: format!(
        "EMF contains {} bytes after EMR_EOF",
        value.trailing_data.len()
      ),
    });
  }
  diagnostics
}

fn wmf_compatibility_diagnostics(value: &WmfMetafile) -> Vec<CompatibilityDiagnostic> {
  let mut diagnostics = Vec::new();
  if let Some(header) = &value.placeable_header {
    push_unique_metafile_diagnostic(&mut diagnostics, Format::Wmf, header.validate());
  }
  for (record_index, record) in value.records.iter().enumerate() {
    match record.parse_data() {
      Ok(data) => {
        if let Err(error) = data.validate_strict() {
          diagnostics.push(CompatibilityDiagnostic {
            format: Format::Wmf,
            record_index: Some(record_index),
            nested_record_index: None,
            message: format!(
              "WMF record {record_index} is nonconforming (function {}): {error}",
              record.function
            ),
          });
        }
        match data.to_record_with_function(record.function) {
          Ok(rebuilt) if rebuilt == *record => {}
          Ok(_) => diagnostics.push(CompatibilityDiagnostic {
            format: Format::Wmf,
            record_index: Some(record_index),
            nested_record_index: None,
            message: format!(
              "WMF record {record_index} differs after typed parse/write (function {})",
              record.function
            ),
          }),
          Err(error) => diagnostics.push(CompatibilityDiagnostic {
            format: Format::Wmf,
            record_index: Some(record_index),
            nested_record_index: None,
            message: format!(
              "WMF record {record_index} cannot be rebuilt (function {}): {error}",
              record.function
            ),
          }),
        }
      }
      Err(error) => diagnostics.push(CompatibilityDiagnostic {
        format: Format::Wmf,
        record_index: Some(record_index),
        nested_record_index: None,
        message: format!(
          "WMF record {record_index} is nonconforming (function {}): {error}",
          record.function
        ),
      }),
    }
  }
  push_unique_metafile_diagnostic(
    &mut diagnostics,
    Format::Wmf,
    value.validate_header_metrics(),
  );
  if !value.trailing_data.is_empty() {
    diagnostics.push(CompatibilityDiagnostic {
      format: Format::Wmf,
      record_index: None,
      nested_record_index: None,
      message: format!(
        "WMF contains {} bytes after META_EOF",
        value.trailing_data.len()
      ),
    });
  }
  diagnostics
}

fn push_unique_metafile_diagnostic(
  diagnostics: &mut Vec<CompatibilityDiagnostic>,
  format: Format,
  result: Result<()>,
) {
  if let Err(error) = result {
    let message = error.to_string();
    if !diagnostics
      .iter()
      .any(|diagnostic| diagnostic.message.ends_with(&message))
    {
      diagnostics.push(CompatibilityDiagnostic {
        format,
        record_index: None,
        nested_record_index: None,
        message,
      });
    }
  }
}

pub fn detect_format(bytes: &[u8]) -> Option<Format> {
  if emf::looks_like_emf(bytes) {
    Some(Format::Emf)
  } else if wmf::looks_like_wmf(bytes) {
    Some(Format::Wmf)
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn minimal_wmf() -> Vec<u8> {
    [
      1u16.to_le_bytes().as_slice(),
      9u16.to_le_bytes().as_slice(),
      0x0300u16.to_le_bytes().as_slice(),
      12u32.to_le_bytes().as_slice(),
      0u16.to_le_bytes().as_slice(),
      3u32.to_le_bytes().as_slice(),
      0u16.to_le_bytes().as_slice(),
      3u32.to_le_bytes().as_slice(),
      META_EOF.to_le_bytes().as_slice(),
    ]
    .concat()
  }

  #[test]
  fn default_parse_preserves_compatibility_and_strict_parse_rejects_it() {
    let mut bytes = minimal_wmf();
    bytes.extend_from_slice(&[0xAA, 0xBB]);

    let metafile = Metafile::from_bytes(&bytes).unwrap();
    assert_eq!(metafile.to_bytes().unwrap(), bytes);
    assert_eq!(metafile.compatibility_diagnostics().len(), 1);
    assert!(metafile.validate_strict().is_err());
    assert!(Metafile::from_bytes_strict(&bytes).is_err());
    assert!(Metafile::from_bytes_strict(&minimal_wmf()).is_ok());
  }

  #[test]
  fn metafile_ref_requires_explicit_owned_materialization() {
    let bytes = minimal_wmf();
    let view = MetafileRef::from_bytes(&bytes).unwrap();
    assert_eq!(view.format(), Format::Wmf);
    let owned = view.into_owned();
    assert_eq!(owned.to_bytes().unwrap(), bytes);

    let mut output = Vec::new();
    owned.write_to(&mut output).unwrap();
    assert_eq!(output, bytes);
  }
}
