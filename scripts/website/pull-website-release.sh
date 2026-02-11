#!/usr/bin/env bash
set -euo pipefail

REPO="${WEBSITE_REPO:-bybrooklyn/wavry}"
TAG="${WEBSITE_CHANNEL_TAG:-website-latest}"
ASSET_NAME="${WEBSITE_ASSET_NAME:-website-build.tar.gz}"
CHECKSUM_NAME="${WEBSITE_CHECKSUM_ASSET:-website-build.sha256}"
DEPLOY_PATH="${WEBSITE_DEPLOY_PATH:-/var/www/wavry.dev}"
STATE_PATH="${WEBSITE_STATE_PATH:-/var/lib/wavry-website}"
KEEP_RELEASES="${WEBSITE_KEEP_RELEASES:-5}"

if ! [[ "${KEEP_RELEASES}" =~ ^[0-9]+$ ]]; then
  echo "WEBSITE_KEEP_RELEASES must be an integer" >&2
  exit 1
fi

mkdir -p "${STATE_PATH}"
RELEASES_DIR="${STATE_PATH}/releases"
mkdir -p "${RELEASES_DIR}"

TMP_DIR="$(mktemp -d "${STATE_PATH}/tmp.XXXXXX")"
cleanup() {
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

ARCHIVE_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}"
CHECKSUM_URL="https://github.com/${REPO}/releases/download/${TAG}/${CHECKSUM_NAME}"

CURL_OPTS=(-fsSL --retry 3 --retry-delay 2)
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  CURL_OPTS+=( -H "Authorization: Bearer ${GITHUB_TOKEN}" )
fi

curl "${CURL_OPTS[@]}" "${ARCHIVE_URL}" -o "${TMP_DIR}/${ASSET_NAME}"

if curl "${CURL_OPTS[@]}" "${CHECKSUM_URL}" -o "${TMP_DIR}/${CHECKSUM_NAME}"; then
  (
    cd "${TMP_DIR}"
    sha256sum -c "${CHECKSUM_NAME}"
  )
else
  echo "Checksum file not found, skipping checksum validation" >&2
fi

RELEASE_ID="$(date -u +%Y%m%d%H%M%S)"
NEW_RELEASE_DIR="${RELEASES_DIR}/${RELEASE_ID}"
mkdir -p "${NEW_RELEASE_DIR}"

tar -xzf "${TMP_DIR}/${ASSET_NAME}" -C "${NEW_RELEASE_DIR}"

if [[ ! -f "${NEW_RELEASE_DIR}/index.html" ]]; then
  echo "Invalid website artifact: index.html missing" >&2
  exit 1
fi

mkdir -p "$(dirname "${DEPLOY_PATH}")"
STAGING_PATH="${DEPLOY_PATH}.next"
PREVIOUS_PATH="${DEPLOY_PATH}.prev"

rm -rf "${STAGING_PATH}"
mkdir -p "${STAGING_PATH}"
rsync -a --delete "${NEW_RELEASE_DIR}/" "${STAGING_PATH}/"

rm -rf "${PREVIOUS_PATH}"
if [[ -e "${DEPLOY_PATH}" ]]; then
  mv "${DEPLOY_PATH}" "${PREVIOUS_PATH}"
fi
mv "${STAGING_PATH}" "${DEPLOY_PATH}"
rm -rf "${PREVIOUS_PATH}"

ln -sfn "${NEW_RELEASE_DIR}" "${STATE_PATH}/current"

mapfile -t EXISTING_RELEASES < <(find "${RELEASES_DIR}" -mindepth 1 -maxdepth 1 -type d | sort)
TOTAL_RELEASES="${#EXISTING_RELEASES[@]}"
if (( TOTAL_RELEASES > KEEP_RELEASES )); then
  REMOVE_COUNT=$((TOTAL_RELEASES - KEEP_RELEASES))
  for ((i=0; i<REMOVE_COUNT; i++)); do
    rm -rf "${EXISTING_RELEASES[$i]}"
  done
fi

echo "Website deployed to ${DEPLOY_PATH} from ${ARCHIVE_URL}"
