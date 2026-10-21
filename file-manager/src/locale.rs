//! Instance-local gettext-style catalogs. Message IDs are the English source text.

use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

static EN: LazyLock<BTreeMap<String, String>> =
    LazyLock::new(|| serde_json::from_str(include_str!("../locales/en.json")).unwrap_or_default());
static RU: LazyLock<BTreeMap<String, String>> =
    LazyLock::new(|| serde_json::from_str(include_str!("../locales/ru.json")).unwrap_or_default());

/// Language and optional host-supplied translations.
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
    /// Returns the JSON error for malformed input or non-string values.
    pub fn with_catalog(mut self, json: &str) -> Result<Self, String> {
        let catalog: BTreeMap<String, String> =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        self.overrides = Arc::new(catalog);
        Ok(self)
    }

    /// Active language tag, including any regional suffix.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Translates a message ID, falling back to its English source.
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
    pub fn format(&self, key: &str, values: &[String]) -> String {
        substitute(self.text(key), values)
    }
}

fn substitute(template: &str, values: &[String]) -> String {
    let mut out = String::new();
    let mut tail = template;
    for value in values {
        let Some(start) = tail.find("{}") else {
            break;
        };
        out.push_str(&tail[..start]);
        out.push_str(value);
        tail = &tail[start + 2..];
    }
    out.push_str(tail);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_parse_and_cover_the_same_keys() {
        let en: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../locales/en.json")).expect("English catalog");
        let ru: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../locales/ru.json")).expect("Russian catalog");
        let missing: Vec<_> = en.keys().filter(|key| !ru.contains_key(*key)).collect();
        assert!(missing.is_empty(), "untranslated: {missing:?}");
        let extra: Vec<_> = ru.keys().filter(|key| !en.contains_key(*key)).collect();
        assert!(extra.is_empty(), "unknown to English: {extra:?}");
    }

    #[test]
    fn falls_back_and_substitutes() {
        let ru = Locale::new("ru_RU");
        assert_eq!(ru.code(), "ru-ru");
        assert_eq!(ru.text("no such message"), "no such message");
        assert_eq!(substitute("{} of {}", &["1".into(), "2".into()]), "1 of 2");
        let custom = Locale::new("en")
            .with_catalog(r#"{"Copy": "Duplicate"}"#)
            .expect("catalog");
        assert_eq!(custom.text("Copy"), "Duplicate");
    }
}
