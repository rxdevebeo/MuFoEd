//! GPU imports must not be resolved by Windows before main can discover them.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/mascot_icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/mascot_icon.ico")
            .set("ProductName", "MuFoEd")
            .set("FileDescription", "MuFoEd — MultiFormatEditor")
            .set("OriginalFilename", "mufoed.exe")
            .compile()
            .expect("compile MuFoEd icon resource");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
        || std::env::var_os("CARGO_FEATURE_IMAGE_CUDA").is_none()
    {
        return;
    }
    println!("cargo:rustc-link-lib=delayimp");
    for dll in [
        "nvcuda.dll",
        "curand64_10.dll",
        "cublas64_12.dll",
        "cudart64_12.dll",
    ] {
        println!("cargo:rustc-link-arg-bin=mufoed=/DELAYLOAD:{dll}");
    }
    if std::env::var_os("CARGO_FEATURE_IMAGE_CUDNN").is_some() {
        println!("cargo:rustc-link-arg-bin=mufoed=/DELAYLOAD:cudnn64_9.dll");
    }
}
