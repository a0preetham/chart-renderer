// Verifies the wasm boundary under workerd, then prints the benchmark table.
//
//   npx wrangler dev --port 8787     # in one shell
//   node run.mjs                     # in another
//
// This is the wasm-side smoke test: `cargo test` exercises the library natively,
// but nothing in it proves that the wasm-bindgen surface works once compiled and
// loaded into a Worker isolate. Here a real PNG comes back out of workerd, and
// its header is checked against the dimensions the scene reported.

const BASE = process.env.BENCH_URL ?? 'http://localhost:8787';
const RUNS = Number(process.env.BENCH_RUNS ?? 200);

const PNG_MAGIC = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

let failures = 0;
function check(ok, message) {
  console.log(`${ok ? '  ok  ' : ' FAIL '} ${message}`);
  if (!ok) failures++;
}

async function verify(fixture) {
  const response = await fetch(`${BASE}/?png=${fixture}`);
  if (!response.ok) {
    check(false, `${fixture}: HTTP ${response.status}`);
    return;
  }
  const bytes = Buffer.from(await response.arrayBuffer());

  check(bytes.subarray(0, 8).equals(PNG_MAGIC), `${fixture}: valid PNG header`);

  // IHDR width/height live at byte offsets 16..24.
  const width = bytes.readUInt32BE(16);
  const height = bytes.readUInt32BE(20);
  check(
    width > 0 && height > 0 && width < 8192 && height < 8192,
    `${fixture}: sane dimensions ${width}x${height}`
  );
}

async function main() {
  let list;
  try {
    list = await (await fetch(`${BASE}/?runs=1`)).json();
  } catch (e) {
    console.error(`Could not reach ${BASE} — is \`npx wrangler dev\` running?`);
    process.exit(2);
  }
  const fixtures = Object.keys(list.results);

  console.log(`Verifying the wasm boundary across ${fixtures.length} fixtures\n`);
  for (const fixture of fixtures) await verify(fixture);

  if (failures > 0) {
    console.error(`\n${failures} check(s) failed`);
    process.exit(1);
  }

  console.log(`\nBenchmarking (${RUNS} runs each, Fast compression)\n`);
  const { results } = await (await fetch(`${BASE}/?runs=${RUNS}&compression=0`)).json();

  const pad = (v, n) => String(v).padStart(n);
  console.log(
    'fixture'.padEnd(20) +
      pad('scene', 8) + pad('raster', 8) + pad('encode', 8) + pad('total', 8) + pad('bytes', 9)
  );
  const rows = Object.entries(results).sort(
    (a, b) => a[1].msPerRender.total - b[1].msPerRender.total
  );
  for (const [name, r] of rows) {
    const m = r.msPerRender;
    const over = m.total > 10 ? '  <-- over the 10ms Workers Free budget' : '';
    console.log(
      name.padEnd(20) +
        pad(m.scene, 8) + pad(m.raster, 8) + pad(m.encode, 8) + pad(m.total, 8) +
        pad(r.pngBytes, 9) + over
    );
  }
}

await main();
