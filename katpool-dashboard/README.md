# KatPool Dashboard

Next.js pool and miner dashboard (App Router). Lives in the [katpool](https://github.com/katpool/katpool) monorepo at `katpool-dashboard/`.

## Local development

```bash
npm ci
cp .env.example .env   # fill in API/metrics URLs and optional Datadog keys
npm run dev
```

Open [http://localhost:3000](http://localhost:3000).

## Railway (monorepo)

This directory is a normal git tree (not a submodule). Deploy it as an **isolated monorepo service**:

| Setting | Value |
|--------|--------|
| Root Directory | `katpool-dashboard` |
| Config file path | `/katpool-dashboard/railway.toml` |

`railway.toml` sets monorepo watch paths and health checks. Builds use Railpack with `npm ci` + `npm run build` (standalone output). The start command binds on `0.0.0.0` and uses Railway’s `PORT`.

Set environment variables in the Railway service (see `.env.example`). Required for production: `API_BASE_URL`, `METRICS_BASE_URL`, and `NEXT_PUBLIC_APP_URL` (public site URL for links).

## Scripts

| Script | Description |
|--------|-------------|
| `npm run dev` | Next.js dev server |
| `npm run build` | Production build + standalone asset copy |
| `npm run start` | Standalone Node server (production) |
| `npm run lint` | ESLint |

Node **20.18+** (see `.node-version`).
