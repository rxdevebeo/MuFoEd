use std::borrow::Cow;

use encoding_rs::{Encoding, UTF_8, UTF_16LE, WINDOWS_1252};

use crate::common::{Error, Reader, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdkEncoding {
  Utf8,
  Utf16Le,
  UnicodeLowByte,
  Windows1252,
  CodePage(u16),
  WmfCharset(u8),
}

impl SdkEncoding {
  pub fn label(self) -> String {
    match self {
      Self::Utf8 => "utf-8".to_string(),
      Self::Utf16Le => "utf-16le".to_string(),
      Self::UnicodeLowByte => "unicode-low-byte".to_string(),
      Self::Windows1252 => "windows-1252".to_string(),
      Self::CodePage(code_page) => format!("cp{code_page}"),
      Self::WmfCharset(charset) => format!("wmf-charset-{charset}"),
    }
  }

  fn encoding_rs(self) -> Result<&'static Encoding> {
    match self {
      Self::Utf8 => Ok(UTF_8),
      Self::Utf16Le => Ok(UTF_16LE),
      Self::UnicodeLowByte => Err(Error::encoding(
        self.label(),
        "Unicode low-byte strings use the SDK's direct byte mapping",
      )),
      Self::Windows1252 => Ok(WINDOWS_1252),
      Self::CodePage(code_page) => code_page_encoding(code_page).ok_or_else(|| {
        Error::encoding(
          self.label(),
          "code page is not supported by the current encoder table",
        )
      }),
      Self::WmfCharset(charset) => {
        let code_page = wmf_charset_code_page(charset).ok_or_else(|| {
          Error::encoding(
            self.label(),
            "WMF charset does not have a code page mapping",
          )
        })?;
        code_page_encoding(code_page).ok_or_else(|| {
          Error::encoding(
            self.label(),
            "WMF charset code page is not supported by the current encoder table",
          )
        })
      }
    }
  }

  pub fn decode(self, bytes: &[u8]) -> Result<String> {
    if self == Self::UnicodeLowByte {
      return Ok(bytes.iter().map(|value| char::from(*value)).collect());
    }
    let (text, _, had_errors) = self.encoding_rs()?.decode(bytes);
    if had_errors {
      return Err(Error::encoding(
        self.label(),
        "input bytes are not valid for this encoding",
      ));
    }
    Ok(text.into_owned())
  }

  pub fn encode(self, value: &str) -> Result<Vec<u8>> {
    if self == Self::UnicodeLowByte {
      return value
        .chars()
        .map(|character| {
          u8::try_from(u32::from(character))
            .map_err(|_| Error::encoding(self.label(), "string contains a character above U+00FF"))
        })
        .collect();
    }
    let (bytes, _, had_errors) = self.encoding_rs()?.encode(value);
    if had_errors {
      return Err(Error::encoding(
        self.label(),
        "string contains characters that cannot be represented in this encoding",
      ));
    }
    Ok(bytes.into_owned())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SdkString {
  Raw {
    bytes: Vec<u8>,
    encoding: SdkEncoding,
  },
  Text {
    value: String,
    encoding: SdkEncoding,
  },
}

impl SdkString {
  pub fn raw(bytes: Vec<u8>, encoding: SdkEncoding) -> Self {
    Self::Raw { bytes, encoding }
  }

  pub fn text(value: impl Into<String>, encoding: SdkEncoding) -> Self {
    Self::Text {
      value: value.into(),
      encoding,
    }
  }

  pub fn read_bytes<R: std::io::Read + std::io::Seek>(
    reader: &mut Reader<R>,
    len: usize,
    encoding: SdkEncoding,
  ) -> Result<Self> {
    Ok(Self::raw(reader.read_vec(len)?, encoding))
  }

  pub fn encoding(&self) -> SdkEncoding {
    match self {
      Self::Raw { encoding, .. } | Self::Text { encoding, .. } => *encoding,
    }
  }

  pub fn raw_bytes(&self) -> Option<&[u8]> {
    match self {
      Self::Raw { bytes, .. } => Some(bytes),
      Self::Text { .. } => None,
    }
  }

  pub fn as_str(&self) -> Result<Cow<'_, str>> {
    match self {
      Self::Raw { bytes, encoding } => Ok(Cow::Owned(encoding.decode(bytes)?)),
      Self::Text { value, .. } => Ok(Cow::Borrowed(value)),
    }
  }

  pub fn to_mut_string(&mut self) -> Result<&mut String> {
    if let Self::Raw { bytes, encoding } = self {
      let value = encoding.decode(bytes)?;
      *self = Self::Text {
        value,
        encoding: *encoding,
      };
    }

    match self {
      Self::Text { value, .. } => Ok(value),
      Self::Raw { .. } => unreachable!("raw string is converted above"),
    }
  }

  pub fn set(&mut self, value: impl Into<String>) {
    let encoding = self.encoding();
    *self = Self::Text {
      value: value.into(),
      encoding,
    };
  }

  pub fn encoded_bytes(&self) -> Result<Cow<'_, [u8]>> {
    match self {
      Self::Raw { bytes, .. } => Ok(Cow::Borrowed(bytes)),
      Self::Text { value, encoding } => Ok(Cow::Owned(encoding.encode(value)?)),
    }
  }

  pub fn into_text(self) -> Result<String> {
    match self {
      Self::Raw { bytes, encoding } => encoding.decode(&bytes),
      Self::Text { value, .. } => Ok(value),
    }
  }
}

