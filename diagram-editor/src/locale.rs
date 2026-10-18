//! Instance-local gettext-style catalogs. Message IDs are the English source text.
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

static EN: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/en.json")).expect("built-in English catalog")
});
static RU: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/ru.json")).expect("built-in Russian catalog")
});

/// Language and optional host-supplied translations. Does not affect scene data/history.
#[derive(Clone, Debug)]
pub struct Locale {
    code: String,
    overrides: Arc<BTreeMap<String, String>>,
}
impl Default for Locale {
    fn default() -> Self {
        Self::new("en")
    }
}
impl Locale {
    /// Selects a BCP-47 language tag. Unsupported messages fall back to English.
    #[must_use]
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_ascii_lowercase().replace('_', "-"),
            overrides: Arc::default(),
        }
    }
    /// Merges a flat JSON catalog over the built-in language, preserving fallback.
    /// # Errors
    /// Returns a JSON/schema error for non-string values or malformed input.
    pub fn with_catalog(mut self, json: &str) -> Result<Self, String> {
        let catalog: BTreeMap<String, String> =
            serde_json::from_str(json).map_err(|e| e.to_string())?;
        self.overrides = Arc::new(catalog);
        Ok(self)
    }
    /// Active language tag, including any regional suffix.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
    /// Translates a message ID, falling back to its English source.
    #[must_use]
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.overrides
            .get(key)
            .or_else(|| {
                if self.code.split('-').next() == Some("ru") {
                    RU.get(key)
                } else {
                    None
                }
            })
            .or_else(|| EN.get(key))
            .map_or(key, String::as_str)
    }
    /// Formats positional `{}` or named `{name}` slots in a catalog message.
    #[must_use]
    pub fn format(&self, key: &str, values: &[String]) -> String {
        substitute(self.text(key), values)
    }
    /// Translates crate diagnostics (including parameterized reports). Foreign
    /// parser, OS and `StrictLib` diagnostic details are retained verbatim.
    #[must_use]
    pub fn diagnostic(&self, message: &str) -> String {
        if EN.contains_key(message) {
            return self.text(message).to_owned();
        }
        for template in EN.keys().filter(|key| key.contains('{')) {
            if let Some(values) = capture(template, message) {
                return self.format(template, &values);
            }
        }
        if message.contains("; ") {
            return message
                .split("; ")
                .map(|part| self.diagnostic(part))
                .collect::<Vec<_>>()
                .join("; ");
        }
        message.to_owned()
    }
}
fn substitute(template: &str, values: &[String]) -> String {
    let mut out = String::new();
    let mut tail = template;
    for value in values {
        let Some(start) = tail.find('{') else {
            break;
        };
        let Some(end) = tail[start..].find('}') else {
            break;
        };
        out.push_str(&tail[..start]);
        out.push_str(value);
        tail = &tail[start + end + 1..];
    }
    out.push_str(tail);
    out
}
fn capture(template: &str, message: &str) -> Option<Vec<String>> {
    let mut template = template;
    let mut message = message;
    let mut values = vec![];
    while let Some(start) = template.find('{') {
        message = message.strip_prefix(&template[..start])?;
        let end = template[start..].find('}')? + start;
        template = &template[end + 1..];
        let next = template.find('{').unwrap_or(template.len());
        let literal = &template[..next];
        let split = if literal.is_empty() {
            message.len()
        } else {
            message.find(literal)?
        };
        values.push(message[..split].to_owned());
        message = &message[split..];
    }
    (message == template).then_some(values)
}
