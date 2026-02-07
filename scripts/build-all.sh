#!/usr/bin/env bash
set -euo pipefail

# Unified release build:
# - Builds all backend binaries for native + cross targets.
# - Builds desktop app bundles and Android APKs.
# - Stages final distributables directly under dist/ root.
# - Signs macOS artifacts and verifies Android APK signatures.
# - Appends logo sidecars or embeds logo in .app bundles.

if [[ -n "${BASH_SOURCE-}" ]]; then
  SCRIPT_PATH="${BASH_SOURCE[0]}"
else
  SCRIPT_PATH="$0"
fi

export PATH="$HOME/.bun/bin:$PATH"

SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$REPO_ROOT/dist"
OS_NAME="$(uname -s)"
HOST_TARGET="$(rustc -vV | awk '/^host:/ {print $2}')"

LOGO_SOURCE="$REPO_ROOT/crates/wavry-desktop/src-tauri/icons/icon.png"
LOGO_NAME="WavryLogo.png"
MACOS_SIGN_IDENTITY="${WAVRY_MACOS_SIGN_IDENTITY:--}"

HAS_MINGW_X64=0
if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  HAS_MINGW_X64=1
fi

NATIVE_BINARY_SPECS=(
  "wavry-master:wavry-master"
  "wavry-relay:wavry-relay"
  "wavry-gateway:wavry-gateway"
  "wavry-server:wavry-server"
  "wavry-cli:wavry"
)

CROSS_BINARY_SPECS=(
  "wavry-master:wavry-master"
  "wavry-relay:wavry-relay"
  "wavry-gateway:wavry-gateway"
  "wavry-cli:wavry"
)

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

info() {
  echo -e "${BLUE}[info]${NC} $*"
}

warn() {
  echo -e "${YELLOW}[warn]${NC} $*"
}

success() {
  echo -e "${GREEN}[ok]${NC} $*"
}

fail() {
  echo -e "${RED}[error]${NC} $*" >&2
  exit 1
}

append_logo_to_dir() {
  local dir="$1"
  mkdir -p "$dir"
  cp "$LOGO_SOURCE" "$dir/$LOGO_NAME"
}

append_logo_sidecar() {
  local artifact="$1"
  if [[ -f "$artifact" ]]; then
    cp "$LOGO_SOURCE" "${artifact}.logo.png"
  fi
}

append_logo_to_macos_bundle() {
  local bundle_path="$1"
  local resources_dir="$bundle_path/Contents/Resources"
  mkdir -p "$resources_dir"
  cp "$LOGO_SOURCE" "$resources_dir/$LOGO_NAME"
}

sign_macos_artifact() {
  local artifact="$1"

  if [[ "$OS_NAME" != "Darwin" ]]; then
    return 0
  fi

  if [[ ! -e "$artifact" ]]; then
    return 0
  fi

  if ! command -v codesign >/dev/null 2>&1; then
    fail "codesign is required on macOS to sign $artifact"
  fi

  local sign_args=(--force --sign "$MACOS_SIGN_IDENTITY")
  if [[ "$MACOS_SIGN_IDENTITY" != "-" ]]; then
    sign_args+=(--timestamp)
  fi

  info "Codesigning: $artifact"
  if [[ -d "$artifact" && "$artifact" == *.app ]]; then
    codesign --deep "${sign_args[@]}" "$artifact"
    codesign --verify --deep --strict "$artifact"
  else
    codesign "${sign_args[@]}" "$artifact"
    codesign --verify --strict "$artifact"
  fi
}

target_suffix() {
  local target="$1"
  case "$target" in
    aarch64-apple-darwin) echo "macos-arm64" ;;
    x86_64-apple-darwin) echo "macos-x64" ;;
    x86_64-unknown-linux-musl) echo "linux-x64" ;;
    x86_64-unknown-linux-gnu) echo "linux-x64-gnu" ;;
    i686-unknown-linux-gnu) echo "linux-x86" ;;
    x86_64-pc-windows-gnu) echo "windows-x64" ;;
    i686-pc-windows-gnu) echo "windows-x86" ;;
    *)
      echo "${target//[^A-Za-z0-9._-]/-}"
      ;;
  esac
}

ensure_rust_target() {
  local target="$1"
  if ! rustup target list --installed | grep -q "^${target}$"; then
    info "Installing Rust target: $target"
    rustup target add "$target"
  fi
}

