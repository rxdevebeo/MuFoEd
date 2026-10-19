//! Spelling: words checked against Hunspell dictionaries (an `.aff` file of
//! rules and a `.dic` file of stems), with the parts of the format most
//! dictionaries rely on: one prefix and one suffix per word, their
//! conditions and cross products, the case rules, `REP` and `TRY` for
//! suggestions. Compounding is left out: a word it would allow is not
//! marked when no stem explains it only if a dictionary lists it.
//!
//! The dictionaries are not shipped: the user is offered them when a
//! document has words in a script none covers, and they are fetched from
//! `github.com/wooorm/dictionaries` (UTF-8 copies of the LibreOffice ones)
//! into the configuration folder.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

type Flag = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlagMode {
    /// One character a flag (the default).
    Char,
    /// Two characters a flag.
    Long,
    /// Numbers, comma-separated.
    Num,
}

/// One position of an affix condition.
#[derive(Clone, Debug)]
enum Cond {
    Any,
    Char(char),
    Set { chars: Vec<char>, negated: bool },
}

impl Cond {
    fn matches(&self, c: char) -> bool {
        match self {
            Self::Any => true,
            Self::Char(want) => *want == c,
            Self::Set { chars, negated } => chars.contains(&c) != *negated,
        }
    }
}

fn parse_condition(text: &str) -> Vec<Cond> {
    if text == "." {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '.' => out.push(Cond::Any),
            '[' => {
                let mut set = Vec::new();
                let mut negated = false;
                let mut first = true;
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    if first && c == '^' {
                        negated = true;
                    } else {
                        set.push(c);
                    }
                    first = false;
                }
                out.push(Cond::Set {
                    chars: set,
                    negated,
                });
            }
            c => out.push(Cond::Char(c)),
        }
    }
    out
}

/// A prefix or suffix rule: `strip` taken off the stem, `add` put on.
#[derive(Clone, Debug)]
struct Affix {
    flag: Flag,
    strip: String,
    condition: Vec<Cond>,
    cross: bool,
}

/// One dictionary: its stems with their flags and its rules.
pub(crate) struct Dictionary {
    stems: HashMap<String, Vec<Flag>>,
    /// Keyed by what they add.
    prefixes: HashMap<String, Vec<Affix>>,
    suffixes: HashMap<String, Vec<Affix>>,
    try_chars: Vec<char>,
    replacements: Vec<(String, String)>,
    conversions: Vec<(String, String)>,
    need_affix: Option<Flag>,
    forbidden: Option<Flag>,
    only_in_compound: Option<Flag>,
    no_suggest: Option<Flag>,
}

