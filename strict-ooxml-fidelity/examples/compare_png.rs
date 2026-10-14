//! Measure a pair of unmodified page rasters using the shared fidelity arithmetic.
use strict_ooxml_fidelity::{load_png_gray, ssim, structural_fidelity, GatePolicy};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: compare_png REFERENCE.png ACTUAL.png".into());
    }
    let reference = load_png_gray(std::path::Path::new(&args[0]))?;
    let actual = load_png_gray(std::path::Path::new(&args[1]))?;
    if (reference.width(), reference.height()) != (actual.width(), actual.height()) {
        println!(
            "Fail\tdimensions\t{}x{}\t{}x{}",
            reference.width(),
            reference.height(),
            actual.width(),
            actual.height()
        );
        return Ok(());
    }
    let score = ssim(&reference, &actual);
    let policy = GatePolicy::shared();
    let threshold = policy.pdf.ssim + policy.pdf.margin;
    let structural = structural_fidelity(&reference, &actual);
    let verdict = if score >= threshold && structural.is_ok() {
        "Pass"
    } else {
        "Fail"
    };
    println!("{verdict}\tssim={score:.8}\tthreshold={threshold:.8}\tstructure={structural:?}");
    Ok(())
}
