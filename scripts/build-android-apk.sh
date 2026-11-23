#!/usr/bin/env bash
set -euo pipefail

# Build an Android APK for Emperor Mod Manager (Tauri v2 Mobile).
#
# Usage:
#   ./scripts/build-android-apk.sh [options]
#
# Options:
#   --debug                 Build a debug APK instead of a release APK
#   --target <arch>         Target arch: aarch64 (default), armv7, x86_64, i686
#   --all-targets           Build universal APK across all Android ABIs
#   --skip-rustup           Do not auto-install missing Rust targets
#   --help, -h              Show this help message

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
GEN_ANDROID="$SRC_TAURI/gen/android"
FOLDABLE_SRC="$SRC_TAURI/android-plugin/FoldablePlugin.kt"
FOLDABLE_DEST="$GEN_ANDROID/app/src/main/java/dev/emperormodmanager/mobile/FoldablePlugin.kt"
UPDATE_SRC="$SRC_TAURI/android-plugin/AppUpdatePlugin.kt"
UPDATE_DEST="$GEN_ANDROID/app/src/main/java/dev/emperormodmanager/mobile/AppUpdatePlugin.kt"
FILE_PATHS_SRC="$SRC_TAURI/android-plugin/file_paths.xml"
FILE_PATHS_DEST="$GEN_ANDROID/app/src/main/res/xml/file_paths.xml"

DEBUG=0
TARGET="aarch64"
ALL_TARGETS=0
SKIP_RUSTUP=0

log() {
  printf '[build:android] %s\n' "$*"
}

fail() {
  printf '[build:android] ERROR: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage: ./scripts/build-android-apk.sh [options]

Options:
  --debug                 Build a debug APK instead of a release APK
  --target <arch>         Target arch: aarch64 (default), armv7, x86_64, i686
  --all-targets           Build universal APK across all Android ABIs
  --skip-rustup           Do not auto-install missing Rust targets
  --help, -h              Show this help message
EOF
}

