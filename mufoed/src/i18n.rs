//! Interface strings loaded from JSON.
//!
//! Every language is one JSON file named by its code (`ru.json`, `en.json`).
//! The file holds a `meta` object (`name`, the language's own name) and a
//! `strings` object; nested objects are flattened with dots, so
//! `{"ribbon": {"home": "Home"}}` is the key `ribbon.home`.
//!
//! English and Russian are compiled in. Files found in `locales/` next to the
//! executable and in the user's configuration directory are read after them: a
//! file with a known code overrides that language key by key, a new code adds
//! a language. A key missing from the chosen language falls back to English,
//! and a key missing there shows as itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The languages compiled into the binary.
const BUILTIN: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en.json")),
    ("ru", include_str!("../locales/ru.json")),
];

/// The language every missing key falls back to.
const FALLBACK: &str = "en";

/// One language.
struct Language {
    /// File stem, `ru`.
    code: String,
    /// The language's own name, `Русский`.
    name: String,
    /// Flattened keys.
    strings: BTreeMap<String, String>,
}

/// The loaded languages and the chosen one.
pub(crate) struct I18n {
    languages: Vec<Language>,
    current: usize,
    fallback: usize,
    /// Files that could not be read, for the status bar.
    pub(crate) problems: Vec<String>,
}

impl I18n {
    /// Loads the built-in languages and every `*.json` in `dirs`.
    pub(crate) fn load(dirs: &[PathBuf]) -> Self {
        let mut this = Self {
            languages: Vec::new(),
            current: 0,
            fallback: 0,
            problems: Vec::new(),
        };
        for (code, text) in BUILTIN {
            if let Err(error) = this.merge(code, text) {
                this.problems.push(format!("{code}: {error}"));
            }
        }
        for dir in dirs {
            this.load_dir(dir);
        }
        this.fallback = this.index_of(FALLBACK).unwrap_or(0);
        this.current = this.fallback;
        this
    }

    fn load_dir(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        for path in files {
            let Some(code) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let code = code.to_owned();
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    if let Err(error) = self.merge(&code, &text) {
                        self.problems.push(format!("{}: {error}", path.display()));
                    }
                }
                Err(error) => self.problems.push(format!("{}: {error}", path.display())),
            }
        }
    }

    /// Adds a language or overrides an existing one key by key.
    fn merge(&mut self, code: &str, text: &str) -> Result<(), String> {
        let root: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let name = root
            .get("meta")
            .and_then(|meta| meta.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut strings = BTreeMap::new();
        if let Some(value) = root.get("strings") {
            flatten("", value, &mut strings);
        }
        if let Some(index) = self.index_of(code) {
            if let Some(language) = self.languages.get_mut(index) {
                language.strings.extend(strings);
                if let Some(name) = name {
                    language.name = name;
                }
            }
        } else {
            self.languages.push(Language {
                code: code.to_owned(),
                name: name.unwrap_or_else(|| code.to_owned()),
                strings,
            });
        }
        Ok(())
    }

    fn index_of(&self, code: &str) -> Option<usize> {
        self.languages
            .iter()
            .position(|language| language.code.eq_ignore_ascii_case(code))
    }

    /// Chooses a language by code; an unknown code keeps the current one.
    pub(crate) fn set_language(&mut self, code: &str) -> bool {
        if let Some(index) = self.index_of(code) {
            self.current = index;
            true
        } else {
            false
        }
    }

    /// The chosen language's code.
    pub(crate) fn language(&self) -> &str {
        self.languages
            .get(self.current)
            .map_or(FALLBACK, |language| language.code.as_str())
    }

    /// `(code, name)` of every language, in load order.
    pub(crate) fn languages(&self) -> impl Iterator<Item = (&str, &str)> {
        self.languages
            .iter()
            .map(|language| (language.code.as_str(), language.name.as_str()))
    }

    /// The string for `key`.
    pub(crate) fn t<'a>(&'a self, key: &'a str) -> &'a str {
        let lookup = |index: usize| {
            self.languages
                .get(index)
                .and_then(|language| language.strings.get(key))
                .map(String::as_str)
        };
        lookup(self.current)
            .or_else(|| lookup(self.fallback))
            .unwrap_or(key)
    }

    /// The string for `key` with `{name}` placeholders replaced.
    pub(crate) fn tf(&self, key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
        let mut text = self.t(key).to_owned();
        for (name, value) in args {
            text = text.replace(&format!("{{{name}}}"), &value.to_string());
        }
        text
    }
}

fn flatten(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&key, value, out);
            }
        }
        Value::String(text) => {
            out.insert(prefix.to_owned(), text.clone());
        }
        Value::Number(_) | Value::Bool(_) => {
            out.insert(prefix.to_owned(), value.to_string());
        }
        Value::Array(_) | Value::Null => {}
    }
}

/// The system language's two-letter code, when it can be read.
pub(crate) fn system_language() -> Option<String> {
    let locale = sys_locale::get_locale()?;
    let code = locale.split(['-', '_']).next()?.to_ascii_lowercase();
    (!code.is_empty()).then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_builtin_languages_parse_and_share_keys() {
        let i18n = I18n::load(&[]);
        assert!(i18n.problems.is_empty(), "{:?}", i18n.problems);
        let en = &i18n.languages[i18n.index_of("en").unwrap()];
        let ru = &i18n.languages[i18n.index_of("ru").unwrap()];
        let missing: Vec<_> = en
            .strings
            .keys()
            .filter(|key| !ru.strings.contains_key(*key))
            .collect();
        assert!(missing.is_empty(), "ru lacks {missing:?}");
        let extra: Vec<_> = ru
            .strings
            .keys()
            .filter(|key| !en.strings.contains_key(*key))
            .collect();
        assert!(extra.is_empty(), "en lacks {extra:?}");
    }

    #[test]
    fn nested_keys_flatten_and_missing_keys_fall_back() {
        let mut i18n = I18n::load(&[]);
        i18n.merge(
            "xx",
            r#"{"meta":{"name":"Test"},"strings":{"a":{"b":"nested"}}}"#,
        )
        .unwrap();
        assert!(i18n.set_language("xx"));
        assert_eq!(i18n.t("a.b"), "nested");
        assert_eq!(i18n.t("app.name"), "MultiFormatEditor");
        assert_eq!(i18n.t("no.such.key"), "no.such.key");
        assert_eq!(
            i18n.tf("status.page", &[("page", &3), ("pages", &7)]),
            i18n.t("status.page")
                .replace("{page}", "3")
                .replace("{pages}", "7")
        );
    }
}
