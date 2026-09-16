/**
 * Aligns the two renderers' output and produces overlay / difference views.
 *
 * The two canvases are **not** the same size, and cannot be: each renderer sizes
 * its image around its own axis labels, and they measure text slightly
 * differently. Diffing them from the top-left corner would therefore report a
 * constant offset as if it were error.
 *
 * What *is* directly comparable is the plot rectangle — the data area both
 * renderers were told to make 300x200. So both images are drawn into a common
 * canvas with their plot rectangles aligned, and the comparison happens there.
 */

export interface Layer {
  /** Source pixels, already at device resolution. */
  image: CanvasImageSource
  /** Natural size of `image`, in device pixels. */
  width: number
  height: number
  /** Plot rectangle within `image`, in device pixels. */
  plot: { x: number; y: number; w: number; h: number }
}

export type CompareMode = 'overlay' | 'difference'

export interface CompareResult {
  /** Fraction of plot-area pixels differing beyond the threshold. */
  mismatch: number
  /**
   * Set when the images would agree far better at some offset — i.e. the
   * comparison is misaligned rather than the renderers disagreeing.
   */
  hint?: string
}

/** Canvas sized to hold both layers once their plot rectangles coincide. */
function alignedBounds(a: Layer, b: Layer) {
  // Work in a space where the plot origin is (0, 0); each layer then extends
  // left/up by its own plot offset and right/down by its remainder.
  const left = Math.max(a.plot.x, b.plot.x)
  const top = Math.max(a.plot.y, b.plot.y)
  const right = Math.max(a.width - a.plot.x, b.width - b.plot.x)
  const bottom = Math.max(a.height - a.plot.y, b.height - b.plot.y)
  return {
    width: Math.ceil(left + right),
    height: Math.ceil(top + bottom),
    originX: left,
    originY: top,
  }
}

function drawAligned(
  ctx: CanvasRenderingContext2D,
  layer: Layer,
  originX: number,
  originY: number
) {
  ctx.drawImage(layer.image, originX - layer.plot.x, originY - layer.plot.y)
}

/**
 * Renders a comparison of two layers into `canvas`.
 *
 * `overlay` tints each side and multiplies them, so anything the two agree on
 * stays neutral and anything only one of them drew picks up that side's colour.
 * `difference` is a straight per-channel absolute difference, inverted so the
 * result reads as ink on white: black means identical.
 *
 * Returns the fraction of plot-area pixels that differ beyond `threshold`.
 */
