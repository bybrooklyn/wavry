#!/usr/bin/env bash
set -euo pipefail

# Works when invoked via bash or zsh.
if [[ -n "${BASH_SOURCE-}" ]]; then
  SCRIPT_PATH="${BASH_SOURCE[0]}"
else
  SCRIPT_PATH="$0"
fi

SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
ANDROID_APP_DIR="$REPO_ROOT/apps/android"

GRADLE_VERSION="8.7"
GRADLE_TASKS=()
CUSTOM_GRADLE_TASKS=0
VARIANT="mobile"
BUILD_TYPE="debug"
FFI_ONLY=0

usage() {
  cat <<USAGE
Build Wavry Android in one command.

Usage:
  ./scripts/dev-android.sh [options]

Options:
  --variant V        Build target: mobile, quest, or both (default: mobile).
  --mobile           Shortcut for --variant mobile.
  --quest            Shortcut for --variant quest.
  --both             Shortcut for --variant both.
  --ffi-only         Only build Rust Android FFI libs.
  --debug            Build debug APKs/libs (default).
  --release          Build release APKs/libs.
  --gradle-task T    Gradle task to run (repeatable). Overrides --variant defaults.
  --gradle-version V Gradle version for auto-downloaded local Gradle (default: 8.7).
  -h, --help         Show this help.

Any other flags are passed through to build-android-ffi.sh.
Examples:
  ./scripts/dev-android.sh
  ./scripts/dev-android.sh --quest
  ./scripts/dev-android.sh --both
  ./scripts/dev-android.sh --ffi-only --variant quest --debug
USAGE
}

FFI_ARGS=()
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
          echo "Invalid --variant value: $1 (expected mobile|quest|both)" >&2
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
    --ffi-only)
      FFI_ONLY=1
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
    --gradle-task)
      shift
      if [[ $# -eq 0 ]]; then
        echo "Missing value for --gradle-task" >&2
        exit 2
      fi
      GRADLE_TASKS+=("$1")
      CUSTOM_GRADLE_TASKS=1
      shift
      ;;
    --gradle-version)
      shift
      if [[ $# -eq 0 ]]; then
        echo "Missing value for --gradle-version" >&2
        exit 2
      fi
      GRADLE_VERSION="$1"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      FFI_ARGS+=("$1")
      shift
      ;;
  esac
done

if [[ "$CUSTOM_GRADLE_TASKS" -eq 0 ]]; then
  case "$VARIANT" in
    mobile)
      if [[ "$BUILD_TYPE" == "release" ]]; then
        GRADLE_TASKS=(":app:assembleMobileRelease")
      else
        GRADLE_TASKS=(":app:assembleMobileDebug")
      fi
      ;;
    quest)
      if [[ "$BUILD_TYPE" == "release" ]]; then
        GRADLE_TASKS=(":app:assembleQuestRelease")
      else
        GRADLE_TASKS=(":app:assembleQuestDebug")
      fi
      ;;
    both)
      if [[ "$BUILD_TYPE" == "release" ]]; then
        GRADLE_TASKS=(":app:assembleMobileRelease" ":app:assembleQuestRelease")
      else
        GRADLE_TASKS=(":app:assembleMobileDebug" ":app:assembleQuestDebug")
      fi
      ;;
  esac
fi

ffi_has_abi=0
ffi_has_debug_flag=0
for arg in "${FFI_ARGS[@]-}"; do
  if [[ "$arg" == "--abi" ]]; then
    ffi_has_abi=1
  fi
  if [[ "$arg" == "--debug" ]]; then
    ffi_has_debug_flag=1
  fi
done

if [[ "$ffi_has_abi" -eq 0 ]]; then
  case "$VARIANT" in
    mobile|both)
      FFI_ARGS+=(--abi arm64-v8a --abi x86_64)
      ;;
    quest)
      FFI_ARGS+=(--abi arm64-v8a)
      ;;
  esac
fi

if [[ "$BUILD_TYPE" == "debug" && "$ffi_has_debug_flag" -eq 0 ]]; then
  FFI_ARGS+=(--debug)
fi

choose_java_home() {
  if [[ -n "${JAVA_HOME-}" && -x "${JAVA_HOME}/bin/java" ]]; then
    return 0
  fi

  local candidates=(
    "/Applications/Android Studio.app/Contents/jbr/Contents/Home"
    "/Applications/Android Studio Preview.app/Contents/jbr/Contents/Home"
    "/Applications/Android Studio.app/Contents/jre/Contents/Home"
  )

  local c
  for c in "${candidates[@]}"; do
    if [[ -x "$c/bin/java" ]]; then
      export JAVA_HOME="$c"
      return 0
    fi
  done

  return 1
}

