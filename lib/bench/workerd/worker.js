// Per-stage CPU benchmark, run under workerd.
//
//   cd bench/workerd && npm install && npx wrangler dev
//   curl 'http://localhost:8787/?fixture=simple_bar&runs=50'
//
// The handoff's CPU estimates were never benchmarked because the environment it
// was written in had no Cloudflare tooling. `wrangler dev` runs workerd — the
// same runtime as production — so these are real numbers, subject to the usual
// caveat that a dev machine is not a Cloudflare edge machine.
//
// Timing uses the `Chart` handle rather than `render_png` so each stage is
// measured separately without re-parsing the spec for every measurement.

import init, { Chart, render_png } from '../../crates/wasm/pkg/chart_renderer_wasm.js';
import wasmModule from '../../crates/wasm/pkg/chart_renderer_wasm_bg.wasm';

import { FIXTURES } from './fixtures.js';

let ready;
function ensureReady() {
  // Instantiating once per isolate matches how a real Worker behaves: the cost
  // is paid on cold start, not per request, so it must stay out of the timings.
  ready ??= init({ module_or_path: wasmModule });
  return ready;
}

/** Median is the honest summary here — a mean is dragged around by GC pauses. */
function quantile(sorted, q) {
  if (sorted.length === 0) return 0;
  const i = (sorted.length - 1) * q;
  const lo = Math.floor(i);
  const hi = Math.ceil(i);
  return lo === hi ? sorted[lo] : sorted[lo] + (sorted[hi] - sorted[lo]) * (i - lo);
}

function summarise(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  return {
    median: +quantile(sorted, 0.5).toFixed(3),
    p95: +quantile(sorted, 0.95).toFixed(3),
    min: +sorted[0].toFixed(3),
    max: +sorted[sorted.length - 1].toFixed(3),
  };
}

/**
 * Mean milliseconds per iteration of `fn`, measured across the whole loop.
 *
 * workerd coarsens `performance.now()` to whole milliseconds as a side-channel
 * mitigation, so timing a single sub-millisecond stage reads as either 0 or 1.
 * Amortising over many iterations recovers the resolution.
 */
function timeLoop(fn, runs) {
  const start = performance.now();
  for (let i = 0; i < runs; i++) fn();
  return (performance.now() - start) / runs;
}

// Server-side renders are 1:1 — `devicePixelRatio` is a display concept and a
// Worker has no display. A caller wanting a 2x image asks for it explicitly.
const SERVER_SCALE = 1;

function benchmark(spec, runs, compression) {
  // Stages are measured cumulatively and differenced, because they are not
  // independent — rasterizing needs a scene, encoding needs a raster.
  const sceneOnly = timeLoop(() => {
    const chart = new Chart(spec);
    chart.free();
  }, runs);

  const throughRaster = timeLoop(() => {
    const chart = new Chart(spec);
    chart.rasterize(SERVER_SCALE);
    chart.free();
  }, runs);

  let bytes = 0;
  let items = 0;
  const throughEncode = timeLoop(() => {
    const chart = new Chart(spec);
    chart.rasterize(SERVER_SCALE);
    const png = chart.encode(compression);
    bytes = png.length;
    items = chart.itemCount;
    chart.free();
  }, runs);

  const round = (v) => +Math.max(v, 0).toFixed(3);

  return {
    runs,
    compression,
    pngBytes: bytes,
    sceneItems: items,
    msPerRender: {
      scene: round(sceneOnly),
      raster: round(throughRaster - sceneOnly),
      encode: round(throughEncode - throughRaster),
      total: round(throughEncode),
    },
  };
}

export default {
  async fetch(request) {
    await ensureReady();
    const url = new URL(request.url);

    // `?png=<fixture>` returns the image itself, for eyeballing what was timed.
    const asPng = url.searchParams.get('png');
    if (asPng) {
      const spec = FIXTURES[asPng];
      if (!spec) return new Response(`unknown fixture ${asPng}`, { status: 404 });
      return new Response(render_png(JSON.stringify(spec), 0, SERVER_SCALE), {
        headers: { 'content-type': 'image/png' },
      });
    }

    const runs = Math.min(Number(url.searchParams.get('runs') ?? 50) || 50, 500);
    const compression = Number(url.searchParams.get('compression') ?? 0) || 0;
    const only = url.searchParams.get('fixture');

    const names = only ? [only] : Object.keys(FIXTURES);
    const results = {};
    for (const name of names) {
      const spec = FIXTURES[name];
      if (!spec) return new Response(`unknown fixture ${name}`, { status: 404 });
      try {
        results[name] = benchmark(JSON.stringify(spec), runs, compression);
      } catch (e) {
        results[name] = { error: String(e) };
      }
    }

    return new Response(
      JSON.stringify(
        {
          note: 'Times are milliseconds of wall clock inside workerd. Workers Free allows 10ms CPU per request.',
          compressionLegend: { 0: 'Fast', 1: 'Balanced', 2: 'Best' },
          results,
        },
        null,
        2
      ),
      { headers: { 'content-type': 'application/json' } }
    );
  },
};
