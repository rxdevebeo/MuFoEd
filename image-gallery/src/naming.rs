//! File name templates.
//!
//! A template is text with fields in braces:
//!
//! | field | value |
//! |---|---|
//! | `{name}` | the source name without its extension |
//! | `{ext}` | the source extension |
//! | `{n}`, `{n:3}` | the running number, optionally zero-padded |
//! | `{w}`, `{h}` | the picture's width and height |
//! | `{date}`, `{time}` | when the shot was taken (EXIF), or the file time |
//! | `{camera}` | the camera model |
//! | `{folder}` | the source folder's name |
//!
//! An unknown field is kept as written. Characters that file systems reject
//! become `_`; the extension of the target format is added by the caller.

use crate::exif::Stamp;

/// What a template can refer to.
#[derive(Clone, Debug, Default)]
pub struct NameContext<'a> {
    /// The source name without its extension.
    pub stem: &'a str,
    /// The source extension, without the dot.
    pub extension: &'a str,
    /// The running number.
    pub index: usize,
    /// Width in pixels (after processing), when known.
    pub width: Option<u32>,
    /// Height in pixels (after processing), when known.
    pub height: Option<u32>,
    /// Shot or file time.
    pub taken: Option<Stamp>,
    /// Camera model.
    pub camera: Option<&'a str>,
    /// The source folder's name.
    pub folder: &'a str,
}

/// Expands `template`. An empty result becomes the source stem.
pub fn render(template: &str, context: &NameContext<'_>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let field = &after[..close];
        if let Some(value) = expand(field, context) {
            out.push_str(&value);
        } else {
            out.push('{');
            out.push_str(field);
            out.push('}');
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    let clean = sanitize(&out);
    if clean.is_empty() {
        sanitize(context.stem)
    } else {
        clean
    }
}

fn expand(field: &str, context: &NameContext<'_>) -> Option<String> {
    let (name, argument) = field.split_once(':').unwrap_or((field, ""));
    Some(match name {
        "name" => context.stem.to_owned(),
        "ext" => context.extension.to_owned(),
        "n" => {
            let width = argument.parse::<usize>().unwrap_or(0).min(12);
            format!("{:0width$}", context.index)
        }
        "w" => context
            .width
            .map(|value| value.to_string())
            .unwrap_or_default(),
        "h" => context
            .height
            .map(|value| value.to_string())
            .unwrap_or_default(),
        "date" => context.taken.map(|t| t.date()).unwrap_or_default(),
        "time" => context.taken.map(|t| t.time()).unwrap_or_default(),
        "camera" => context.camera.unwrap_or_default().to_owned(),
        "folder" => context.folder.to_owned(),
        _ => return None,
    })
}

/// Replaces characters Windows, macOS and Linux reject in file names, and
/// trims the dots and blanks Windows drops at the end.
pub fn sanitize(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = replaced.trim().trim_end_matches(['.', ' ']);
    let upper = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        format!("_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_fields() {
        let context = NameContext {
            stem: "IMG_0001",
            extension: "jpg",
            index: 7,
            width: Some(1920),
            height: Some(1080),
            taken: Stamp::parse("2024:01:02 03:04:05"),
            camera: Some("X100V"),
            folder: "trip",
        };
        assert_eq!(render("{name}", &context), "IMG_0001");
        assert_eq!(render("{folder}-{n:4}", &context), "trip-0007");
        assert_eq!(
            render("{date}_{time}_{camera}", &context),
            "2024-01-02_03-04-05_X100V"
        );
        assert_eq!(render("{name}_{w}x{h}", &context), "IMG_0001_1920x1080");
        assert_eq!(render("{unknown} {name", &context), "{unknown} {name");
        assert_eq!(render("", &context), "IMG_0001");
    }

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize("a/b:c*?.  "), "a_b_c__");
        assert_eq!(sanitize("con"), "_con");
        assert_eq!(sanitize("COM1.txt"), "_COM1.txt");
        assert_eq!(sanitize("console"), "console");
    }
}
