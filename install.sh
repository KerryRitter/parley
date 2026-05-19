#!/bin/sh
set -eu

REPO="${PAR_REPO:-KerryRitter/programmatic-agent-router}"
REF="${PAR_REF:-main}"
INSTALL_DIR="${PAR_INSTALL_DIR:-$HOME/.local/bin}"
GIT_PROTOCOL="${PAR_GIT_PROTOCOL:-https}"
INSTALL_PAR=1
DRY_RUN=0
FROM_SOURCE=0

usage() {
  cat <<'EOF'
Usage: install.sh [options]

Installs agent-router from GitHub.

Options:
  --install-dir <dir>  Install directory (default: ~/.local/bin)
  --repo <owner/repo>  GitHub repository (default: KerryRitter/programmatic-agent-router)
  --ref <ref>          Git ref for cargo source fallback (default: main)
  --git-protocol <p>   Git protocol for source fallback: https or ssh (default: https)
  --from-source        Skip release binaries and install with cargo from GitHub
  --no-par             Do not create the par convenience symlink
  --dry-run            Print actions without installing
  -h, --help           Show this help

Environment:
  PAR_INSTALL_DIR      Install directory
  PAR_REPO             GitHub repository
  PAR_REF              Git ref for cargo source fallback
  PAR_GIT_PROTOCOL     Git protocol for source fallback: https or ssh
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
  if [ "$DRY_RUN" -eq 1 ]; then
    printf 'dry-run:'
    for arg in "$@"; do
      printf ' %s' "$arg"
    done
    printf '\n'
    return 0
  fi
  "$@"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --install-dir)
      [ "$#" -ge 2 ] || fail "--install-dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --repo)
      [ "$#" -ge 2 ] || fail "--repo requires a value"
      REPO="$2"
      shift 2
      ;;
    --ref)
      [ "$#" -ge 2 ] || fail "--ref requires a value"
      REF="$2"
      shift 2
      ;;
    --git-protocol)
      [ "$#" -ge 2 ] || fail "--git-protocol requires a value"
      GIT_PROTOCOL="$2"
      shift 2
      ;;
    --from-source)
      FROM_SOURCE=1
      shift
      ;;
    --no-par)
      INSTALL_PAR=0
      shift
      ;;
    --dry-run)
      DRY_RUN=1
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

detect_target() {
  os="$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m 2>/dev/null)"

  case "$os" in
    linux)
      os_part="unknown-linux-gnu"
      ;;
    darwin)
      os_part="apple-darwin"
      ;;
    *)
      return 1
      ;;
  esac

  case "$arch" in
    x86_64|amd64)
      arch_part="x86_64"
      ;;
    arm64|aarch64)
      arch_part="aarch64"
      ;;
    *)
      return 1
      ;;
  esac

  printf '%s-%s\n' "$arch_part" "$os_part"
}

download() {
  url="$1"
  dest="$2"

  if have curl; then
    run curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$dest"
    return $?
  fi

  if have wget; then
    run wget -q "$url" -O "$dest"
    return $?
  fi

  fail "curl or wget is required to download release binaries"
}

install_from_release() {
  target="$1"
  tmp_dir="$2"
  archive="$tmp_dir/agent-router.tar.gz"
  url="https://github.com/$REPO/releases/latest/download/agent-router-$target.tar.gz"

  info "trying release binary: $url"

  if ! download "$url" "$archive"; then
    warn "release binary not available for $target"
    return 1
  fi

  run tar -xzf "$archive" -C "$tmp_dir"
  [ "$DRY_RUN" -eq 1 ] || [ -x "$tmp_dir/agent-router" ] || fail "release archive did not contain executable agent-router"

  run mkdir -p "$INSTALL_DIR"
  run install -m 0755 "$tmp_dir/agent-router" "$INSTALL_DIR/agent-router"
  return 0
}

install_from_source() {
  if ! have cargo; then
    cargo_bin="$HOME/.cargo/bin/cargo"
    if [ -x "$cargo_bin" ]; then
      PATH="$HOME/.cargo/bin:$PATH"
      export PATH
    fi
  fi

  have cargo || fail "cargo is required for source install. Install Rust from https://rustup.rs/"

  case "$GIT_PROTOCOL" in
    https)
      repo_url="https://github.com/$REPO.git"
      ;;
    ssh)
      repo_url="ssh://git@github.com/$REPO.git"
      ;;
    *)
      fail "--git-protocol must be https or ssh"
      ;;
  esac
  info "installing from source: $repo_url#$REF"
  if [ "$DRY_RUN" -eq 1 ]; then
    run env CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --git "$repo_url" --branch "$REF" --force
  else
    CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --git "$repo_url" --branch "$REF" --force
  fi

  installed="$(command -v agent-router || true)"
  if [ -z "$installed" ] && [ -x "$HOME/.cargo/bin/agent-router" ]; then
    installed="$HOME/.cargo/bin/agent-router"
  fi

  if [ "$DRY_RUN" -eq 0 ]; then
    [ -n "$installed" ] || fail "cargo install finished but agent-router was not found"
    run mkdir -p "$INSTALL_DIR"
    run install -m 0755 "$installed" "$INSTALL_DIR/agent-router"
  else
    run mkdir -p "$INSTALL_DIR"
    run install -m 0755 "\$HOME/.cargo/bin/agent-router" "$INSTALL_DIR/agent-router"
  fi
}

tmp_dir="$(mktemp -d 2>/dev/null || mktemp -d -t agent-router)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

target="$(detect_target || true)"

if [ "$FROM_SOURCE" -eq 0 ] && [ -n "$target" ]; then
  if install_from_release "$target" "$tmp_dir"; then
    :
  else
    install_from_source
  fi
else
  if [ "$FROM_SOURCE" -eq 0 ]; then
    warn "no release target mapping for this platform; falling back to source install"
  fi
  install_from_source
fi

if [ "$INSTALL_PAR" -eq 1 ]; then
  run mkdir -p "$INSTALL_DIR"
  run ln -sf "$INSTALL_DIR/agent-router" "$INSTALL_DIR/par"
fi

info "installed agent-router to $INSTALL_DIR/agent-router"
if [ "$INSTALL_PAR" -eq 1 ]; then
  info "installed par alias to $INSTALL_DIR/par"
fi

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) warn "$INSTALL_DIR is not on PATH" ;;
esac
