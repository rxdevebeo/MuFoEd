//! Explicit live Hub API check; never downloads model weights.
use image_editor::{assets::Family, huggingface};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let page = huggingface::search("stable-diffusion-xl-base-1.0", None, "")?;
    assert!(!page.models.is_empty());
    let repo = huggingface::repository("stabilityai/stable-diffusion-xl-base-1.0", "main", "")?;
    let plan = huggingface::diffusers_plan(&repo, Family::Sdxl)?;
    assert!(plan.prepare_tokenizers);
    assert!(plan.size > 1_000_000_000);
    println!("Hub search: {} repositories; pinned revision: {}; Diffusers files: {}; tokenizer preparation: {}",page.models.len(),repo.sha,plan.files.len(),plan.prepare_tokenizers);
    Ok(())
}