impl Dictionary {
    /// Reads the two files' text.
    pub(crate) fn parse(aff: &str, dic: &str) -> Self {
        let mut dictionary = Self {
            stems: HashMap::new(),
            prefixes: HashMap::new(),
            suffixes: HashMap::new(),
            try_chars: Vec::new(),
            replacements: Vec::new(),
            conversions: Vec::new(),
            need_affix: None,
            forbidden: None,
            only_in_compound: None,
            no_suggest: None,
        };
        let mut mode = FlagMode::Char;
        // Rules still expected under each `PFX`/`SFX` header, and whether
        // the header allows cross products.
        let mut pending: HashMap<(bool, String), (usize, bool)> = HashMap::new();
        for line in aff.lines() {
            let line = line.trim_start_matches('\u{feff}');
            let fields: Vec<&str> = line.split_whitespace().collect();
            let Some(&key) = fields.first() else {
                continue;
            };
            if key.starts_with('#') {
                continue;
            }
            let one = fields.get(1).copied().unwrap_or_default();
            match key {
                "FLAG" => {
                    mode = match one {
                        "long" => FlagMode::Long,
                        "num" => FlagMode::Num,
                        _ => FlagMode::Char,
                    };
                }
                "TRY" => dictionary.try_chars = one.chars().collect(),
                "NEEDAFFIX" | "PSEUDOROOT" => {
                    dictionary.need_affix = flags(one, mode).first().copied();
                }
                "FORBIDDENWORD" => {
                    dictionary.forbidden = flags(one, mode).first().copied();
                }
                "ONLYINCOMPOUND" => {
                    dictionary.only_in_compound = flags(one, mode).first().copied();
                }
                "NOSUGGEST" => dictionary.no_suggest = flags(one, mode).first().copied(),
                "REP" | "ICONV" => {
                    if let [_, from, to, ..] = fields[..] {
                        let pair = (from.replace('_', " "), to.replace('_', " "));
                        if key == "REP" {
                            dictionary.replacements.push(pair);
                        } else {
                            dictionary.conversions.push(pair);
                        }
                    }
                }
                "PFX" | "SFX" => {
                    let prefix = key == "PFX";
                    let Some(&name) = fields.get(1) else {
                        continue;
                    };
                    let slot = (prefix, name.to_owned());
                    match pending.get_mut(&slot) {
                        Some((left, cross)) if *left > 0 => {
                            *left -= 1;
                            let cross = *cross;
                            let (Some(strip), Some(add)) = (fields.get(2), fields.get(3)) else {
                                continue;
                            };
                            let Some(flag) = flags(name, mode).first().copied() else {
                                continue;
                            };
                            let zero = |s: &str| {
                                if s == "0" {
                                    String::new()
                                } else {
                                    s.to_owned()
                                }
                            };
                            // Continuation classes after `/` are not followed.
                            let add = zero(add.split('/').next().unwrap_or_default());
                            let affix = Affix {
                                flag,
                                strip: zero(strip),
                                condition: parse_condition(fields.get(4).copied().unwrap_or(".")),
                                cross,
                            };
                            let table = if prefix {
                                &mut dictionary.prefixes
                            } else {
                                &mut dictionary.suffixes
                            };
                            table.entry(add).or_default().push(affix);
                        }
                        _ => {
                            let count = fields.get(3).and_then(|n| n.parse().ok()).unwrap_or(0);
                            let cross = fields.get(2).is_some_and(|y| *y == "Y");
                            pending.insert(slot, (count, cross));
                        }
                    }
                }
                _ => {}
            }
        }
        let mut lines = dic.lines();
        // The first line counts the stems.
        let _ = lines.next();
        for line in lines {
            let line = line.trim_start_matches('\u{feff}');
            let entry = line.split('\t').next().unwrap_or_default();
            let entry = entry.split_whitespace().next().unwrap_or_default();
            if entry.is_empty() {
                continue;
            }
            let (word, flag_text) = split_entry(entry);
            let list = dictionary.stems.entry(word).or_default();
            for flag in flags(flag_text, mode) {
                if !list.contains(&flag) {
                    list.push(flag);
                }
            }
        }
        dictionary
    }

    fn has(&self, stem: &str, flag: Flag) -> bool {
        self.stems.get(stem).is_some_and(|flags| {
            flags.contains(&flag) && !self.forbidden.is_some_and(|f| flags.contains(&f))
        })
    }

    /// The word as it is, without case changes.
    fn check_exact(&self, word: &str) -> bool {
        if let Some(flags) = self.stems.get(word) {
            let blocked = [self.need_affix, self.forbidden, self.only_in_compound]
                .into_iter()
                .flatten()
                .any(|flag| flags.contains(&flag));
            if !blocked {
                return true;
            }
            if self.forbidden.is_some_and(|f| flags.contains(&f)) {
                return false;
            }
        }
        self.check_suffix(word, None) || self.check_prefix(word)
    }

    /// A stem and one suffix make the word (with the prefix rule `prefix`
    /// already taken off, when there is one).
    fn check_suffix(&self, word: &str, prefix: Option<&Affix>) -> bool {
        for (at, _) in word
            .char_indices()
            .chain(std::iter::once((word.len(), ' ')))
        {
            let Some(rules) = self.suffixes.get(word.get(at..).unwrap_or_default()) else {
                continue;
            };
            let base = word.get(..at).unwrap_or_default();
            for rule in rules {
                if prefix.is_some() && !rule.cross {
                    continue;
                }
                let root = format!("{base}{}", rule.strip);
                if root.is_empty() || !ends_with_condition(&root, &rule.condition) {
                    continue;
                }
                if self.has(&root, rule.flag) && prefix.is_none_or(|p| self.has(&root, p.flag)) {
                    return true;
                }
            }
        }
        false
    }

