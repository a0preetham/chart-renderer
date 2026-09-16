// Dumps reference scenegraphs from the real Vega-Lite, for the layer-8 cross-check.
//
//   node tests/vega-ref/dump.mjs
//
// Vega builds an internal scenegraph — the same kind of structure as our Scene
// IR — and `renderer: 'none'` gets at it with no canvas and no native
// dependency.
//
// One caveat drives the whole output shape. Without a canvas, Vega cannot call
// `measureText`, so it falls back to a crude width estimate. That makes its
// *absolute* coordinates unusable as a reference, because the plot origin
// depends on how wide it thinks the y tick labels are. Everything below is
// therefore normalised relative to the plot group's own origin, which cancels
// the text-measurement difference out and leaves the thing we actually want to
// check: do our scales agree with Vega's.

import { readFileSync, writeFileSync, readdirSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

import * as vega from 'vega';
import * as vegaLite from 'vega-lite';

const here = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(here, '../../crates/core/tests/fixtures');

/**
 * Depth-first walk yielding every *mark* node.
 *
 * A scenegraph alternates levels: a mark node holds `items` of scene items, and
 * a group item holds `items` of further mark nodes. Recursing at both levels
 * would visit everything twice over, so descend only through group items.
 */
function* walk(node) {
  yield node;
  for (const item of node.items ?? []) {
    for (const child of item.items ?? []) {
      if (child.marktype) yield* walk(child);
    }
  }
}

/** The group holding the plot marks, and its absolute origin. */
function findPlotGroup(root) {
  for (const node of walk(root)) {
    if (node.marktype === 'group') {
      for (const item of node.items ?? []) {
        // The plot group is the one carrying an explicit width and height.
        if (typeof item.width === 'number' && typeof item.height === 'number') {
          return item;
        }
      }
    }
  }
  return null;
}

function collectMarks(root, origin) {
  const marks = {};
  for (const node of walk(root)) {
    if (!node.marktype || node.marktype === 'group') continue;
    const items = (node.items ?? []).map((it) => {
      const out = { role: node.role ?? null };
      if (typeof it.x === 'number') out.x = round(it.x - origin.x);
      if (typeof it.y === 'number') out.y = round(it.y - origin.y);
      if (typeof it.x2 === 'number') out.x2 = round(it.x2 - origin.x);
      if (typeof it.y2 === 'number') out.y2 = round(it.y2 - origin.y);
      if (typeof it.width === 'number') out.width = round(it.width);
      if (typeof it.height === 'number') out.height = round(it.height);
      if (typeof it.size === 'number') out.size = round(it.size);
      if (it.text !== undefined) out.text = String(it.text);
      if (it.fill !== undefined) out.fill = it.fill;
      if (it.stroke !== undefined) out.stroke = it.stroke;
      if (it.shape !== undefined) out.shape = it.shape;
      // Arc slices. Vega stores the angle pair either way round, so normalise to
      // [min, max] — a filled slice covers the same wedge regardless of which end
      // it calls the start.
      if (typeof it.startAngle === 'number' && typeof it.endAngle === 'number') {
        out.angle0 = round(Math.min(it.startAngle, it.endAngle));
        out.angle1 = round(Math.max(it.startAngle, it.endAngle));
      }
      if (typeof it.outerRadius === 'number') out.outerRadius = round(it.outerRadius);
      if (typeof it.innerRadius === 'number') out.innerRadius = round(it.innerRadius);
      // A line item carries its vertices on the item itself, not as children.
      if (node.marktype === 'line' && Array.isArray(node.items)) {
        out.vertices = node.items
          .filter((v) => typeof v.x === 'number' && typeof v.y === 'number')
          .map((v) => [round(v.x - origin.x), round(v.y - origin.y)]);
      }
      return out;
    });
    if (items.length === 0) continue;
    const key = node.role ?? node.marktype;
    (marks[key] ??= []).push(...items);
  }
  return marks;
}

const round = (v) => Math.round(v * 1000) / 1000;

async function dump(name) {
  const spec = JSON.parse(readFileSync(join(fixturesDir, `${name}.json`), 'utf8'));
  const vgSpec = vegaLite.compile(spec).spec;
  const view = new vega.View(vega.parse(vgSpec), { renderer: 'none' }).initialize();
  await view.runAsync();

  const root = view.scenegraph().root;
  const plot = findPlotGroup(root);
  if (!plot) throw new Error(`${name}: could not locate the plot group`);

  const origin = { x: plot.x ?? 0, y: plot.y ?? 0 };
  const out = {
    note: 'Coordinates are relative to the plot group origin; see dump.mjs.',
    vegaLite: JSON.parse(readFileSync(join(here, 'node_modules/vega-lite/package.json'), 'utf8'))
      .version,
    plot: { width: round(plot.width), height: round(plot.height) },
    marks: collectMarks(root, origin),
  };

  writeFileSync(
    join(fixturesDir, `${name}.vega.json`),
    JSON.stringify(out, null, 2) + '\n'
  );
  return out;
}

const names = readdirSync(fixturesDir)
  .filter((f) => f.endsWith('.json') && !f.endsWith('.vega.json'))
  .map((f) => f.replace(/\.json$/, ''))
  .sort();

for (const name of names) {
  try {
    const out = await dump(name);
    const counts = Object.entries(out.marks)
      .map(([k, v]) => `${k}=${v.length}`)
      .join(' ');
    console.log(`${name}: plot ${out.plot.width}x${out.plot.height} ${counts}`);
  } catch (e) {
    console.error(`${name}: ${e.message}`);
  }
}
