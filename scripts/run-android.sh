#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${BASH_SOURCE-}" ]]; then
  SCRIPT_PATH="${BASH_SOURCE[0]}"
else
  SCRIPT_PATH="$0"
fi

SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

VARIANT="mobile"
BUILD_TYPE="debug"
DEVICE_SERIAL="${ANDROID_SERIAL-}"
ANDROID_USER_ID="${ANDROID_USER_ID-0}"
DO_BUILD=1
DO_INSTALL=1
DO_LAUNCH=1
LAUNCH_TARGET=""

usage() {
  cat <<USAGE
Build, install, and launch Wavry Android in one command.

Usage:
  ./scripts/run-android.sh [options]

Options:
  --variant V         mobile, quest, or both (default: mobile)
  --mobile            Shortcut for --variant mobile
  --quest             Shortcut for --variant quest
  --both              Shortcut for --variant both
  --debug             Build/install debug APKs (default)
  --release           Build/install release APKs
  --serial SERIAL     Use a specific adb device serial
  --user ID           Android user id for launch (default: 0)
  --launch-target V   Which app to launch: mobile or quest
  --no-build          Skip build
  --no-install        Skip install
  --no-launch         Skip launch
  -h, --help          Show help

Examples:
  ./scripts/run-android.sh
  ./scripts/run-android.sh --quest
  ./scripts/run-android.sh --both --launch-target quest
  ./scripts/run-android.sh --mobile --serial ABC123 --release
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --variant)
      shift
      if [[ $# -eq 0 ]]; then
        echo "Missing value for --variant" >&2
        exit 2
      fi
      case "$1" in
        mobile|quest|both)
          VARIANT="$1"
          ;;
        *)
          echo "Invalid variant: $1 (expected mobile|quest|both)" >&2
          exit 2
          ;;
      esac
      shift
      ;;
    --mobile)
      VARIANT="mobile"
      shift
      ;;
    --quest)
      VARIANT="quest"
      shift
      ;;
    --both)
      VARIANT="both"
      shift
      ;;
    --debug)
      BUILD_TYPE="debug"
      shift
      ;;
    --release)
      BUILD_TYPE="release"
      shift
      ;;
    --serial)
      shift
      if [[ $# -eq 0 ]]; then
        echo "Missing value for --serial" >&2
        exit 2
      fi
      DEVICE_SERIAL="$1"
      shift
      ;;
    --user)
      shift
      if [[ $# -eq 0 ]]; then
        echo "Missing value for --user" >&2
        exit 2
      fi
      ANDROID_USER_ID="$1"
      shift
      ;;
    --launch-target)
      shift
      if [[ $# -eq 0 ]]; then
        echo "Missing value for --launch-target" >&2
        exit 2
      fi
      case "$1" in
        mobile|quest)
          LAUNCH_TARGET="$1"
          ;;
        *)
          echo "Invalid launch target: $1 (expected mobile|quest)" >&2
          exit 2
          ;;
      esac
      shift
      ;;
    --no-build)
      DO_BUILD=0
      shift
      ;;
    --no-install)
      DO_INSTALL=0
      shift
      ;;
    --no-launch)
      DO_LAUNCH=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ "$DO_INSTALL" -eq 1 || "$DO_LAUNCH" -eq 1 ]]; then
  if ! command -v adb >/dev/null 2>&1; then
    echo "adb is required. Install Android platform-tools and ensure adb is on PATH." >&2
    exit 1
  fi
fi

if [[ -z "$LAUNCH_TARGET" ]]; then
  if [[ "$VARIANT" == "quest" ]]; then
    LAUNCH_TARGET="quest"
  else
    LAUNCH_TARGET="mobile"
  fi
fi

if [[ "$DO_BUILD" -eq 1 ]]; then
  build_args=("--variant" "$VARIANT" "--$BUILD_TYPE")
  echo "[1/3] Building Android artifacts (${VARIANT}, ${BUILD_TYPE})..."
  "$SCRIPT_DIR/dev-android.sh" "${build_args[@]}"
fi

if [[ "$DO_INSTALL" -eq 0 && "$DO_LAUNCH" -eq 0 ]]; then
  echo "Done (build only)."
  exit 0
fi

ADB_CMD=(adb)
if [[ -z "$DEVICE_SERIAL" ]]; then
  device_count=0
  first_device=""

  while read -r serial status _rest; do
    [[ -z "${serial:-}" ]] && continue
    [[ "$serial" == "List" ]] && continue
    if [[ "$status" == "device" ]]; then
      device_count=$((device_count + 1))
      if [[ $device_count -eq 1 ]]; then
        first_device="$serial"
      fi
    fi
  done <<EOF_DEVICES
