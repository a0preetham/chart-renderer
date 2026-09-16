// Live server-side rendering, actually deployed to Cloudflare — as opposed to
// bench/workerd in the lib/ repo, which only ever ran locally against
// `wrangler dev`. This is what closes that gap: real requests, on Cloudflare's
// actual edge network, with real CPU-time accounting.
//
//   GET  /api/render?fixture=simple_bar[&scale=1][&compression=0]
//   POST /api/render[?scale=1&compression=0]      body: a Vega-Lite-style spec
//   GET  /api/bench?runs=50[&compression=0]
//   anything else                                  -> static assets (the demo)
//
// `scale` is device pixels per scene unit (default 1 — a server has no
// display, so there is no devicePixelRatio to match; see the crate README on
// why cost is quadratic in scale). `compression` is 0=Fast (default), 1=Balanced,
// 2=Best.
//
// Every response carries a standard `Server-Timing` header, visible in curl -D-
// or a browser's Network tab, reporting this specific request's measured
// scene/raster/encode/total milliseconds — not an amortized average, the actual
// single invocation. Compare that against `wrangler tail`'s reported `cpuTime`
// for the same request: our in-isolate `performance.now()` measures wall time
// including I/O waits; Cloudflare's own accounting is what the 10ms Workers
// Free limit actually enforces against.

import init, { Chart, render_png } from '../web/vendor/chart-renderer/chart_renderer_wasm.js'
import wasmModule from '../web/vendor/chart-renderer/chart_renderer_wasm_bg.wasm'

import { FIXTURES } from './fixtures.js'

let ready
function ensureReady() {
  // One instantiation per isolate, matching how a real Worker behaves: paid on
  // cold start, not per request.
  ready ??= init({ module_or_path: wasmModule })
  return ready
}

function serverTiming(stages) {
  return Object.entries(stages)
    .map(([name, ms]) => `${name};dur=${ms.toFixed(3)}`)
    .join(', ')
}

async function handleRender(request, url) {
  const scale = Number(url.searchParams.get('scale') ?? 1) || 1
  const compression = Number(url.searchParams.get('compression') ?? 0) || 0

  let spec
  if (request.method === 'POST') {
    spec = await request.text()
  } else {
    const name = url.searchParams.get('fixture') ?? 'simple_bar'
    if (!(name in FIXTURES)) {
      return new Response(`unknown fixture ${JSON.stringify(name)}. Known: ${Object.keys(FIXTURES).join(', ')}`, {
        status: 404,
      })
    }
    spec = JSON.stringify(FIXTURES[name])
  }

  await ensureReady()

  const t0 = performance.now()
  let chart
  try {
    chart = new Chart(spec)
  } catch (e) {
    return new Response(`invalid spec: ${e}`, { status: 400 })
  }
  const t1 = performance.now()

  try {
    chart.rasterize(scale)
  } catch (e) {
    chart.free()
    return new Response(`rasterize failed: ${e}`, { status: 400 })
  }
  const t2 = performance.now()

  const png = chart.encode(compression)
  const t3 = performance.now()
  chart.free()

  return new Response(png, {
    headers: {
      'content-type': 'image/png',
      'content-length': String(png.length),
      // One real request, not an amortized average. workerd/Cloudflare
      // coarsen `performance.now()` to whole milliseconds as a side-channel
      // mitigation, so a sub-millisecond stage reads as 0 here — that is the
      // runtime's real resolution for a single invocation, not a bug in this
      // code. See /api/bench for the amortized, sub-ms-resolution version.
      'server-timing': serverTiming({
        scene: t1 - t0,
        raster: t2 - t1,
        encode: t3 - t2,
        total: t3 - t0,
      }),
      'x-scale': String(scale),
      'x-compression': String(compression),
      'cache-control': 'no-store',
    },
  })
}

function timeLoop(fn, runs) {
  const start = performance.now()
  for (let i = 0; i < runs; i++) fn()
  return (performance.now() - start) / runs
}

async function handleBench(url) {
  const runs = Math.min(Number(url.searchParams.get('runs') ?? 50) || 50, 500)
  const compression = Number(url.searchParams.get('compression') ?? 0) || 0

  await ensureReady()

  const results = {}
  for (const [name, spec] of Object.entries(FIXTURES)) {
    const specJson = JSON.stringify(spec)

    const sceneOnly = timeLoop(() => {
      const chart = new Chart(specJson)
      chart.free()
    }, runs)

    const throughRaster = timeLoop(() => {
      const chart = new Chart(specJson)
      chart.rasterize(1)
      chart.free()
    }, runs)

    let bytes = 0
    const throughEncode = timeLoop(() => {
      const chart = new Chart(specJson)
      chart.rasterize(1)
      bytes = chart.encode(compression).length
      chart.free()
    }, runs)

    const round = (v) => +Math.max(v, 0).toFixed(3)
    results[name] = {
      pngBytes: bytes,
      msPerRender: {
        scene: round(sceneOnly),
        raster: round(throughRaster - sceneOnly),
        encode: round(throughEncode - throughRaster),
        total: round(throughEncode),
      },
    }
  }

  return new Response(
    JSON.stringify(
      {
        note: [
          'Amortized over `runs` iterations inside one live Cloudflare invocation —',
          'compare against the same fixtures benchmarked locally under `wrangler dev`',
          'in lib/bench/workerd. This is the real edge; that was a local emulation.',
        ].join(' '),
        runs,
        compression,
        results,
      },
      null,
      2
    ),
    { headers: { 'content-type': 'application/json', 'cache-control': 'no-store' } }
  )
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url)

    if (url.pathname === '/api/render') {
      return handleRender(request, url)
    }
    if (url.pathname === '/api/bench') {
      return handleBench(url)
    }

    // Everything else is the static demo.
    return env.ASSETS.fetch(request)
  },
}
