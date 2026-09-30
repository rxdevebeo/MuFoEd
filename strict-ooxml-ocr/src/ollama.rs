//! The ollama client, behind the `ocr-ollama` feature.
//!
//! # Why the API and not a library
//!
//! Ollama has an OpenAI-compatible endpoint and a native one. The native
//! `/api/chat` is used because it takes an image per message and a
//! `format`-free free-form answer, which is exactly what a caption and a page
//! transcription are, and because a caller with ollama already has it running -
//! no daemon to install, no key to manage, nothing to serve on a port.
//!
//! # What is deliberately absent
//!
//! No streaming, no tool calls, no multiple images per message, no retries. A
//! retry against a model that is already busy with something else makes the
//! queue longer; a caller who wants that can build the client out of the trait.

use std::fmt::Write as _;
use std::time::Duration;

use crate::traits::{FigureClassifier, Recovered, TextRecovery, VisionError};
use crate::{Image, DEFAULT_TIMEOUT_SECS, MAX_RESPONSE_BYTES};

/// The daemon's address when nothing says otherwise.
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";

/// The environment variable that overrides the address.
pub const ENDPOINT_VARIABLE: &str = "OLLAMA_HOST";

/// The environment variable that overrides the model.
pub const MODEL_VARIABLE: &str = "OLLAMA_MODEL";

/// Which model to use when nothing says otherwise.
///
/// A vision model, and one that is actually installed on the machines this runs
/// on - the default is a *configuration*, and a default that names a model
/// nobody has is a default that always fails.
pub const DEFAULT_MODEL: &str = "qwen3-vl:8b-instruct-ctx16k";

/// How the client talks to a daemon.
#[derive(Clone, Debug, PartialEq)]
pub struct OllamaConfig {
    /// Base address, without a trailing slash.
    pub endpoint: String,
    /// The model to ask.
    pub model: String,
    /// How long one request may take.
    pub timeout: Duration,
    /// The most answer the client will read.
    pub max_response_bytes: usize,
    /// `options.temperature`, recorded in the report.
    pub temperature: f64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_response_bytes: MAX_RESPONSE_BYTES,
            temperature: 0.0,
        }
    }
}

impl OllamaConfig {
    /// Reads the endpoint and the model from the environment, falling back to
    /// the defaults.
    ///
    /// `OLLAMA_HOST` is ollama's own variable and may carry a scheme or a bare
    /// `host:port`; both are accepted because both appear in the wild.
    #[must_use]
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(host) = std::env::var(ENDPOINT_VARIABLE) {
            let host = host.trim();
            if !host.is_empty() {
                config.endpoint = if host.starts_with("http://") || host.starts_with("https://") {
                    host.trim_end_matches('/').to_owned()
                } else {
                    format!("http://{}", host.trim_end_matches('/'))
                };
            }
        }
        if let Ok(model) = std::env::var(MODEL_VARIABLE) {
            let model = model.trim();
            if !model.is_empty() {
                config.model.clone_from(&model.to_string());
            }
        }
        config
    }

    /// The URL of one call.
    #[must_use]
    pub fn chat_url(&self) -> String {
        format!("{}/api/chat", self.endpoint.trim_end_matches('/'))
    }
}

/// A client for a local ollama daemon.
#[derive(Clone, Debug)]
pub struct OllamaVision {
    config: OllamaConfig,
}

impl OllamaVision {
    /// A client with the default configuration, adjusted by the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(OllamaConfig::from_env())
    }

    /// A client with an explicit configuration.
    #[must_use]
    pub fn new(config: OllamaConfig) -> Self {
        Self { config }
    }

    /// The configuration in force.
    #[must_use]
    pub fn config(&self) -> &OllamaConfig {
        &self.config
    }

    /// The version the daemon reports for the model, when it reports one.
    ///
    /// This is what makes an answer reproducible: the same prompt against a
    /// different build is a different answer, and the report has to say which
    /// build answered.
    ///
    /// # Errors
    ///
    /// Returns a [`VisionError`] when the daemon cannot be reached.
    pub fn model_version(&self) -> Result<String, VisionError> {
        let url = format!("{}/api/show", self.config.endpoint.trim_end_matches('/'));
        let body = format!("{{\"model\":{}}}", json_string(&self.config.model));
        let response = self.post(&url, &body)?;
        let digest = response
            .get("details")
            .and_then(|details| details.get("digest"))
            .and_then(|digest| digest.as_str())
            .unwrap_or("unknown");
        Ok(digest.to_owned())
    }

    /// One chat call with an image, returning the assistant's text.
    fn ask(
        &self,
        prompt: &str,
        image: &Image,
        context: &str,
    ) -> Result<(String, String), VisionError> {
        let version = self.model_version()?;
        let user = if context.is_empty() {
            prompt.to_owned()
        } else {
            format!("{prompt}\n\nContext: {context}")
        };
        let body = format!(
            "{{\"model\":{},\"stream\":false,\"options\":{{\"temperature\":{}}},\"messages\":[\
             {{\"role\":\"user\",\"content\":{},\"images\":[{}]}}]}}",
            json_string(&self.config.model),
            self.config.temperature,
            json_string(&user),
            json_string(&image.to_base64()),
        );
        let response = self.post(&self.config.chat_url(), &body)?;
        let text = response
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
            .map(str::trim)
            .filter(|content| !content.is_empty())
            .ok_or_else(|| VisionError::Unusable("the answer carried no content".to_owned()))?;
        Ok((text.to_owned(), version))
    }

    /// Posts a body and parses the answer, with the budget applied first.
    fn post(&self, url: &str, body: &str) -> Result<serde_json::Value, VisionError> {
        // `ureq` 3 configures an agent through a builder on the config, not on
        // the agent itself; the timeout is global so a stalled connection and a
        // stalled model are both bounded.
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.config.timeout))
            .build()
            .new_agent();
        let response = agent
            .post(url)
            .header("content-type", "application/json")
            .send(body)
            .map_err(|error| {
                if matches!(error, ureq::Error::Timeout(_)) {
                    VisionError::TimedOut
                } else {
                    VisionError::Unreachable(error.to_string())
                }
            })?;
        // The budget is applied to the *read*, not to the request: a daemon
        // that answers with a hundred megabytes is a memory exhaustion bug in the
        // caller, and a local daemon is not a trusted source the way a file the
        // user picked is.
        let mut response = response;
        let text = response
            .body_mut()
            .with_config()
            .limit(self.config.max_response_bytes as u64)
            .read_to_string()
            .map_err(|error| VisionError::Unusable(error.to_string()))?;
        if text.len() > self.config.max_response_bytes {
            return Err(VisionError::Unusable(format!(
                "the answer is larger than the {} byte budget",
                self.config.max_response_bytes
            )));
        }
        serde_json::from_str(&text)
            .map_err(|error| VisionError::Unusable(format!("the answer is not JSON: {error}")))
    }
}

