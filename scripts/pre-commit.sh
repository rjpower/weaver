#!/usr/bin/env bash
#
# Shared pre-commit / CI gate: formatting and lints must both pass.
#
# This is the single source of truth for "is the tree clean?". It is run by:
#   - the git pre-commit hook (.githooks/pre-commit), enabled per-clone with
#     `git config core.hooksPath .githooks` (see AGENTS.md); and
#   - the CI `lint` job (.github/workflows/ci.yml),
# so a commit that passes locally passes CI. Bypass the hook for one commit with
# `git commit --no-verify`.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

echo "▶ cargo fmt --all --check"
if ! cargo fmt --all --check; then
  echo >&2
  echo "✗ formatting check failed — run \`cargo fmt --all\`, then re-stage and commit." >&2
  exit 1
fi

echo "▶ cargo clippy --workspace --all-targets --locked -- -D warnings"
cargo clippy --workspace --all-targets --locked -- -D warnings

echo "✓ fmt + clippy clean"

# Agent lint review: a headless Claude sub-agent applies the docs/lint.md catalog
# to the staged diff and errors on the agent-slop it can't be linted for
# mechanically (naming, shape, dead code, comments, tests). lint-review.py is a
# `uv run` script and self-skips when `claude` isn't on PATH or there are no
# staged Rust/TS/Vue changes; we gate it on `uv` here so the CI lint job (no uv,
# no agent) runs only the fmt+clippy gate above. Disable it for a run with
# WEAVER_SKIP_AGENT_LINT=1; tune it via the env vars in lint-review.py.
if command -v uv >/dev/null 2>&1; then
  "$(git rev-parse --show-toplevel)/scripts/lint-review.py" --staged
else
  echo "  ⓘ agent lint skipped: uv not found on PATH (needed to run lint-review.py)"
fi