    fn check_prefix(&self, word: &str) -> bool {
        for (at, _) in word
            .char_indices()
            .skip(1)
            .chain(std::iter::once((word.len(), ' ')))
        {
            let Some(rules) = self.prefixes.get(word.get(..at).unwrap_or_default()) else {
                continue;
            };
            let rest = word.get(at..).unwrap_or_default();
            for rule in rules {
                let root = format!("{}{rest}", rule.strip);
                if root.is_empty() || !starts_with_condition(&root, &rule.condition) {
                    continue;
                }
                if self.has(&root, rule.flag)
                    || (rule.cross && self.check_suffix(&root, Some(rule)))
                {
                    return true;
                }
            }
        }
        false
    }

    fn convert(&self, word: &str) -> String {
        let mut word = word.to_owned();
        for (from, to) in &self.conversions {
            if word.contains(from.as_str()) {
                word = word.replace(from.as_str(), to);
            }
        }
        word
    }

    /// Whether the dictionary knows the word, allowing a capital first
    /// letter or all capitals on a word it has in lower case.
    pub(crate) fn check(&self, word: &str) -> bool {
        let word = self.convert(word);
        if self.check_exact(&word) {
            return true;
        }
        let lower = word.to_lowercase();
        if lower == word {
            return false;
        }
        if self.check_exact(&lower) {
            return true;
        }
        // All capitals: also the capitalised form ("ПАРИЖ" for "Париж").
        word.chars().all(|c| !c.is_lowercase()) && self.check_exact(&capitalised(&lower))
    }

    fn suggestible(&self, word: &str) -> bool {
        self.check(word)
            && !self.no_suggest.is_some_and(|flag| {
                self.stems
                    .get(word)
                    .is_some_and(|flags| flags.contains(&flag))
            })
    }

    /// Words one edit away the dictionary knows, the likeliest first.
    pub(crate) fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let capital = word.chars().next().is_some_and(char::is_uppercase);
        let all_caps = word.chars().count() > 1 && word.chars().all(|c| !c.is_lowercase());
        let lower = word.to_lowercase();
        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut offer = |candidate: String, out: &mut Vec<String>| {
            if out.len() < limit
                && candidate != lower
                && seen.insert(candidate.clone())
                && self.suggestible(&candidate)
            {
                out.push(candidate);
            }
        };
        for (from, to) in &self.replacements {
            let mut start = 0;
            while let Some(found) = lower.get(start..).and_then(|rest| rest.find(from.as_str())) {
                let at = start + found;
                let candidate = format!(
                    "{}{to}{}",
                    lower.get(..at).unwrap_or_default(),
                    lower.get(at + from.len()..).unwrap_or_default()
                );
                offer(candidate, &mut out);
                start = at + from.len().max(1);
            }
        }
        let chars: Vec<char> = lower.chars().collect();
        let joined = |parts: &[char]| parts.iter().collect::<String>();
        // Two letters swapped.
        for i in 0..chars.len().saturating_sub(1) {
            let mut c = chars.clone();
            c.swap(i, i + 1);
            offer(joined(&c), &mut out);
        }
        // A letter wrong.
        for i in 0..chars.len() {
            for t in &self.try_chars {
                if t.is_lowercase() && chars.get(i) != Some(t) {
                    let mut c = chars.clone();
                    if let Some(slot) = c.get_mut(i) {
                        *slot = *t;
                    }
                    offer(joined(&c), &mut out);
                }
            }
        }
        // A letter too many.
        for i in 0..chars.len() {
            let mut c = chars.clone();
            c.remove(i);
            offer(joined(&c), &mut out);
        }
        // A letter missing.
        for i in 0..=chars.len() {
            for t in &self.try_chars {
                if t.is_lowercase() {
                    let mut c = chars.clone();
                    c.insert(i, *t);
                    offer(joined(&c), &mut out);
                }
            }
        }
        // Two words run together.
        for i in 2..chars.len().saturating_sub(1) {
            let (a, b) = (joined(&chars[..i]), joined(&chars[i..]));
            if self.check(&a) && self.check(&b) && out.len() < limit {
                out.push(format!("{a} {b}"));
            }
        }
        out.into_iter()
            .map(|s| {
                if all_caps {
                    s.to_uppercase()
                } else if capital {
                    capitalised(&s)
                } else {
                    s
                }
            })
            .collect()
    }
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn ends_with_condition(root: &str, condition: &[Cond]) -> bool {
    let mut chars = root.chars().rev();
    condition
        .iter()
        .rev()
        .all(|cond| chars.next().is_some_and(|c| cond.matches(c)))
}

