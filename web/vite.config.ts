import { defineConfig } from 'vite'

export default defineConfig({
  // Served at the Worker's own root, not nested under a monorepo path.
  base: '/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    // vega-embed is a multi-megabyte dependency. That is the point of the demo,
    // not an accident, so don't warn about it on every build.
    chunkSizeWarningLimit: 4000,
  },
  server: {
    host: '0.0.0.0',
    proxy: {
      // The API only exists once deployed (it's part of the Worker script, not
      // this Vite dev server). Proxying to the live deployment means the
      // server-rendered pane shows real production numbers even while iterating
      // on the UI locally, rather than a "not available in dev" placeholder.
      '/api': {
        target: 'https://chart-renderer.preetham.workers.dev',
        changeOrigin: true,
      },
    },
  },
})
