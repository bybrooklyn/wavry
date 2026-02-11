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
