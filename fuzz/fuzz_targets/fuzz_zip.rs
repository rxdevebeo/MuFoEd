#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use strict_ooxml_core::limits::ResourceLimits;
use strict_ooxml_core::opc::{OpenOptions, Package};

fuzz_target!(|data: &[u8]| {
    let options = OpenOptions::default().limits(ResourceLimits {
        max_single_uncompressed: 1 << 20,
        max_total_uncompressed: 4 << 20,
        ..ResourceLimits::default()
    });
    let _ = Package::open_reader(Cursor::new(data.to_vec()), &options);
});
