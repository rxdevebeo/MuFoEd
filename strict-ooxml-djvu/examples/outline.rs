//! Print a DjVu file's `NAVM` outline as an S-expression, for the SC-D9 oracle.
//!
//! The outline driver (`xtool/djvu-outline-oracle.py`) compares this against
//! `djvused -e print-outline`. The notation is the one DjVuLibre uses: a
//! `(bookmarks …)` wrapper around a pre-order forest of `("title" "url" child…)`
//! nodes, so the driver can parse both with the same reader.
#![allow(clippy::doc_markdown)]
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits, NavItem};

/// A quoted string with the two characters DjVuLibre escapes: `\` and `"`.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn write_node(node: &NavItem, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    out.push_str(&pad);
    out.push('(');
    out.push_str(&quote(&node.title));
    out.push(' ');
    out.push_str(&quote(&node.url));
    if node.children.is_empty() {
        out.push_str(")\n");
        return;
    }
    out.push('\n');
    for child in &node.children {
        write_node(child, depth + 1, out);
    }
    out.push_str(&pad);
    out.push_str(")\n");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("requires a DjVu path")?;
    let bytes = std::fs::read(&path)?;
    let document = DjvuDocument::open(&bytes, DjvuLimits::default())?;
    let mut out = String::from("(bookmarks\n");
    for node in &document.outline {
        write_node(node, 1, &mut out);
    }
    out.push_str(")\n");
    print!("{out}");
    Ok(())
}