fn starts_with_condition(root: &str, condition: &[Cond]) -> bool {
    let mut chars = root.chars();
    condition
        .iter()
        .all(|cond| chars.next().is_some_and(|c| cond.matches(c)))
}

/// A `.dic` entry's word and its flags, `\/` being a slash in the word.
fn split_entry(entry: &str) -> (String, &str) {
    let mut word = String::new();
    let mut chars = entry.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' if chars.peek().is_some_and(|(_, next)| *next == '/') => {
                word.push('/');
                chars.next();
            }
            '/' => return (word, entry.get(at + 1..).unwrap_or_default()),
            c => word.push(c),
        }
    }
    (word, "")
}

fn flags(text: &str, mode: FlagMode) -> Vec<Flag> {
    match mode {
        FlagMode::Char => text.chars().map(u32::from).collect(),
        FlagMode::Long => {
            let chars: Vec<char> = text.chars().collect();
            chars
                .chunks(2)
                .map(|pair| pair.iter().fold(0, |flag, c| (flag << 16) | u32::from(*c)))
                .collect()
        }
        FlagMode::Num => text
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect(),
    }
}

/// The writing systems the dictionaries are chosen by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Script {
    Latin,
    Cyrillic,
}

fn script_of(word: &str) -> Option<Script> {
    let mut found = None;
    for c in word.chars().filter(|c| c.is_alphabetic()) {
        let script = match u32::from(c) {
            0x41..=0x5a | 0x61..=0x7a | 0xc0..=0x24f => Script::Latin,
            0x400..=0x52f => Script::Cyrillic,
            _ => return None,
        };
        if found.is_some_and(|f| f != script) {
            return None;
        }
        found = Some(script);
    }
    found
}

/// A language a dictionary can be fetched for.
pub(crate) struct Language {
    /// The folder name under `wooorm/dictionaries`, and the file names.
    pub(crate) code: &'static str,
    /// The locale key of its name.
    pub(crate) name_key: &'static str,
    pub(crate) script: Script,
    /// Roughly, in megabytes, to say before fetching.
    pub(crate) size_mb: f32,
}

pub(crate) const LANGUAGES: &[Language] = &[
    Language {
        code: "ru",
        name_key: "spell.lang_ru",
        script: Script::Cyrillic,
        size_mb: 3.5,
    },
    Language {
        code: "en",
        name_key: "spell.lang_en",
        script: Script::Latin,
        size_mb: 0.6,
    },
    Language {
        code: "en-GB",
        name_key: "spell.lang_en_gb",
        script: Script::Latin,
        size_mb: 0.6,
    },
    Language {
        code: "uk",
        name_key: "spell.lang_uk",
        script: Script::Cyrillic,
        size_mb: 8.7,
    },
    Language {
        code: "de",
        name_key: "spell.lang_de",
        script: Script::Latin,
        size_mb: 1.1,
    },
    Language {
        code: "fr",
        name_key: "spell.lang_fr",
        script: Script::Latin,
        size_mb: 1.4,
    },
    Language {
        code: "es",
        name_key: "spell.lang_es",
        script: Script::Latin,
        size_mb: 0.9,
    },
];

/// The language offered first for a script, when the words do not say which.
pub(crate) fn language_for(script: Script) -> &'static str {
    match script {
        Script::Cyrillic => "ru",
        Script::Latin => "en",
    }
}

struct Probe {
    code: &'static str,
    words: &'static [&'static str],
    letters: &'static [char],
}

