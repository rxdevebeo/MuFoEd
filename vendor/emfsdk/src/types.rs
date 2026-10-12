use emfsdk_derive::SdkObject;

use crate::common::{Error, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "shared")]
pub struct PointL {
  pub x: i32,
  pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "shared")]
pub struct PointS {
  pub x: i16,
  pub y: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "shared")]
pub struct SizeL {
  pub cx: i32,
  pub cy: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "shared")]
pub struct SizeS {
  pub cx: i16,
  pub cy: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "shared")]
pub struct RectL {
  pub left: i32,
  pub top: i32,
  pub right: i32,
  pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "shared")]
pub struct RectS {
  pub left: i16,
  pub top: i16,
  pub right: i16,
  pub bottom: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
pub struct ColorRef {
  pub red: u8,
  pub green: u8,
  pub blue: u8,
  pub reserved: u8,
}

impl ColorRef {
  pub const fn is_reserved_zero(self) -> bool {
    self.reserved == 0
  }

  pub fn validate_strict(self) -> Result<()> {
    validate_color_ref(&self)
  }
}

fn validate_color_ref(value: &ColorRef) -> Result<()> {
  if value.is_reserved_zero() {
    Ok(())
  } else {
    Err(Error::invalid(0, "ColorRef Reserved must be 0"))
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct EmfPlusArgb {
  pub blue: u8,
  pub green: u8,
  pub red: u8,
  pub alpha: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, SdkObject)]
#[sdk(format = "emf")]
pub struct XForm {
  pub m11: f32,
  pub m12: f32,
  pub m21: f32,
  pub m22: f32,
  pub dx: f32,
  pub dy: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct PointF {
  pub x: f32,
  pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct SizeF {
  pub width: f32,
  pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, SdkObject)]
#[sdk(format = "emfplus")]
pub struct RectF {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, SdkObject)]
#[sdk(format = "emf")]
pub struct TriVertex {
  pub x: i32,
  pub y: i32,
  pub red: u16,
  pub green: u16,
  pub blue: u16,
  pub alpha: u16,
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use super::*;
  use crate::common::{Reader, SdkRead, SdkWrite, Writer};

  #[test]
  fn color_ref_preserves_reserved_byte_and_validates_it_strictly() {
    let valid = ColorRef {
      red: 1,
      green: 2,
      blue: 3,
      reserved: 0,
    };
    let mut bytes = Vec::new();
    valid.write_to(&mut Writer::new(&mut bytes)).unwrap();
    assert_eq!(bytes, [1, 2, 3, 0]);

    let parsed = ColorRef::read_from(&mut Reader::new(Cursor::new(bytes))).unwrap();
    assert_eq!(parsed, valid);

    let invalid = [1, 2, 3, 4];
    let parsed = ColorRef::read_from(&mut Reader::new(Cursor::new(invalid))).unwrap();
    assert_eq!(parsed.reserved, 4);
    assert!(parsed.validate_strict().is_err());
    let mut roundtripped = Vec::new();
    parsed
      .write_to(&mut Writer::new(&mut roundtripped))
      .unwrap();
    assert_eq!(roundtripped, invalid);
  }
}
