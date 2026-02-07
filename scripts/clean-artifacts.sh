#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${BASH_SOURCE-}" ]]; then
  SCRIPT_PATH="${BASH_SOURCE[0]}"
else
  SCRIPT_PATH="$0"
fi

SCRIPT_PATH="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)/$(basename "$SCRIPT_PATH")"
SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
SELF_REL_PATH=""
case "$SCRIPT_PATH" in
  "$REPO_ROOT"/*)
    SELF_REL_PATH="${SCRIPT_PATH#$REPO_ROOT/}"
    ;;
esac

DRY_RUN=0
# Default to cleaning gitignored files too; this catches most "unneeded" junk.
CLEAN_IGNORED=1
CLEAN_UNTRACKED=0
ASSUME_YES=0

usage() {
  cat <<USAGE
Clean Wavry build artifacts and unneeded generated files.

Usage:
  ./scripts/clean-artifacts.sh [options]

Options:
  --dry-run        Show what would be deleted without deleting it.
  --git-ignored    Purge gitignored files (default on).
  --no-git-ignored Skip gitignored purge.
  --all-untracked  Also delete ALL untracked files (not just ignored ones).
  --yes            Skip confirmation prompt.
  -h, --help      Show this help.

Examples:
  ./scripts/clean-artifacts.sh
  ./scripts/clean-artifacts.sh --dry-run
  ./scripts/clean-artifacts.sh --yes
  ./scripts/clean-artifacts.sh --all-untracked --yes
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --git-ignored)
      CLEAN_IGNORED=1
      shift
      ;;
    --no-git-ignored)
      CLEAN_IGNORED=0
      shift
      ;;
    --all-untracked)
      CLEAN_UNTRACKED=1
      CLEAN_IGNORED=1
      shift
      ;;
    --yes)
      ASSUME_YES=1
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

if [[ ! -d "$REPO_ROOT/.git" ]]; then
  echo "Expected git repo at: $REPO_ROOT" >&2
  exit 1
fi

if [[ "$ASSUME_YES" -eq 0 && "$DRY_RUN" -eq 0 ]]; then
  echo "This will delete build artifacts under: $REPO_ROOT"
  if [[ "$CLEAN_IGNORED" -eq 1 ]]; then
    echo "It will also remove gitignored files."
  fi
  if [[ "$CLEAN_UNTRACKED" -eq 1 ]]; then
    echo "It will also remove ALL untracked files."
  fi
  read -r -p "Continue? [y/N] " answer
  case "$answer" in
    y|Y|yes|YES) ;;
    *)
      echo "Aborted."
      exit 0
      ;;
  esac
fi

removed_count=0
SEEN_PATHS_FILE="$(mktemp)"
trap 'rm -f "$SEEN_PATHS_FILE"' EXIT

remove_path() {
  local path="$1"
  if grep -Fx -- "$path" "$SEEN_PATHS_FILE" >/dev/null 2>&1; then
    return
  fi
  echo "$path" >> "$SEEN_PATHS_FILE"

  if [[ -e "$path" ]]; then
    removed_count=$((removed_count + 1))
    if [[ "$DRY_RUN" -eq 1 ]]; then
      echo "[dry-run] rm -rf $path"
    else
      rm -rf "$path"
      echo "removed: ${path#$REPO_ROOT/}"
    fi
  fi
}

# Known top-level artifact paths.
COMMON_PATHS=(
  "$REPO_ROOT/target"
  "$REPO_ROOT/debug"
  "$REPO_ROOT/release"
  "$REPO_ROOT/.build"
  "$REPO_ROOT/.swiftpm"
  "$REPO_ROOT/DerivedData"
  "$REPO_ROOT/dist"
  "$REPO_ROOT/.svelte-kit"
  "$REPO_ROOT/coverage"
  "$REPO_ROOT/crates/wavry-desktop/.svelte-kit"
  "$REPO_ROOT/crates/wavry-desktop/dist"
  "$REPO_ROOT/crates/wavry-desktop/coverage"
  "$REPO_ROOT/apps/android/.gradle"
  "$REPO_ROOT/apps/android/app/build"
  "$REPO_ROOT/apps/macos/.build"
)

for path in "${COMMON_PATHS[@]}"; do
  remove_path "$path"
done

# Also clean common nested artifacts in case new modules were added.
find_and_remove_dirs() {
  local name="$1"
  while IFS= read -r dir; do
    [[ -z "$dir" ]] && continue
    remove_path "$dir"
  done < <(
    find "$REPO_ROOT" \
      -path "$REPO_ROOT/.git" -prune -o \
      -type d -name "$name" -print
  )
}

find_and_remove_dirs "node_modules"
find_and_remove_dirs ".cxx"
find_and_remove_dirs ".externalNativeBuild"
find_and_remove_dirs ".kotlin"

# Remove generated build files that may sit outside removed directories.
while IFS= read -r file; do
  [[ -z "$file" ]] && continue
  remove_path "$file"
done < <(
  find "$REPO_ROOT" \
    -path "$REPO_ROOT/.git" -prune -o \
    -type d \( -name target -o -name build -o -name .gradle -o -name .cxx -o -name .externalNativeBuild -o -name .kotlin -o -name node_modules -o -name .build -o -name .svelte-kit \) -prune -o \
    -type f \( -name "*.apk" -o -name "*.aab" -o -name "*.apks" -o -name "*.idsig" -o -name "*.profraw" -o -name "*.profdata" \) -print
)

is_kept_ignored_path() {
  local rel="$1"
  case "$rel" in
    .vscode|.vscode/*|*/.vscode|*/.vscode/*) return 0 ;;
    .idea|.idea/*|*/.idea|*/.idea/*) return 0 ;;
    apps/android/local.properties|apps/android/*/local.properties) return 0 ;;
  esac
  return 1
}

clean_git_list() {
  local mode="$1"
  local command=()
  if [[ "$mode" == "ignored" ]]; then
    command=(git -C "$REPO_ROOT" ls-files --others --ignored --exclude-standard -z)
  fi
  if [[ "$mode" == "untracked" ]]; then
    command=(git -C "$REPO_ROOT" ls-files --others --exclude-standard -z)
  fi

  while IFS= read -r -d '' rel; do
    [[ -z "$rel" ]] && continue
    if [[ "$mode" == "untracked" && -n "$SELF_REL_PATH" && "$rel" == "$SELF_REL_PATH" ]]; then
      continue
    fi
    if [[ "$mode" == "ignored" ]] && is_kept_ignored_path "$rel"; then
      continue
    fi
    remove_path "$REPO_ROOT/$rel"
  done < <("${command[@]}")
}

if [[ "$CLEAN_IGNORED" -eq 1 ]]; then
  clean_git_list "ignored"
fi

if [[ "$CLEAN_UNTRACKED" -eq 1 ]]; then
  clean_git_list "untracked"
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "Dry run complete. Matched $removed_count path(s)."
else
  echo "Cleanup complete. Removed $removed_count path(s)."
fi