const PROBES: &[Probe] = &[
    Probe {
        code: "en",
        words: &[
            "the", "and", "of", "to", "in", "is", "that", "for", "with", "on", "was", "are",
            "this", "from", "have", "they", "their", "not", "you", "we",
        ],
        letters: &[],
    },
    Probe {
        code: "de",
        words: &[
            "der", "die", "das", "und", "den", "dem", "des", "ein", "eine", "nicht", "ist", "von",
            "mit", "auf", "für", "sich", "auch", "oder", "nach", "bei", "aus", "dass", "wir",
        ],
        letters: &['ä', 'ö', 'ü', 'ß'],
    },
    Probe {
        code: "fr",
        words: &[
            "les", "des", "une", "est", "pas", "qui", "dans", "pour", "sur", "avec", "sont",
            "cette", "aux", "nous", "vous", "elle", "mais", "leur", "plus",
        ],
        letters: &[
            'ç', 'œ', 'à', 'è', 'ê', 'ë', 'â', 'î', 'ï', 'ô', 'ù', 'û', 'ÿ',
        ],
    },
    Probe {
        code: "es",
        words: &[
            "los", "las", "una", "por", "con", "para", "del", "está", "están", "también", "porque",
            "cuando", "sobre", "entre", "hay", "muy", "esto", "pero", "como", "sus",
        ],
        letters: &['ñ', 'á', 'í', 'ó', 'ú', '¿', '¡'],
    },
    Probe {
        code: "ru",
        words: &[
            "не",
            "что",
            "это",
            "как",
            "все",
            "она",
            "его",
            "для",
            "если",
            "или",
            "уже",
            "только",
            "когда",
            "чтобы",
            "можно",
            "был",
            "была",
            "они",
            "этот",
            "есть",
        ],
        letters: &['ы', 'э', 'ё'],
    },
    Probe {
        code: "uk",
        words: &[
            "що",
            "як",
            "це",
            "від",
            "але",
            "або",
            "вже",
            "їх",
            "також",
            "між",
            "під",
            "його",
            "цей",
            "якщо",
            "немає",
            "які",
            "який",
            "вони",
            "була",
            "тільки",
        ],
        letters: &['і', 'ї', 'є', 'ґ'],
    },
];

const BRITISH: &[&str] = &[
    "colour",
    "colours",
    "favour",
    "honour",
    "organise",
    "organised",
    "organisation",
    "whilst",
    "amongst",
    "centre",
    "theatre",
    "grey",
    "defence",
    "practise",
];

struct ProbeScore {
    code: &'static str,
    best: u32,
    second: u32,
    british: u32,
}

fn probe_score<'a>(words: impl IntoIterator<Item = &'a str>) -> ProbeScore {
    let mut scores = [0u32; PROBES.len()];
    let mut british = 0u32;
    for (seen, word) in words.into_iter().enumerate() {
        if seen >= 400 {
            break;
        }
        let lower = word.to_lowercase();
        if BRITISH.contains(&lower.as_str()) {
            british += 1;
        }
        for (index, probe) in PROBES.iter().enumerate() {
            if probe.words.contains(&lower.as_str()) {
                scores[index] += 2;
            }
            if lower.chars().any(|c| probe.letters.contains(&c)) {
                scores[index] += 3;
            }
        }
    }
    let mut best_at = 0usize;
    let mut best = 0u32;
    let mut second = 0u32;
    for (index, score) in scores.iter().copied().enumerate() {
        if score > best {
            second = best;
            best = score;
            best_at = index;
        } else if score > second {
            second = score;
        }
    }
    ProbeScore {
        code: PROBES[best_at].code,
        best,
        second,
        british,
    }
}

fn pick(score: &ProbeScore, minimum: u32, lead: u32) -> Option<&'static str> {
    if score.best < minimum || score.best < score.second.saturating_add(lead) {
        return None;
    }
    if score.code == "en" && score.british >= 2 {
        Some("en-GB")
    } else {
        Some(score.code)
    }
}

/// The dictionary the words most resemble, when one language leads clearly.
/// At most the first 400 words are read. `None` when the sample is too small
/// or two languages stay close.
pub(crate) fn detect_language<'a>(
    words: impl IntoIterator<Item = &'a str>,
) -> Option<&'static str> {
    pick(&probe_score(words), 6, 4)
}

/// A weaker reading, used only to offer a dictionary.
///
/// A short paragraph can lead by a couple of function words. That is enough
/// to offer its dictionary, and not enough to treat the language as known.
pub(crate) fn language_hint<'a>(words: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    pick(&probe_score(words), 2, 1)
}

pub(crate) fn language(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|language| language.code == code)
}

/// What a word is to the speller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Right,
    Wrong,
    /// No dictionary for its script.
    Unknown(Script),
}

/// The installed dictionaries and the user's own words.
pub(crate) struct Speller {
    dictionaries: Vec<(Script, Dictionary)>,
    own: RwLock<HashSet<String>>,
    own_path: Option<PathBuf>,
    /// Words skipped for this session.
    ignored: RwLock<HashSet<String>>,
}

impl std::fmt::Debug for Speller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speller")
            .field("dictionaries", &self.dictionaries.len())
            .finish_non_exhaustive()
    }
}

