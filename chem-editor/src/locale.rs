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

/// Language and optional host-supplied translations. Does not affect document data/history.
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
    ///
    /// # Errors
    ///
    /// Returns a JSON/schema error for non-string values or malformed input.
    pub fn with_catalog(mut self, json: &str) -> Result<Self, String> {
        let catalog: BTreeMap<String, String> =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
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

    /// Formats positional `{}` slots in a catalog message.
    #[must_use]
    pub fn format(&self, key: &str, values: &[String]) -> String {
        substitute(self.text(key), values)
    }

    /// Translates crate diagnostics, retaining unknown detail verbatim.
    #[must_use]
    pub fn diagnostic(&self, message: &str) -> String {
        if EN.contains_key(message) {
            return self.text(message).to_owned();
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
