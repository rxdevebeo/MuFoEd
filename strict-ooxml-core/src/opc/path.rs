//! Canonicalization of package and relationship target paths.
//!
//! Implements the path rules of OPC (ISO/IEC 29500-2): part names are relative
//! to the package root, use `/` as the separator, and may not escape the root.
//! Relationship targets may additionally be absolute (leading `/`) or relative
//! to the directory of the source part (stage task S1.5).
//!
//! All functions are panic-free and use checked arithmetic; on any unsafe input
//! they return [`StrictError::InvalidPartName`].

use crate::error::{Result, StrictError};
use crate::part::PartId;

/// Canonicalizes a ZIP entry name into an absolute [`PartId`].
///
/// The name must be non-empty, relative, use `/` separators and must not
/// escape the package root via `..`. Backslashes and NUL bytes are rejected.
///
/// # Errors
///
/// Returns [`StrictError::InvalidPartName`] when the name is unsafe or empty.
pub fn canonicalize_part_name(name: &str) -> Result<PartId> {
    if name.is_empty() {
        return Err(StrictError::InvalidPartName("empty part name".to_owned()));
    }
    if name.starts_with('/') {
        return Err(StrictError::InvalidPartName(format!(
            "part name must be relative: {name}"
        )));
    }
    let segments = split_and_normalize(name)?;
    let joined = segments.join("/");
    if joined.is_empty() {
        return Err(StrictError::InvalidPartName(format!(
            "part name resolves to the package root: {name}"
        )));
    }
    Ok(PartId::new(format!("/{joined}").as_str()))
}

/// Resolves a relationship target relative to `base` into an absolute part id.
///
/// `base` is the part that owns the `.rels` file (or the package root, modelled
/// as `/`). A target starting with `/` is treated as absolute; otherwise it is
/// resolved against the directory of `base`. Returns `Ok(None)` for external
/// targets, which are recorded but never resolved or fetched.
///
/// # Errors
///
/// Returns [`StrictError::InvalidPartName`] for unsafe or escaping targets.
pub fn resolve_target(base: &PartId, target: &str, external: bool) -> Result<Option<PartId>> {
    if external {
        return Ok(None);
    }
    if target.is_empty() {
        return Err(StrictError::InvalidPartName(
            "empty relationship target".to_owned(),
        ));
    }
    let absolute = target.starts_with('/');
    let combined = if absolute {
        target.trim_start_matches('/').to_owned()
    } else {
        let mut path = String::with_capacity(base.as_str().len() + target.len());
        // Directory of the base part: everything up to and including the last
        // `/`. For `/word/document.xml` this is `/word/`.
        if let Some(slash) = base.as_str().rfind('/') {
            path.push_str(&base.as_str()[..=slash]);
        }
        path.push_str(target);
        path
    };
    let segments = split_and_normalize(&combined)?;
    let joined = segments.join("/");
    if joined.is_empty() {
        return Err(StrictError::InvalidPartName(format!(
            "relationship target resolves to the package root: {target}"
        )));
    }
    Ok(Some(PartId::new(format!("/{joined}").as_str())))
}

/// Splits a package path on `/`, rejecting unsafe segments and normalizing
/// `.`/`..`. Returns the normalized segments (without a leading empty segment).
fn split_and_normalize(path: &str) -> Result<Vec<String>> {
    if path.contains('\\') {
        return Err(StrictError::InvalidPartName(format!(
            "backslash is not a valid separator: {path}"
        )));
    }
    if path.contains('\0') {
        return Err(StrictError::InvalidPartName("NUL byte in path".to_owned()));
    }
    let mut segments: Vec<String> = Vec::new();
    for raw in path.split('/') {
        if raw.is_empty() {
            // Leading or repeated separators collapse; a leading `/` was
            // already handled by the callers.
            continue;
        }
        if raw.chars().any(char::is_control) {
            return Err(StrictError::InvalidPartName(format!(
                "control character in path: {path}"
            )));
        }
        match raw {
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(StrictError::InvalidPartName(format!(
                        "path escapes the package root: {path}"
                    )));
                }
            }
            other => segments.push(other.to_owned()),
        }
    }
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_part_name, resolve_target};
    use crate::part::PartId;

    #[test]
    fn canonicalizes_plain_relative_names() {
        let id = canonicalize_part_name("word/document.xml").unwrap();
        assert_eq!(id.as_str(), "/word/document.xml");
    }

    #[test]
    fn normalizes_dot_segments() {
        assert_eq!(
            canonicalize_part_name("word/./media/../document.xml")
                .unwrap()
                .as_str(),
            "/word/document.xml"
        );
    }

    #[test]
    fn rejects_traversal_and_absolute_and_backslash() {
        assert!(canonicalize_part_name("../evil").is_err());
        assert!(canonicalize_part_name("a/../../evil").is_err());
        assert!(canonicalize_part_name("/abs").is_err());
        assert!(canonicalize_part_name("a\\b").is_err());
        assert!(canonicalize_part_name("").is_err());
    }

    #[test]
    fn resolves_relative_targets() {
        let base = PartId::new("/word/document.xml");
        let resolved = resolve_target(&base, "media/image1.png", false)
            .unwrap()
            .unwrap();
        assert_eq!(resolved.as_str(), "/word/media/image1.png");
    }

    #[test]
    fn resolves_absolute_and_parent_targets() {
        let base = PartId::new("/word/document.xml");
        assert_eq!(
            resolve_target(&base, "/word/styles.xml", false)
                .unwrap()
                .unwrap()
                .as_str(),
            "/word/styles.xml"
        );
        assert_eq!(
            resolve_target(&base, "../docProps/core.xml", false)
                .unwrap()
                .unwrap()
                .as_str(),
            "/docProps/core.xml"
        );
    }

    #[test]
    fn external_targets_are_not_resolved() {
        let base = PartId::new("/word/document.xml");
        assert!(resolve_target(&base, "http://example.com/x", true)
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejects_escaping_targets() {
        let base = PartId::new("/word/document.xml");
        assert!(resolve_target(&base, "../../../etc/passwd", false).is_err());
    }
}