impl Speller {
    pub(crate) fn new(
        dictionaries: Vec<(Script, Dictionary)>,
        own: HashSet<String>,
        own_path: Option<PathBuf>,
    ) -> Self {
        Self {
            dictionaries,
            own: RwLock::new(own),
            own_path,
            ignored: RwLock::new(HashSet::new()),
        }
    }

    /// Loads every installed dictionary and the user's words.
    pub(crate) fn load() -> Self {
        let dir = folder();
        let mut dictionaries = Vec::new();
        for language in LANGUAGES {
            let Some(dir) = dir.as_ref() else {
                break;
            };
            let (aff, dic) = files(dir, language.code);
            if let (Ok(aff), Ok(dic)) = (std::fs::read(&aff), std::fs::read(&dic)) {
                let dictionary = Dictionary::parse(
                    &String::from_utf8_lossy(&aff),
                    &String::from_utf8_lossy(&dic),
                );
                dictionaries.push((language.script, dictionary));
            }
        }
        let own_path = dir.map(|dir| dir.join("user.txt"));
        let own = own_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Self::new(dictionaries, own, own_path)
    }

    pub(crate) fn verdict(&self, word: &str) -> Verdict {
        let Some(script) = script_of(word) else {
            return Verdict::Right;
        };
        let known = |set: &RwLock<HashSet<String>>| {
            set.read()
                .is_ok_and(|set| set.contains(word) || set.contains(&word.to_lowercase()))
        };
        if known(&self.own) || known(&self.ignored) {
            return Verdict::Right;
        }
        let mut any = false;
        for (_, dictionary) in self.dictionaries.iter().filter(|(s, _)| *s == script) {
            any = true;
            if dictionary.check(word) {
                return Verdict::Right;
            }
        }
        if any {
            Verdict::Wrong
        } else {
            Verdict::Unknown(script)
        }
    }

    pub(crate) fn suggest(&self, word: &str) -> Vec<String> {
        let script = script_of(word);
        let mut out = Vec::new();
        for (_, dictionary) in self.dictionaries.iter().filter(|(s, _)| Some(*s) == script) {
            for suggestion in dictionary.suggest(word, 8) {
                if !out.contains(&suggestion) {
                    out.push(suggestion);
                }
            }
        }
        out.truncate(8);
        out
    }

    /// Skips the word until the editor closes.
    pub(crate) fn ignore(&self, word: &str) {
        if let Ok(mut ignored) = self.ignored.write() {
            ignored.insert(word.to_owned());
        }
    }

    /// The user's own words, in alphabetical order.
    pub(crate) fn own_words(&self) -> Vec<String> {
        let Ok(own) = self.own.read() else {
            return Vec::new();
        };
        let mut words: Vec<String> = own.iter().cloned().collect();
        words.sort();
        words
    }

    /// Adds the word to the user's dictionary file.
    pub(crate) fn add_own(&self, word: &str) {
        if let Ok(mut own) = self.own.write() {
            own.insert(word.to_owned());
            if let Some(path) = &self.own_path {
                write_own(path, &own);
            }
        }
    }

    /// Takes the word back out of the user's dictionary file.
    pub(crate) fn remove_own(&self, word: &str) {
        if let Ok(mut own) = self.own.write() {
            own.remove(word);
            if let Some(path) = &self.own_path {
                write_own(path, &own);
            }
        }
    }
}

/// One word per line, in order, so the file is stable.
fn write_own(path: &Path, own: &HashSet<String>) {
    let mut words: Vec<&String> = own.iter().collect();
    words.sort();
    let mut text = String::new();
    for word in words {
        text.push_str(word);
        text.push('\n');
    }
    let _ = std::fs::write(path, text);
}

