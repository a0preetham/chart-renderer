import './style.css'

import embed, { type Result as EmbedResult, type VisualizationSpec } from 'vega-embed'

import init, { Chart, version } from '../vendor/chart-renderer/chart_renderer_wasm.js'
import wasmUrl from '../vendor/chart-renderer/chart_renderer_wasm_bg.wasm?url'
import { compare, vegaPlotRect, type CompareMode, type Layer } from './compare'
import { SAMPLES } from './samples'

/**
 * The font the library embeds. Vega is pointed at the same face so the two panes
 * differ by layout, not by typeface — otherwise every comparison is dominated by
 * the fact that one side is Liberation Sans and the other is whatever the browser
 * resolves `sans-serif` to.
 */
const FONT = 'ChartRenderer Sans'

/** `Compression::Fast`. See the crate README for why it is the default. */
const FAST = 0

/**
 * Device pixels per logical pixel.
 *
 * Vega's canvas renderer draws at this ratio, so our PNG has to as well. At 1:1
 * on a HiDPI screen the browser upscales our bitmap and 10px axis labels turn to
 * mush — which reads as a glyph-rendering bug but is really a sampling one.
 */
const dpr = () => Math.min(window.devicePixelRatio || 1, 4)

type Timings = { scene: number; raster: number; encode: number; total: number; bytes: number }
type ViewMode = 'split' | 'overlay' | 'difference'

const app = document.querySelector<HTMLDivElement>('#app')!

app.innerHTML = `
  <header>
    <h1>chart-renderer</h1>
    <p>
      A Vega-Lite-style chart renderer written in Rust and compiled to WebAssembly —
      no Vega, no canvas, no reactive dataflow. Just
      <code>spec → scales → marks → pixels</code>. The right pane is a PNG produced by
      that renderer; the left is the real Vega-Lite rendering the same spec. Edit the
      spec and both update.
    </p>
  </header>

  <div class="samples" id="samples"></div>

  <div class="layout">
    <section class="panel">
      <h2>Spec</h2>
      <textarea id="spec" spellcheck="false"></textarea>
      <p class="error" id="error" hidden></p>
    </section>

    <div>
      <div class="modes" role="group" aria-label="View mode">
        <button data-mode="split" aria-pressed="true">Side by side</button>
        <button data-mode="overlay" aria-pressed="false">Overlay</button>
        <button data-mode="difference" aria-pressed="false">Difference</button>
      </div>

      <div class="panes" id="panes">
        <section class="panel">
          <h2>Vega-Lite (reference)</h2>
          <div class="render" id="vega"><span class="loading">rendering…</span></div>
          <div class="stats" id="vega-stats"></div>
        </section>

        <section class="panel">
          <h2>chart-renderer (Rust → WASM)</h2>
          <div class="render" id="ours"><span class="loading">loading wasm…</span></div>
          <div class="stats" id="ours-stats"></div>
        </section>
      </div>

      <section class="panel" id="compare-panel" hidden>
        <h2 id="compare-title">Comparison</h2>
        <div class="render"><canvas id="compare-canvas"></canvas></div>
        <div class="stats" id="compare-stats"></div>
      </section>
    </div>
  </div>

  <p class="footnote" id="footnote"></p>
`

const specInput = app.querySelector<HTMLTextAreaElement>('#spec')!
const errorBox = app.querySelector<HTMLParagraphElement>('#error')!
const vegaPane = app.querySelector<HTMLDivElement>('#vega')!
const oursPane = app.querySelector<HTMLDivElement>('#ours')!
const vegaStats = app.querySelector<HTMLDivElement>('#vega-stats')!
const oursStats = app.querySelector<HTMLDivElement>('#ours-stats')!
const sampleBar = app.querySelector<HTMLDivElement>('#samples')!
const footnote = app.querySelector<HTMLParagraphElement>('#footnote')!
const modeBar = app.querySelector<HTMLDivElement>('.modes')!
const panes = app.querySelector<HTMLDivElement>('#panes')!
const comparePanel = app.querySelector<HTMLElement>('#compare-panel')!
const compareTitle = app.querySelector<HTMLHeadingElement>('#compare-title')!
const compareCanvas = app.querySelector<HTMLCanvasElement>('#compare-canvas')!
const compareStats = app.querySelector<HTMLDivElement>('#compare-stats')!

let mode: ViewMode = 'split'
let objectUrl: string | null = null
let ourLayer: Layer | null = null
let referenceLayer: Layer | null = null

