// Shim for @tauri-apps/api/core in web mode
// Redirects invoke() calls through the REST transport layer

// Response transformers — normalize REST responses to Tauri format
const TRANSFORMS: Record<string, (data: any) => any> = {
  list_sessions: (data: any) => {
    if (data?.sessions) {
      return data.sessions.map((s: any) => ({
        name: s.name,
        created_at: s.created_at || new Date().toISOString(),
        is_connected: false,
      }));
    }
    return data;
  },
  interpret_session: (data: any) => {
    // REST returns {session, output} — Tauri returns just the string
    if (typeof data === 'object' && data?.output) return data.output;
    return data;
  },
  get_session_summary: (data: any) => {
    // Normalize if wrapped
    if (typeof data === 'object' && data?.summary) return data.summary;
    return data;
  },
  capture_session_output: (data: any) => {
    // REST may wrap in {output: "..."}
    if (typeof data === 'object' && data?.output) return data.output;
    return data;
  },
};

const API_MAP: Record<string, { method: string; path: string | ((args: any) => string) }> = {
  list_sessions: { method: 'GET', path: '/api/sessions' },
  connect_session: { method: 'POST', path: (a: any) => `/api/sessions/${a.name}/connect` },
  disconnect_session: { method: 'POST', path: '/api/sessions/disconnect' },
  stop_session: { method: 'DELETE', path: (a: any) => `/api/sessions/${a.name}` },
  send_message: { method: 'POST', path: '/api/sessions/message' },
  create_session: { method: 'POST', path: '/api/sessions' },
  list_project_directories: { method: 'GET', path: '/api/projects/directories' },
  list_adapters: { method: 'GET', path: '/api/adapters' },
  interpret_session: { method: 'POST', path: (a: any) => `/api/sessions/${a.name}/interpret` },
  get_session_summary: { method: 'POST', path: (a: any) => `/api/sessions/${a.name}/summary` },
  capture_session_output: { method: 'POST', path: (a: any) => `/api/sessions/${a.name}/capture` },
  get_bot_status: { method: 'GET', path: '/api/bot/status' },
  list_processes: { method: 'GET', path: '/api/processes' },
  kill_stale_processes: { method: 'POST', path: '/api/processes/clean' },
  rename_session: { method: 'POST', path: '/api/sessions/rename' },
};

export async function invoke(command: string, args?: Record<string, any>): Promise<any> {
  const mapping = API_MAP[command];
  if (!mapping) throw new Error(`Unknown command: ${command}`);

  const path = typeof mapping.path === 'function' ? mapping.path(args || {}) : mapping.path;
  const opts: RequestInit = {
    method: mapping.method,
    headers: { 'Content-Type': 'application/json' },
  };
  if (mapping.method !== 'GET' && args) {
    opts.body = JSON.stringify(args);
  }

  const resp = await fetch(path, opts);
  if (!resp.ok) throw new Error(await resp.text() || `API ${resp.status}`);

  const ct = resp.headers.get('content-type');
  let result = ct?.includes('json') ? await resp.json() : await resp.text();

  const transform = TRANSFORMS[command];
  if (transform) result = transform(result);

  return result;
}
