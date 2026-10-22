//! Read-only live catalog check; does not download weights or require a token.
use image_editor::{assets::Kind, civitai};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let page = civitai::search("detail", Kind::Lora, "SDXL 1.0", None, "")?;
    assert!(!page.resources.is_empty(), "No safetensors files returned");
    assert!(page
        .resources
        .iter()
        .all(|r| r.sha256.len() == 64 && r.kind == Kind::Lora));
    println!(
        "Civitai: {} version/files, next cursor: {}",
        page.resources.len(),
        page.cursor.is_some()
    );
    if let Some(cursor) = page.cursor {
        let next = civitai::search("detail", Kind::Lora, "SDXL 1.0", Some(&cursor), "")?;
        println!("Next page: {} version/files", next.resources.len());
    }
    Ok(())
}
