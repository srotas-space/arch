#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DEPLOY_BRANCH="${DEPLOY_BRANCH:-${1:-gh-pages}}"
REMOTE="${REMOTE:-origin}"
SKIP_BUILD="${SKIP_BUILD:-0}"

usage() {
  cat <<'EOF'
Deploy static docs by pushing only public/ contents to a deploy branch.

Env vars:
  DEPLOY_BRANCH      Target deploy branch (default: gh-pages)
  REMOTE             Git remote name (default: origin)
  SKIP_BUILD         Set to 1 to skip CSS/site build before push (default: 0)

Examples:
  ./scripts/deploy-public.sh
  ./scripts/deploy-public.sh any-branch-name
  DEPLOY_BRANCH=docs ./scripts/deploy-public.sh
  SKIP_BUILD=1 ./scripts/deploy-public.sh
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if ! git -C "$ROOT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Error: not a git repository: $ROOT_DIR" >&2
  exit 1
fi

current_branch="$(git -C "$ROOT_DIR" rev-parse --abbrev-ref HEAD)"

if ! git -C "$ROOT_DIR" remote get-url "$REMOTE" >/dev/null 2>&1; then
  echo "Error: remote '$REMOTE' not found." >&2
  exit 1
fi

if [[ "$SKIP_BUILD" != "1" ]]; then
  (
    cd "$ROOT_DIR"
    npm run build:css
    cargo run --manifest-path docsgen/Cargo.toml -- build
  )
fi

if [[ ! -d "$ROOT_DIR/public" ]]; then
  echo "Error: public/ folder does not exist. Build may have failed." >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

rsync -a --delete --exclude '.DS_Store' "$ROOT_DIR/public/" "$tmp_dir/"
touch "$tmp_dir/.nojekyll"

remote_url="$(git -C "$ROOT_DIR" remote get-url "$REMOTE")"
short_sha="$(git -C "$ROOT_DIR" rev-parse --short HEAD)"

(
  cd "$tmp_dir"
  git init -q
  git checkout -q -b "$DEPLOY_BRANCH"
  git add -A
  git -c user.name="${GIT_AUTHOR_NAME:-docs-deploy-bot}" \
      -c user.email="${GIT_AUTHOR_EMAIL:-docs-deploy@example.com}" \
      commit -q -m "Deploy from ${current_branch}@${short_sha}"
  git remote add "$REMOTE" "$remote_url"
  git push --force "$REMOTE" "$DEPLOY_BRANCH"
)

echo "Deployed public/ from '$current_branch' to '$REMOTE/$DEPLOY_BRANCH'."
