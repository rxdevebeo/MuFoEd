//! A live check against the ollama on this machine.
//!
//! Opt-in by environment variable, because it loads a model and that is not
//! something a test suite should do behind anyone's back. What it proves is the
//! part a mock cannot: that the endpoint, the model name and the request shape
//! are what a real daemon expects.
//!
//! ```text
//! STRICT_OCR_LIVE=1 cargo run -p strict-ooxml-ocr --features ocr-ollama \
//!     --example live_probe -- <image.png>
//! ```

#[cfg(feature = "ocr-ollama")]
fn main() {
    use std::time::Instant;
    use strict_ooxml_ocr::ollama::OllamaVision;
    use strict_ooxml_ocr::traits::{FigureClassifier, TextRecovery};
    use strict_ooxml_ocr::Image;

    let path = std::env::args().nth(1).expect("an image path");
    let bytes = std::fs::read(&path).expect("readable image");
    // The name the caller gave decides how the bytes are labelled; a wrong guess
    // here is the probe's, not the client's.
    let image = if path.to_ascii_lowercase().ends_with(".png") {
        Image::png(bytes)
    } else {
        Image::jpeg(bytes)
    };

    let client = OllamaVision::from_env();
    println!("endpoint: {}", client.config().chat_url());
    // Both traits answer `model_name`, so it is named rather than inferred.
    println!("model: {}", FigureClassifier::model_name(&client));

    let started = Instant::now();
    match client.model_version() {
        Ok(version) => println!("version: {version}"),
        Err(error) => {
            println!("model_version: {error}");
            return;
        }
    }
    println!("version took {:?}", started.elapsed());

    let started = Instant::now();
    match client.describe(&image, "page 1 of a test document") {
        Ok(answer) => {
            println!("kind+description: {}", answer.text);
            println!("model: {} {}", answer.model, answer.version);
            println!("describe took {:?}", started.elapsed());
        }
        Err(error) => println!("describe: {error}"),
    }

    let started = Instant::now();
    match client.recover_page(&image) {
        Ok(Some(answer)) => {
            println!("page text: {}", answer.text);
            println!("recover took {:?}", started.elapsed());
        }
        Ok(None) => println!("page text: none (the model saw no text)"),
        Err(error) => println!("recover: {error}"),
    }
}

#[cfg(not(feature = "ocr-ollama"))]
fn main() {
    eprintln!("this example needs --features ocr-ollama");
}
