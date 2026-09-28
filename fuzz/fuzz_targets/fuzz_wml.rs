#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions};
use strict_ooxml_core::opc::Package;
use strict_ooxml_wml::{parse_document, ParseOptions};

fuzz_target!(|data: &[u8]| {
    let options = OpenOptions::default().conformance(ConformancePolicy::Permissive);
    if let Ok(package) = Package::open_reader(Cursor::new(data), &options) {
        let parse_options = ParseOptions {
            conformance: ConformancePolicy::Permissive,
            limits: strict_ooxml_core::limits::ResourceLimits::default(),
        };
        let _ = parse_document(&package, &parse_options);
    }
});
