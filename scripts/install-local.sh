#!/usr/bin/env bash
# Builds Emperor Mod Manager from the local checkout and installs it for the
# current user:
#   - Binary:  ~/.local/bin/emperor-mod-manager        (override: EMM_BIN_DIR)
#   - Desktop: ~/.local/share/applications/emperor-mod-manager.desktop
#   - Icons:   ~/.local/share/icons/hicolor/*/apps/emperor-mod-manager.png
#   - Schemes: x-scheme-handler/nxm, x-scheme-handler/emperor
#
# Usage: ./scripts/install-local.sh [--skip-build] [--debug] [--uninstall]
set -euo pipefail

APP_ID="emperor-mod-manager"
DESKTOP_FILE="${APP_ID}.desktop"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN_DIR="${EMM_BIN_DIR:-$HOME/.local/bin}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS_DIR="$DATA_HOME/applications"
ICONS_DIR="$DATA_HOME/icons/hicolor"

SKIP_BUILD=0
BUILD_PROFILE="release"
UNINSTALL=0

die() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

info() {
  printf '==> %s\n' "$1"
}

warn() {
  printf 'warning: %s\n' "$1" >&2
}

usage() {
  cat <<'EOF'
Usage: ./scripts/install-local.sh [options]

Builds Emperor Mod Manager from the local repository, installs the binary to
~/.local/bin, installs the .desktop entry and icons, and registers nxm:// and
emperor:// URL handlers.

Options:
  --skip-build   Install using the already-built binary without recompiling
  --debug        Build/install the debug binary instead of release
  --uninstall    Remove the installed binary, .desktop entry, and icons
  -h, --help     Show this help message

Environment:
  EMM_BIN_DIR    Override binary install directory (default: ~/.local/bin)
  XDG_DATA_HOME  Override XDG data directory (default: ~/.local/share)
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build)
      SKIP_BUILD=1
      ;;
    --debug)
      BUILD_PROFILE="debug"
      ;;
    --uninstall)
      UNINSTALL=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1 (try --help)"
      ;;
  esac
  shift
done

[ "$(uname -s)" = "Linux" ] || die "this script is for Linux only"
[ "$(id -u)" -ne 0 ] || die "run this as your normal user, not root"

refresh_desktop_caches() {
  if command -v update-desktop-database >/dev/null 2>&1 && [ -d "$APPS_DIR" ]; then
    update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
  fi
  if [ -d "$ICONS_DIR" ]; then
    touch "$ICONS_DIR" >/dev/null 2>&1 || true
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
      gtk-update-icon-cache -f -t "$ICONS_DIR" >/dev/null 2>&1 || true
    fi
  fi
  rm -f "${XDG_CACHE_HOME:-$HOME/.cache}/icon-cache.kcache" >/dev/null 2>&1 || true
  if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 >/dev/null 2>&1 || true
  elif command -v kbuildsycoca5 >/dev/null 2>&1; then
    kbuildsycoca5 >/dev/null 2>&1 || true
  fi
}

remove_installed_icons() {
  if [ -d "$ICONS_DIR" ]; then
    find "$ICONS_DIR" -maxdepth 3 \( -path "*/apps/$APP_ID.png" -o -path "*/apps/$APP_ID.svg" \) -delete
  fi
}

if [ "$UNINSTALL" -eq 1 ]; then
  info "Uninstalling local $APP_ID"
  rm -f "$BIN_DIR/$APP_ID"
  rm -f "$APPS_DIR/$DESKTOP_FILE"
  remove_installed_icons
  refresh_desktop_caches
  info "Removed binary, desktop entry, and icons from $BIN_DIR and $DATA_HOME"
  exit 0
fi

