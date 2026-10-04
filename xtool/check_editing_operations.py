"""Compile an isolated adapter stub and require every operation test to fail.

Run from any directory with Python 3.11+ and cargo +1.92.0 installed.
Only target/editing-operations is written; repository sources stay untouched.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib


def main():
    root = Path(__file__).resolve().parents[1]
    output = root / "target" / "editing-operations"
    isolated = output / "disabled-adapter"
    isolated.mkdir(parents=True, exist_ok=True)
    crate = root / "strict-ooxml-edit"
    for directory in ("src", "tests"):
        shutil.copytree(crate / directory, isolated / directory, dirs_exist_ok=True)
    shutil.copyfile(crate / "README.md", isolated / "README.md")
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8-sig"))["workspace"]
    manifest = (crate / "Cargo.toml").read_text(encoding="utf-8-sig")
    for key, value in workspace["package"].items():
        manifest = re.sub(rf"(?m)^{re.escape(key)}\.workspace = true$", f"{key} = {json.dumps(value)}", manifest)
    manifest = re.sub(r'path = "\.\./([^"]+)"', lambda m: f'path = "{(root / m[1]).as_posix()}"', manifest)
    manifest = manifest.replace("png.workspace = true", f'png = {json.dumps(workspace["dependencies"]["png"])}')
    manifest = re.sub(r"\[lints\]\s*workspace = true", "", manifest)
    (isolated / "Cargo.toml").write_text(manifest + "\n[workspace]\n", encoding="utf-8")
    shutil.copyfile(root / "Cargo.lock", isolated / "Cargo.lock")
    path = isolated / "src" / "operations.rs"
    source = path.read_text(encoding="utf-8-sig")
    prefix = source.split("impl<'session, 'document> Operations<'session, 'document> {", 1)[0]
    stub = """
impl<'session, 'document> Operations<'session, 'document> {
    pub fn new(editor: &'session mut Editor<'document>, limits: OperationLimits) -> Self { Self { editor, limits } }
    pub fn search(&self, revision: u64, _: &SearchScope, _: &SearchQuery) -> Result<SearchResult, OperationError> {
        Ok(SearchResult { revision, hits: vec![] })
    }
    pub fn replace_all(&mut self, _: u64, _: &SearchScope, _: &TextQuery, _: &str, _: ReplacePolicy) -> Result<ReplaceReport, OperationError> {
        Err(OperationError::ProtectedContent)
    }
    pub fn move_block(&mut self, _: u64, _: &Address, _: &Address) -> Result<MoveReport, OperationError> {
        Err(OperationError::UnsupportedMove)
    }
    pub fn move_row(&mut self, _: u64, _: &Address, _: usize, _: usize) -> Result<MoveReport, OperationError> {
        Err(OperationError::UnsupportedMove)
    }
}
"""
    path.write_text(prefix + stub, encoding="utf-8")
    # The existing editing kernel remains byte-identical in this copy.
    for original in (crate / "src").glob("*.rs"):
        if original.name != "operations.rs":
            assert original.read_bytes() == (isolated / "src" / original.name).read_bytes()
    env = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
    run = subprocess.run(["cargo", "+1.92.0", "test", "--manifest-path", str(isolated / "Cargo.toml"),
                          "--no-default-features", "--test", "operations", "--offline"],
                         cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    log = run.stdout.decode("utf-8", errors="replace")
    (output / "disabled-adapter.log").write_text(log, encoding="utf-8")
    expected = (crate / "tests" / "operations.rs").read_text(encoding="utf-8-sig").count("#[test]")
    result = re.search(r"test result: FAILED\. (\d+) passed; (\d+) failed;", log)
    if run.returncode != 101 or result is None or int(result[1]) != 0 or int(result[2]) != expected:
        raise SystemExit(f"Inverse check failed; see {output / 'disabled-adapter.log'}")
    print(f"PASS: adapter stub compiled; {expected}/{expected} behavior tests failed; kernel unchanged.")


if __name__ == "__main__":
    main()
