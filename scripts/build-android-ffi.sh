#!/usr/bin/env bash
set -euo pipefail

# Works when run via bash (`./script`) and when explicitly invoked with zsh (`zsh script`).
if [[ -n "${BASH_SOURCE-}" ]]; then
  SCRIPT_PATH="${BASH_SOURCE[0]}"
else
  SCRIPT_PATH="$0"
fi

SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
OUT_DIR="$REPO_ROOT/apps/android/app/src/main/cpp/prebuilt"

PROFILE="release"
ABIS=()
AUTO_INSTALL_CARGO_NDK=1
AUTO_INSTALL_NDK=1

detect_android_sdk_root() {
  if [[ -n "${ANDROID_SDK_ROOT-}" && -d "${ANDROID_SDK_ROOT}" ]]; then
    return 0
  fi
  if [[ -n "${ANDROID_HOME-}" && -d "${ANDROID_HOME}" ]]; then
    export ANDROID_SDK_ROOT="${ANDROID_HOME}"
    return 0
  fi

  for candidate in \
    "$HOME/Library/Android/sdk" \
    "$HOME/Android/Sdk" \
    "/opt/android-sdk" \
    "/usr/local/share/android-sdk"
  do
    if [[ -d "$candidate" ]]; then
      export ANDROID_SDK_ROOT="$candidate"
      return 0
    fi
  done

  return 1
}

detect_android_ndk_home() {
  if [[ -n "${ANDROID_NDK_HOME-}" && -d "${ANDROID_NDK_HOME}" ]]; then
    return 0
  fi

  if ! detect_android_sdk_root; then
    return 1
  fi

  if [[ -d "${ANDROID_SDK_ROOT}/ndk-bundle" ]]; then
    export ANDROID_NDK_HOME="${ANDROID_SDK_ROOT}/ndk-bundle"
    return 0
  fi

  latest_ndk=""
  if [[ -d "${ANDROID_SDK_ROOT}/ndk" ]]; then
    while IFS= read -r dir; do
      latest_ndk="$dir"
    done < <(find "${ANDROID_SDK_ROOT}/ndk" -mindepth 1 -maxdepth 1 -type d | sort)
  fi

  if [[ -n "$latest_ndk" ]]; then
    export ANDROID_NDK_HOME="$latest_ndk"
    return 0
  fi

  return 1
}

find_sdkmanager() {
  if [[ -n "${ANDROID_SDK_ROOT-}" ]]; then
    for candidate in \
      "${ANDROID_SDK_ROOT}/cmdline-tools/latest/bin/sdkmanager" \
      "${ANDROID_SDK_ROOT}/cmdline-tools/bin/sdkmanager" \
      "${ANDROID_SDK_ROOT}/tools/bin/sdkmanager"
    do
      if [[ -x "$candidate" ]]; then
        echo "$candidate"
        return 0
      fi
    done
  fi

  if command -v sdkmanager >/dev/null 2>&1; then
    command -v sdkmanager
    return 0
  fi

  return 1
}

guess_latest_ndk_package() {
  local sdkmanager_bin="$1"
  local latest

  latest="$("$sdkmanager_bin" --list 2>/dev/null \
    | sed -n 's/^[[:space:]]*\(ndk;[0-9][^[:space:]]*\)[[:space:]].*/\1/p' \
    | tail -n 1)"

  if [[ -n "$latest" ]]; then
    echo "$latest"
  else
    # Stable fallback package id.
    echo "ndk;26.3.11579264"
  fi
}

usage() {
  cat <<USAGE
Build wavry-ffi static libs for Android ABIs and place them under apps/android.

Usage:
  ./scripts/build-android-ffi.sh [--debug] [--abi <abi>]...

Options:
  --debug        Build debug profile (default is release).
  --abi <abi>    ABI to build (repeatable). Supported: arm64-v8a, x86_64.
  --no-install   Do not auto-install cargo-ndk if missing.
  --no-ndk-install  Do not auto-install Android NDK when sdkmanager is available.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)
      PROFILE="debug"
      shift
      ;;
    --abi)
      shift
      if [[ $# -eq 0 ]]; then
        echo "Missing value for --abi" >&2
        exit 2
      fi
      ABIS+=("$1")
      shift
      ;;
    --no-install)
      AUTO_INSTALL_CARGO_NDK=0
      shift
      ;;
    --no-ndk-install)
      AUTO_INSTALL_NDK=0
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

