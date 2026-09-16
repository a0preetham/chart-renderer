# chart-renderer

A Vega-Lite-style chart renderer written in Rust and compiled to WebAssembly —
no DOM, no canvas, no native dependencies — plus a demo that renders the same
spec through this library and through real Vega-Lite side by side, so you can
see how closely they agree.

Deployed as a single Cloudflare Worker: a small script handling `/api/render`
and `/api/bench` server-side, falling through to the static demo for
everything else.

## Layout

```
lib/       the Rust library (crate: chart-renderer) — see lib/README.md
web/       the Vite demo that consumes it — see web/ for the source
server/    the deployed Worker script: /api/render, /api/bench, else -> assets
wrangler.toml   Worker config: server/worker.js + web/dist as static assets
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

## Server-side rendering

```
GET  /api/render?fixture=simple_bar[&scale=1][&compression=0]
POST /api/render[?scale=1&compression=0]      body: a Vega-Lite-style spec
GET  /api/bench?runs=50[&compression=0]
```

This is the actual serverless path the library targets — the same wasm module
as the browser demo, running in `server/worker.js` on Cloudflare's edge instead
of a browser tab. `scale` defaults to 1: a server has no `devicePixelRatio`.

Test the whole thing locally, worker script and static assets together, with
the real workerd runtime rather than Vite's dev server:

```bash
npm run build && npx wrangler dev
```

**A caveat worth knowing before you trust a number from `/api/render`'s
response:** the `Server-Timing` header it returns is measured with
`performance.now()` *inside the isolate*, and production Cloudflare
deliberately coarsens that clock as a Spectre-style timing-attack mitigation —
more aggressively than local `wrangler dev` does. In production every stage
reads `0.000`, even though the same code reports real sub-10ms numbers
locally. This is not a bug in the endpoint; the isolate genuinely cannot
measure itself precisely on the real edge.

The number that is real: Cloudflare's own per-invocation CPU accounting, via
`wrangler tail`:

```bash
npx wrangler tail chart-renderer --format json
# in another shell:
curl -s -o /dev/null "https://chart-renderer.preetham.workers.dev/api/render?fixture=pie"
```

Measured this way, single real requests on live production came back at
**5–24ms CPU time** across the sample fixtures — higher than the sub-10ms
figures `lib/bench/workerd` measured locally against `wrangler dev`, and above
the commonly-cited Workers Free 10ms CPU budget for several of them. All
still completed (`outcome: "ok"`), so whatever this account's actual limit is,
it isn't a hard kill at 10ms. Local `wrangler dev` is a faithful emulator for
*correctness*; it is evidently not a reliable stand-in for *production CPU
timing* — real edge hardware, isolate scheduling, and (for the very first
request after a deploy) JIT warm-up all differ from a local dev machine.

## License

MIT — see [LICENSE](LICENSE).
