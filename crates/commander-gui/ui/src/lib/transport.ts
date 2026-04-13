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
  get_config: { method: 'GET', path: '/api/config' },
  save_config: { method: 'POST', path: '/api/config' },
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

// Response transformers — normalize REST API responses to match Tauri command format
const RESPONSE_TRANSFORMS: Record<string, (data: any) => any> = {
  list_sessions: (data) => {
    // REST returns {sessions: [{name, pane_count, ...}]}
    // Tauri returns [{name, created_at, is_connected}]
    if (data?.sessions) {
      return data.sessions.map((s: any) => ({
        name: s.name,
        created_at: s.created_at || new Date().toISOString(),
        is_connected: false,
      }));
    }
    return data;
  },
  list_project_directories: (data) => {
    // REST may wrap in {directories: [...]}
    if (data?.directories) return data.directories;
    return data;
  },
  list_processes: (data) => {
    // REST returns {processes: [...], total}; frontend expects flat array
    if (data?.processes) return data.processes;
    return data;
  },
  interpret_session: (data: Record<string, unknown>) => {
    // REST returns {session, output}; frontend expects the output string
    if (typeof data === 'object' && data?.output) return data.output;
    return data;
  },
};

/** Call a backend command — auto-detects Tauri vs web */
export async function api(command: string, args?: Record<string, unknown>): Promise<unknown> {
  if (isTauri) {
    return tauriInvoke(command, args);
  }
  let result = await fetchApi(command, args);
  // Apply response transformer if one exists
  const transform = RESPONSE_TRANSFORMS[command];
  if (transform) {
    result = transform(result);
  }
  return result;
}

/** Check if running in Tauri desktop app */
export function isDesktop(): boolean {
  return isTauri;
}

/** Session event received from SSE */
export interface SessionEventData {
  session_name: string;
  event_type: string;
  content: string;
  timestamp: number;
  adapter?: string;
}

/**
 * Subscribe to SSE session events (web mode only).
 * In Tauri mode this is a no-op since Tauri events handle live updates.
 * Returns a cleanup function to close the connection.
 */
export function subscribeSessionEvents(
  sessionName: string,
  onEvent: (event: SessionEventData) => void,
  onError?: (error: globalThis.Event) => void,
): () => void {
  // SSE is not needed in Tauri mode — native events handle updates
  if (isTauri) {
    return () => {};
  }

  const url = `/api/sessions/${encodeURIComponent(sessionName)}/events`;
  const eventSource = new EventSource(url);

  eventSource.onmessage = (e) => {
    try {
      const data: SessionEventData = JSON.parse(e.data);
      onEvent(data);
    } catch {
      // Ignore malformed events
    }
  };

  eventSource.onerror = (e) => {
    if (onError) onError(e);
  };

  return () => eventSource.close();
}

/**
 * Drop-in replacement for Tauri's invoke().
 * Components can import { invoke } from '../transport' instead of '@tauri-apps/api/core'
 * and it will work in both Tauri and web contexts.
 */
export async function invoke(command: string, args?: Record<string, unknown>): Promise<any> {
  return api(command, args);
}