detect_android_sdk_root() {
  if [[ -n "${ANDROID_SDK_ROOT-}" && -d "${ANDROID_SDK_ROOT}" ]]; then
    return 0
  fi

  if [[ -n "${ANDROID_HOME-}" && -d "${ANDROID_HOME}" ]]; then
    export ANDROID_SDK_ROOT="${ANDROID_HOME}"
    return 0
  fi

  local candidates=(
    "$HOME/Library/Android/sdk"
    "$HOME/Android/Sdk"
    "/opt/android-sdk"
    "/usr/local/share/android-sdk"
  )

  local c
  for c in "${candidates[@]}"; do
    if [[ -d "$c" ]]; then
      export ANDROID_SDK_ROOT="$c"
      return 0
    fi
  done

  return 1
}

ensure_android_local_properties() {
  local sdk_path="$1"
  local local_properties_file="$ANDROID_APP_DIR/local.properties"
  local escaped_sdk_path="${sdk_path//\\/\\\\}"
  escaped_sdk_path="${escaped_sdk_path//:/\\:}"

  if [[ -f "$local_properties_file" ]] && grep -q '^sdk.dir=' "$local_properties_file"; then
    return 0
  fi

  if [[ -f "$local_properties_file" ]]; then
    printf "\nsdk.dir=%s\n" "$escaped_sdk_path" >> "$local_properties_file"
  else
    printf "sdk.dir=%s\n" "$escaped_sdk_path" > "$local_properties_file"
  fi
}

resolve_gradle_cmd() {
  if [[ -x "$ANDROID_APP_DIR/gradlew" ]]; then
    echo "$ANDROID_APP_DIR/gradlew"
    return 0
  fi

  if command -v gradle >/dev/null 2>&1; then
    command -v gradle
    return 0
  fi

  local cache_dir="$HOME/.cache/wavry/gradle"
  local zip_path="$cache_dir/gradle-${GRADLE_VERSION}-bin.zip"
  local dist_dir="$cache_dir/gradle-${GRADLE_VERSION}"
  local gradle_bin="$dist_dir/bin/gradle"

  mkdir -p "$cache_dir"

  if [[ ! -x "$gradle_bin" ]]; then
    local url="https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip"
    echo "gradle not found. Downloading local Gradle ${GRADLE_VERSION}..." >&2
    if [[ ! -f "$zip_path" ]]; then
      curl -fL "$url" -o "$zip_path"
    fi
    unzip -q -o "$zip_path" -d "$cache_dir"
  fi

  if [[ ! -x "$gradle_bin" ]]; then
    echo "Failed to prepare local Gradle at $gradle_bin" >&2
    exit 1
  fi

  echo "$gradle_bin"
}

echo "[1/2] Building Rust Android FFI (variant: $VARIANT)..."
"$SCRIPT_DIR/build-android-ffi.sh" "${FFI_ARGS[@]}"

if [[ "$FFI_ONLY" -eq 1 ]]; then
  echo "FFI build complete (ffi-only mode)."
  exit 0
fi

if ! choose_java_home && ! command -v java >/dev/null 2>&1; then
  cat >&2 <<ERR
No Java runtime found.
Install Android Studio (includes JBR) or set JAVA_HOME to a Java 17+ runtime.
ERR
  exit 1
fi

if ! detect_android_sdk_root; then
  cat >&2 <<ERR
No Android SDK found.
Install Android Studio SDK components, or set ANDROID_SDK_ROOT / ANDROID_HOME.
ERR
  exit 1
fi

export ANDROID_HOME="$ANDROID_SDK_ROOT"
ensure_android_local_properties "$ANDROID_SDK_ROOT"

GRADLE_CMD="$(resolve_gradle_cmd)"

if [[ -n "${JAVA_HOME-}" ]]; then
  echo "Using JAVA_HOME=$JAVA_HOME"
fi
echo "Using ANDROID_SDK_ROOT=$ANDROID_SDK_ROOT"

echo "[2/2] Building Android app (${GRADLE_TASKS[*]})..."
(
  cd "$ANDROID_APP_DIR"
  "$GRADLE_CMD" --no-daemon "${GRADLE_TASKS[@]}"
)

echo "Android build complete."
