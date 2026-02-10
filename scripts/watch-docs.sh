#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

build() {
  cargo run --manifest-path "$ROOT_DIR/docsgen/Cargo.toml" -- build
}

if command -v watchexec >/dev/null 2>&1; then
  watchexec -w "$ROOT_DIR/docs" -w "$ROOT_DIR/docsgen/templates" -e md,html --shell bash -- "${BASH_SOURCE[0]}" --build
  exit 0
fi

if command -v fswatch >/dev/null 2>&1; then
  echo "Using fswatch. Press Ctrl+C to stop."
  fswatch -o "$ROOT_DIR/docs" "$ROOT_DIR/docsgen/templates" | while read -r _; do
    build
  done
  exit 0
fi

if [[ "${1:-}" == "--build" ]]; then
  build
  exit 0
fi

echo "No watcher found. Install one of:"
echo "  - watchexec (recommended)"
echo "  - fswatch"
exit 1
