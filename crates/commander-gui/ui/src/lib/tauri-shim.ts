// Shim for @tauri-apps/api/core in web mode
// Redirects invoke() calls through the REST transport layer

import { api } from './transport';

export async function invoke(command: string, args?: Record<string, unknown>): Promise<any> {
  return api(command, args);
}