for (const sample of SAMPLES) {
  const button = document.createElement('button')
  button.textContent = sample.name.replace(/_/g, ' ')
  button.title = sample.description
  button.setAttribute('aria-pressed', String(sample === SAMPLES[0]))
  button.addEventListener('click', () => {
    for (const other of sampleBar.querySelectorAll('button')) {
      other.setAttribute('aria-pressed', String(other === button))
    }
    specInput.value = JSON.stringify(sample.spec, null, 2)
    render()
  })
  sampleBar.append(button)
}

for (const button of modeBar.querySelectorAll('button')) {
  button.addEventListener('click', () => {
    mode = button.dataset.mode as ViewMode
    for (const other of modeBar.querySelectorAll('button')) {
      other.setAttribute('aria-pressed', String(other === button))
    }
    applyMode()
  })
}

function showError(message: string | null) {
  errorBox.hidden = message === null
  errorBox.textContent = message ?? ''
}

/** Renders with our wasm build, timing each pipeline stage separately. */
function renderOurs(spec: string, scale: number): Timings {
  const t0 = performance.now()
  const chart = new Chart(spec)
  const t1 = performance.now()
  chart.rasterize(scale)
  const t2 = performance.now()
  const png = chart.encode(FAST)
  const t3 = performance.now()

  const logicalWidth = chart.width
  const logicalHeight = chart.height
  const plot = {
    x: chart.plotX * scale,
    y: chart.plotY * scale,
    w: chart.plotWidth * scale,
    h: chart.plotHeight * scale,
  }
  chart.free()

  // Revoking the previous URL keeps a long editing session from leaking blobs.
  if (objectUrl) URL.revokeObjectURL(objectUrl)
  // wasm-bindgen types the return as a possibly-shared buffer; copy into a plain
  // one so it is a valid BlobPart.
  objectUrl = URL.createObjectURL(
    new Blob([new Uint8Array(png).slice().buffer], { type: 'image/png' })
  )

  const img = new Image()
  img.src = objectUrl
  img.alt = 'Chart rendered by chart-renderer'
  // The bitmap is `scale` times larger than the layout; display it at the logical
  // size so the extra pixels become sharpness rather than a bigger picture.
  img.style.width = `${logicalWidth}px`
  img.style.height = `${logicalHeight}px`
  oursPane.replaceChildren(img)

  ourLayer = {
    image: img,
    width: logicalWidth * scale,
    height: logicalHeight * scale,
    plot,
  }

  return { scene: t1 - t0, raster: t2 - t1, encode: t3 - t2, total: t3 - t0, bytes: png.length }
}

async function renderVega(spec: unknown, scale: number): Promise<number> {
  const start = performance.now()
  const result: EmbedResult = await embed(vegaPane, spec as VisualizationSpec, {
    // Canvas, not SVG, so it is compared like-for-like against a raster image.
    renderer: 'canvas',
    actions: false,
    config: { font: FONT },
  })
  const elapsed = performance.now() - start

  const canvas = vegaPane.querySelector('canvas')
  const rect = vegaPlotRect(result.view as never)
  referenceLayer =
    canvas && rect
      ? {
          image: canvas,
          width: canvas.width,
          height: canvas.height,
          // The scenegraph reports logical units; the canvas is in device pixels.
          plot: { x: rect.x * scale, y: rect.y * scale, w: rect.w * scale, h: rect.h * scale },
        }
      : null

  return elapsed
}

const ms = (v: number) => `${v.toFixed(2)} ms`
const kb = (v: number) => `${(v / 1024).toFixed(1)} KB`
const pct = (v: number) => `${(v * 100).toFixed(2)}%`

const stat = (label: string, value: string) => `<span>${label} <b>${value}</b></span>`

function applyMode() {
  const split = mode === 'split'
  panes.hidden = !split
  comparePanel.hidden = split
  if (split) return

  compareTitle.textContent = mode === 'overlay' ? 'Overlay' : 'Difference'

  if (!referenceLayer || !ourLayer) {
    compareStats.innerHTML = stat('status', 'both renders must succeed first')
    return
  }

  const result = compare(compareCanvas, referenceLayer, ourLayer, mode as CompareMode)
  // Show the canvas at logical size, since it holds device pixels.
  compareCanvas.style.width = `${compareCanvas.width / dpr()}px`
  compareCanvas.style.height = `${compareCanvas.height / dpr()}px`

  const rect = (l: Layer) =>
    `${l.plot.x.toFixed(1)},${l.plot.y.toFixed(1)} ${l.plot.w.toFixed(1)}x${l.plot.h.toFixed(1)}`

  compareStats.innerHTML =
    stat('plot-area mismatch', pct(result.mismatch)) +
    // The two plot rectangles are what the comparison aligns on; showing them
    // makes an alignment error distinguishable from a rendering one.
    stat('vega plot', rect(referenceLayer)) +
    stat('our plot', rect(ourLayer)) +
    (result.hint ? stat('note', result.hint) : '') +
    stat(
      'legend',
      mode === 'overlay' ? 'cyan = Vega only, red = ours only' : 'darker = larger difference'
    )
}

