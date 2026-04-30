import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [svelte()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },

  build: {
    // Mermaid is hefty (lots of diagram types). Splitting it into its own
    // chunk + lazy-importing DiagramView lets the welcome screen and the
    // Files tab load without paying the mermaid tax.
    rollupOptions: {
      output: {
        manualChunks: {
          mermaid: ['mermaid'],
          marked: ['marked'],
        },
      },
    },
    // Tauri targets a known modern engine (system webview); we don't need
    // ES2015 transpilation.
    target: 'es2022',
    // Squelch the 500 KB warning — we already split the heavyweights.
    chunkSizeWarningLimit: 1000,
  },
}));
