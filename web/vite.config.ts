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
  },
})