build_backend_target() {
  local target="$1"
  local specs=()
  local non_cli_packages=()
  local spec package_name bin_name
  local contains_cli=0
  local suffix
  local exe_ext="" source_path dest_path

  if [[ "$target" == *"apple-darwin"* ]]; then
    specs=("${NATIVE_BINARY_SPECS[@]}")
  else
    specs=("${CROSS_BINARY_SPECS[@]}")
  fi

  for spec in "${specs[@]}"; do
    package_name="${spec%%:*}"
    if [[ "$package_name" == "wavry-cli" ]]; then
      contains_cli=1
      continue
    fi
    non_cli_packages+=("$package_name")
  done

  ensure_rust_target "$target"
  info "Building backend binaries for $target (cargo build)"
  cd "$REPO_ROOT"

  if [[ "$target" == "x86_64-pc-windows-gnu" ]]; then
    if [[ "$HAS_MINGW_X64" -eq 0 ]]; then
      fail "x86_64-w64-mingw32-gcc is required for Windows GNU cross-builds"
    fi
    if [[ "${#non_cli_packages[@]}" -gt 0 ]]; then
      CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}" \
        cargo build --release --target "$target" $(printf ' -p %q' "${non_cli_packages[@]}")
    fi
    if [[ "$contains_cli" -eq 1 ]]; then
      CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}" \
        cargo build --release --target "$target" -p wavry-cli --bin wavry
    fi
  elif [[ "$target" == "x86_64-unknown-linux-musl" ]]; then
    if ! command -v x86_64-linux-musl-gcc >/dev/null 2>&1; then
      fail "x86_64-linux-musl-gcc is required for Linux musl cross-builds"
    fi
    if [[ "${#non_cli_packages[@]}" -gt 0 ]]; then
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER:-x86_64-linux-musl-gcc}" \
        cargo build --release --target "$target" $(printf ' -p %q' "${non_cli_packages[@]}")
    fi
    if [[ "$contains_cli" -eq 1 ]]; then
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER:-x86_64-linux-musl-gcc}" \
        cargo build --release --target "$target" -p wavry-cli --bin wavry
    fi
  else
    if [[ "${#non_cli_packages[@]}" -gt 0 ]]; then
      cargo build --release --target "$target" $(printf ' -p %q' "${non_cli_packages[@]}")
    fi
    if [[ "$contains_cli" -eq 1 ]]; then
      cargo build --release --target "$target" -p wavry-cli --bin wavry
    fi
  fi

  suffix="$(target_suffix "$target")"
  if [[ "$target" == *"windows"* ]]; then
    exe_ext=".exe"
  fi

  for spec in "${specs[@]}"; do
    bin_name="${spec##*:}"
    source_path="$REPO_ROOT/target/$target/release/${bin_name}${exe_ext}"
    dest_path="$DIST_DIR/${bin_name}-${suffix}${exe_ext}"

    if [[ ! -f "$source_path" ]]; then
      fail "Expected binary missing for $target: $source_path"
    fi

    cp "$source_path" "$dest_path"
    if [[ "$target" == *"apple-darwin"* && "$OS_NAME" == "Darwin" ]]; then
      sign_macos_artifact "$dest_path"
    fi
    append_logo_sidecar "$dest_path"
    success "Staged backend binary: $dest_path"
  done
}

build_backend_binaries() {
  local targets=("$HOST_TARGET")
  local target
  local uniq_targets=()
  local seen

  if [[ "$OS_NAME" == "Darwin" ]]; then
    targets+=("aarch64-apple-darwin" "x86_64-apple-darwin" "x86_64-unknown-linux-musl" "x86_64-pc-windows-gnu")
  elif [[ "$OS_NAME" == "Linux" ]]; then
    targets+=("x86_64-unknown-linux-musl" "x86_64-pc-windows-gnu")
  fi

  for target in "${targets[@]}"; do
    seen=0
    for existing in "${uniq_targets[@]:-}"; do
      if [[ "$existing" == "$target" ]]; then
        seen=1
        break
      fi
    done
    if [[ "$seen" -eq 0 ]]; then
      uniq_targets+=("$target")
    fi
  done

  for target in "${uniq_targets[@]}"; do
    build_backend_target "$target"
  done
}

