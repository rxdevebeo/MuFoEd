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

/// Tokens an answer may take when nothing says otherwise: a dense page as
/// layout JSON is about 3 000; the image and the answer fit a 16k context.
pub const DEFAULT_MAX_ANSWER_TOKENS: u32 = 8192;

/// The context window when nothing says otherwise: a page picture at 144 dpi
/// (about 2 000 tokens), the prompt and a full answer.
pub const DEFAULT_CONTEXT_TOKENS: u32 = 16384;

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
    /// `options.num_predict`: the most tokens one answer may have.
    ///
    /// Stated, because a model's own default cut the layout of a dense page
    /// off mid-string at about 4 500 characters. An answer that reaches the
    /// limit, or at the end of the context (`done_reason: "length"` either
    /// way), is refused, not used truncated.
    pub max_answer_tokens: u32,
    /// `options.num_ctx`: the context window, picture and prompt and answer.
    ///
    /// Stated too: the daemon's default window (4096 tokens) held a page
    /// picture of about 1 900 tokens and left the answer the rest, which is
    /// where the layout of a dense page stopped.
    pub context_tokens: u32,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_response_bytes: MAX_RESPONSE_BYTES,
            temperature: 0.0,
            max_answer_tokens: DEFAULT_MAX_ANSWER_TOKENS,
            context_tokens: DEFAULT_CONTEXT_TOKENS,
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
            "{{\"model\":{},\"stream\":false,\"options\":{{\"temperature\":{},\"num_predict\":{},\"num_ctx\":{}}},\
             \"messages\":[{{\"role\":\"user\",\"content\":{},\"images\":[{}]}}]}}",
            json_string(&self.config.model),
            self.config.temperature,
            self.config.max_answer_tokens,
            self.config.context_tokens,
            json_string(&user),
            json_string(&image.to_base64()),
        );
        let response = self.post(&self.config.chat_url(), &body)?;
        if response
            .get("done_reason")
            .and_then(|reason| reason.as_str())
            == Some("length")
        {
            return Err(VisionError::Unusable(format!(
                "the answer was cut off at the length limit ({} answer tokens, a {}-token context)",
                self.config.max_answer_tokens, self.config.context_tokens
            )));
        }
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
        // A server error (a model reloading, a daemon restarting under an
        // update) is retried twice, after 3 and 10 s: over a 258-page book
        // eight pages lost their layout to a passing HTTP 500. A refused
        // connection or a timeout is not: nothing suggests it will pass.
        let mut pauses = [3u64, 10].into_iter();
        let response = loop {
            match agent
                .post(url)
                .header("content-type", "application/json")
                .send(body)
            {
                Ok(response) => break response,
                Err(ureq::Error::StatusCode(code)) if code >= 500 => match pauses.next() {
                    Some(seconds) => std::thread::sleep(std::time::Duration::from_secs(seconds)),
                    None => {
                        return Err(VisionError::Unreachable(format!(
                            "http status: {code}, three times"
                        )))
                    }
                },
                Err(ureq::Error::Timeout(_)) => return Err(VisionError::TimedOut),
                Err(error) => return Err(VisionError::Unreachable(error.to_string())),
            }
        };
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

/// The prompt that asks for the formula an image shows, as MathML.
const FORMULA_PROMPT: &str = "This image shows a mathematical formula. Write it as presentation MathML: one <math xmlns=\"http://www.w3.org/1998/Math/MathML\"> element using mi, mn, mo, mrow, mfrac, msqrt, mroot, msup, msub, msubsup, munder, mover, munderover and mtable, and nothing else - no explanation, no LaTeX, no code fence. Transcribe exactly what is shown. If the image shows no formula, reply with exactly: NO FORMULA";

/// The prompt that asks for a page's layout: blocks in reading order, boxes on
/// Qwen-VL's 0-1000 grid, text in Markdown.
///
/// It asks for no language: a model told a block is Serbian Cyrillic
/// transliterates the Latin lines of that block into Cyrillic.
pub const LAYOUT_PROMPT: &str = "You are a document layout parser. Return the layout of this \
scanned page as a JSON array with one object per block, in reading order, and nothing else. \
Each object has: \"type\": one of \"title\", \"heading\", \"paragraph\", \"list_item\", \
\"table\", \"figure\", \"caption\", \"header\", \"footer\", \"page_number\"; \"bbox\": \
[x1, y1, x2, y2], the block's box in coordinates from 0 to 1000 relative to the page's \
width and height; \"level\": 1 to 3 for a title or heading; \"align\": \"left\", \"center\", \
\"right\" or \"justify\"; \"text\": the exact text of the block in Markdown, keeping **bold** \
and *italic*, joining the lines of a paragraph; a table as a Markdown table; for a figure, \
the text written inside it, or an empty string. Transcribe exactly: do not translate, \
correct or summarise.";

/// The prompt that asks for a cropped table as Markdown.
///
/// The layout answer gives a table's text as lines (`jutro Dobro jutro.`),
/// which says nothing about where one cell ends; the crop asked on its own,
/// as a table, comes back with its cells.
pub const TABLE_PROMPT: &str = "This image is a table cropped from a scanned page. Transcribe \
it as a Markdown table: one line per row, cells separated by |, and a |---| rule after the \
first row. Keep empty cells empty and keep every row. Transcribe exactly: do not translate, \
correct or summarise. Reply with the table only.";