/// Deletes a fetched dictionary. Only a known language code is accepted.
pub(crate) fn remove(code: &str) -> Result<(), String> {
    if language(code).is_none() {
        return Err(format!("unknown dictionary {code}"));
    }
    let dir = folder().ok_or_else(|| "no configuration folder".to_owned())?;
    let (aff, dic) = files(&dir, code);
    for path in [&aff, &dic] {
        if path.is_file() {
            std::fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// A word in a paragraph's text, by character offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) text: String,
}

/// The words worth checking: letters, with apostrophes between them;
/// words in capitals, of one letter or touching digits are left alone.
pub(crate) fn words(text: &str) -> Vec<Word> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars.get(i).is_some_and(|c| c.is_alphabetic()) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() {
            let c = chars.get(i).copied().unwrap_or(' ');
            let inner_apostrophe = matches!(c, '\'' | '\u{2019}')
                && chars.get(i + 1).is_some_and(|n| n.is_alphabetic())
                && i > start;
            if c.is_alphabetic() || is_mark(c) || inner_apostrophe {
                i += 1;
            } else {
                break;
            }
        }
        let word: String = chars.get(start..i).unwrap_or_default().iter().collect();
        let touches_digit = start
            .checked_sub(1)
            .and_then(|p| chars.get(p))
            .is_some_and(char::is_ascii_digit)
            || chars.get(i).is_some_and(char::is_ascii_digit);
        let capitals = word.chars().all(|c| !c.is_lowercase());
        if word.chars().count() > 1 && !touches_digit && !capitals {
            out.push(Word {
                start,
                end: i,
                text: word.replace('\u{2019}', "'"),
            });
        }
    }
    out
}

fn is_mark(c: char) -> bool {
    matches!(u32::from(c), 0x300..=0x36f)
}

/// Where the dictionaries live.
pub(crate) fn folder() -> Option<PathBuf> {
    crate::settings::config_dir().map(|dir| dir.join("dictionaries"))
}

fn files(dir: &Path, code: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("{code}.aff")),
        dir.join(format!("{code}.dic")),
    )
}

pub(crate) fn installed(code: &str) -> bool {
    folder().is_some_and(|dir| {
        let (aff, dic) = files(&dir, code);
        aff.is_file() && dic.is_file()
    })
}

