#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_HOME="${CARGO_HOME:-${HOME}/.cargo}"
INSTALL=0
STRICT_HARNESSES=0
RUN_CLIPPY=1
RUN_FMT=1
RUN_TESTS=1
RUN_BUILD=1
INSTALL_DIR="${HOME}/.local/bin"

usage() {
  cat <<'EOF'
Usage: scripts/setup.sh [options]

Validates this machine for developing and running agent-router.

Options:
  --install              Install agent-router with cargo install --path .
  --install-dir <dir>    Directory for the optional par symlink (default: ~/.local/bin)
  --strict-harnesses     Fail if none of the supported downstream harness CLIs are installed
  --no-clippy            Skip cargo clippy
  --no-fmt               Skip cargo fmt --check
  --no-tests             Skip cargo test
  --no-build             Skip cargo build --release
  -h, --help             Show this help
EOF
}

info() {
  printf 'info: %s\n' "$*"
}

warn() {
  printf 'warn: %s\n' "$*" >&2
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

have() {
  command -v "$1" >/dev/null 2>&1
}

run() {
  info "running: $*"
  "$@"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --install)
      INSTALL=1
      shift
      ;;
    --install-dir)
      [ "$#" -ge 2 ] || fail "--install-dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --strict-harnesses)
      STRICT_HARNESSES=1
      shift
      ;;
    --no-clippy)
      RUN_CLIPPY=0
      shift
      ;;
    --no-fmt)
      RUN_FMT=0
      shift
      ;;
    --no-tests)
      RUN_TESTS=0
      shift
      ;;
    --no-build)
      RUN_BUILD=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown option: $1"
      ;;
  esac
done

cd "$ROOT_DIR" || fail "failed to enter repository root: $ROOT_DIR"

info "repository: $ROOT_DIR"
info "system: $(uname -s 2>/dev/null || printf unknown) $(uname -m 2>/dev/null || printf unknown)"

[ -f Cargo.toml ] || fail "Cargo.toml not found; run this from the agent-router repository"
[ -f src/main.rs ] || fail "src/main.rs not found; repository layout is incomplete"

if ! have cargo && [ -x "${CARGO_HOME}/bin/cargo" ]; then
  export PATH="${CARGO_HOME}/bin:${PATH}"
  info "added ${CARGO_HOME}/bin to PATH for this setup run"
fi

if ! have cargo; then
  fail "cargo is required. Install Rust from https://rustup.rs/ or your system package manager."
fi

if ! have rustc; then
  fail "rustc is required. Your Rust installation appears incomplete."
fi

info "cargo: $(cargo --version)"
info "rustc: $(rustc --version)"

if have rustup; then
  info "rustup: found"
else
  warn "rustup not found; cannot auto-check rustfmt/clippy components"
fi

if [ "$RUN_FMT" -eq 1 ]; then
  if cargo fmt --version >/dev/null 2>&1; then
    run cargo fmt --check
  else
    warn "cargo fmt unavailable; skipping format check"
    if have rustup; then
      warn "install it with: rustup component add rustfmt"
    fi
  fi
fi

if [ "$RUN_TESTS" -eq 1 ]; then
  run cargo test
fi

if [ "$RUN_CLIPPY" -eq 1 ]; then
  if cargo clippy --version >/dev/null 2>&1; then
    run cargo clippy --all-targets -- -D warnings
  else
    warn "cargo clippy unavailable; skipping lint check"
    if have rustup; then
      warn "install it with: rustup component add clippy"
    fi
  fi
fi

if [ "$RUN_BUILD" -eq 1 ]; then
  run cargo build --release
fi

supported_harnesses=(
  "claude:claude"
  "codex:codex"
  "cursor:cursor-agent"
  "gemini:gemini"
  "goose:goose"
  "opencode:opencode"
  "qwen:qwen"
  "aider:aider"
  "amazon-q:q"
  "copilot:copilot"
)

installed_harnesses=0
info "checking downstream harness CLIs"
for entry in "${supported_harnesses[@]}"; do
  harness="${entry%%:*}"
  binary="${entry#*:}"
  if have "$binary"; then
    installed_harnesses=$((installed_harnesses + 1))
    info "  ok: $harness ($binary)"
  else
    warn "  missing: $harness ($binary)"
  fi
done

if [ "$STRICT_HARNESSES" -eq 1 ] && [ "$installed_harnesses" -eq 0 ]; then
  fail "no supported downstream harness CLIs found on PATH"
fi

if [ "$INSTALL" -eq 1 ]; then
  run cargo install --path . --force

  mkdir -p "$INSTALL_DIR" || fail "failed to create install dir: $INSTALL_DIR"
  [ -w "$INSTALL_DIR" ] || fail "install dir is not writable: $INSTALL_DIR"

  agent_router_path="$(command -v agent-router || true)"
  if [ -z "$agent_router_path" ]; then
    if [ -x "${CARGO_HOME}/bin/agent-router" ]; then
      agent_router_path="${CARGO_HOME}/bin/agent-router"
    fi
  fi
  [ -n "$agent_router_path" ] || fail "cargo install completed, but agent-router could not be found"

  ln -sf "$agent_router_path" "$INSTALL_DIR/par" || fail "failed to create par symlink in $INSTALL_DIR"
  info "installed: $agent_router_path"
  info "linked: $INSTALL_DIR/par"

  if ! printf '%s' "$PATH" | grep -q "$INSTALL_DIR"; then
    warn "$INSTALL_DIR is not on PATH; add it before using par"
  fi
fi

info "setup validation complete"