export function compare(
  canvas: HTMLCanvasElement,
  reference: Layer,
  ours: Layer,
  mode: CompareMode,
  threshold = 24
): CompareResult {
  const bounds = alignedBounds(reference, ours)
  canvas.width = bounds.width
  canvas.height = bounds.height

  const ctx = canvas.getContext('2d', { willReadFrequently: true })
  if (!ctx) return { mismatch: 0 }

  // Both layers are rasterized separately first, so their pixels can be compared
  // numerically rather than only blended visually.
  const scratch = (layer: Layer) => {
    const c = document.createElement('canvas')
    c.width = bounds.width
    c.height = bounds.height
    const g = c.getContext('2d', { willReadFrequently: true })!
    // Unpainted area is white, matching both renderers' background, so the
    // padding around a smaller image does not register as a difference.
    g.fillStyle = '#fff'
    g.fillRect(0, 0, bounds.width, bounds.height)
    drawAligned(g, layer, bounds.originX, bounds.originY)
    return g.getImageData(0, 0, bounds.width, bounds.height)
  }

  const left = scratch(reference)
  const right = scratch(ours)

  const out = ctx.createImageData(bounds.width, bounds.height)

  let differing = 0
  let counted = 0

  // Only the plot area counts towards the score: the surrounding chrome legitimately
  // differs (bold titles, legend placement) and would swamp the signal.
  const plotLeft = bounds.originX
  const plotTop = bounds.originY
  const plotRight = plotLeft + Math.min(reference.plot.w, ours.plot.w)
  const plotBottom = plotTop + Math.min(reference.plot.h, ours.plot.h)

  for (let i = 0; i < out.data.length; i += 4) {
    const pixel = i / 4
    const x = pixel % bounds.width
    const y = (pixel / bounds.width) | 0

    const dr = Math.abs(left.data[i] - right.data[i])
    const dg = Math.abs(left.data[i + 1] - right.data[i + 1])
    const db = Math.abs(left.data[i + 2] - right.data[i + 2])
    const delta = Math.max(dr, dg, db)

    const inPlot = x >= plotLeft && x < plotRight && y >= plotTop && y < plotBottom
    if (inPlot) {
      counted++
      if (delta > threshold) differing++
    }

    if (mode === 'difference') {
      // Ink on white: identical reads as blank, so what is left is the error.
      const v = 255 - delta
      out.data[i] = v
      out.data[i + 1] = v
      out.data[i + 2] = v
      out.data[i + 3] = 255
    } else {
      // Where only the reference drew, lean cyan; where only ours did, lean red;
      // agreement stays grey. Both sides draw dark ink on white, so comparing
      // luminance is enough.
      const lumA = (left.data[i] + left.data[i + 1] + left.data[i + 2]) / 3
      const lumB = (right.data[i] + right.data[i + 1] + right.data[i + 2]) / 3
      const inkA = 255 - lumA
      const inkB = 255 - lumB
      out.data[i] = 255 - inkA * 0.25 - inkB
      out.data[i + 1] = 255 - inkA - inkB * 0.25
      out.data[i + 2] = 255 - inkA - inkB * 0.25
      out.data[i + 3] = 255
    }
  }

  ctx.putImageData(out, 0, 0)

  const mismatch = counted === 0 ? 0 : differing / counted

  // A large mismatch is ambiguous: the renderers may genuinely disagree, or the
  // two images may simply be a few pixels apart. Searching a small neighbourhood
  // distinguishes the two, and is the difference between "our bars are wrong"
  // and "the plot rectangles were computed wrong" — which looked identical until
  // it was measured. Only run it when there is something to explain.
  if (mismatch <= 0.02) return { mismatch }

  const plotW = Math.min(reference.plot.w, ours.plot.w)
  const plotH = Math.min(reference.plot.h, ours.plot.h)
  const x0 = Math.round(bounds.originX)
  const y0 = Math.round(bounds.originY)

  const scoreAt = (dx: number, dy: number) => {
    let bad = 0
    let seen = 0
    // Subsampled: an offset is a whole-image property, so every second pixel is
    // plenty and keeps this interactive.
    for (let y = 2; y < plotH - 2; y += 2) {
      for (let x = 2; x < plotW - 2; x += 2) {
        const ai = ((y0 + y + dy) * bounds.width + (x0 + x + dx)) * 4
        const bi = ((y0 + y) * bounds.width + (x0 + x)) * 4
        if (ai < 0 || ai >= left.data.length) continue
        const d = Math.max(
          Math.abs(left.data[ai] - right.data[bi]),
          Math.abs(left.data[ai + 1] - right.data[bi + 1]),
          Math.abs(left.data[ai + 2] - right.data[bi + 2])
        )
        seen++
        if (d > threshold) bad++
      }
    }
    return seen === 0 ? 1 : bad / seen
  }

  let best = { dx: 0, dy: 0, score: scoreAt(0, 0) }
  for (let dy = -8; dy <= 8; dy++) {
    for (let dx = -8; dx <= 8; dx++) {
      const score = scoreAt(dx, dy)
      if (score < best.score) best = { dx, dy, score }
    }
  }

  if (best.dx === 0 && best.dy === 0) return { mismatch }
  return {
    mismatch,
    hint: `misaligned — shifting the reference by ${best.dx},${best.dy} would give ${(
      best.score * 100
    ).toFixed(1)}%`,
  }
}

/**
 * Locates Vega's plot group in a rendered view, **in canvas coordinates**.
 *
 * Vega's scenegraph nests marks under group items, and a group item's `x`/`y` are
 * relative to its parent — not to the canvas. The root group is additionally
 * translated by `view.origin()`, which is where the padding and axis allowance
 * live. Forgetting that term offsets the whole comparison by tens of pixels and
 * turns an alignment error into what looks like a rendering difference.
 */
export function vegaPlotRect(view: {
  scenegraph(): { root: unknown }
  origin(): number[]
  padding(): number | { left?: number; top?: number }
}): { x: number; y: number; w: number; h: number } | null {
  type Node = { marktype?: string; items?: Node[]; x?: number; y?: number; width?: number; height?: number }

  const walk = function* (node: Node): Generator<Node> {
    yield node
    for (const item of node.items ?? []) {
      for (const child of item.items ?? []) {
        if (child.marktype) yield* walk(child)
      }
    }
  }

  const [originX = 0, originY = 0] = view.origin() ?? []

  // `view.origin()` does **not** include the view padding, so the canvas position
  // is origin + padding. Omitting this term shifts the whole reference image by
  // the padding (5px by default) and makes an otherwise pixel-identical render
  // look ~13% wrong.
  const padding = view.padding()
  const padLeft = typeof padding === 'number' ? padding : (padding?.left ?? 0)
  const padTop = typeof padding === 'number' ? padding : (padding?.top ?? 0)

  for (const node of walk(view.scenegraph().root as Node)) {
    if (node.marktype !== 'group') continue
    for (const item of node.items ?? []) {
      if (typeof item.width === 'number' && typeof item.height === 'number') {
        return {
          x: originX + padLeft + (item.x ?? 0),
          y: originY + padTop + (item.y ?? 0),
          w: item.width,
          h: item.height,
        }
      }
    }
  }
  return null
}
