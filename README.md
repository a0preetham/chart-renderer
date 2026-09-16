# chart-renderer

A Vega-Lite-style chart renderer written in Rust and compiled to WebAssembly —
no DOM, no canvas, no native dependencies — plus a demo that renders the same
spec through this library and through real Vega-Lite side by side, so you can
see how closely they agree.

Deployed as a single Cloudflare Worker serving static assets.

## Layout

```
lib/    the Rust library (crate: chart-renderer) — see lib/README.md
web/    the Vite demo that consumes it — see web/ for the source
wrangler.toml   Worker config: serves web/dist as static assets
```

`lib/` is self-contained and has no dependency on `web/`; it could be lifted
back out into its own repo without changes. `web/` consumes a **committed**
build artifact (`web/vendor/chart-renderer/`) rather than building the crate at
deploy time, so the Cloudflare build needs no Rust toolchain.

## Working on the library

```bash
cd lib
cargo test
cargo run --example render -- tests/fixtures/simple_bar.json out.png
```

Full details — measured bundle sizes, per-stage CPU timings, the Vega
cross-check, design notes — are in [`lib/README.md`](lib/README.md).

## Working on the demo

```bash
cd web
npm install
npm run dev            # http://localhost:5173
```

After changing the library:

```bash
cd web
npm run sync-wasm      # rebuilds the wasm, vendors it, regenerates samples
```

## Deploying

```bash
npm install             # installs wrangler at the repo root
npm run build            # builds web/dist
npx wrangler deploy      # or: npm run deploy
```

First time, authenticate with `npx wrangler login`, or set `CLOUDFLARE_API_TOKEN`
for a non-interactive deploy.
