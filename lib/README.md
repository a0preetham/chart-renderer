# chart-renderer

A lightweight, Vega-Lite-style declarative chart renderer in Rust. Spec in, PNG
out, on a straight pipeline:

```
spec → data columns → scales → marks (Scene IR) → raster → PNG
```

**Not Vega.** No reactive dataflow engine, no view composition. It targets
serverless: AWS Lambda natively, Cloudflare Workers via WASM.

```rust
use chart_renderer::{render_png, RenderOptions};

let png = render_png(r#"{
  "data": {"values": [{"a": "A", "b": 28}, {"a": "B", "b": 55}]},
  "mark": "bar",
  "encoding": {
    "x": {"field": "a", "type": "nominal"},
    "y": {"field": "b", "type": "quantitative"}
  }
}"#, &RenderOptions::default())?;
```

## What it supports

| | |
|---|---|
| Marks | `bar` (vertical and horizontal), `line`, `point`/`circle`, `arc` (pie and donut) |
| Encodings | `x`, `y`, `color`, `theta` |
| Field types | `quantitative`, `nominal`, `ordinal` |
| Scales | linear, band, point, angular |
| Transforms | `stack` (drives arc slices; also what stacked bars will need) |
| Chrome | axes with ticks/labels/titles, gridlines, categorical legend |
| Output | PNG at any device pixel ratio (`RenderOptions::scale`) |
| Data | inline `data.values` only |

Out of scope for now: `temporal` fields, `area`/`tick` marks, aggregation,
binning, faceting, layering, and remote `data.url`.

Unknown *fields* in a spec are ignored, so a real Vega-Lite spec with `$schema`
and `description` parses. Unsupported *values* — an `area` mark, a `temporal`
type — are typed errors rather than a silently blank chart.

## Measured numbers

All of these are measurements, not estimates. Anything still unverified says so.

### Bundle size

`wasm-pack --release --target web`, after `wasm-opt -O3`, gzipped (Cloudflare
applies its script limit to the compressed upload):

| Build | Shipped wasm | Gzipped | vs Workers Free (3MB) |
|---|---|---|---|
| `simple-text` (default) | 638,920 B | **278,693 B (272 KB)** | 9% of the limit |
| `full-text` | — | — | **not yet measurable**, see below |

Reproduce with `./measure-size.sh`.

The `full-text` number is deliberately absent. `CosmicShaper` is still a stub, so
nothing calls into cosmic-text and LTO strips it from the binary entirely — the
figure that falls out measures "no text backend at all", not "full text". It has
to be re-measured once the backend exists.

### CPU per render

Measured under **workerd** via `wrangler dev` — the same runtime Cloudflare runs
in production, though on a dev machine rather than an edge machine. Amortised
over 200 runs, because workerd coarsens `performance.now()` to whole
milliseconds and a single sub-millisecond stage reads as 0 or 1.

Milliseconds per render, default `Fast` compression:

| Fixture | Scene | Raster | Encode | **Total** |
|---|---|---|---|---|
| `empty_data` | 0.02 | 0.59 | 0.69 | **1.29** |
| `single_datum` | 0.04 | 1.08 | 0.78 | **1.88** |
| `negative_bar` | 0.06 | 1.21 | 1.35 | **2.60** |
| `line_discrete_x` | 0.06 | 1.72 | 1.23 | **2.98** |
| `simple_bar` | 0.06 | 1.67 | 1.55 | **3.26** |
| `colored_bar` | 0.15 | 2.24 | 1.72 | **4.09** |
| `horizontal_bar` | 0.06 | 2.87 | 1.64 | **4.54** |
| `line` | 0.07 | 2.90 | 1.90 | **4.84** |
| `scatter` | 0.08 | 3.12 | 1.89 | **5.05** |
| `multi_series_line` | 0.09 | 3.45 | 1.99 | **5.49** |

Reproduce with `cd bench/workerd && npm install && npx wrangler dev`, then
`curl 'http://localhost:8787/?runs=200'`.