rust_triple_for() {
  case "$1" in
    aarch64) echo "aarch64-linux-android" ;;
    armv7)   echo "armv7-linux-androideabi" ;;
    x86_64)  echo "x86_64-linux-android" ;;
    i686)    echo "i686-linux-android" ;;
    *)       fail "Invalid --target '$1'. Valid values: aarch64, armv7, x86_64, i686" ;;
  esac
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)
      DEBUG=1
      shift
      ;;
    --all-targets)
      ALL_TARGETS=1
      shift
      ;;
    --skip-rustup)
      SKIP_RUSTUP=1
      shift
      ;;
    --target)
      [[ $# -ge 2 ]] || fail "--target requires an argument"
      TARGET="$2"
      rust_triple_for "$TARGET" >/dev/null
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      fail "Unknown argument: $1. Run with --help for usage."
      ;;
  esac
done

# 1. Detect Android SDK / NDK / Java
if [[ -z "${ANDROID_HOME:-}" ]]; then
  for candidate in "${ANDROID_SDK_ROOT:-}" "$HOME/Android/Sdk" "$HOME/Library/Android/sdk" "/opt/android-sdk"; do
    if [[ -n "$candidate" && -d "$candidate" ]]; then
      export ANDROID_HOME="$candidate"
      break
    fi
  done
fi

if [[ -z "${ANDROID_HOME:-}" || ! -d "$ANDROID_HOME" ]]; then
  fail "ANDROID_HOME is not set and no Android SDK was found in ~/Android/Sdk or /opt/android-sdk."
fi
export ANDROID_SDK_ROOT="$ANDROID_HOME"

if [[ -z "${NDK_HOME:-}" || ! -d "${NDK_HOME:-}" ]]; then
  if [[ -d "$ANDROID_HOME/ndk" ]]; then
    latest_ndk="$(ls -1 "$ANDROID_HOME/ndk" | sort -V | tail -n 1 || true)"
    if [[ -n "$latest_ndk" && -d "$ANDROID_HOME/ndk/$latest_ndk" ]]; then
      export NDK_HOME="$ANDROID_HOME/ndk/$latest_ndk"
    fi
  fi
fi

if [[ -z "${NDK_HOME:-}" || ! -d "$NDK_HOME" ]]; then
  fail "Android NDK not found under $ANDROID_HOME/ndk. Install via sdkmanager and set NDK_HOME."
fi

if ! command -v java >/dev/null 2>&1; then
  fail "Java (JDK 17+) was not found on PATH. Install OpenJDK 17+ and configure JAVA_HOME / PATH."
fi

log "ANDROID_HOME=$ANDROID_HOME"
log "NDK_HOME=$NDK_HOME"

# 2. Ensure Rust Android targets are installed
if [[ "$SKIP_RUSTUP" -eq 0 ]]; then
  installed_targets="$(rustup target list --installed)"
  needed_targets=()
  if [[ "$ALL_TARGETS" -eq 1 ]]; then
    needed_targets=(
      "aarch64-linux-android"
      "armv7-linux-androideabi"
      "x86_64-linux-android"
      "i686-linux-android"
    )
  else
    needed_targets=("$(rust_triple_for "$TARGET")")
  fi

  missing_targets=()
  for t in "${needed_targets[@]}"; do
    if ! grep -qx "$t" <<<"$installed_targets"; then
      missing_targets+=("$t")
    fi
  done

  if [[ "${#missing_targets[@]}" -gt 0 ]]; then
    log "Installing missing Rust targets: ${missing_targets[*]}"
    rustup target add "${missing_targets[@]}"
  fi
fi

cd "$ROOT"

# 3. Initialize Tauri Android project if needed
if [[ ! -d "$GEN_ANDROID" ]]; then
  log "Initializing Tauri Android project (src-tauri/gen/android)..."
  npx tauri android init --ci
fi

# 4. Sync FoldablePlugin.kt, AppUpdatePlugin.kt, file_paths.xml, AndroidManifest, and Gradle config
if [[ -f "$FOLDABLE_SRC" ]]; then
  mkdir -p "$(dirname "$FOLDABLE_DEST")"
  cp "$FOLDABLE_SRC" "$FOLDABLE_DEST"
  log "Synced FoldablePlugin.kt -> ${FOLDABLE_DEST#"$ROOT/"}"
fi
if [[ -f "$UPDATE_SRC" ]]; then
  mkdir -p "$(dirname "$UPDATE_DEST")"
  cp "$UPDATE_SRC" "$UPDATE_DEST"
  log "Synced AppUpdatePlugin.kt -> ${UPDATE_DEST#"$ROOT/"}"
fi
if [[ -f "$FILE_PATHS_SRC" ]]; then
  mkdir -p "$(dirname "$FILE_PATHS_DEST")"
  cp "$FILE_PATHS_SRC" "$FILE_PATHS_DEST"
  log "Synced file_paths.xml -> ${FILE_PATHS_DEST#"$ROOT/"}"
fi

MANIFEST="$GEN_ANDROID/app/src/main/AndroidManifest.xml"
if [[ -f "$MANIFEST" ]]; then
  for perm in \
    "android.permission.INTERNET" \
    "android.permission.ACCESS_NETWORK_STATE" \
    "android.permission.ACCESS_WIFI_STATE" \
    "android.permission.CHANGE_WIFI_MULTICAST_STATE" \
    "android.permission.REQUEST_INSTALL_PACKAGES"; do
    if ! grep -q "$perm" "$MANIFEST"; then
      sed -i "s|<application|    <uses-permission android:name=\"${perm}\" />\n    <application|" "$MANIFEST"
      log "Added permission $perm to AndroidManifest.xml"
    fi
  done
  if ! grep -q '\.fileprovider' "$MANIFEST"; then
    sed -i 's|</application>|        <provider android:name="androidx.core.content.FileProvider" android:authorities="${applicationId}.fileprovider" android:exported="false" android:grantUriPermissions="true"><meta-data android:name="android.support.FILE_PROVIDER_PATHS" android:resource="@xml/file_paths" /></provider>\n    </application>|' "$MANIFEST"
    log "Added FileProvider to AndroidManifest.xml"
  fi
fi

GRADLE_FILE="$GEN_ANDROID/app/build.gradle.kts"
if [[ -f "$GRADLE_FILE" ]]; then
  if grep -Eq 'minSdk\s*=\s*([0-9]|1[0-9]|2[0-8])\b' "$GRADLE_FILE"; then
    sed -i -E 's/minSdk\s*=\s*[0-9]+/minSdk = 29/' "$GRADLE_FILE"
    log "Updated minSdk = 29 in app/build.gradle.kts"
  fi
  if ! grep -q 'androidx.window:window' "$GRADLE_FILE"; then
    sed -i 's|dependencies {|dependencies {\n    implementation("androidx.window:window:1.3.0")|' "$GRADLE_FILE"
    log "Added androidx.window:window:1.3.0 to app/build.gradle.kts"
  fi
fi

# 5. Build the APK
TAURI_ARGS=(tauri android build --apk)
if [[ "$DEBUG" -eq 1 ]]; then
  TAURI_ARGS+=(--debug)
fi
if [[ "$ALL_TARGETS" -eq 0 ]]; then
  TAURI_ARGS+=(--target "$TARGET")
fi

log "$ npx ${TAURI_ARGS[*]}"
npx "${TAURI_ARGS[@]}"

# 6. Sign any *-unsigned.apk so it can be installed directly on Android
APK_DIR="$GEN_ANDROID/app/build/outputs/apk"
if [[ -d "$APK_DIR" ]]; then
  BT_DIR=""
  if [[ -d "$ANDROID_HOME/build-tools" ]]; then
    latest_bt="$(ls -1 "$ANDROID_HOME/build-tools" | sort -V | tail -n 1 || true)"
    if [[ -n "$latest_bt" ]]; then
      BT_DIR="$ANDROID_HOME/build-tools/$latest_bt"
    fi
  fi

  mapfile -t UNSIGNED_APKS < <(find "$APK_DIR" -type f -name '*-unsigned.apk' | sort)
  if [[ "${#UNSIGNED_APKS[@]}" -gt 0 && -n "$BT_DIR" && -x "$BT_DIR/apksigner" ]]; then
    KS_PATH="${ANDROID_KEYSTORE:-$HOME/.android/emperor-local.keystore}"
    KS_PASS="${ANDROID_KEYSTORE_PASSWORD:-emperor-local-dev}"
    KS_ALIAS="${ANDROID_KEY_ALIAS:-emperor}"
    KEY_PASS="${ANDROID_KEY_PASSWORD:-$KS_PASS}"

    if [[ ! -f "$KS_PATH" ]]; then
      mkdir -p "$(dirname "$KS_PATH")"
      log "Generating persistent local signing keystore at $KS_PATH..."
      keytool -genkeypair -v \
        -keystore "$KS_PATH" \
        -storepass "$KS_PASS" \
        -keypass "$KEY_PASS" \
        -alias "$KS_ALIAS" \
        -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=Emperor Mod Manager, OU=Dev, O=Emperor, L=Local, ST=Local, C=US" >/dev/null 2>&1
    fi

    for unsigned_apk in "${UNSIGNED_APKS[@]}"; do
      aligned_apk="${unsigned_apk%-unsigned.apk}-aligned.apk"
      signed_apk="${unsigned_apk%-unsigned.apk}-signed.apk"
      if [[ -x "$BT_DIR/zipalign" ]]; then
        "$BT_DIR/zipalign" -p -f 4 "$unsigned_apk" "$aligned_apk"
      else
        cp "$unsigned_apk" "$aligned_apk"
      fi
      "$BT_DIR/apksigner" sign \
        --ks "$KS_PATH" \
        --ks-pass "pass:$KS_PASS" \
        --ks-key-alias "$KS_ALIAS" \
        --key-pass "pass:$KEY_PASS" \
        --out "$signed_apk" \
        "$aligned_apk"
      rm -f "$aligned_apk"
      log "Signed APK -> ${signed_apk#"$ROOT/"}"
    done
  fi

  mapfile -t APKS < <(find "$APK_DIR" -type f -name '*.apk' ! -name '*-unsigned.apk' | sort)
  if [[ "${#APKS[@]}" -eq 0 ]]; then
    mapfile -t APKS < <(find "$APK_DIR" -type f -name '*.apk' | sort)
  fi
  if [[ "${#APKS[@]}" -gt 0 ]]; then
    log "Installable Android APK(s):"
    for apk in "${APKS[@]}"; do
      size_mb="$(awk -v bytes="$(stat -c%s "$apk")" 'BEGIN { printf "%.2f", bytes / (1024 * 1024) }')"
      log "  - ${apk#"$ROOT/"} (${size_mb} MB)"
    done
  fi
fi
