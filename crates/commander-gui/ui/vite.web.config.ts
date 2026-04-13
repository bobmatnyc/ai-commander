import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import path from 'path';

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
  resolve: {
    alias: {
      // Redirect Tauri API imports to our transport shim in web mode.
      // Components that `import { invoke } from '@tauri-apps/api/core'`
      // will get our fetch-based implementation instead.
      '@tauri-apps/api/core': path.resolve(__dirname, 'src/lib/tauri-shim.ts'),
      '@tauri-apps/api/event': path.resolve(__dirname, 'src/lib/tauri-event-shim.ts'),
    },
  },
  root: '.',
});
