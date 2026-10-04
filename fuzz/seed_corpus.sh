#!/usr/bin/env bash
# AUD-92: copy small Strict/Transitional/PDF fixtures into fuzz corpora (gitignored).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FUZZ="$(cd "$(dirname "$0")" && pwd)"

seed_dir() {
  local target="$1"
  shift
  local dest="$FUZZ/corpus/$target"
  mkdir -p "$dest"
  local n=0
  for src in "$@"; do
    [[ -f "$src" ]] || continue
    # Keep seeds small enough for the reduced limits in the targets.
    local size
    size=$(wc -c <"$src")
    if [[ "$size" -gt $((2 * 1024 * 1024)) ]]; then
      continue
    fi
    local base
    base=$(basename "$src")
    cp -n "$src" "$dest/$base" 2>/dev/null || cp "$src" "$dest/$base"
    n=$((n + 1))
    [[ "$n" -ge 20 ]] && break
  done
  echo "$target: $n seeds in $dest"
}

mapfile -t DOCX < <(find "$ROOT/strict-ooxml-core/tests/strict" "$ROOT/strict-ooxml-core/tests/docx" \
  -type f -name '*.docx' 2>/dev/null | head -40)
mapfile -t PDF < <(find "$ROOT" -type f -name '*.pdf' \
  ! -path '*/target/*' ! -path '*/vendor/*' ! -path '*/_tmp*' 2>/dev/null | head -40)
mapfile -t XML < <(find "$ROOT/strict-ooxml-core/tests" -type f \( -name '*.xml' -o -name 'document.xml' \) 2>/dev/null | head -20)

seed_dir fuzz_docx_full "${DOCX[@]}"
seed_dir fuzz_wml "${DOCX[@]}"
seed_dir fuzz_zip "${DOCX[@]}"
seed_dir fuzz_normalize "${XML[@]}" "${DOCX[@]}"
seed_dir fuzz_pdf "${PDF[@]}"
seed_dir fuzz_convert "${PDF[@]}"
