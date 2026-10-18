//! Hostile input: a bounded importer must error, not abort or exhaust memory.
//!
//! Each input runs on the testkit's 1 MiB stack with a 10 s limit, the way the
//! other importers are judged (`strict-ooxml-testkit::harness`). A stack
//! overflow is not a panic and cannot be caught: it aborts the whole test
//! process, which is exactly the failure these tests exist to make impossible.

use std::iter::repeat_n;

use strict_ooxml_latex::{import, import_with_source, LatexError, LatexOptions, MemorySource};
use strict_ooxml_testkit::harness::assert_survives;

fn options() -> LatexOptions {
    LatexOptions::default()
}

fn assert_limit(input: Vec<u8>, limit: &str) {
    let result = assert_survives("import", move || import(&input, &options()));
    match result {
        Err(LatexError::LimitExceeded { limit: got, .. }) => assert_eq!(got, limit),
        other => panic!("expected limit `{limit}` exceeded, got {other:?}"),
    }
}

#[test]
fn deeply_nested_groups_do_not_overflow_the_stack() {
    let mut input = vec![b'{'; 100_000];
    input.extend(repeat_n(b'}', 100_000));
    assert_limit(input, "nesting");
}

#[test]
fn deeply_nested_environments_do_not_overflow_the_stack() {
    let mut input = Vec::new();
    for _ in 0..100_000 {
        input.extend_from_slice(b"\\begin{center}");
    }
    input.push(b'x');
    for _ in 0..100_000 {
        input.extend_from_slice(b"\\end{center}");
    }
    assert_limit(input, "nesting");
}

#[test]
fn deeply_nested_math_does_not_overflow_the_stack() {
    for (open, close) in [("\\frac{1}{", "}"), ("\\left(", "\\right)"), ("{{", "}")] {
        let mut input = vec![b'$'];
        for _ in 0..100_000 {
            input.extend_from_slice(open.as_bytes());
        }
        input.push(b'1');
        for _ in 0..100_000 {
            input.extend_from_slice(close.as_bytes());
        }
        input.push(b'$');
        assert_limit(input, "nesting");
    }
}

#[test]
fn a_column_pack_does_not_allocate_billions_of_columns() {
    assert_limit(
        b"\\begin{tabular}{*{2000000000}{l}}x\\end{tabular}".to_vec(),
        "table_columns",
    );
}

#[test]
fn an_over_wide_multicolumn_is_refused() {
    assert_limit(
        b"\\begin{tabular}{l}\\multicolumn{2000000000}{c}{x}\\end{tabular}".to_vec(),
        "table_columns",
    );
}

#[test]
fn a_padded_grid_is_refused_before_it_is_allocated() {
    // 4096 columns * 500 rows is 2 048 000 cells, over the 1 000 000 budget, and
    // every row is padded to the full width.
    let mut input = b"\\begin{tabular}{*{4096}{l}}".to_vec();
    for _ in 0..500 {
        input.extend_from_slice(b"x\\\\");
    }
    input.extend_from_slice(b"\\end{tabular}");
    assert_limit(input, "table_cells");
}

#[test]
fn a_huge_graphicspath_does_not_explode_the_candidate_list() {
    // Each search directory multiplies the names tried per figure; a declaration
    // with hundreds of thousands of them must be capped, not turned into a
    // candidate list (and set of provider reads) of the same order.
    let mut input = b"\\graphicspath{".to_vec();
    for _ in 0..200_000 {
        input.extend_from_slice(b"{d/}");
    }
    input.extend_from_slice(b"}\\includegraphics{x}");
    let imported = assert_survives("import", move || import(&input, &options()));
    let imported = imported.expect("import");
    assert!(
        imported
            .report
            .feature_counts
            .keys()
            .any(|feature| feature.contains("graphicspath")),
        "the dropped directories should be named"
    );
}

#[test]
fn an_include_cycle_terminates() {
    // `a` includes `b` includes `a`; the cycle is a recorded loss, not a hang.
    let mut source = MemorySource::new();
    source.insert("a", b"A \\input{b}".as_slice());
    source.insert("b", b"B \\input{a}".as_slice());
    let imported = assert_survives("import", move || {
        import_with_source("a", b"A \\input{b}", &source, &options())
    });
    assert!(imported.is_ok());
}

#[test]
fn ordinary_nesting_is_not_mistaken_for_a_bomb() {
    // Twenty nested groups and a nested environment are well within budget and
    // must import, so the guard is not a blanket refusal.
    let mut input = vec![b'{'; 20];
    input.extend_from_slice(b"text");
    input.extend(repeat_n(b'}', 20));
    input.extend_from_slice(b"\\begin{center}deep\\end{center}");
    let imported = assert_survives("import", move || import(&input, &options()));
    assert!(imported.is_ok(), "{imported:?}");
}