/// Fetches a language's two files into the dictionaries folder.
pub(crate) fn download(code: &str) -> Result<(), String> {
    use std::io::Read as _;
    let dir = folder().ok_or("no configuration folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .timeout_global(Some(std::time::Duration::from_secs(600)))
        .build()
        .into();
    let (aff, dic) = files(&dir, code);
    for (extension, path) in [("aff", &aff), ("dic", &dic)] {
        let url = format!("https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries/{code}/index.{extension}");
        let mut response = agent.get(&url).call().map_err(|e| format!("{url}: {e}"))?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(64 << 20)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{url}: {e}"))?;
        if std::str::from_utf8(&bytes).is_err() {
            return Err(format!("{url}: not UTF-8 text"));
        }
        let part = path.with_extension(format!("{extension}.part"));
        std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&part, path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "SET UTF-8\nTRY оеаитнс\nREP 1\nREP 1\nREP ться тся\nNEEDAFFIX z\n\
        SFX A Y 2\nSFX A 0 ы [^ая]\nSFX A а и а\n\
        SFX B N 1\nSFX B ть л ть\n\
        PFX P Y 1\nPFX P 0 пере .\n\
        SFX E Y 1\nSFX E 0 s .\n";
    const DIC: &str = "5\nкот/A\nрука/A\nделать/BP\ncat/E\nкорень/z\n";

    fn dictionary() -> Dictionary {
        Dictionary::parse(AFF, DIC)
    }

    #[test]
    fn stems_and_suffixes_are_known() {
        let d = dictionary();
        for word in [
            "кот",
            "коты",
            "рука",
            "руки",
            "делать",
            "делал",
            "cats",
            "Коты",
            "переделать",
        ] {
            assert!(d.check(word), "{word}");
        }
        for word in ["котs", "рукаы", "делаты", "переделал", "корень", "dog"]
        {
            assert!(!d.check(word), "{word}");
        }
    }

    #[test]
    fn suggestions_are_one_edit_away() {
        let d = dictionary();
        assert_eq!(d.suggest("кто", 5).first().map(String::as_str), Some("кот"));
        assert!(
            d.suggest("Руку", 5).contains(&"Рука".to_owned()),
            "{:?}",
            d.suggest("Руку", 5)
        );
        assert!(d.suggest("котыкот", 5).iter().any(|s| s == "коты кот"));
    }

    #[test]
    fn words_skip_capitals_digits_and_single_letters() {
        let found: Vec<String> = words("Он в 2-х ЧПУ, don\u{2019}t 3rd кот-рыба")
            .into_iter()
            .map(|w| w.text)
            .collect();
        assert_eq!(found, ["Он", "don't", "кот", "рыба"]);
    }

    #[test]
    fn a_paragraph_picks_its_language() {
        let guess = |text: &str| detect_language(words(text).iter().map(|word| word.text.as_str()));
        assert_eq!(
            guess("Der Hund und die Katze sind nicht im Haus, und das ist auch so."),
            Some("de")
        );
        assert_eq!(
            guess("Les chats sont dans la maison avec nous, mais cette porte est pour vous."),
            Some("fr")
        );
        assert_eq!(
            guess("Los gatos están en la casa porque hay muy poco para ellos, pero esto es suyo."),
            Some("es")
        );
        assert_eq!(
            guess("The cat and the dog are in the house with their food from the shop."),
            Some("en")
        );
        assert_eq!(
            guess("The colour of the theatre and the colour of the centre are not grey."),
            Some("en-GB")
        );
        assert_eq!(
            guess("Это не дом для кота, если он был там только когда все уже ушли."),
            Some("ru")
        );
        assert_eq!(
            guess("Це не дім для кота, якщо він уже був там, але вони також під вікном."),
            Some("uk")
        );
        assert_eq!(guess("кот котт"), None);
        assert_eq!(language_hint(["und", "der"]), Some("de"));
        assert_eq!(detect_language(["und", "der"]), None);
        assert_eq!(language_hint(["кот", "котт"]), None);
    }

    #[test]
    fn a_script_without_a_dictionary_is_unknown() {
        let speller = Speller::new(vec![(Script::Cyrillic, dictionary())], HashSet::new(), None);
        assert_eq!(speller.verdict("коты"), Verdict::Right);
        assert_eq!(speller.verdict("котт"), Verdict::Wrong);
        assert_eq!(speller.verdict("hello"), Verdict::Unknown(Script::Latin));
    }

    #[test]
    fn a_personal_word_can_be_taken_back() {
        let path = std::env::temp_dir().join(format!("mufoed-own-{}.txt", std::process::id()));
        let speller = Speller::new(Vec::new(), HashSet::new(), Some(path.clone()));
        speller.add_own("MuFoEd");
        speller.add_own("кот");
        assert_eq!(
            speller.own_words(),
            vec!["MuFoEd".to_owned(), "кот".to_owned()]
        );
        speller.remove_own("MuFoEd");
        assert_eq!(speller.own_words(), vec!["кот".to_owned()]);
        let stored = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(stored, "кот\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_unknown_dictionary_is_not_removed() {
        assert!(remove("not-a-language").is_err());
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// The real dictionaries in `MUFOED_DICTS` (a folder of `ru.aff`,
    /// `ru.dic`, `en.aff`, `en.dic`).
    #[test]
    #[ignore = "needs dictionaries in MUFOED_DICTS"]
    fn real_dictionaries() {
        let dir = PathBuf::from(std::env::var("MUFOED_DICTS").unwrap());
        let load = |code: &str| {
            let started = std::time::Instant::now();
            let (aff, dic) = files(&dir, code);
            let d = Dictionary::parse(
                &std::fs::read_to_string(aff).unwrap(),
                &std::fs::read_to_string(dic).unwrap(),
            );
            eprintln!(
                "{code}: {} stems in {} ms",
                d.stems.len(),
                started.elapsed().as_millis()
            );
            d
        };
        let ru = load("ru");
        let en = load("en");
        let text = "Мама мыла раму. Документы были сохранены, а редактирование продолжалось быстрее, чем ожидалось. \
            Переписанные, красивейшие, неудобный, пробежавший, Москвой, Россия, ПРИВЕТ";
        for w in words(text) {
            assert!(ru.check(&w.text), "ru: {}", w.text);
        }
        for w in words("The quick brown fox jumps over the lazy dog. Running, reopened, unhappily, children's, Paris, I'm") {
            assert!(en.check(&w.text), "en: {}", w.text);
        }
        for bad in ["превет", "дакумент", "сохронены"] {
            assert!(!ru.check(bad), "ru: {bad}");
            let started = std::time::Instant::now();
            let s = ru.suggest(bad, 8);
            eprintln!("{bad} -> {s:?} in {} ms", started.elapsed().as_millis());
        }
        for bad in ["teh", "recieve", "documnet"] {
            assert!(!en.check(bad), "en: {bad}");
            eprintln!("{bad} -> {:?}", en.suggest(bad, 8));
        }
    }
}