resolve_tauri_bundle_dir() {
  local candidate
  for candidate in \
    "$REPO_ROOT/target/release/bundle" \
    "$REPO_ROOT/crates/wavry-desktop/src-tauri/target/release/bundle"
  do
    if [[ -d "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

stage_matching_files() {
  local source_dir="$1"
  local pattern="$2"
  local prefix="$3"
  local file dest

  [[ -d "$source_dir" ]] || return 0

  while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    dest="$DIST_DIR/${prefix}$(basename "$file")"
    cp "$file" "$dest"
    append_logo_sidecar "$dest"
    success "Staged desktop artifact: $dest"
  done < <(find "$source_dir" -maxdepth 1 -type f -name "$pattern" 2>/dev/null)
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

verify_signed_apk() {
  local apk_path="$1"

  if [[ ! -f "$apk_path" ]]; then
    fail "APK missing: $apk_path"
  fi

  if [[ "$apk_path" == *"-unsigned.apk" ]]; then
    fail "Unsigned APK is not allowed: $apk_path"
  fi

  local apksigner_bin
  apksigner_bin="$(find_apksigner || true)"
  if [[ -n "${apksigner_bin:-}" && -x "$apksigner_bin" ]]; then
    "$apksigner_bin" verify "$apk_path" >/dev/null
  else
    warn "apksigner not found; skipped cryptographic signature verification for $apk_path"
  fi
}

resolve_release_apk() {
  local variant="$1"
  local variant_dir="$REPO_ROOT/apps/android/app/build/outputs/apk/$variant/release"
  local preferred="$variant_dir/app-${variant}-release.apk"

  if [[ -f "$preferred" ]]; then
    echo "$preferred"
    return 0
  fi

  if [[ -d "$variant_dir" ]]; then
    local fallback
    fallback="$(find "$variant_dir" -maxdepth 1 -type f -name "*.apk" ! -name "*-unsigned.apk" | sort | head -n 1)"
    if [[ -n "$fallback" ]]; then
      echo "$fallback"
      return 0
    fi
  fi

  return 1
}

build_desktop_tauri() {
  info "Building desktop Tauri bundles"

  local desktop_root="$REPO_ROOT/crates/wavry-desktop"
  local bundle_dir
  local app_bundle
  local dmg_file
  local staged_app
  local staged_dmg

  bundle_dir=""

  cd "$desktop_root"

  if [[ ! -d "$desktop_root/node_modules" ]]; then
    info "Installing desktop dependencies with bun"
    bun install
  fi

  bun tauri build

  if ! bundle_dir="$(resolve_tauri_bundle_dir)"; then
    fail "Unable to locate Tauri bundle output directory"
  fi

  if [[ "$OS_NAME" == "Darwin" ]]; then
    app_bundle="$(find "$bundle_dir/macos" -maxdepth 1 -type d -name "*.app" 2>/dev/null | head -n 1 || true)"
    if [[ -n "$app_bundle" ]]; then
      staged_app="$DIST_DIR/Wavry-tauri.app"
      rm -rf "$staged_app"
      cp -R "$app_bundle" "$staged_app"
      append_logo_to_macos_bundle "$staged_app"
      sign_macos_artifact "$staged_app"
      success "Staged desktop app bundle: $staged_app"
    fi

    while IFS= read -r dmg_file; do
      [[ -f "$dmg_file" ]] || continue
      staged_dmg="$DIST_DIR/tauri-$(basename "$dmg_file")"
      cp "$dmg_file" "$staged_dmg"
      append_logo_sidecar "$staged_dmg"
      sign_macos_artifact "$staged_dmg"
      success "Staged desktop DMG: $staged_dmg"
    done < <(find "$bundle_dir/dmg" -maxdepth 1 -type f -name "*.dmg" 2>/dev/null)
  fi

  stage_matching_files "$bundle_dir/deb" "*.deb" "tauri-"
  stage_matching_files "$bundle_dir/appimage" "*.AppImage" "tauri-"
  stage_matching_files "$bundle_dir/appimage" "*.appimage" "tauri-"
  stage_matching_files "$bundle_dir/rpm" "*.rpm" "tauri-"
  stage_matching_files "$bundle_dir/msi" "*.msi" "tauri-"
  stage_matching_files "$bundle_dir/nsis" "*.exe" "tauri-"
}

build_native_macos_app() {
  if [[ "$OS_NAME" != "Darwin" ]]; then
    info "Skipping native macOS app build on non-macOS host"
    return 0
  fi

  info "Building native macOS Swift app"
  "$SCRIPT_DIR/build-macos.sh" release

  local source_bundle="$DIST_DIR/Wavry.app"
  local staged_bundle="$DIST_DIR/Wavry-native.app"

  if [[ ! -d "$source_bundle" ]]; then
    fail "Native macOS bundle missing after build: $source_bundle"
  fi

  rm -rf "$staged_bundle"
  mv "$source_bundle" "$staged_bundle"
  append_logo_to_macos_bundle "$staged_bundle"
  sign_macos_artifact "$staged_bundle"
  success "Native macOS app built and staged -> $staged_bundle"
}

build_android_release_apks() {
  info "Building Android release APKs (mobile + quest)"

  "$SCRIPT_DIR/dev-android.sh" --both --release

  local variant
  local resolved_apk
  local staged_apk
  for variant in mobile quest; do
    if ! resolved_apk="$(resolve_release_apk "$variant")"; then
      fail "Unable to locate signed release APK for variant: $variant"
    fi

    verify_signed_apk "$resolved_apk"
    staged_apk="$DIST_DIR/Wavry-${variant}-release.apk"
    cp "$resolved_apk" "$staged_apk"
    append_logo_sidecar "$staged_apk"
    success "Staged Android ${variant} release APK: $staged_apk"
  done
}

main() {
  echo -e "${BLUE}============================================================${NC}"
  echo -e "${BLUE}Wavry Unified Build Pipeline${NC}"
  echo -e "${BLUE}============================================================${NC}"

  if [[ ! -f "$LOGO_SOURCE" ]]; then
    fail "Logo source not found: $LOGO_SOURCE"
  fi

  rm -rf "$DIST_DIR"
  mkdir -p "$DIST_DIR"
  append_logo_to_dir "$DIST_DIR"

  build_backend_binaries
  build_desktop_tauri
  build_native_macos_app
  build_android_release_apks

  echo -e "\n${GREEN}============================================================${NC}"
  echo -e "${GREEN}All builds completed successfully${NC}"
  echo -e "${GREEN}============================================================${NC}"
  echo -e "Artifacts available in: ${YELLOW}$DIST_DIR${NC}"
  find "$DIST_DIR" -mindepth 1 -maxdepth 1 ! -name '.DS_Store' -print | sed 's|^|  - |'
}

main "$@"
