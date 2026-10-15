//! A word list that gives recognised text back the diacritics a model dropped.
//!
//! A vision model reading Serbian, Croatian or Czech print often returns
//! `nemacki` for `nemački` and `Dorde` for `Đorđe`: the letters are right and
//! the marks are gone. A list of the language's correct word forms repairs
//! that without guessing: a word that is not in the list, whose letters with
//! some marks restored *are* a word of the list, is that word. A word that is
//! already a word (`sto`, a hundred, beside `što`) is left alone, and so is a
//! word whose restorations are equally likely.
//!
//! The list is built from a Hunspell dictionary (`.dic` and `.aff`, suffixes
//! and prefixes expanded; continuation classes and compounding are not) and,
//! optionally, a frequency list (`word count` lines) that decides between
//! restorations (`predavače` over `predavaće`). Nothing here reads a file or
//! the network: the caller hands in the text, so a library build stays offline
//! and the dictionary is whatever the caller pinned.
//!
//! Memory is kept to the forms' 64-bit hashes plus the forms that carry a mark:
//! about 70 MB for the 3.2 million forms of the Serbian dictionary.

use std::collections::{HashMap, HashSet};

/// Why a dictionary could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexiconError(String);

impl std::fmt::Display for LexiconError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the dictionary could not be read: {}", self.0)
    }
}

impl std::error::Error for LexiconError {}

/// The most word forms one dictionary may expand to.
pub const MAX_FORMS: usize = 40_000_000;

/// A language's word forms, for restoring diacritics.
#[derive(Clone, Debug)]
pub struct Lexicon {
    language: String,
    /// Hashes of every form, lower-cased, sorted.
    forms: Vec<u64>,
    /// The forms that carry a mark, by the hash of their unmarked skeleton.
    candidates: Vec<Candidate>,
    /// The text of those forms, back to back.
    arena: String,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    skeleton: u64,
    start: u32,
    len: u32,
    frequency: u32,
}

/// One affix rule.
struct Affix {
    strip: String,
    add: String,
    condition: Vec<CharClass>,
}

enum CharClass {
    Any,
    Literal(char),
    Set(Vec<char>, bool),
}

