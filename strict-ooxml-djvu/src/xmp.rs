//! Minimal XMP (RDF/XML) reading: the document properties DjVu stores in a
//! `(xmp "…")` annotation, used as a fallback for `docProps/core.xml`.
//!
//! Only the fields that map to `docProps/core.xml` are read (`dc:title`,
//! `dc:creator`, `dc:description`, `dc:subject`, `pdf:Keywords`); anything else
//! stays in the raw packet. The scan is textual, bounded and tolerant: a
//! malformed packet yields whatever was found, never a panic.

/// Caps a packet's contribution; a document with more is not a document.
const MAX_ENTRIES: usize = 16;

/// Canonical core-property entries from an XMP packet, in `(metadata …)`
/// spelling so they merge with a DjVu `(metadata …)` block.
pub(crate) fn entries(packet: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut add = |key: &str, value: Option<String>| {
        if out.len() >= MAX_ENTRIES {
            return;
        }
        if let Some(value) = value {
            let value = value.trim();
            if !value.is_empty() {
                out.push((key.to_owned(), value.to_owned()));
            }
        }
    };
    add("Title", first(packet, "dc:title"));
    add("Author", join(packet, "dc:creator", "; "));
    add("Description", first(packet, "dc:description"));
    add("Subject", join(packet, "dc:subject", ", "));
    add("Keywords", join(packet, "pdf:Keywords", ", "));
    out
}

/// The first `rdf:li` of `tag`, or its direct text when it has none.
fn first(packet: &str, tag: &str) -> Option<String> {
    items(element(packet, tag)?).into_iter().next()
}

/// Every `rdf:li` of `tag` joined by `sep`, or its direct text when it has none.
fn join(packet: &str, tag: &str, sep: &str) -> Option<String> {
    let items = items(element(packet, tag)?);
    let joined = items.join(sep);
    (!joined.trim().is_empty()).then_some(joined)
}

/// The text between `<tag …>` and `</tag>`, prefixes not confused with names.
fn element<'a>(packet: &'a str, tag: &str) -> Option<&'a str> {
    let needle = format!("<{tag}");
    let mut at = 0;
    let open = loop {
        let rel = packet[at..].find(&needle)? + at;
        if is_name_end(packet.as_bytes().get(rel + needle.len()).copied()) {
            break rel;
        }
        at = rel + 1;
    };
    let after = &packet[open + needle.len()..];
    let gt = after.find('>')?;
    if after[..gt].ends_with('/') {
        return None;
    }
    let start = open + needle.len() + gt + 1;
    let close = format!("</{tag}>");
    let end = packet[start..].find(&close)? + start;
    Some(&packet[start..end])
}

/// The byte that ends an XML name: whitespace, `>`, `/`, or nothing.
fn is_name_end(byte: Option<u8>) -> bool {
    matches!(
        byte,
        None | Some(b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/')
    )
}

/// `rdf:li` texts, or the element's own (tag-stripped) text when it has none.
fn items(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = inner;
    while let Some(li) = rest.find("<rdf:li") {
        let after = &rest[li + "<rdf:li".len()..];
        let Some(gt) = after.find('>') else { break };
        let start = li + "<rdf:li".len() + gt + 1;
        let Some(close) = rest[start..].find("</rdf:li>") else {
            break;
        };
        out.push(unescape(&rest[start..start + close]));
        rest = &rest[start + close + "</rdf:li>".len()..];
    }
    if out.is_empty() {
        let text = strip_tags(inner);
        if !text.trim().is_empty() {
            out.push(text);
        }
    }
    out
}

/// Text with every `<…>` element removed, entities decoded.
fn strip_tags(value: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for ch in value.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    unescape(&out)
}

/// The five XML entities and numeric character references.
fn unescape(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut at = 0;
    while at < value.len() {
        if value.as_bytes()[at] != b'&' {
            let ch = value[at..].chars().next().expect("boundary");
            out.push(ch);
            at += ch.len_utf8();
            continue;
        }
        if let Some(semi) = value[at + 1..].find(';').map(|p| at + 1 + p) {
            let entity = &value[at + 1..semi];
            if let Some(ch) = decode_entity(entity) {
                out.push(ch);
                at = semi + 1;
            } else {
                out.push('&');
                at += 1;
            }
        } else {
            out.push('&');
            at += 1;
        }
    }
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let code = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .and_then(|dec| dec.parse::<u32>().ok())
                })?;
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKET: &str = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about="">
        <dc:title><rdf:Alt><rdf:li xml:lang="x-default">A DjVu Book</rdf:li></rdf:Alt></dc:title>
        <dc:creator><rdf:Seq><rdf:li>Ada Lovelace</rdf:li><rdf:li>Alan Turing</rdf:li></rdf:Seq></dc:creator>
        <dc:description><rdf:Alt><rdf:li xml:lang="x-default">An &amp; account</rdf:li></rdf:Alt></dc:description>
        <dc:subject><rdf:Bag><rdf:li>math</rdf:li><rdf:li>history</rdf:li></rdf:Bag></dc:subject>
        <pdf:Keywords>scan, ocr</pdf:Keywords>
      </rdf:Description>
    </rdf:RDF>"#;

    #[test]
    fn core_fields_are_read_from_the_packet() {
        let found = entries(PACKET);
        let get = |key: &str| {
            found
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(get("Title"), Some("A DjVu Book"));
        assert_eq!(get("Author"), Some("Ada Lovelace; Alan Turing"));
        assert_eq!(get("Description"), Some("An & account"));
        assert_eq!(get("Subject"), Some("math, history"));
        assert_eq!(get("Keywords"), Some("scan, ocr"));
    }

    #[test]
    fn a_direct_title_element_is_read_without_rdf_li() {
        let packet = "<dc:title>A plain title</dc:title>";
        assert_eq!(first(packet, "dc:title").as_deref(), Some("A plain title"));
    }

    #[test]
    fn a_prefix_is_not_a_different_element() {
        // `dc:titleX` must not match `dc:title`.
        assert_eq!(first("<dc:titleX>nope</dc:titleX>", "dc:title"), None);
    }

    #[test]
    fn malformed_and_empty_packets_never_panic() {
        for packet in [
            "",
            "<dc:title>",
            "<dc:title></dc",
            "<dc:title>&nope;</dc:title>",
            "<rdf:li>orphan",
        ] {
            let _ = entries(packet);
        }
        assert!(entries("<dc:title>   </dc:title>").is_empty());
    }

    #[test]
    fn entities_decode_and_numeric_references_work() {
        assert_eq!(
            unescape("a &amp; b &lt;c&gt; &#65; &#x42;"),
            "a & b <c> A B"
        );
        assert_eq!(unescape("keep & alone"), "keep & alone");
    }
}
