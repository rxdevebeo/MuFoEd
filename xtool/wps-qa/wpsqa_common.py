"""Shared paths, corpus registry and small helpers for the WPS QA harness.

The corpora are gitignored and live in the *main* copy of the repository, so a
worktree (where this QA branch is checked out) finds them through
`git rev-parse --git-common-dir`. Everything the harness writes goes into one
store directory (`--store`, default `<main copy>/wps-qa`), which is gitignored.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterator

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent


@dataclass(frozen=True)
class Corpus:
    """One named set of documents: a directory and its globs under the data root."""

    name: str
    path: str
    globs: tuple[str, ...] = ("*.docx", "*.rtf")


# What a corpus holds that WPS lays out the way we do: .docx as it is, .rtf
# opened as it is by WPS and imported to Strict DOCX by us (`import-rtf`).
DOCUMENT_SUFFIXES = (".docx", ".rtf")

# Corpora discovery would not find: outside testdata/, or nested. Every other
# directory of testdata/ holding a .docx or an .rtf is a corpus named after
# it, so a corpus added there is measured without a change here; the PDF and
# DjVu sets (importer inputs) hold neither and are left out.
REGISTERED: tuple[Corpus, ...] = (
    Corpus("core-strict", "strict-ooxml-core/tests/strict"),
    Corpus("core-samples", "strict-ooxml-core/tests/samples"),
    Corpus("core-docx", "strict-ooxml-core/tests/docx"),
    Corpus("pdf-docx", "testdata/pdf", ("**/*.docx",)),
)


def _is_document(path: Path) -> bool:
    # Word lock files (~$name.docx) are not documents.
    return path.is_file() and path.suffix.lower() in DOCUMENT_SUFFIXES and not path.name.startswith("~$")


def corpora(data_root: Path) -> list[Corpus]:
    """The registered corpora, then every testdata/ directory holding a
    document, by name."""
    found = list(REGISTERED)
    taken = {c.path for c in found} | {c.name for c in found}
    testdata = data_root / "testdata"
    if testdata.is_dir():
        for directory in sorted(p for p in testdata.iterdir() if p.is_dir()):
            rel = f"testdata/{directory.name}"
            if rel in taken or directory.name in taken:
                continue
            if any(_is_document(child) for child in directory.iterdir()):
                found.append(Corpus(directory.name, rel))
    return found


def main_copy_root() -> Path:
    """The main working copy (where the gitignored corpora are)."""
    try:
        out = subprocess.run(
            ["git", "-C", str(REPO), "rev-parse", "--path-format=absolute", "--git-common-dir"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        return Path(out).parent
    except (OSError, subprocess.CalledProcessError):
        return REPO


def default_store() -> Path:
    return main_copy_root() / "wps-qa"


@dataclass(frozen=True)
class Doc:
    """One document of one corpus. `key` is unique within the corpus."""

    corpus: str
    path: Path
    key: str

    @property
    def ident(self) -> str:
        return f"{self.corpus}/{self.key}"

    @property
    def is_rtf(self) -> bool:
        return self.path.suffix.lower() == ".rtf"


def safe_key(rel: str) -> str:
    """A file-system-safe, stable key for a corpus-relative path."""
    stem = rel
    for suffix in (".docx", ".rtf"):
        if rel.lower().endswith(suffix):
            stem = rel[: -len(suffix)]
            break
    stem = stem.replace("\\", "/").replace("/", "__")
    bad = '<>:"|?*'
    stem = "".join("_" if ch in bad or ord(ch) < 32 else ch for ch in stem)
    stem = stem.strip(" .")
    if len(stem) > 120:
        digest = hashlib.sha1(rel.encode("utf-8")).hexdigest()[:10]
        stem = stem[:100].rstrip(" .") + "~" + digest
    return stem


def iter_docs(data_root: Path, names: list[str] | None) -> Iterator[Doc]:
    known = {c.name: c for c in corpora(data_root)}
    unknown = [n for n in names or [] if n not in known]
    if unknown:
        raise SystemExit(f"wpsqa: unknown corpus {', '.join(unknown)}; known: {', '.join(known)}")
    selected = list(known.values()) if not names else [known[n] for n in names]
    for corpus in selected:
        root = data_root / corpus.path
        if not root.is_dir():
            print(f"wpsqa: corpus {corpus.name} absent at {root}", file=sys.stderr)
            continue
        paths = sorted({path for pattern in corpus.globs for path in root.glob(pattern) if _is_document(path)})
        for path in paths:
            rel = path.relative_to(root).as_posix()
            yield Doc(corpus.name, path, safe_key(rel))


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def write_json(path: Path, obj: Any) -> None:
    """Writes through a temp file and a rename, so a reader never sees half."""
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(obj, ensure_ascii=False, indent=1), encoding="utf-8")
    os.replace(tmp, path)


def tail(text: str, lines: int = 15) -> str:
    return "\n".join(text.strip().splitlines()[-lines:])
