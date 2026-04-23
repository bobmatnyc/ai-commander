// Shim for @tauri-apps/api/core in web mode.
// Vite aliases '@tauri-apps/api/core' → this file for the web build.
// Re-export invoke from transport.ts which has the complete, up-to-date
// REST API mapping — avoids duplicating the command table here.
export { invoke } from './transport';
