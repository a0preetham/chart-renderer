// Drives the demo across every sample and reports how closely our renderer
// matches Vega-Lite, plus a contact sheet for review.
//
//   npm run dev &                      # or: npm run build && vite preview
//   node visual-check.mjs
//
// The headline number per sample is the demo's own plot-area mismatch: the two
// images are aligned on their plot rectangles, then compared pixel by pixel.
// Because both sides render at the same scale with the same embedded font, a
// non-trivial mismatch is a real difference rather than a sampling artefact.
//
// Still deliberately **not** a hard pass/fail gate on the pixel number — see the
// crate README. What it does assert is that every sample renders in both panes
// with no error and no console noise.

import { mkdirSync, writeFileSync } from 'fs'
import { dirname, join } from 'path'
import { fileURLToPath } from 'url'

import { chromium } from 'playwright'

const here = dirname(fileURLToPath(import.meta.url))
const BASE = process.env.DEMO_URL ?? 'http://localhost:5173/'
const outDir = join(here, 'visual-out')

const failures = []

async function main() {
  mkdirSync(outDir, { recursive: true })

  const browser = await chromium.launch()
  const page = await browser.newPage({
    viewport: { width: 1400, height: 1000 },
    deviceScaleFactor: 1,
  })

  const consoleErrors = []
  page.on('pageerror', (e) => consoleErrors.push(String(e)))
  page.on('console', (m) => {
    if (m.type() === 'error') consoleErrors.push(m.text())
  })

  await page.goto(BASE, { waitUntil: 'networkidle' })
  await page.waitForSelector('#ours img', { timeout: 30_000 })
  await page.waitForSelector('#vega canvas', { timeout: 30_000 })

  const samples = await page.$$eval('#samples button', (els) => els.map((e) => e.textContent))
  console.log(`Checking ${samples.length} samples at ${BASE}\n`)

  const rows = []

  for (let i = 0; i < samples.length; i++) {
    const name = samples[i].replace(/ /g, '_')
    await page.$$eval('#samples button', (els, idx) => els[idx].click(), i)
    await page.waitForTimeout(700)
    await page.waitForSelector('#ours img', { timeout: 15_000 })

    const error = await page.$eval('#error', (el) => (el.hidden ? null : el.textContent))
    const stats = await page.$eval('#ours-stats', (el) => el.textContent?.trim() ?? '')

    // Switch into difference mode to read the mismatch, then back.
    await page.click('button[data-mode="difference"]')
    await page.waitForTimeout(250)
    const compareText = await page.$eval('#compare-stats', (el) => el.textContent ?? '')
    const mismatch = compareText.match(/mismatch\s+([\d.]+)%/)?.[1] ?? '?'
    const note = compareText.match(/misaligned[^|]*/)?.[0]?.trim()
    const diffShot = await (await page.$('#compare-canvas')).screenshot()
    await page.click('button[data-mode="split"]')
    await page.waitForTimeout(200)

    const reference = await (await page.$('#vega')).screenshot()
    const ours = await (await page.$('#ours')).screenshot()
    writeFileSync(join(outDir, `${name}.reference.png`), reference)
    writeFileSync(join(outDir, `${name}.ours.png`), ours)
    writeFileSync(join(outDir, `${name}.diff.png`), diffShot)

    const ok = !error && !note
    if (!ok) failures.push(`${name}: ${error ?? note}`)
    console.log(
      `${ok ? '  ok  ' : ' FAIL '} ${name.padEnd(20)} mismatch ${String(mismatch).padStart(6)}%` +
        (note ? `  ${note}` : '')
    )

    rows.push(`
      <section>
        <h2>${name} <span>mismatch ${mismatch}%</span>${error ? ` <em>${error}</em>` : ''}</h2>
        <div class="trio">
          <figure><img src="${name}.reference.png" alt=""><figcaption>Vega-Lite</figcaption></figure>
          <figure><img src="${name}.ours.png" alt=""><figcaption>chart-renderer</figcaption></figure>
          <figure><img src="${name}.diff.png" alt=""><figcaption>difference</figcaption></figure>
        </div>
        <p class="stats">${stats}</p>
      </section>`)
  }

  writeFileSync(
    join(outDir, 'index.html'),
    `<!doctype html><meta charset="utf-8"><title>chart-renderer contact sheet</title>
<style>
  body { font: 14px system-ui, sans-serif; margin: 24px; background: #f6f7f9; }
  h1 { font-size: 20px; }
  section { background: #fff; border: 1px solid #dcdfe5; border-radius: 8px; padding: 14px; margin-bottom: 18px; }
  h2 { font-size: 14px; margin: 0 0 10px; font-family: ui-monospace, monospace; }
  h2 span { color: #666; font-weight: normal; }
  h2 em { color: #b00; font-style: normal; font-weight: normal; }
  .trio { display: grid; grid-template-columns: repeat(3, 1fr); gap: 14px; }
  figure { margin: 0; }
  figcaption { font-size: 11px; color: #666; margin-top: 6px; text-align: center; }
  img { width: 100%; border: 1px solid #eceef2; background: #fff; }
  .stats { font-family: ui-monospace, monospace; font-size: 11px; color: #666; margin: 10px 0 0; }
</style>
<h1>chart-renderer vs Vega-Lite</h1>
<p>Aligned on the plot rectangle, same font, same device pixel ratio. Darker means larger difference.</p>
${rows.join('')}
`
  )

  await browser.close()

  if (consoleErrors.length) {
    console.log(`\nConsole errors:\n  ${consoleErrors.join('\n  ')}`)
    failures.push(`${consoleErrors.length} console error(s)`)
  }

  console.log(`\nContact sheet: ${join(outDir, 'index.html')}`)

  if (failures.length) {
    console.error(`\n${failures.length} failure(s):\n  ${failures.join('\n  ')}`)
    process.exit(1)
  }
}

await main()
