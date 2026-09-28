//! Unit conversions and deterministic number formatting (ADR-0006).
//!
//! The renderer's output unit is **px at `RenderOptions::scale`** (default 96
//! DPI). All numbers written to SVG go through [`fmt_num`] so the output is
//! byte-stable and never contains `NaN`, `inf` or `-0`.

/// Twips (1/20 pt) per inch.
pub const TWIPS_PER_INCH: f64 = 1440.0;
/// English Metric Units per inch.
pub const EMU_PER_INCH: f64 = 914_400.0;
/// Points per inch.
pub const PT_PER_INCH: f64 = 72.0;

/// Converts twips to px.
#[must_use]
pub fn twips_to_px(twips: i32, scale: f64) -> f64 {
    (f64::from(twips) / TWIPS_PER_INCH) * scale
}

/// Converts an unsigned twip value to px.
#[must_use]
pub fn utwips_to_px(twips: u32, scale: f64) -> f64 {
    (f64::from(twips) / TWIPS_PER_INCH) * scale
}

/// Converts half-points to px.
#[must_use]
pub fn half_points_to_px(half_points: i32, scale: f64) -> f64 {
    pt_to_px(f64::from(half_points) / 2.0, scale)
}

/// Converts eighths of a point to px.
#[must_use]
pub fn eighths_point_to_px(eighths: u16, scale: f64) -> f64 {
    pt_to_px(f64::from(eighths) / 8.0, scale)
}

/// Converts points to px.
#[must_use]
pub fn pt_to_px(points: f64, scale: f64) -> f64 {
    (points / PT_PER_INCH) * scale
}

/// Converts EMU to px.
#[must_use]
pub fn emu_to_px(emu: i64, scale: f64) -> f64 {
    (emu as f64 / EMU_PER_INCH) * scale
}

/// Returns a finite, non-negative value (non-finite input becomes `0`).
#[must_use]
pub fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

/// Formats a number deterministically (millipixel precision, no `-0`).
///
/// Non-finite values render as `0`. Integers render without a decimal part.
#[must_use]
pub fn fmt_num(value: f64) -> String {
    let value = finite(value);
    let rounded = (value * 1000.0).round() / 1000.0;
    let normalized = if rounded == 0.0 { 0.0 } else { rounded };
    let mut rendered = format!("{normalized:.3}");
    while rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    if rendered.is_empty() || rendered == "-" {
        rendered = String::from("0");
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::{eighths_point_to_px, emu_to_px, fmt_num, half_points_to_px, twips_to_px};

    #[test]
    fn conversions_at_96_dpi() {
        assert!((twips_to_px(1440, 96.0) - 96.0).abs() < 1e-9);
        assert!((half_points_to_px(24, 96.0) - 16.0).abs() < 1e-9);
        assert!((eighths_point_to_px(8, 96.0) - (96.0 / 72.0)).abs() < 1e-9);
        assert!((emu_to_px(914_400, 96.0) - 96.0).abs() < 1e-9);
    }

    #[test]
    fn fmt_num_is_stable() {
        assert_eq!(fmt_num(0.0), "0");
        assert_eq!(fmt_num(-0.0), "0");
        assert_eq!(fmt_num(12.0), "12");
        assert_eq!(fmt_num(12.5), "12.5");
        assert_eq!(fmt_num(12.3456), "12.346");
        assert_eq!(fmt_num(f64::NAN), "0");
        assert_eq!(fmt_num(f64::INFINITY), "0");
        assert_eq!(fmt_num(-0.0004), "0");
    }
}
