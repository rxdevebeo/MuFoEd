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
    """One named set of documents: a directory and a glob under the data root."""

    name: str
    path: str
    glob: str = "*.docx"


# Every .docx corpus the repository knows about. The PDF/DjVu sets are inputs
# of importers, not documents WPS lays out; the RTF sets need RTF import, which
# is not on master. Add them here when the CLI reads them.
CORPORA: tuple[Corpus, ...] = (
    Corpus("core-strict", "strict-ooxml-core/tests/strict"),
    Corpus("core-samples", "strict-ooxml-core/tests/samples"),
    Corpus("core-docx", "strict-ooxml-core/tests/docx"),
    Corpus("CC0", "testdata/CC0"),
    Corpus("CC0_DOCX", "testdata/CC0_DOCX"),
    Corpus("CC0_DOCX_1", "testdata/CC0_DOCX_1"),
    Corpus("CC0_DOCX_MATH", "testdata/CC0_DOCX_MATH"),
    Corpus("CCX", "testdata/CCX"),
    Corpus("pdf-docx", "testdata/pdf", "**/*.docx"),
)
CORPUS_BY_NAME = {c.name: c for c in CORPORA}


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


def safe_key(rel: str) -> str:
    """A file-system-safe, stable key for a corpus-relative path."""
    stem = rel[:-5] if rel.lower().endswith(".docx") else rel
    stem = stem.replace("\\", "/").replace("/", "__")
    bad = '<>:"|?*'
    stem = "".join("_" if ch in bad or ord(ch) < 32 else ch for ch in stem)
    stem = stem.strip(" .")
    if len(stem) > 120:
        digest = hashlib.sha1(rel.encode("utf-8")).hexdigest()[:10]
        stem = stem[:100].rstrip(" .") + "~" + digest
    return stem


def iter_docs(data_root: Path, names: list[str] | None) -> Iterator[Doc]:
    selected = CORPORA if not names else [CORPUS_BY_NAME[n] for n in names]
    for corpus in selected:
        root = data_root / corpus.path
        if not root.is_dir():
            print(f"wpsqa: corpus {corpus.name} absent at {root}", file=sys.stderr)
            continue
        for path in sorted(root.glob(corpus.glob)):
            # Word lock files (~$name.docx) are not documents.
            if not path.is_file() or path.name.startswith("~$"):
                continue
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
