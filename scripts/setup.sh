#!/usr/bin/env bash
#
# One-shot setup: build the weaver + loom binaries and put them on your PATH.
#
# Point your coding agent at this script (or the "Getting Started" section of
# the README) and let it do the install for you. Safe to re-run — it rebuilds
# and refreshes the symlinks.
#
#   ./scripts/setup.sh                  # build (debug) + symlink into ~/.local/bin
#   BIN_DIR=~/bin ./scripts/setup.sh    # symlink somewhere else
#   PROFILE=release ./scripts/setup.sh  # build optimized binaries instead
#
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
cd "$repo"

bin_dir="${BIN_DIR:-$HOME/.local/bin}"
profile="${PROFILE:-debug}"

# 1. Rust toolchain. `cargo build` needs it; install via rustup if it's absent.
if ! command -v cargo >/dev/null 2>&1; then
  echo "▶ cargo not found — installing the Rust toolchain via rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # rustup writes the PATH line into your shell profile for future shells;
  # source it now so the rest of this script can use cargo immediately.
  . "$HOME/.cargo/env"
fi

# 2. Build the tooling. `cargo build` also bundles the Vue dashboard into the
#    loom binary (needs Node + npm; a Node-less checkout serves a placeholder).
if [ "$profile" = "release" ]; then
  echo "▶ cargo build --release"
  cargo build --release
else
  echo "▶ cargo build"
  cargo build
fi

# 3. Symlink the binaries onto your PATH.
mkdir -p "$bin_dir"
for bin in weaver loom; do
  ln -sf "$repo/target/$profile/$bin" "$bin_dir/$bin"
  echo "  linked $bin_dir/$bin -> $repo/target/$profile/$bin"
done

# 4. Make sure $bin_dir is actually on PATH.
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *)
    echo
    echo "⚠ $bin_dir is not on your PATH. Add it, e.g.:"
    echo "    echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.profile && . ~/.profile"
    ;;
esac

echo
echo "✓ weaver + loom installed. Next:"
echo "    loom serve     # start the orchestrator (REST + UI + tmux)"
echo "    loom open      # open the dashboard"