impl CharClass {
    fn matches(&self, ch: char) -> bool {
        match self {
            Self::Any => true,
            Self::Literal(expected) => *expected == ch,
            Self::Set(chars, negated) => chars.contains(&ch) != *negated,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlagKind {
    Char,
    Long,
    Num,
}

struct Rules {
    suffixes: HashMap<String, (bool, Vec<Affix>)>,
    prefixes: HashMap<String, (bool, Vec<Affix>)>,
    flags: FlagKind,
}

impl Lexicon {
    /// Builds the list from a Hunspell dictionary and an optional frequency
    /// list, for `language` (a BCP 47 tag such as `sr-Latn`).
    ///
    /// # Errors
    ///
    /// The affix file uses a flag type this reader does not know, or the
    /// dictionary expands past [`MAX_FORMS`].
    pub fn from_hunspell(
        language: &str,
        dic: &str,
        aff: &str,
        frequencies: Option<&str>,
    ) -> Result<Self, LexiconError> {
        let rules = parse_aff(aff)?;
        let frequency: HashMap<u64, u32> = frequencies
            .map(|text| {
                text.lines()
                    .filter_map(|line| {
                        let (word, count) = line.trim().rsplit_once(' ')?;
                        Some((hash(&word.to_lowercase()), count.trim().parse().ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut lexicon = Self {
            language: language.trim().to_owned(),
            forms: Vec::new(),
            candidates: Vec::new(),
            arena: String::new(),
        };
        let mut marked = HashSet::new();
        for line in dic.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (word, flags) = line.split_once('/').unwrap_or((line, ""));
            // A morphological field follows whitespace.
            let flags = flags.split_whitespace().next().unwrap_or("");
            let flags = split_flags(flags, rules.flags);
            for form in expand(word, &flags, &rules) {
                lexicon.add(&form.to_lowercase(), &frequency, &mut marked);
                if lexicon.forms.len() > MAX_FORMS {
                    return Err(LexiconError(format!("more than {MAX_FORMS} word forms")));
                }
            }
        }
        lexicon.forms.sort_unstable();
        lexicon.forms.dedup();
        lexicon
            .candidates
            .sort_unstable_by_key(|candidate| candidate.skeleton);
        Ok(lexicon)
    }

    fn add(&mut self, form: &str, frequency: &HashMap<u64, u32>, marked: &mut HashSet<u64>) {
        let key = hash(form);
        self.forms.push(key);
        if !form.chars().any(is_marked) || !marked.insert(key) {
            return;
        }
        let (Ok(start), Ok(len)) = (u32::try_from(self.arena.len()), u32::try_from(form.len()))
        else {
            return;
        };
        self.arena.push_str(form);
        self.candidates.push(Candidate {
            skeleton: hash(&skeleton(form)),
            start,
            len,
            frequency: frequency.get(&key).copied().unwrap_or(0),
        });
    }

    /// The language the list is for.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// How many distinct forms the list holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.forms.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.forms.is_empty()
    }

    /// Whether `word` (any case) is a form of the list.
    #[must_use]
    pub fn contains(&self, word: &str) -> bool {
        self.forms
            .binary_search(&hash(&word.to_lowercase()))
            .is_ok()
    }

    /// `word` with its diacritics restored, when it is not a word and exactly
    /// one restoration is - or one is clearly the most frequent. The case of
    /// the first letter, or of the whole word, is kept.
    #[must_use]
    pub fn restore(&self, word: &str) -> Option<String> {
        if word.chars().count() < 2 || self.contains(word) {
            return None;
        }
        let lower = word.to_lowercase();
        if !lower
            .chars()
            .any(|ch| matches!(ch, 'c' | 's' | 'z' | 'd' | 'č' | 'ć'))
        {
            return None;
        }
        let mut keys = vec![skeleton(&lower)];
        if lower.contains("dj") {
            // `Djordje` is how `Đorđe` is typed without the letter.
            keys.push(skeleton(&lower.replace("dj", "đ")));
        }
        let mut found: Vec<(u32, &str)> = Vec::new();
        for key in &keys {
            let wanted = hash(key);
            let first = self
                .candidates
                .partition_point(|candidate| candidate.skeleton < wanted);
            for candidate in self.candidates.get(first..).unwrap_or_default() {
                if candidate.skeleton != wanted {
                    break;
                }
                let start = candidate.start as usize;
                let Some(form) = self.arena.get(start..start + candidate.len as usize) else {
                    continue;
                };
                // A hash is not a proof: the skeleton must be the word's.
                if skeleton(form) == *key && form != lower && !found.iter().any(|(_, f)| *f == form)
                {
                    found.push((candidate.frequency, form));
                }
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let best = match found.as_slice() {
            [] => return None,
            [(_, only)] => *only,
            [(top, best), (second, _), ..] if top > second => *best,
            _ => return None,
        };
        Some(with_case_of(word, best))
    }

    /// `text` with every word [`Self::restore`] can repair repaired, and how
    /// many were.
    #[must_use]
    pub fn restore_text(&self, text: &str) -> (String, usize) {
        let mut out = String::with_capacity(text.len() + 16);
        let mut word = String::new();
        let mut fixed = 0;
        let mut flush = |word: &mut String, out: &mut String| {
            if word.is_empty() {
                return;
            }
            if let Some(restored) = self.restore(word) {
                out.push_str(&restored);
                fixed += 1;
            } else {
                out.push_str(word);
            }
            word.clear();
        };
        for ch in text.chars() {
            if ch.is_alphabetic() {
                word.push(ch);
            } else {
                flush(&mut word, &mut out);
                out.push(ch);
            }
        }
        flush(&mut word, &mut out);
        (out, fixed)
    }
}

/// Whether `ch` is a letter with a mark the skeleton removes.
fn is_marked(ch: char) -> bool {
    matches!(ch, 'č' | 'ć' | 'š' | 'ž' | 'đ')
}

/// The word with the marks removed: what a model that dropped them returns.
fn skeleton(word: &str) -> String {
    word.chars()
        .map(|ch| match ch {
            'č' | 'ć' => 'c',
            'š' => 's',
            'ž' => 'z',
            'đ' => 'd',
            other => other,
        })
        .collect()
}

/// `form` (lower case) in the case pattern of `word`.
fn with_case_of(word: &str, form: &str) -> String {
    let letters: Vec<char> = word.chars().filter(|ch| ch.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|ch| ch.is_uppercase()) {
        return form.to_uppercase();
    }
    if word.chars().next().is_some_and(char::is_uppercase) {
        let mut chars = form.chars();
        return chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        });
    }
    form.to_owned()
}

/// FNV-1a: stable across runs and platforms, unlike the standard hasher.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |state, byte| {
        (state ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn parse_aff(aff: &str) -> Result<Rules, LexiconError> {
    let mut rules = Rules {
        suffixes: HashMap::new(),
        prefixes: HashMap::new(),
        flags: FlagKind::Char,
    };
    for line in aff.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["FLAG", kind, ..] => {
                rules.flags = match *kind {
                    "num" => FlagKind::Num,
                    "long" => FlagKind::Long,
                    "UTF-8" => FlagKind::Char,
                    other => return Err(LexiconError(format!("FLAG {other} is not supported"))),
                };
            }
            [kind @ ("SFX" | "PFX"), flag, cross @ ("Y" | "N"), count]
                if count.parse::<usize>().is_ok() =>
            {
                let table = if *kind == "SFX" {
                    &mut rules.suffixes
                } else {
                    &mut rules.prefixes
                };
                table
                    .entry((*flag).to_owned())
                    .or_insert_with(|| (*cross == "Y", Vec::new()))
                    .0 = *cross == "Y";
            }
            [kind @ ("SFX" | "PFX"), flag, strip, add, rest @ ..] => {
                let suffix = *kind == "SFX";
                let table = if suffix {
                    &mut rules.suffixes
                } else {
                    &mut rules.prefixes
                };
                let strip = if *strip == "0" { "" } else { strip };
                let add = add.split('/').next().unwrap_or("");
                let add = if add == "0" { "" } else { add };
                let condition = parse_condition(rest.first().copied().unwrap_or("."));
                table
                    .entry((*flag).to_owned())
                    .or_insert_with(|| (false, Vec::new()))
                    .1
                    .push(Affix {
                        strip: strip.to_owned(),
                        add: add.to_owned(),
                        condition,
                    });
            }
            _ => {}
        }
    }
    Ok(rules)
}

fn parse_condition(text: &str) -> Vec<CharClass> {
    if text == "." {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '.' => out.push(CharClass::Any),
            '[' => {
                let mut set = Vec::new();
                let mut negated = false;
                for (index, inner) in chars.by_ref().enumerate() {
                    match inner {
                        ']' => break,
                        '^' if index == 0 => negated = true,
                        other => set.push(other),
                    }
                }
                out.push(CharClass::Set(set, negated));
            }
            other => out.push(CharClass::Literal(other)),
        }
    }
    out
}

fn split_flags(flags: &str, kind: FlagKind) -> Vec<String> {
    match kind {
        FlagKind::Num => flags
            .split(',')
            .filter(|flag| !flag.is_empty())
            .map(str::to_owned)
            .collect(),
        FlagKind::Long => {
            let chars: Vec<char> = flags.chars().collect();
            chars.chunks(2).map(|pair| pair.iter().collect()).collect()
        }
        FlagKind::Char => flags.chars().map(String::from).collect(),
    }
}

/// The word and the forms its flags derive.
fn expand(word: &str, flags: &[String], rules: &Rules) -> Vec<String> {
    let mut forms = vec![word.to_owned()];
    let mut crossable = Vec::new();
    for flag in flags {
        let Some((cross, affixes)) = rules.suffixes.get(flag) else {
            continue;
        };
        for affix in affixes {
            if let Some(form) = apply_suffix(word, affix) {
                if *cross {
                    crossable.push(form.clone());
                }
                forms.push(form);
            }
        }
    }
    let mut prefixed = Vec::new();
    for flag in flags {
        let Some((cross, affixes)) = rules.prefixes.get(flag) else {
            continue;
        };
        for affix in affixes {
            prefixed.extend(apply_prefix(word, affix));
            if *cross {
                for form in &crossable {
                    prefixed.extend(apply_prefix(form, affix));
                }
            }
        }
    }
    forms.extend(prefixed);
    forms
}

fn apply_suffix(word: &str, affix: &Affix) -> Option<String> {
    let stem = word.strip_suffix(affix.strip.as_str())?;
    let chars: Vec<char> = word.chars().collect();
    let tail = chars.len().checked_sub(affix.condition.len())?;
    let matched = affix
        .condition
        .iter()
        .zip(chars.get(tail..)?)
        .all(|(class, ch)| class.matches(*ch));
    matched.then(|| format!("{stem}{}", affix.add))
}

fn apply_prefix(word: &str, affix: &Affix) -> Option<String> {
    let stem = word.strip_prefix(affix.strip.as_str())?;
    let matched = affix
        .condition
        .iter()
        .zip(word.chars())
        .all(|(class, ch)| class.matches(ch))
        && word.chars().count() >= affix.condition.len();
    matched.then(|| format!("{}{stem}", affix.add))
}

#[cfg(test)]
mod tests {
    use super::Lexicon;

    const AFF: &str = "SET UTF-8\nFLAG num\n\
        SFX 1 Y 3\nSFX 1 0 a .\nSFX 1 0 om .\nSFX 1 0 e [^k]\n\
        SFX 2 Y 1\nSFX 2 i og i\n\
        PFX 3 Y 1\nPFX 3 0 ne .\n";
    const DIC: &str = "6\nčovek/1\nnemački/2,3\nsto\nšto\nđorđe\npredavače\npredavaće\n";
    const FREQ: &str = "što 1775707\nsto 79637\npredavače 40\npredavaće 3\n";

    fn lexicon() -> Lexicon {
        Lexicon::from_hunspell("sr-Latn", DIC, AFF, Some(FREQ)).expect("parses")
    }

    #[test]
    fn affixes_expand_into_forms() {
        let lexicon = lexicon();
        for form in [
            "čovek",
            "čoveka",
            "čovekom",
            "nemačkog",
            "nenemački",
            "nenemačkog",
        ] {
            assert!(lexicon.contains(form), "{form}");
        }
        // The condition `[^k]` keeps `čoveke` out.
        assert!(!lexicon.contains("čoveke"));
    }

    #[test]
    fn dropped_marks_come_back_and_words_stay_words() {
        let lexicon = lexicon();
        assert_eq!(lexicon.restore("covek").as_deref(), Some("čovek"));
        assert_eq!(lexicon.restore("Nemackog").as_deref(), Some("Nemačkog"));
        assert_eq!(lexicon.restore("NEMACKI").as_deref(), Some("NEMAČKI"));
        assert_eq!(lexicon.restore("Dorde").as_deref(), Some("Đorđe"));
        assert_eq!(lexicon.restore("Djordje").as_deref(), Some("Đorđe"));
        // `sto` is a word: a hundred.
        assert_eq!(lexicon.restore("sto"), None);
        // The more frequent of two restorations.
        assert_eq!(lexicon.restore("predavace").as_deref(), Some("predavače"));
        // Nothing to restore, or no such word.
        assert_eq!(lexicon.restore("mama"), None);
        assert_eq!(lexicon.restore("Krizkovic"), None);
    }

    #[test]
    fn text_keeps_its_punctuation_and_counts_repairs() {
        let (text, fixed) = lexicon().restore_text("Covek (nemacki) — sto, Đorđe; Писац.");
        assert_eq!(text, "Čovek (nemački) — sto, Đorđe; Писац.");
        assert_eq!(fixed, 2);
    }

    #[test]
    fn ties_without_frequencies_are_left_alone() {
        let lexicon = Lexicon::from_hunspell("sr-Latn", DIC, AFF, None).expect("parses");
        assert_eq!(lexicon.restore("predavace"), None);
        assert_eq!(lexicon.restore("covek").as_deref(), Some("čovek"));
    }
}
