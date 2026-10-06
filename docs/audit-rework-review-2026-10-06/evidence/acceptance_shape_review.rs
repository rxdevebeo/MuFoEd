//! Independent acceptance probe for Unicode cluster preservation.
use strict_ooxml_render_svg::font::{shape_bundled, unicode_cluster_map, unicode_x_positions_px};

#[test]
fn acceptance_shaping_maps_every_scalar_even_when_degraded() {
    for text in ["office", "e\u{301}", "سلام", "தமிழ்"] {
        let shaped = shape_bundled(text, "Carlito", false, false).expect("bundled");
        let mapping = unicode_cluster_map(text, &shaped);
        let xs = unicode_x_positions_px(text, &shaped, 96.0, 16.0);
        println!("{text:?}: chars={} map={} xs={} clusters={:?}", text.chars().count(), mapping.len(), xs.len(), shaped.clusters);
        assert!(shaped.clusters.iter().all(|c| c.byte_start < c.byte_end && text.is_char_boundary(c.byte_start) && text.is_char_boundary(c.byte_end)), "invalid Unicode cluster interval {text:?}");
        assert_eq!(mapping.iter().map(|(ch, _, _)| *ch).collect::<String>(), text, "Unicode mapping {text:?}");
        assert_eq!(xs.len(), text.chars().count(), "x positions {text:?}");
    }
}