/// The blocks of a model's layout answer: a JSON array, perhaps inside a
/// Markdown code fence. Blocks without a usable box are dropped.
///
/// # Errors
///
/// The answer holds no JSON array of objects.
pub fn parse_layout(
    answer: &str,
    image_size: Option<(u32, u32)>,
) -> Result<Vec<crate::LayoutBlock>, VisionError> {
    use crate::layout::{normalize_bbox, Align, BlockKind, LayoutBlock};
    let start = answer.find('[');
    let end = answer.rfind(']');
    let json = match (start, end) {
        (Some(start), Some(end)) if end > start => answer.get(start..=end).unwrap_or(""),
        _ => {
            return Err(VisionError::Unusable(
                "the layout answer holds no JSON array".to_owned(),
            ))
        }
    };
    let value: serde_json::Value = serde_json::from_str(json).map_err(|error| {
        VisionError::Unusable(format!("the layout answer is not JSON: {error}"))
    })?;
    let items = value
        .as_array()
        .ok_or_else(|| VisionError::Unusable("the layout answer is not an array".to_owned()))?;
    let mut blocks = Vec::with_capacity(items.len());
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let Some(raw) = object.get("bbox").and_then(|bbox| bbox.as_array()) else {
            continue;
        };
        let numbers: Vec<f64> = raw.iter().filter_map(serde_json::Value::as_f64).collect();
        let [x1, y1, x2, y2] = numbers.as_slice() else {
            continue;
        };
        let Some(bbox) = normalize_bbox([*x1, *y1, *x2, *y2], image_size) else {
            continue;
        };
        let text = |key: &str| {
            object
                .get(key)
                .and_then(|value| value.as_str())
                .unwrap_or("")
        };
        blocks.push(LayoutBlock {
            kind: BlockKind::parse(text("type")),
            bbox,
            level: object
                .get("level")
                .and_then(serde_json::Value::as_u64)
                .and_then(|level| u8::try_from(level).ok())
                .filter(|level| (1..=6).contains(level)),
            align: Align::parse(text("align")),
            text: text("text").trim().to_owned(),
        });
    }
    Ok(blocks)
}

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

    fn recover_table(&self, table: &Image) -> Result<Option<Recovered>, VisionError> {
        self.ask_for_text(TABLE_PROMPT, table)
    }

    fn recover_formula(&self, formula: &Image) -> Result<Option<Recovered>, VisionError> {
        let (text, version) = self.ask(FORMULA_PROMPT, formula, "")?;
        if text.trim().eq_ignore_ascii_case("NO FORMULA") {
            return Ok(None);
        }
        let mathml = crate::traits::mathml_in(&text).ok_or_else(|| {
            VisionError::Unusable("the answer holds no <math> element".to_owned())
        })?;
        Ok(Some(Recovered {
            text: mathml.to_owned(),
            model: self.config.model.clone(),
            version,
            confidence: None,
        }))
    }

    fn recover_layout(&self, page: &Image) -> Result<Option<crate::Layout>, VisionError> {
        let (text, version) = self.ask(LAYOUT_PROMPT, page, "")?;
        let blocks = parse_layout(&text, crate::layout::png_size(&page.bytes))?;
        Ok(Some(crate::Layout {
            blocks,
            model: self.config.model.clone(),
            version,
        }))
    }
}

impl OllamaVision {
    /// The model's answer to `prompt` about `image`, and the model's digest.
    ///
    /// For probes that try a prompt of their own (`layout_probe`); the
    /// converter goes through [`TextRecovery`] and [`FigureClassifier`].
    ///
    /// # Errors
    ///
    /// The daemon is unreachable, timed out or answered nothing usable.
    pub fn ask_with_prompt(
        &self,
        prompt: &str,
        image: &Image,
    ) -> Result<(String, String), VisionError> {
        self.ask(prompt, image, "")
    }

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
    fn a_layout_answer_becomes_blocks() {
        use crate::{Align, BlockKind};
        let answer = "```json\n[{\"type\": \"title\", \"bbox\": [214, 67, 425, 91], \
            \"level\": 1, \"align\": \"center\", \"text\": \"REČ O KNJIZI\"},\n\
            {\"type\": \"paragraph\", \"bbox\": [100, 100, 900, 300], \"text\": \"Ovo je **šesto**\"},\n\
            {\"type\": \"figure\", \"bbox\": [1, 2, 1, 9], \"text\": \"\"}]\n```";
        let blocks = super::parse_layout(answer, None).expect("parses");
        assert_eq!(blocks.len(), 2, "the figure without area is dropped");
        assert_eq!(blocks[0].kind, BlockKind::Title);
        assert_eq!(blocks[0].level, Some(1));
        assert_eq!(blocks[0].align, Some(Align::Center));
        assert_eq!(blocks[1].text, "Ovo je **šesto**");
        assert!((blocks[1].bbox[2] - 0.9).abs() < 1e-9);
        assert!(super::parse_layout("no blocks here", None).is_err());
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
