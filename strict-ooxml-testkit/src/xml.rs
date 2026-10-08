//! Generators for deep and wide markup.

/// `open` repeated `depth` times, then `inner`, then `close` repeated `depth`
/// times: `nested("<a>", "</a>", "x", 2)` is `<a><a>x</a></a>`.
pub fn nested(open: &str, close: &str, inner: &str, depth: usize) -> String {
    let mut out = String::with_capacity((open.len() + close.len()) * depth + inner.len());
    for _ in 0..depth {
        out.push_str(open);
    }
    out.push_str(inner);
    for _ in 0..depth {
        out.push_str(close);
    }
    out
}

/// `depth` tables, each holding the next in its only cell, with `inner` in the
/// innermost cell. A cell must end in a paragraph, so each one gets an empty
/// `w:p` after the nested table.
pub fn nested_tables(depth: usize, inner: &str) -> String {
    nested(
        "<w:tbl><w:tblGrid><w:gridCol w:w=\"2000\"/></w:tblGrid><w:tr><w:tc>",
        "<w:p/></w:tc></w:tr></w:tbl>",
        inner,
        depth,
    )
}

/// `depth` text boxes (`wps:txbx` inside an inline `w:drawing`), each holding
/// the next, with `inner` (block content) in the innermost one.
pub fn nested_text_boxes(depth: usize, inner: &str) -> String {
    nested(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/>\
<wp:docPr id=\"1\" name=\"box\"/><a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
<wps:wsp><wps:spPr/><wps:txbx><w:txbxContent>",
        "</w:txbxContent></wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
        inner,
        depth,
    )
}

/// An inline `w:drawing` paragraph holding `depth` DrawingML groups, each the
/// only child of the one around it (`wpg:wgp`, then `wpg:grpSp` inside it), with
/// one shape in the innermost.
pub fn nested_groups(depth: usize) -> String {
    let groups = nested(
        "<wpg:grpSp><wpg:grpSpPr/>",
        "</wpg:grpSp>",
        "<wps:wsp><wps:spPr/><wps:bodyPr/></wps:wsp>",
        depth.saturating_sub(1),
    );
    format!(
        "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"914400\" cy=\"914400\"/>\
<wp:docPr id=\"1\" name=\"group\"/><a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\">\
<wpg:wgp><wpg:grpSpPr/>{groups}</wpg:wgp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    )
}

/// `count` copies of `item`, concatenated.
pub fn repeated(item: &str, count: usize) -> String {
    item.repeat(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_wraps_symmetrically() {
        assert_eq!(nested("<a>", "</a>", "x", 2), "<a><a>x</a></a>");
        assert_eq!(nested("<a>", "</a>", "x", 0), "x");
    }

    #[test]
    fn nested_tables_balance() {
        let xml = nested_tables(3, "<w:p/>");
        assert_eq!(xml.matches("<w:tbl>").count(), 3);
        assert_eq!(xml.matches("</w:tbl>").count(), 3);
    }
}
