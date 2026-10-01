#!/bin/sh
# Install what is needed to build, test and run the app on macOS or
# Debian/Ubuntu. Idempotent. Steps (flags combine; none means --system --rust
# --js):
#   --system    OS packages (Xcode CLT + Homebrew bundle, or apt)
#   --rust      rustup and the toolchain named in rust-toolchain.toml
#   --js        npm ci and the Playwright chromium browser
#   --coverage  opt-in: nightly, llvm-tools-preview, cargo-llvm-cov
set -eu
cd "$(dirname "$0")/.."

TAURI_DOCS=https://v2.tauri.app/start/prerequisites/
APT_PACKAGES="libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
  libxdo-dev libssl-dev build-essential curl wget file"

usage() {
  echo "usage: scripts/setup.sh [--system] [--rust] [--js] [--coverage]" >&2
}

die() {
  echo "setup: $*" >&2
  exit 1
}

have() {
  command -v "$1" >/dev/null 2>&1
}

node_major() {
  tr -d 'v\n ' <.nvmrc | cut -d. -f1
}

system_darwin() {
  xcode-select -p >/dev/null 2>&1 || {
    xcode-select --install || true
    die "install the Xcode command line tools in the dialog, then re-run this script"
  }
  have brew || die "install Homebrew from https://brew.sh, then re-run this script"
  brew bundle --file=Brewfile
  want=$(node_major)
  have_major=$(node --version 2>/dev/null | tr -d 'v' | cut -d. -f1 || true)
  if [ "$have_major" != "$want" ]; then
    echo "node $want is installed but not first on PATH; add this to your shell profile:" >&2
    echo "  export PATH=\"\$(brew --prefix node@$want)/bin:\$PATH\"" >&2
    PATH="$(brew --prefix "node@$want")/bin:$PATH"
    export PATH
    [ "$(node --version | tr -d 'v' | cut -d. -f1)" = "$want" ] \
      || die "node $want not found after brew bundle"
  fi
}

system_linux() {
  have apt-get || die "only Debian/Ubuntu is automated; see $TAURI_DOCS"
  if [ "$(id -u)" -eq 0 ]; then
    sudo=""
  else
    have sudo || die "sudo is required to install packages (or run as root)"
    sudo="sudo"
  fi
  # shellcheck disable=SC2086
  $sudo apt-get update
  # shellcheck disable=SC2086
  $sudo apt-get install -y $APT_PACKAGES
}

step_system() {
  case "$(uname -s)" in
    Darwin) system_darwin ;;
    Linux) system_linux ;;
    *) die "unsupported OS $(uname -s); see $TAURI_DOCS" ;;
  esac
}

load_cargo_env() {
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  return 0
}

step_rust() {
  load_cargo_env
  if ! have rustup; then
    have curl || die "curl is required to install rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain none
    load_cargo_env
  fi
  # Reads rust-toolchain.toml; installs the channel and components if missing.
  rustup toolchain install
}

step_js() {
  have npm || die "npm not found; run scripts/setup.sh --system and check PATH"
  npm ci
  case "$(uname -s)" in
    Linux) npx playwright install --with-deps chromium ;;
    *) npx playwright install chromium ;;
  esac
}

step_coverage() {
  load_cargo_env
  have rustup || die "rustup not found; run scripts/setup.sh --rust first"
  rustup toolchain install nightly --profile minimal --component llvm-tools-preview
  if have cargo-llvm-cov; then
    return 0
  elif have cargo-binstall; then
    cargo binstall -y cargo-llvm-cov
  else
    cargo install --locked cargo-llvm-cov
  fi
}

versions() {
  load_cargo_env
  for tool in rustc cargo node npm; do
    if have "$tool"; then
      echo "$tool: $("$tool" --version)"
    else
      echo "$tool: not found"
    fi
  done
}

do_system=0 do_rust=0 do_js=0 do_coverage=0
if [ $# -eq 0 ]; then
  do_system=1 do_rust=1 do_js=1
fi
for arg in "$@"; do
  case "$arg" in
    --system) do_system=1 ;;
    --rust) do_rust=1 ;;
    --js) do_js=1 ;;
    --coverage) do_coverage=1 ;;
    *) usage; exit 2 ;;
  esac
done

[ "$do_system" -eq 1 ] && step_system
[ "$do_rust" -eq 1 ] && step_rust
[ "$do_js" -eq 1 ] && step_js
[ "$do_coverage" -eq 1 ] && step_coverage
versions
