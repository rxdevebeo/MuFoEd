#!/usr/bin/env bash
# AUD-92: one-hour acceptance session for every fuzz target (Linux/WSL).
set -euo pipefail
source "$HOME/.cargo/env"
export PATH="$HOME/llvm/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/local/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"
export CC=clang
export CXX=clang++
cd "$(dirname "$0")"
LOG="aud92-1h-$(date +%Y%m%d-%H%M%S).log"
{
  echo "Date: $(date -Iseconds)"
  echo "Host: $(uname -a)"
  echo "Rust: $(rustc +nightly --version)"
  echo "Commit: $(git -C .. rev-parse HEAD)"
  echo
  for t in fuzz_zip fuzz_xml fuzz_relpath fuzz_wml fuzz_normalize fuzz_docx_full fuzz_pdf fuzz_convert; do
    EXTRA=""
    case "$t" in
      fuzz_normalize|fuzz_docx_full|fuzz_xml|fuzz_wml) EXTRA="-dict=dict/wml.dict" ;;
    esac
    echo "=== START $t $(date -Iseconds) ==="
    cargo +nightly fuzz run --target x86_64-unknown-linux-gnu "$t" -- \
      -max_total_time=3600 -rss_limit_mb=2048 -timeout=10 $EXTRA
    echo "=== DONE $t $(date -Iseconds) ==="
  done
  echo "ALL_1H_OK $(date -Iseconds)"
} 2>&1 | tee "$LOG"
