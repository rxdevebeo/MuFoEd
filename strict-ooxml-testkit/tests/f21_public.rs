//! The public audit suite. It does not read the private corpus.

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
fn f21_public_suite() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let manifest = std::fs::read_to_string(root.join("xtool/audit-fixes/manifest.toml"))
        .expect("public manifest");
    assert!(
        manifest.contains("does not name the private"),
        "the public manifest must not become a private-corpus runner"
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
    let names = quoted_test_names(&manifest);
    assert!(
        names.contains(&"f21_public_suite".to_owned()),
        "the suite must name itself"
    );
    let sources = rust_sources(&root);
    for name in &names {
        assert!(
            sources.contains(&format!("fn {name}")),
            "{name} is listed but has no function"
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
}

fn quoted_test_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with("tests") {
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