# Escape an executable path for a Desktop Entry Exec= line if it contains
# whitespace or reserved characters per the freedesktop Desktop Entry Spec.
format_desktop_exec_path() {
  local p="$1"
  if [[ "$p" =~ [[:space:]\"\'\\\$\`\&\|\;\<\>\(\)\*\?\~\#] ]]; then
    p="${p//\\/\\\\}"
    p="${p//\"/\\\"}"
    p="${p//\$/\\\$}"
    p="${p//\`/\\\`}"
    printf '"%s"' "$p"
  else
    printf '%s' "$p"
  fi
}

if [ "$SKIP_BUILD" -eq 0 ]; then
  command -v npm >/dev/null 2>&1 || die "npm is required to build"
  command -v cargo >/dev/null 2>&1 || die "cargo is required to build"

  info "Building $APP_ID ($BUILD_PROFILE, --no-bundle)"
  build_args=(run tauri -- build --no-bundle)
  if [ "$BUILD_PROFILE" = "debug" ]; then
    build_args+=(--debug)
  fi
  (cd "$REPO_ROOT" && npm "${build_args[@]}")
fi

BINARY_SRC="$REPO_ROOT/src-tauri/target/$BUILD_PROFILE/$APP_ID"
[ -f "$BINARY_SRC" ] || die "missing built binary at $BINARY_SRC (run without --skip-build)"

DESKTOP_SRC="$REPO_ROOT/src-tauri/$DESKTOP_FILE"
[ -f "$DESKTOP_SRC" ] || die "missing desktop template at $DESKTOP_SRC"

TMP_BIN=""
cleanup() {
  if [ -n "$TMP_BIN" ] && [ -e "$TMP_BIN" ]; then
    rm -f "$TMP_BIN"
  fi
}
trap cleanup EXIT

info "Installing binary to $BIN_DIR/$APP_ID"
mkdir -p "$BIN_DIR"
TMP_BIN="$(mktemp "$BIN_DIR/.$APP_ID.XXXXXX")"
install -m 755 "$BINARY_SRC" "$TMP_BIN"
mv -f "$TMP_BIN" "$BIN_DIR/$APP_ID"
TMP_BIN=""

info "Installing icons to $ICONS_DIR"
remove_installed_icons
install_icon() {
  local src="$REPO_ROOT/src-tauri/icons/$1"
  local size="$2"
  if [ -f "$src" ]; then
    install -Dm644 "$src" "$ICONS_DIR/$size/apps/$APP_ID.png"
  else
    warn "missing icon $src; skipping $size"
  fi
}
install_icon "32x32.png" "32x32"
install_icon "64x64.png" "64x64"
install_icon "128x128.png" "128x128"
install_icon "128x128@2x.png" "256x256"
install_icon "icon.png" "512x512"

info "Installing desktop entry to $APPS_DIR/$DESKTOP_FILE"
mkdir -p "$APPS_DIR"
EXEC_PATH="$(format_desktop_exec_path "$BIN_DIR/$APP_ID")"
DESKTOP_DEST="$APPS_DIR/$DESKTOP_FILE"
awk -v exec_line="Exec=$EXEC_PATH %u" '
  /^Exec=/ { print exec_line; replaced=1; next }
  { print }
  END { if (!replaced) print exec_line }
' "$DESKTOP_SRC" > "$DESKTOP_DEST"
chmod 644 "$DESKTOP_DEST"

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$DESKTOP_DEST" || warn "desktop-file-validate reported warnings"
fi

refresh_desktop_caches

if command -v xdg-mime >/dev/null 2>&1; then
  info "Registering nxm:// and emperor:// URL handlers"
  mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}"
  xdg-mime default "$DESKTOP_FILE" x-scheme-handler/nxm
  xdg-mime default "$DESKTOP_FILE" x-scheme-handler/emperor
  nxm_handler="$(xdg-mime query default x-scheme-handler/nxm 2>/dev/null || true)"
  emperor_handler="$(xdg-mime query default x-scheme-handler/emperor 2>/dev/null || true)"
  info "Handlers: nxm -> ${nxm_handler:-<unset>}, emperor -> ${emperor_handler:-<unset>}"
else
  warn "xdg-mime not found; install xdg-utils to register nxm:// and emperor:// links"
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    warn "$BIN_DIR is not in your PATH; add 'export PATH=\"$BIN_DIR:\$PATH\"' to your shell profile"
    ;;
esac

resolved_bin="$(command -v "$APP_ID" 2>/dev/null || true)"
if [ -n "$resolved_bin" ] && [ "$resolved_bin" != "$BIN_DIR/$APP_ID" ]; then
  warn "'$APP_ID' in PATH currently resolves to $resolved_bin instead of $BIN_DIR/$APP_ID"
fi

info "Done. Launch with: $BIN_DIR/$APP_ID"
