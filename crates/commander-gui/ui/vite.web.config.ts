import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: 'dist-web',
    emptyOutDir: true,
    rollupOptions: {
      input: 'web.html',
    },
  },
  define: {
    '__WEB_MODE__': true,
  },
  // Web mode uses a different entry point
  root: '.',
});
