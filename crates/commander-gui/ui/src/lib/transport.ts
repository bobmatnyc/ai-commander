// Transport abstraction — works in both Tauri and browser contexts

const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

async function tauriInvoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(command, args);
}

// Map Tauri command names to REST API endpoints
const API_MAP: Record<string, { method: string; path: string | ((args: Record<string, unknown>) => string) }> = {
  list_sessions: { method: 'GET', path: '/api/sessions' },
  connect_session: { method: 'POST', path: (args) => `/api/sessions/${args.name}/connect` },
  disconnect_session: { method: 'POST', path: '/api/sessions/disconnect' },
  stop_session: { method: 'DELETE', path: (args) => `/api/sessions/${args.name}` },
  send_message: { method: 'POST', path: '/api/sessions/message' },
  create_session: { method: 'POST', path: '/api/sessions' },
  list_project_directories: { method: 'GET', path: '/api/projects/directories' },
  list_adapters: { method: 'GET', path: '/api/adapters' },
  interpret_session: { method: 'POST', path: (args) => `/api/sessions/${args.name}/interpret` },
  get_session_summary: { method: 'POST', path: (args) => `/api/sessions/${args.name}/summary` },
  capture_session_output: { method: 'POST', path: (args) => `/api/sessions/${args.name}/capture` },
  get_bot_status: { method: 'GET', path: '/api/bot/status' },
  list_processes: { method: 'GET', path: '/api/processes' },
  kill_stale_processes: { method: 'POST', path: '/api/processes/clean' },
  rename_session: { method: 'POST', path: '/api/sessions/rename' },
};

async function fetchApi(command: string, args?: Record<string, unknown>): Promise<unknown> {
  const mapping = API_MAP[command];
  if (!mapping) {
    throw new Error(`Unknown API command: ${command}`);
  }

  const path = typeof mapping.path === 'function' ? mapping.path(args || {}) : mapping.path;
  const url = path; // Relative URL — works when served from same origin

  const options: RequestInit = {
    method: mapping.method,
    headers: { 'Content-Type': 'application/json' },
  };

  if (mapping.method !== 'GET' && args) {
    options.body = JSON.stringify(args);
  }

  // Add auth token if stored
  const token = localStorage.getItem('aic-auth-token');
  if (token) {
    options.headers = { ...options.headers as Record<string, string>, Authorization: `Bearer ${token}` };
  }

  const response = await fetch(url, options);
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `API error: ${response.status}`);
  }

  const contentType = response.headers.get('content-type');
  if (contentType?.includes('application/json')) {
    return response.json();
  }
  return response.text();
}

/** Call a backend command — auto-detects Tauri vs web */
export async function api(command: string, args?: Record<string, unknown>): Promise<unknown> {
  if (isTauri) {
    return tauriInvoke(command, args);
  }
  return fetchApi(command, args);
}

/** Check if running in Tauri desktop app */
export function isDesktop(): boolean {
  return isTauri;
}
