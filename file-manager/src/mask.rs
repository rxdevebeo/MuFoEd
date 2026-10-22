//! File masks in the manner of Total Commander.
//!
//! A mask is a list of wildcard patterns separated by blanks or `;`; `*`
//! matches any run of characters and `?` exactly one; matching ignores case.
//! Patterns after a `|` exclude: `*.jpg *.png | thumb*` selects JPEG and PNG
//! files whose names do not start with `thumb`. A pattern without `.` and
//! wildcards also matches names that merely contain it, the way TC's quick
//! filter does when typed without wildcards.

/// A parsed mask.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mask {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl Mask {
    /// Parses a mask; an empty include list matches everything.
    pub fn parse(text: &str) -> Self {
        let (include, exclude) = text.split_once('|').unwrap_or((text, ""));
        let patterns = |part: &str| -> Vec<String> {
            part.split(|c: char| c == ';' || c.is_whitespace())
                .map(str::trim)
                .filter(|pattern| !pattern.is_empty())
                .map(str::to_lowercase)
                .collect()
        };
        Self {
            include: patterns(include),
            exclude: patterns(exclude),
        }
    }

    /// Whether the mask is empty (matches everything).
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Whether `name` (a file name, not a path) matches.
    pub fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        let included = self.include.is_empty()
            || self
                .include
                .iter()
                .any(|pattern| pattern_matches(pattern, &name));
        included
            && !self
                .exclude
                .iter()
                .any(|pattern| pattern_matches(pattern, &name))
    }
}

fn pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern == "*.*" || pattern == "*" {
        return true;
    }
    if !pattern.contains(['*', '?', '.']) {
        return name.contains(pattern);
    }
    // `name.*` should also match a name without any extension, as in TC.
    if let Some(stem) = pattern.strip_suffix(".*") {
        if !name.contains('.') && wildcard(stem, name) {
            return true;
        }
    }
    wildcard(pattern, name)
}

/// Iterative wildcard matching with backtracking over the last `*`.
fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((star_p, star_n)) = star {
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, star_n + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_and_exclusions() {
        let mask = Mask::parse("*.jpg; *.PNG | thumb*");
        assert!(mask.matches("Photo.JPG"));
        assert!(mask.matches("a.png"));
        assert!(!mask.matches("thumb_1.jpg"));
        assert!(!mask.matches("notes.txt"));
        assert!(Mask::parse("").matches("anything"));
        assert!(Mask::parse("*.*").matches("noextension"));
        assert!(Mask::parse("readme.*").matches("README"));
        assert!(Mask::parse("?at.txt").matches("cat.txt"));
        assert!(!Mask::parse("?at.txt").matches("at.txt"));
        assert!(Mask::parse("a*b*c").matches("aXXbYYc"));
        assert!(!Mask::parse("a*b*c").matches("aXXbYY"));
        assert!(Mask::parse("report").matches("Annual-Report-2024.pdf"));
        assert!(Mask::parse("| *.tmp").matches("keep.txt"));
        assert!(!Mask::parse("| *.tmp").matches("drop.tmp"));
    }
}