if ! cargo ndk --help >/dev/null 2>&1; then
  if [[ "$AUTO_INSTALL_CARGO_NDK" -eq 1 ]]; then
    echo "cargo-ndk not found. Installing it now..."
    cargo install cargo-ndk
  else
    echo "cargo-ndk is required. Install with: cargo install cargo-ndk" >&2
    exit 1
  fi
fi

if ! detect_android_ndk_home; then
  sdkmanager_bin="$(find_sdkmanager || true)"

  if [[ "$AUTO_INSTALL_NDK" -eq 1 && -n "${sdkmanager_bin:-}" ]]; then
    ndk_pkg="$(guess_latest_ndk_package "$sdkmanager_bin")"
    echo "Android NDK not found. Installing ${ndk_pkg} via sdkmanager..."
    echo "Using sdkmanager at: $sdkmanager_bin"

    # Best-effort license acceptance (safe to ignore if already accepted).
    yes | "$sdkmanager_bin" --licenses >/dev/null 2>&1 || true

    if ! "$sdkmanager_bin" "$ndk_pkg" "cmake;3.22.1"; then
      cat >&2 <<ERR
Automatic NDK install failed.
Install from Android Studio > Settings > Android SDK > SDK Tools:
  - NDK (Side by side)
  - Android SDK Command-line Tools (latest)
  - CMake
ERR
      exit 1
    fi

    if ! detect_android_ndk_home; then
      echo "NDK install completed but ANDROID_NDK_HOME still not detected." >&2
      exit 1
    fi
  fi

  if ! detect_android_ndk_home; then
  cat >&2 <<ERR
Could not find any Android NDK.

Detected ANDROID_SDK_ROOT: ${ANDROID_SDK_ROOT:-<not set>}

Install Android NDK from Android Studio (SDK Manager), or set:
  export ANDROID_NDK_HOME=/path/to/Android/Sdk/ndk/<version>
  export ANDROID_SDK_ROOT=/path/to/Android/Sdk
ERR
  exit 1
  fi
fi

if [[ ${#ABIS[@]} -eq 0 ]]; then
  ABIS=("arm64-v8a" "x86_64")
fi

# Remove duplicates while preserving order (Bash 3.2 compatible).
filtered_abis=()
for abi in "${ABIS[@]}"; do
  already_present=0
  for existing in "${filtered_abis[@]-}"; do
    if [[ "$existing" == "$abi" ]]; then
      already_present=1
      break
    fi
  done
  if [[ "$already_present" -eq 0 ]]; then
    filtered_abis+=("$abi")
  fi
done
ABIS=("${filtered_abis[@]}")

for abi in "${ABIS[@]}"; do
  case "$abi" in
    arm64-v8a|x86_64) ;;
    *)
      echo "Unsupported ABI: $abi" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$OUT_DIR"

profile_dir="$PROFILE"
if [[ "$PROFILE" == "debug" ]]; then
  profile_dir="debug"
fi

abi_to_target_triple() {
  case "$1" in
    arm64-v8a) echo "aarch64-linux-android" ;;
    x86_64) echo "x86_64-linux-android" ;;
    *)
      echo "Unsupported ABI: $1" >&2
      return 1
      ;;
  esac
}

echo "Building wavry-ffi for Android ABIs: ${ABIS[*]} (${PROFILE})"
echo "Using ANDROID_NDK_HOME=$ANDROID_NDK_HOME"

for abi in "${ABIS[@]}"; do
  target_triple="$(abi_to_target_triple "$abi")"
  echo "-> Building ABI ${abi} (${target_triple})"

  CARGO_ARGS=(ndk -t "$abi" build -p wavry-ffi --no-default-features)
  if [[ "$PROFILE" == "release" ]]; then
    CARGO_ARGS+=(--release)
  fi

  (
    cd "$REPO_ROOT"
    cargo "${CARGO_ARGS[@]}"
  )

  source_lib="$REPO_ROOT/target/$target_triple/$profile_dir/libwavry_ffi.a"
  dest_dir="$OUT_DIR/$abi"
  dest_lib="$dest_dir/libwavry_ffi.a"
  mkdir -p "$dest_dir"

  if [[ ! -f "$source_lib" ]]; then
    echo "Expected output missing: $source_lib" >&2
    exit 1
  fi

  cp "$source_lib" "$dest_lib"
  echo "Built: $dest_lib"
done

echo "Android FFI build complete."
