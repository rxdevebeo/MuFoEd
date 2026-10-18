//! Geometric classification between distinct mathematical layout regions.
//! Callers supply semantic neighbours (rows, display lines or fraction parts),
//! never arbitrary glyph pairs such as a base and its legitimate subscript.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ClearanceIssue {
    Overlap,
    TooClose,
    NonFinite,
}

pub(super) fn classify(upper_bottom: f64, lower_top: f64, minimum: f64) -> Option<ClearanceIssue> {
    if ![upper_bottom, lower_top, minimum]
        .iter()
        .all(|v| v.is_finite())
    {
        return Some(ClearanceIssue::NonFinite);
    }
    let gap = lower_top - upper_bottom;
    if gap < -0.01 {
        Some(ClearanceIssue::Overlap)
    } else if gap + 0.01 < minimum.max(0.0) {
        Some(ClearanceIssue::TooClose)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{classify, ClearanceIssue};
    #[test]
    fn detects_overlap_touching_and_insufficient_clearance() {
        assert_eq!(classify(10.0, 9.0, 3.0), Some(ClearanceIssue::Overlap));
        assert_eq!(classify(10.0, 10.0, 3.0), Some(ClearanceIssue::TooClose));
        assert_eq!(classify(10.0, 12.0, 3.0), Some(ClearanceIssue::TooClose));
        assert_eq!(classify(10.0, 13.0, 3.0), None);
        assert_eq!(classify(10.0, 20.0, 3.0), None);
        assert_eq!(
            classify(f64::NAN, 20.0, 3.0),
            Some(ClearanceIssue::NonFinite)
        );
    }
    #[test]
    fn clearance_threshold_scales_with_font_size() {
        for em in [8.0, 16.0, 32.0] {
            assert_eq!(
                classify(0.0, em * 0.2, em * 0.3),
                Some(ClearanceIssue::TooClose)
            );
            assert_eq!(classify(0.0, em * 0.3, em * 0.3), None);
        }
    }
}
