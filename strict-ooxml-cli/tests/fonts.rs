//! Process-level contracts for the bounded `--fonts` manifest (RTF RC02).
use std::path::{Path, PathBuf};
use std::process::Command;

const CJK_SHA: &str = "226a61ed488949f0809ff9961e722274464a2b50297aa80a40ab07358f974d67";

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "strictlib-cli-fonts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn witness(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-render-svg/fonts/witnesses")
        .join(path)
}

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_strict-ooxml"))
}

fn manifest(sha: &str) -> String {
    format!(
        r#"{{
  "schema": "strictlib.font-assets.v1",
  "assets": [
    {{
      "family": "StrictLib Witness CJK SC",
      "resolved_family": "StrictLib Witness CJK SC",
      "path": "cjk.otf",
      "sha256": "{sha}",
      "source": "pinned Noto CJK derivative; fonts/witnesses/manifest.json",
      "license_id": "OFL-1.1",
      "license_reference": "fonts/witnesses/CJK-OFL.txt",
      "embedding": "embed-and-subset"
    }}
  ]
}}"#
    )
}

#[test]
fn explicit_manifest_embeds_the_pinned_face_and_a_bad_hash_is_refused() {
    let temp = Temp::new();
    let rtf = temp.0.join("source.rtf");
    let docx = temp.0.join("source.docx");
    let out = temp.0.join("page.svg");
    let fonts = temp.0.join("fonts.json");
    std::fs::write(
        &rtf,
        br"{\rtf1\ansi{\fonttbl{\f0 Arial;}{\f1 StrictLib Witness CJK SC;}}\loch\f0 AV \dbch\af1\u28450?\u23383?}",
    )
    .unwrap();
    let imported = binary()
        .arg("import-rtf")
        .arg(&rtf)
        .arg("--out")
        .arg(&docx)
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );

    std::fs::copy(witness("cjk.otf"), temp.0.join("cjk.otf")).unwrap();
    std::fs::write(&fonts, manifest(CJK_SHA)).unwrap();
    let rendered = binary()
        .arg("render")
        .arg(&docx)
        .arg("--fonts")
        .arg(&fonts)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let svg = std::fs::read_to_string(&out).unwrap();
    assert!(
        svg.contains("StrictAsset-"),
        "the explicit face must be embedded"
    );
    assert!(svg.contains("漢字"), "the CJK text must still be painted");

    std::fs::write(&fonts, manifest(&"0".repeat(64))).unwrap();
    let refused = binary()
        .arg("render")
        .arg(&docx)
        .arg("--fonts")
        .arg(&fonts)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("pinned sha256"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}