**These are local numbers, and local turned out not to predict production.**
`wrangler dev` runs real workerd — faithful for *correctness* — but is not a
stand-in for *production CPU timing*. Deploying the same code as a live Worker
and reading Cloudflare's own per-invocation accounting (via `wrangler tail`,
not `performance.now()` — see the root README) gave single real requests of
**5–24ms CPU time**, several times what this table suggests. See the root
[`README.md`](../README.md#server-side-rendering) for the measurement and the
full explanation, including why the endpoint's own `Server-Timing` header
cannot be trusted on real Cloudflare hardware.

### Two findings that contradict the original estimates

**1. PNG encoding is not the bottleneck; rasterization is.** The handoff called
deflate "the single most expensive step, often exceeding rasterization cost" and
named compression level as the first lever to pull. Measured, rasterization is
consistently the larger of the two — roughly 1.5–2x encode across every fixture,
before *and* after optimisation. Compression tuning is a real lever, but it is
the second one.

**2. `opt-level = "z"` cost 4.8x.** Optimising the wasm for size is the
conventional default and it was badly wrong here: a software rasterizer lives on
inlining and vectorisation.

| Build | Sum of totals, all fixtures | Gzipped |
|---|---|---|
| `opt-level = "z"`, no SIMD | 172.8 ms | 258,834 B |
| `opt-level = 3`, no SIMD | 49.5 ms (3.5x faster) | 320,906 B |
| `opt-level = 3` + `simd128` | **36.0 ms (4.8x faster)** | **278,693 B** |

Note the last row is both the fastest *and* smaller than `opt-level = 3` alone —
vector ops are more compact than the unrolled scalar loops they replace. The net
cost of going from the size-optimised build to the fast one is ~20 KB gzipped for
a 4.8x speedup. Both settings are committed (`Cargo.toml`, `.cargo/config.toml`).

With that, every fixture fits the Workers Free 10 ms CPU budget in this *local*
measurement, with room to spare — under the old settings, most did not. Take
the margin with a grain of salt, though: real production numbers (see above)
came in meaningfully higher than local, to the point that several fixtures
exceed 10ms on the actual edge. The relative win from this change (4.8x) is
still real and still the right call; the absolute "fits comfortably" claim
was a local-only artifact.

### The compression lever

`Fast` is the default. `Best` is ~6x slower to encode for ~5x smaller output, and
pushes most charts back over the 10 ms Free budget:

| | Encode | PNG size |
|---|---|---|
| `Fast` | 0.7–2.0 ms | 6–39 KB |
| `Best` | 3.8–11.1 ms | 1.9–10.3 KB |

Worth taking on Workers Paid, or anywhere bytes on the wire cost more than CPU.

### How close is it?

The demo aligns both renderers' output on the plot rectangle and diffs them pixel
by pixel, at the same scale and with the same embedded font. Across the sample
gallery:

| Fixture | Plot-area mismatch |
|---|---|
| `colored_bar` | **0.00%** |
| `line_discrete_x` | **0.00%** |
| `simple_bar` | **0.00%** |
| `pie` | 0.12% |
| `donut` | 0.28% |
| `multi_series_line` | 0.45% |
| `negative_bar` | 0.50% |
| `horizontal_bar` | 0.52% |
| `empty_data` | 0.67% |
| `single_datum` | 0.67% |
| `scatter` | 0.75% |
| `line` | 0.82% |

Three of the ten are **pixel-identical** to Vega across the plot area, and the
plot rectangles land on exactly the same pixel in all of them — for
`colored_bar`, both renderers put it at `39.0,10.0 300x200`.

What remains is confined to diagonal and curved geometry: line marks, point
symbols and arc rims, where tiny-skia and the browser's Skia antialias slightly
differently. The all-rectilinear charts have nothing left to disagree about, and
a pie's slice *interiors* are pixel-identical — only the curved edges differ. Reproduce with
`node visual-check.mjs` in `web/`.

The comparison carries its own self-check: when the mismatch exceeds 2% it
searches a small neighbourhood of offsets, and reports if some shift would agree
better. That distinguishes "the renderers disagree" from "the two images are a
few pixels apart", which look identical in a difference image and are not.

It earned its place twice. A 13% mismatch that looked like wrong bar geometry was
the reference being offset by Vega's 5px padding — 0.0% once aligned. Then, at
`devicePixelRatio` 3 on a phone, it flagged a 1px shift that turned out to be the
hairline bug above, which had been invisible at 1:1 for the whole project.

## Design notes

**Transforms are a pipeline stage, not a mark's private business.** Everything
except arcs maps one data row straight onto one piece of geometry. A pie slice
cannot: its start angle is the running total of every row before it. That made
`stack` the first real transform, and it lives in its own stage because stacked
*bars* need exactly the same thing.

**Mark generation emits a vector IR, not rasterizer paths.** `marks` produces a
`Scene` of plain data that a backend lowers to tiny-skia. The reason is
testability above all: a test can assert "bar 3 sits at x=142.5 with width 18.3"
against a `Scene`, where a `tiny_skia::Path` would mean decoding path verbs or
diffing pixels. It also keeps an SVG backend cheap to add later.

**`SceneItem::Text` carries the string, not glyph outlines.** Shaping happens in
the render backend. Layout still needs text *measurement*, so it takes a
`TextShaper` and calls only `measure_width`. That split is what lets the IR stay
inspectable, and what would let an SVG backend emit real `<text>`.

**Rendering scale is separate from layout.** `RenderOptions::scale` is device
pixels per scene unit; layout always happens in logical units, so raising it
changes sampling density and nothing else. Pass a display's `devicePixelRatio`
and text comes out as crisp as a canvas renderer's — at 1:1 on a HiDPI screen the
browser upscales the bitmap and 10px labels turn to mush, which reads as a
glyph bug but is a sampling one.

Cost is **quadratic** in scale, and it lands on the two most expensive stages.
Measured on a phone at `scale: 3` (9x the pixels): raster 13ms, encode 19ms, and
a 91KB PNG, against ~4ms and 24KB at 1:1. Scale is a client-side display
concern — a server-side render has no display and should stay at 1, which is what
`bench/workerd` measures and what the CPU table above reflects.

**Text backends are chosen at compile time**, via Cargo feature, so the unused
one never enters the binary:

```toml
[features]
default     = ["simple-text"]   # ab_glyph + one embedded Latin font
full-text   = ["cosmic-text", "fontdb"]   # scaffolded, not yet implemented
```

`simple-text` does cmap + hmtx + outline extraction with no shaping engine: no
ligatures, no bidi, no complex-script reordering. That is correct for the
Latin-family axis labels and titles this library draws.

The embedded font is **Liberation Sans**, subsetted to Latin, Latin-1, Latin
Extended-A and common punctuation (410 KB → 20.7 KB). Liberation Sans is
metrically compatible with Arial, which is what browsers resolve `sans-serif` to
for Vega's default font — so our measured widths track the reference
implementation's closely.

**CJK is out of scope for bundling**, on either plan. Noto Sans CJK alone is
5–15 MB and busts both the 3 MB and 10 MB script limits. If it is ever needed,
fonts have to be lazy-loaded from R2/KV at request time.

## Testing

`cargo test`. The layers, roughly in order of how much they carry:

1. **Scale unit tests** — linear/band/point arithmetic, `nice`, tick generation.
   Ported from d3, which is what Vega uses.
2. **Vega cross-check** (`tests/vega_crosscheck.rs`) — runs every fixture through
   the *real* `vega-lite` in Node, extracts its scenegraph, and compares geometry
   numerically. This is the oracle; see below.
3. **Geometric invariants** (`tests/invariants.rs`) — marks inside the plot, bars
   sharing a baseline, and a rasterized check that no ink escapes the canvas.
4. **Scene snapshots** (`tests/scene_snapshots.rs`) — `insta` over the IR, so a
   regression diff reads `"x": 142.5 → 141.0` instead of "the image changed".
5. **Browser metrics** (`tests/browser_metrics.rs`) — advance widths and baseline
   offsets pinned to Chrome's `measureText` for the same font file. Regenerate
   the expectations with `web/metrics.html`.
6. **Degenerate inputs** — empty data, single datum, all-equal values, NaN, huge
   magnitudes, missing fields, zero-size canvas. None may panic: a panic in wasm
   aborts the whole isolate.

### Conventions worth knowing before you implement them

Arc support went in without a single geometry bug, because the conventions were
measured from Vega first rather than guessed. All four of these are things a
reasonable implementation gets wrong:

- **Slices stack in the colour scale's domain order**, which is sorted — not in
  row order. Data `Zeta, Alpha, Mid` renders as `Alpha, Mid, Zeta`. Exactly the
  same trap as nominal axis domains.
- **Angle 0 points up and increases clockwise**, so the parametric conversion is
  `φ = θ − π/2` and screen-y being flipped already gives the clockwise sweep.
- **Radius is `min(width, height) / 2`**, centred in the plot — a pie in a
  non-square plot fits the smaller dimension.
- **Vega stores the angle pair either way round.** `startAngle` is often the
  larger of the two; a filled wedge covers the same region regardless, but a
  comparison that assumes ordering will report phantom differences.

### The Vega cross-check

Vega builds an internal scenegraph — the same kind of structure as our `Scene` —
reachable in Node with `renderer: 'none'` and therefore no canvas and no native
dependency. `tests/vega-ref/dump.mjs` walks it and commits the result next to
each fixture.

Both sides are normalised to the plot rectangle. Without a canvas Vega cannot
call `measureText` and falls back to a crude width estimate, so its *absolute*
coordinates are not a usable reference — the plot origin moves with how wide it
guesses the y labels are. Normalising cancels that out and leaves the question
worth asking: do our scales agree with Vega's?

Regenerate with `cd tests/vega-ref && npm install && node dump.mjs`. The
vega-lite version is pinned, because upgrading it can shift layout defaults and
light up the whole comparison at once.

This is not ceremony. Together with the demo's pixel comparison it has caught ten
real bugs that unit tests alone would have missed, each one a case where the
plausible implementation is the wrong one:

- **`nice()` iterates.** Widening a domain changes its span, which can change the
  tick increment, which widens it further; d3 repeats until the step stabilises.
  For (0, 31) the sequence is 31 → 32 → 35. Stopping at 32 puts every gridline in
  the wrong place.
- **`nice()` uses a count of 10**, not the axis tick count.
- **Nominal domains sort alphabetically.** Rows `Zeta, Alpha, Mid` render as
  `Alpha, Mid, Zeta` — first-appearance order is wrong.
- **`zero` inclusion is channel-based, and not uniform.** y always includes zero;
  x does for `bar` and `point` but *not* for `line*`. Transposing a line chart
  keeps zero off x and on y, so it is genuinely about the channel.
- **Axis tick positions snap to whole pixels**, while mark geometry does not —
  and labels stay on the true tick value, unsnapped.
- **Negative tick labels use U+2212 MINUS SIGN**, not an ASCII hyphen.
- **Discrete x-axis labels are rotated 270°.** A discrete *y* axis is not. The
  scenegraph check could not catch this one — it compares positions, and a
  rotated label sits at the same anchor — so it took the visual comparison.
- **A font size means pixels per em for advances, but the font's height for
  baselines.** `ab_glyph`'s `PxScale` is height-relative, so passing a size
  straight through made every advance ~12% too narrow. Advances need
  `size * height / em`; baselines do not. Getting either wrong moves every label,
  and label widths set the axis insets, which place the plot. Pinned against
  Chrome's `measureText` in `tests/browser_metrics.rs`.
- **Axis extents are whole pixels.** Vega reserves 34px for an axis whose parts
  sum to 33.12. Leaving the fraction in shifts the plot origin by up to a pixel.
- **Hairlines need a half-pixel nudge.** A 1px stroke centred on an integer
  coordinate straddles two pixel rows at half coverage each, so every gridline and
  axis rule rendered washed out. Snapping the stroke's leading edge to a pixel
  boundary took three fixtures to a 0.00% match. This one only became visible at
  `devicePixelRatio` 3, where half a logical pixel is a pixel and a half — at 1:1
  it hid inside the antialiasing.

### Known remaining difference

Vega draws axis **titles in bold**; ours are regular, because only a regular face
is subsetted into the binary. Adding a bold face would roughly double the font
payload, so this is recorded rather than fixed. Titles sit outside the plot
rectangle, so this does not show up in the mismatch figures below.

## Layout

```
crates/core/     the library
crates/wasm/     wasm-bindgen surface (render_png, plus a staged Chart handle)
bench/workerd/   per-stage CPU benchmark, run under wrangler dev
tests/vega-ref/  generates reference scenegraphs from the real vega-lite
measure-size.sh  bundle size per feature
```

Render a spec from the command line:

```bash
cargo run --example render -- crates/core/tests/fixtures/simple_bar.json out.png
```