fn code_page_encoding(code_page: u16) -> Option<&'static Encoding> {
  match code_page {
    65001 => Some(UTF_8),
    1200 => Some(UTF_16LE),
    874 => Encoding::for_label(b"windows-874"),
    932 => Encoding::for_label(b"shift_jis"),
    936 => Encoding::for_label(b"gbk"),
    949 => Encoding::for_label(b"euc-kr"),
    950 => Encoding::for_label(b"big5"),
    1250 => Encoding::for_label(b"windows-1250"),
    1251 => Encoding::for_label(b"windows-1251"),
    1252 => Some(WINDOWS_1252),
    1253 => Encoding::for_label(b"windows-1253"),
    1254 => Encoding::for_label(b"windows-1254"),
    1255 => Encoding::for_label(b"windows-1255"),
    1256 => Encoding::for_label(b"windows-1256"),
    1257 => Encoding::for_label(b"windows-1257"),
    1258 => Encoding::for_label(b"windows-1258"),
    _ => None,
  }
}

fn wmf_charset_code_page(charset: u8) -> Option<u16> {
  match charset {
    0..=2 => Some(1252),
    77 => Some(10000),
    128 => Some(932),
    129 | 130 => Some(949),
    134 => Some(936),
    136 => Some(950),
    161 => Some(1253),
    162 => Some(1254),
    163 => Some(1258),
    177 => Some(1255),
    178 => Some(1256),
    186 => Some(1257),
    204 => Some(1251),
    222 => Some(874),
    238 => Some(1250),
    255 => Some(1252),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn raw_string_writes_original_bytes_until_modified() {
    let mut value = SdkString::raw(vec![0xE9], SdkEncoding::Windows1252);
    assert_eq!(value.as_str().unwrap(), "é");
    assert_eq!(value.encoded_bytes().unwrap().as_ref(), &[0xE9]);

    value.set("test");
    assert_eq!(value.encoded_bytes().unwrap().as_ref(), b"test");
  }

  #[test]
  fn to_mut_string_converts_raw_to_text() {
    let mut value = SdkString::raw(b"abc".to_vec(), SdkEncoding::Utf8);
    value.to_mut_string().unwrap().push('d');
    assert_eq!(value.encoded_bytes().unwrap().as_ref(), b"abcd");
  }

  #[test]
  fn unicode_low_byte_does_not_apply_windows_1252_remapping() {
    assert_eq!(
      SdkEncoding::UnicodeLowByte.decode(&[0x80]).unwrap(),
      "\u{80}"
    );
    assert_eq!(
      SdkEncoding::UnicodeLowByte.encode("\u{80}\u{ff}").unwrap(),
      [0x80, 0xFF]
    );
    assert!(SdkEncoding::UnicodeLowByte.encode("\u{100}").is_err());
  }
}