async function render() {
  const source = specInput.value
  const scale = dpr()
  let parsed: unknown
  try {
    parsed = JSON.parse(source)
  } catch (e) {
    showError(`Invalid JSON: ${(e as Error).message}`)
    return
  }

  let ourError: string | null = null
  try {
    const t = renderOurs(source, scale)
    oursStats.innerHTML = [
      stat('scene', ms(t.scene)),
      stat('raster', ms(t.raster)),
      stat('encode', ms(t.encode)),
      stat('total', ms(t.total)),
      stat('png', kb(t.bytes)),
      stat('scale', `${scale}x`),
    ].join('')
  } catch (e) {
    // A spec our renderer rejects is still worth showing in the reference pane,
    // so report and carry on rather than bailing out of the whole render.
    ourError = String((e as Error)?.message ?? e)
    ourLayer = null
    oursPane.replaceChildren(
      Object.assign(document.createElement('span'), {
        className: 'loading',
        textContent: 'not rendered',
      })
    )
    oursStats.textContent = ''
  }

  try {
    const elapsed = await renderVega(parsed, scale)
    vegaStats.innerHTML = stat('render', ms(elapsed))
  } catch (e) {
    referenceLayer = null
    vegaStats.textContent = ''
    vegaPane.replaceChildren(
      Object.assign(document.createElement('span'), {
        className: 'loading',
        textContent: `Vega-Lite error: ${(e as Error).message}`,
      })
    )
  }

  showError(ourError && `chart-renderer: ${ourError}`)

  // Our <img> has to have decoded before it can be drawn into the comparison.
  if (ourLayer) {
    try {
      await (ourLayer.image as HTMLImageElement).decode()
    } catch {
      // A decode failure only affects the comparison view; the panes are fine.
    }
  }
  applyMode()
}

let debounce: ReturnType<typeof setTimeout>
specInput.addEventListener('input', () => {
  clearTimeout(debounce)
  debounce = setTimeout(render, 250)
})

async function main() {
  await init({ module_or_path: wasmUrl })

  // Vega sizes and *rotates* axis labels based on canvas `measureText`. If the
  // embedded face has not loaded by the time it measures, it measures a fallback,
  // decides the labels will not fit, and turns them 90 degrees — which looks like
  // a layout bug in our renderer when it is really a font-loading race. Force the
  // face to load before the first render.
  try {
    await document.fonts.load(`10px "${FONT}"`)
    await document.fonts.load(`11px "${FONT}"`)
    await document.fonts.ready
  } catch {
    // Font loading is best-effort; a failure here degrades the comparison but
    // must not stop the demo from rendering.
  }

  const response = await fetch(wasmUrl)
  const wasmBytes = (await response.arrayBuffer()).byteLength

  footnote.innerHTML = `
    <strong>Why bother?</strong> Not bundle size, as it turns out — Vega, Vega-Lite
    and vega-embed come to about 299&nbsp;KB gzipped, and the WASM on the right is
    <b>${kb(wasmBytes)}</b> raw, roughly 272&nbsp;KB gzipped. They are comparable.
    <br /><br />
    The difference is <em>where they can run</em>. Vega renders through a DOM and a
    canvas, and a Cloudflare Worker has neither; the usual workarounds
    (<code>jsdom</code>, <code>node-canvas</code>) are native dependencies that a
    Worker cannot load at all. chart-renderer rasterizes in pure Rust with no DOM,
    no canvas and no native code, so the same spec renders at the edge — inside the
    free tier's 10&nbsp;ms CPU budget, with room to spare.
    <br /><br />
    Both panes render at <code>devicePixelRatio</code> and draw with the same
    embedded font, so the comparison views show real layout differences rather than
    sampling or typeface artefacts. The library's tests additionally cross-check its
    geometry against Vega's own scenegraph on every one of these samples.
    (chart-renderer v${version()})
  `

  specInput.value = JSON.stringify(SAMPLES[0]?.spec ?? {}, null, 2)
  await render()
}

main().catch((e) => {
  oursPane.textContent = `Failed to load: ${e}`
})
