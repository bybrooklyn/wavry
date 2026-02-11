# Wavry Website (`apps/website`)

Public-facing Docusaurus site for `wavry.dev`.

This site covers:

- Product overview
- OSS vs commercial vs hosted usage
- Technical architecture and security docs
- Operations and release guidance

## Local Development (Bun)

```bash
cd apps/website
bun install
bun run start
```

## Build

```bash
cd apps/website
bun run build
bun run serve
```

## Type Check

```bash
cd apps/website
bun run typecheck
```

## Style Regression Check

```bash
cd apps/website
bun run build
./node_modules/.bin/playwright install chromium
bun run check:style
```

## Notes

- Docs are authored locally in `apps/website/docs`.
- Site uses Docusaurus classic preset.
- Do not use npm scripts for this app; use Bun.
- CI publishes `website-build.tar.gz` + checksum to the `website-latest` GitHub Release tag.
- Self-hosted servers should pull this artifact over HTTPS (no inbound SSH from GitHub required).

## Self-Hosted Pull Deploy

Run the pull script from your server:

```bash
WEBSITE_DEPLOY_PATH=/var/www/wavry.dev \
/path/to/wavry/scripts/website/pull-website-release.sh
```

Optional environment variables:

- `WEBSITE_REPO` (default: `bybrooklyn/wavry`)
- `WEBSITE_CHANNEL_TAG` (default: `website-latest`)
- `WEBSITE_ASSET_NAME` (default: `website-build.tar.gz`)
- `WEBSITE_CHECKSUM_ASSET` (default: `website-build.sha256`)
- `WEBSITE_STATE_PATH` (default: `/var/lib/wavry-website`)
- `WEBSITE_KEEP_RELEASES` (default: `5`)
- `GITHUB_TOKEN` (optional; needed only for private repos)
