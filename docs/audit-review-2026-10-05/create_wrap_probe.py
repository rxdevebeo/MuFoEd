from pathlib import Path

root = Path(__file__).resolve().parents[2]
source = (root / 'strict-ooxml-render-svg/tests/f15_wrap.rs').read_text(encoding='utf-8')
source = source.replace('fn render_wrap(element: &str, wrap_text: &str) -> String {', 'fn render_wrap(element: &str, wrap_text: &str) -> String { render_case(element, wrap_text, false) }\n\nfn render_case(element: &str, wrap_text: &str, following: bool) -> String {')
source = source.replace('    let rels = format!(', '''    let body = if following {
        format!("{body}<w:p><w:pPr><w:ind w:start=\\\"1800\\\"/></w:pPr><w:r><w:t>FOLLOWING</w:t></w:r></w:p>")
    } else { body };
    let rels = format!(''', 1)
source += '''

#[test]
fn acceptance_square_wrap_covers_following_paragraph() {
    let svg = render_case("wrapSquare", "bothSides", true);
    let (x, y, w, h) = object_box(&svg);
    let (tx, ty, _) = text_items(&svg).into_iter().find(|(_, _, t)| t.contains("FOLLOWING")).expect("following text");
    assert!(!(tx > x && tx < x + w && ty > y && ty < y + h), "FOLLOWING starts inside image: text=({tx},{ty}), image=({x},{y},{w},{h})");
}

#[test]
fn acceptance_top_bottom_reserves_object_height() {
    let svg = render_case("wrapTopAndBottom", "", false);
    let (x, y, w, h) = object_box(&svg);
    let texts = text_items(&svg);
    assert!(!texts.is_empty(), "text must survive");
    for (tx, ty, text) in texts {
        assert!(ty >= y + h, "TopAndBottom text {text:?} starts at ({tx},{ty}), object=({x},{y},{w},{h}); expected baseline below object bottom");
    }
}
'''
destination = root / 'strict-ooxml-render-svg/tests/acceptance_wrap_probe.rs'
assert not destination.exists()
destination.write_text(source, encoding='utf-8', newline='\n')
(Path(__file__).parent / 'acceptance_wrap_probe.rs').write_text(source, encoding='utf-8', newline='\n')
print(destination)