/// The prompt that asks for a page's text.
///
/// It is explicit that this is transcription and not interpretation, because a
/// model asked to "describe" a page will happily describe it.
const PAGE_PROMPT: &str = "Transcribe the text of this page exactly as it appears. \
Preserve the reading order and the line breaks. Do not summarise, do not correct \
spelling, and do not add anything that is not written on the page. If there is no \
readable text, reply with exactly: NO TEXT";

/// The prompt that asks for the text inside one region of a page.
///
/// The difference from [`PAGE_PROMPT`] is one word and it is load-bearing: the
/// image is a **crop**, and a model told it is looking at a page will account for
/// the page — inventing a heading, a page number, a caption — and those words then
/// go into the document as if the page had said them. It is also told that the
/// rest of the page is already in hand, so the text around the crop is not
/// repeated back at us.
const REGION_PROMPT: &str = "Transcribe the text visible inside this image exactly as it \
appears. The image is one region cropped from a larger page whose other text has already \
been captured elsewhere, so transcribe only what is inside this image and do not add a \
heading, a page number, a caption or any other text that is not visible in it. If there \
is no readable text, reply with exactly: NO TEXT";

/// The prompt that asks what a graphic region is.
const FIGURE_PROMPT: &str = "This is one region of a document page. Answer with the \
single word naming what it is - table, diagram, formula, logo, illustration, scan or \
unknown - then a new line, then one sentence describing it for someone who cannot \
see it. Do not describe anything outside the image.";

impl FigureClassifier for OllamaVision {
    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn describe(&self, region: &Image, context: &str) -> Result<Recovered, VisionError> {
        let (text, version) = self.ask(FIGURE_PROMPT, region, context)?;
        Ok(Recovered {
            text,
            model: self.config.model.clone(),
            version,
            confidence: None,
        })
    }
}

impl TextRecovery for OllamaVision {
    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn recover_page(&self, page: &Image) -> Result<Option<Recovered>, VisionError> {
        self.ask_for_text(PAGE_PROMPT, page)
    }

    fn recover_region(&self, region: &Image) -> Result<Option<Recovered>, VisionError> {
        self.ask_for_text(REGION_PROMPT, region)
    }
}

impl OllamaVision {
    /// Asks for a transcription and turns «NO TEXT» into the answer it is.
    fn ask_for_text(&self, prompt: &str, image: &Image) -> Result<Option<Recovered>, VisionError> {
        let (text, version) = self.ask(prompt, image, "")?;
        if text.eq_ignore_ascii_case("NO TEXT") {
            return Ok(None);
        }
        Ok(Some(Recovered {
            text,
            model: self.config.model.clone(),
            version,
            confidence: None,
        }))
    }
}

/// A JSON string literal, quotes and all.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{json_string, OllamaConfig, DEFAULT_ENDPOINT, DEFAULT_MODEL, ENDPOINT_VARIABLE};

    #[test]
    fn the_default_endpoint_is_local() {
        let config = OllamaConfig::default();
        assert_eq!(config.endpoint, DEFAULT_ENDPOINT);
        assert!(
            config.endpoint.starts_with("http://127.0.0.1"),
            "{}",
            config.endpoint
        );
        assert_eq!(config.chat_url(), "http://127.0.0.1:11434/api/chat");
    }

    #[test]
    fn the_default_model_is_a_vision_model() {
        // A default that names a model nobody has is a default that always fails.
        assert!(DEFAULT_MODEL.contains("vl") || DEFAULT_MODEL.contains("vision"));
    }

    #[test]
    fn a_trailing_slash_does_not_double() {
        let config = OllamaConfig {
            endpoint: "http://example.test:1234/".to_owned(),
            ..OllamaConfig::default()
        };
        assert_eq!(config.chat_url(), "http://example.test:1234/api/chat");
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("line\nbreak"), "\"line\\nbreak\"");
        // A model name with a slash - the `hf.co/...` form - must survive.
        assert_eq!(json_string("hf.co/a/B:Q4"), "\"hf.co/a/B:Q4\"");
    }

    #[test]
    fn the_endpoint_variable_is_the_one_ollama_uses() {
        assert_eq!(ENDPOINT_VARIABLE, "OLLAMA_HOST");
    }
}
