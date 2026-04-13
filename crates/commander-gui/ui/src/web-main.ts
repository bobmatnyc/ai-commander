import { api } from './lib/transport';
import App from './WebApp.svelte';

// Polyfill @tauri-apps/api/core invoke() for web mode.
// Components that import { invoke } from '@tauri-apps/api/core' will
// get this shim instead (via Vite's resolve.alias in the web config).
(window as any).__TAURI_INVOKE_POLYFILL__ = api;

const app = new App({
  target: document.getElementById('app')!,
});

export default app;
