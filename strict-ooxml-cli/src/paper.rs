//! The default paper size a render substitutes when a section declares no
//! usable `w:pgSz`.
//!
//! OOXML leaves the default page size to the application, and Word and WPS
//! take it from the machine: US Letter in the US and Canada, A4 on metric
//! machines. `--default-page-size auto` (the default) reads the same locale
//! setting; `letter` and `a4` force one. The library itself stays
//! machine-independent (ADR-0006) — this is the CLI boundary, like
//! `--system-fonts`.

use strict_ooxml::DefaultPaper;

/// Parses `letter`, `a4` or `auto` (case-insensitive).
pub(super) fn parse(value: &str) -> Result<DefaultPaper, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "letter" => Ok(DefaultPaper::Letter),
        "a4" => Ok(DefaultPaper::A4),
        "auto" => Ok(detect()),
        other => Err(format!(
            "unknown paper '{other}' (expected letter, a4 or auto)"
        )),
    }
}

/// The paper size this machine's locale uses by default.
///
/// On Windows this is `LOCALE_IPAPERSIZE` for the user locale — the same
/// setting Word and WPS read (a `DMPAPER_*` value: the A4 family is A4,
/// everything else Letter). Elsewhere the default is A4, the metric majority.
#[must_use]
pub(super) fn detect() -> DefaultPaper {
    #[cfg(windows)]
    if let Some(paper) = windows_paper() {
        return match paper {
            // DMPAPER_A4, DMPAPER_A4SMALL, DMPAPER_A4TRANSVERSE.
            9..=11 => DefaultPaper::A4,
            _ => DefaultPaper::Letter,
        };
    }
    DefaultPaper::A4
}

#[cfg(windows)]
// The one call the platform needs: `GetLocaleInfoW` has no safe wrapper in std,
// and pulling a Windows binding crate in for a 40-line read is not worth it.
// The block is confined to this function, with the buffer contract written out.
#[allow(unsafe_code)]
fn windows_paper() -> Option<i32> {
    use std::os::raw::{c_int, c_uint};

    // LOCALE_USER_DEFAULT; LOCALE_IPAPERSIZE (returns a decimal DMPAPER_* id).
    const LOCALE_USER_DEFAULT: c_uint = 0x0400;
    const LOCALE_IPAPERSIZE: c_uint = 0x100A;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocaleInfoW(locale: c_uint, kind: c_uint, buffer: *mut u16, length: c_int) -> c_int;
    }

    let mut buffer = [0u16; 16];
    // SAFETY: `buffer` is a valid, writable array of `buffer.len()` u16s and the
    // length is passed exactly; the API writes at most that many and returns the
    // count (including the NUL) it wrote.
    let written = unsafe {
        GetLocaleInfoW(
            LOCALE_USER_DEFAULT,
            LOCALE_IPAPERSIZE,
            buffer.as_mut_ptr(),
            c_int::try_from(buffer.len()).unwrap_or(c_int::MAX),
        )
    };
    if written <= 0 {
        return None;
    }
    let len = usize::try_from(written).ok()?.saturating_sub(1);
    String::from_utf16_lossy(buffer.get(..len)?).trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse;
    use strict_ooxml::DefaultPaper;

    #[test]
    fn explicit_names_parse() {
        assert_eq!(parse("letter").unwrap(), DefaultPaper::Letter);
        assert_eq!(parse(" A4 ").unwrap(), DefaultPaper::A4);
        assert!(parse("auto").is_ok());
        assert!(parse("tabloid").is_err());
    }
}