$(adb devices)
EOF_DEVICES

  if [[ $device_count -eq 0 ]]; then
    echo "No adb device detected. Connect your Android device and run 'adb devices'." >&2
    exit 1
  fi

  if [[ $device_count -gt 1 ]]; then
    echo "Multiple adb devices detected. Re-run with --serial <device-id>." >&2
    adb devices
    exit 1
  fi

  DEVICE_SERIAL="$first_device"
fi

ADB_CMD+=( -s "$DEVICE_SERIAL" )
echo "Using adb device: $DEVICE_SERIAL"

apk_path() {
  local variant="$1"
  local profile="$2"
  echo "$REPO_ROOT/apps/android/app/build/outputs/apk/${variant}/${profile}/app-${variant}-${profile}.apk"
}

find_apksigner() {
  if command -v apksigner >/dev/null 2>&1; then
    command -v apksigner
    return 0
  fi

  local sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
  if [[ -d "$sdk_root/build-tools" ]]; then
    find "$sdk_root/build-tools" -name apksigner -type f 2>/dev/null | sort | tail -n 1
    return 0
  fi

  return 1
}

resolve_apk_path() {
  local variant="$1"
  local profile="$2"
  local dir="$REPO_ROOT/apps/android/app/build/outputs/apk/${variant}/${profile}"
  local preferred
  preferred="$(apk_path "$variant" "$profile")"

  if [[ -f "$preferred" ]]; then
    echo "$preferred"
    return 0
  fi

  if [[ -d "$dir" ]]; then
    local fallback
    fallback="$(find "$dir" -maxdepth 1 -type f -name "*.apk" ! -name "*-unsigned.apk" | sort | head -n 1)"
    if [[ -n "$fallback" ]]; then
      echo "$fallback"
      return 0
    fi
  fi

  return 1
}

ensure_release_apk_signed() {
  local apk="$1"
  if [[ "$apk" == *"-unsigned.apk" ]]; then
    echo "Release APK is unsigned: $apk" >&2
    return 1
  fi

  local apksigner_bin
  apksigner_bin="$(find_apksigner || true)"
  if [[ -n "${apksigner_bin:-}" && -x "$apksigner_bin" ]]; then
    if ! "$apksigner_bin" verify "$apk" >/dev/null 2>&1; then
      echo "Release APK signature verification failed: $apk" >&2
      return 1
    fi
  fi

  return 0
}

package_name() {
  local variant="$1"
  if [[ "$variant" == "quest" ]]; then
    echo "com.wavry.android.quest"
  else
    echo "com.wavry.android.mobile"
  fi
}

install_variant() {
  local variant="$1"
  local apk
  if ! apk="$(resolve_apk_path "$variant" "$BUILD_TYPE")"; then
    echo "APK not found for ${variant}/${BUILD_TYPE}" >&2
    echo "Build first with: ./scripts/dev-android.sh --variant $variant --$BUILD_TYPE" >&2
    exit 1
  fi

  if [[ "$BUILD_TYPE" == "release" ]]; then
    if ! ensure_release_apk_signed "$apk"; then
      echo "Release signing is required for install." >&2
      echo "Set WAVRY_ANDROID_RELEASE_* env vars for production keystore or use the default local signing path." >&2
      exit 1
    fi
  fi

  echo "Installing ${variant} APK..."
  echo "APK: $apk"
  "${ADB_CMD[@]}" install -r "$apk" >/dev/null
  echo "Installed: $apk"
}

if [[ "$DO_INSTALL" -eq 1 ]]; then
  echo "[2/3] Installing APK(s)..."
  if [[ "$VARIANT" == "both" ]]; then
    install_variant "mobile"
    install_variant "quest"
  else
    install_variant "$VARIANT"
  fi
fi

if [[ "$DO_LAUNCH" -eq 1 ]]; then
  echo "[3/3] Launching app..."
  if [[ "$VARIANT" == "both" && "$LAUNCH_TARGET" != "mobile" && "$LAUNCH_TARGET" != "quest" ]]; then
    LAUNCH_TARGET="mobile"
  fi

  pkg="$(package_name "$LAUNCH_TARGET")"
  activity="com.wavry.android.MainActivity"

  if ! "${ADB_CMD[@]}" shell am start --user "$ANDROID_USER_ID" -n "${pkg}/${activity}" >/dev/null; then
    echo "Failed to launch ${pkg}/${activity}. Ensure the package is installed for user ${ANDROID_USER_ID}." >&2
    exit 1
  fi

  echo "Launched: ${pkg}/${activity} (user ${ANDROID_USER_ID})"

  if [[ "$VARIANT" == "both" ]]; then
    if [[ "$LAUNCH_TARGET" == "mobile" ]]; then
      echo "Quest package also installed: com.wavry.android.quest"
    else
      echo "Mobile package also installed: com.wavry.android.mobile"
    fi
  fi
fi

echo "Done."
