//! Public audit suite metadata gate.
//!
//! Behavioral execution of F00–F20 is owned by `xtool/audit-fixes/run.py`
//! (`kind = "acceptance"`). This Rust test only checks manifest/matrix wiring
//! and does not claim a full audit by itself.

/// Vector primitives this suite names and does not claim to draw.
const UNSUPPORTED_VECTOR_PRIMITIVES: &[&str] = &[
    "cubic",
    "clip",
    "pattern",
    "tight-contour",
    "through-contour",
];

/// Complex frame schemes have no Word reference here.
const F16_COMPLEX_SCHEMES: &str = "BLOCKED";

#[test]
fn f21_public_manifest_metadata() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let manifest = std::fs::read_to_string(root.join("xtool/audit-fixes/manifest.toml"))
        .expect("public manifest");
    assert!(
        manifest.contains("does not name the private"),
        "the public manifest must not become a private-corpus runner"
    );
    assert!(
        manifest.contains("kind = \"acceptance\""),
        "F21 must be an acceptance task that runs the behavioral suite"
    );
    assert!(
        manifest.contains("f21_public_manifest_metadata"),
        "the suite must name its metadata test"
    );
    for card in 0..=21 {
        let id = format!("F{card:02}");
        assert!(
            manifest.contains(&format!("id = \"{id}\"")),
            "{id} is missing from the public manifest"
        );
    }
    for finding in 1..=21 {
        let id = format!("A{finding:02}");
        assert!(manifest.contains(&format!("\"{id}\"")), "{id} has no card");
    }

    let matrix =
        std::fs::read_to_string(root.join("xtool/audit-fixes/matrix.toml")).expect("public matrix");
    assert!(
        matrix.contains("status = \"blocked\""),
        "matrix must keep honest blocked rows; empty blockers would fake completeness"
    );
    assert!(
        matrix.contains("F21-behavioral-suite"),
        "matrix must name the behavioral F21 gate"
    );

    let names = quoted_test_names(&manifest);
    assert!(
        names.contains(&"f21_public_manifest_metadata".to_owned()),
        "the suite must name its metadata test in tests/metadata_tests"
    );
    let sources = rust_sources(&root);
    for name in &names {
        if name == "ci-feature-matrix" {
            continue;
        }
        assert!(
            sources.contains(&format!("fn {name}"))
                || name.starts_with("xtool/")
                || name.contains("::"),
            "{name} is listed but has no function"
        );
    }
    // Manifest cargo tests must exist as Rust fn items.
    for name in quoted_cargo_test_names(&manifest) {
        assert!(
            sources.contains(&format!("fn {name}")),
            "{name} is listed in a card tests array but has no function"
        );
    }

    let vectors = std::fs::read_to_string(root.join("strict-ooxml-convert/src/vectors.rs"))
        .expect("vector ledger");
    assert!(
        vectors.contains("vector.unsupported"),
        "an unsupported path must keep its id"
    );
    assert!(
        !UNSUPPORTED_VECTOR_PRIMITIVES.is_empty(),
        "A02 keeps an explicit unsupported list"
    );
    assert_eq!(
        F16_COMPLEX_SCHEMES, "BLOCKED",
        "complex frame schemes are not accepted without a Word reference"
    );

    let runner = std::fs::read_to_string(root.join("xtool/audit-fixes/run.py")).expect("runner");
    assert!(
        runner.contains("dirty_tree_hash"),
        "runner must record dirty-tree fingerprint"
    );
    assert!(
        runner.contains("synthetic_public_suite"),
        "runner must distinguish synthetic suite from full audit"
    );
    assert!(
        runner.contains("full_audit"),
        "runner must refuse silent full-audit claims"
    );
}

fn quoted_test_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with("tests") || line.starts_with("metadata_tests") {
            inside = true;
        }
        if inside {
            if let Some(start) = line.find('"') {
                let rest = &line[start + 1..];
                if let Some(end) = rest.find('"') {
                    names.push(rest[..end].to_owned());
                }
            }
            if line.contains(']') {
                inside = false;
            }
        }
    }
    names
}

fn quoted_cargo_test_names(manifest: &str) -> Vec<String> {
    quoted_test_names(manifest)
        .into_iter()
        .filter(|name| name.starts_with('f') && name.contains('_'))
        .filter(|name| !name.starts_with("f21_"))
        .collect()
}

fn rust_sources(root: &std::path::Path) -> String {
    let mut out = String::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in read.flatten() {
            let path = item.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name == "target" || name == ".git" || name == "vendor" {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else if std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
            {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
    }
    out
}
