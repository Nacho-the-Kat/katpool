# KatPool Landing

Marketing landing site (Next.js 15, App Router). Lives in the [katpool](https://github.com/katpool/katpool) monorepo at `katpool-landing/`.

## Local development

```bash
npm ci
npm run dev
```

Open [http://localhost:3000](http://localhost:3000).

## Railway (monorepo)

This directory is a normal git tree (not a submodule). Deploy it as an **isolated monorepo service**:

| Setting | Value |
|--------|--------|
| Root Directory | `katpool-landing` |
| Config file path | `/katpool-landing/railway.toml` |

`railway.toml` sets monorepo watch paths and health checks. Builds use Railpack with `npm ci` + `npm run build` (standalone output). The start command binds on `0.0.0.0` and uses Railway’s `PORT`.

No backend env vars are required for the static marketing pages.

## Scripts

| Script | Description |
|--------|-------------|
| `npm run dev` | Next.js dev server (Turbopack) |
| `npm run build` | Production build + standalone asset copy |
| `npm run start` | Standalone Node server (production) |
| `npm run lint` | ESLint |

Node **20.18+** (see `.node-version`).
