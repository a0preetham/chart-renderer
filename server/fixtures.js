// A handful of the crate's own test fixtures, embedded so the live server-side
// render endpoint (server/worker.js) has real, tested charts to render without
// depending on lib/ at deploy time. Copied verbatim from
// lib/crates/core/tests/fixtures/ — if you touch these, keep them in sync.

export const FIXTURES = {
  simple_bar: {
    $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
    description: 'A basic bar chart with a nominal x axis.',
    width: 300,
    height: 200,
    data: {
      values: [
        { category: 'A', amount: 28 },
        { category: 'B', amount: 55 },
        { category: 'C', amount: 43 },
        { category: 'D', amount: 91 },
        { category: 'E', amount: 81 },
        { category: 'F', amount: 53 },
      ],
    },
    mark: 'bar',
    encoding: {
      x: { field: 'category', type: 'nominal' },
      y: { field: 'amount', type: 'quantitative' },
    },
  },

  multi_series_line: {
    $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
    description: 'A colour encoding splits the data into one line per series, with a legend.',
    width: 300,
    height: 200,
    data: {
      values: [
        { step: 1, value: 10, series: 'north' },
        { step: 2, value: 18, series: 'north' },
        { step: 3, value: 15, series: 'north' },
        { step: 4, value: 27, series: 'north' },
        { step: 1, value: 22, series: 'south' },
        { step: 2, value: 14, series: 'south' },
        { step: 3, value: 31, series: 'south' },
        { step: 4, value: 20, series: 'south' },
      ],
    },
    mark: 'line',
    encoding: {
      x: { field: 'step', type: 'quantitative' },
      y: { field: 'value', type: 'quantitative' },
      color: { field: 'series', type: 'nominal' },
    },
  },

  pie: {
    $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
    description:
      'A pie chart. Categories are unsorted in the data, since Vega stacks slices in colour-domain order rather than row order.',
    data: {
      values: [
        { category: 'Zeta', value: 10 },
        { category: 'Alpha', value: 20 },
        { category: 'Mid', value: 5 },
        { category: 'Beta', value: 12 },
      ],
    },
    mark: 'arc',
    encoding: {
      theta: { field: 'value', type: 'quantitative' },
      color: { field: 'category', type: 'nominal' },
    },
  },

  donut: {
    $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
    description: 'A donut — the same arc mark with an inner radius punched out.',
    width: 260,
    height: 260,
    data: {
      values: [
        { browser: 'Chrome', share: 64 },
        { browser: 'Safari', share: 19 },
        { browser: 'Edge', share: 5 },
        { browser: 'Firefox', share: 3 },
        { browser: 'Other', share: 9 },
      ],
    },
    mark: { type: 'arc', innerRadius: 60 },
    encoding: {
      theta: { field: 'share', type: 'quantitative' },
      color: { field: 'browser', type: 'nominal' },
    },
  },
}
