// Transport abstraction — works in both Tauri and browser contexts

import { get } from 'svelte/store';
import { currentSession } from './stores/app';

// Tauri v2 exposes `window.__TAURI_INTERNALS__`; v1 used `window.__TAURI__`.
// We check both so the same transport works whichever ships in the desktop bundle.
// Without this, the desktop app falls through to REST and tries to hit relative
// `/api/...` URLs against the `tauri://` origin, which silently fails — leaving
// modals (e.g. CreateSessionModal) with empty data and no error surfaced.
const isTauri = typeof window !== 'undefined' &&
  ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);

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
  capture_session_output: { method: 'POST', path: (args) => `/api/sessions/${encodeURIComponent(args.name as string)}/capture` },
  get_bot_status: { method: 'GET', path: '/api/bot/status' },
  list_processes: { method: 'GET', path: '/api/processes' },
  kill_stale_processes: { method: 'POST', path: '/api/processes/clean' },
  rename_session: { method: 'POST', path: '/api/sessions/rename' },
  set_session_nickname: { method: 'POST', path: '/api/sessions/nickname' },
  get_config: { method: 'GET', path: '/api/config' },
  save_config: { method: 'POST', path: '/api/config' },
  get_github_stats: { method: 'GET', path: '/api/github-stats' },
  list_session_log_dates: { method: 'GET', path: (args) => `/api/sessions/${args.name}/logs` },
  get_session_log: { method: 'GET', path: (args) => `/api/sessions/${args.name}/logs/${args.date}` },
  archive_session_logs: { method: 'POST', path: (args) => `/api/sessions/${args.name}/logs/archive` },
  delete_registration: { method: 'DELETE', path: (args) => `/api/sessions/${encodeURIComponent(args.name as string)}/registration` },
  unregister_session: { method: 'DELETE', path: (args) => `/api/sessions/${encodeURIComponent((args.session_name ?? args.name) as string)}/unregister` },
};

// Request transformers — remap frontend args to REST API format
const REQUEST_TRANSFORMS: Record<string, (args: Record<string, unknown>) => Record<string, unknown>> = {
  send_message: (args) => ({
    session: get(currentSession)?.name || '',
    message: args.content || '',
  }),
  rename_session: (args) => ({
    old_name: args.oldName || args.old_name || '',
    new_name: args.newName || args.new_name || '',
  }),
  set_session_nickname: (args) => ({
    session_name: args.session_name || args.sessionName || '',
    nickname: args.nickname ?? '',
  }),
  disconnect_session: (args) => ({ session: args.name ?? args.session ?? '' }),
};

async function fetchApi(command: string, args?: Record<string, unknown>): Promise<unknown> {
  const mapping = API_MAP[command];
  if (!mapping) {
    throw new Error(`Unknown API command: ${command}`);
  }

  // Apply request transform before building the fetch request
  const reqTransform = REQUEST_TRANSFORMS[command];
  const body = reqTransform ? reqTransform(args || {}) : args;

  const path = typeof mapping.path === 'function' ? mapping.path(body || {}) : mapping.path;
  const url = path; // Relative URL — works when served from same origin

  const options: RequestInit = {
    method: mapping.method,
    headers: { 'Content-Type': 'application/json' },
  };

  if (mapping.method !== 'GET' && body) {
    options.body = JSON.stringify(body);
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
    // REST returns {sessions: [{name, pane_count, session_state, ...}]}
    // Tauri returns [{name, created_at, is_connected, session_state}]
    // Preserve session_state / nickname / path so the UI can render the
    // tri-state (connected / disconnected / registered) visuals in both modes.
    if (data?.sessions) {
      return data.sessions.map((s: any) => ({
        name: s.name,
        created_at: s.created_at || new Date().toISOString(),
        is_connected: s.session_state === 'connected',
        path: s.path,
        nickname: s.nickname,
        session_state: s.session_state || 'disconnected',
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
  capture_session_output: (data: any) => {
    // REST returns {session, output, adapter}; ChatView expects a plain string for
    // Raw mode. Without this transform, `rawContent.replace(...)` fails on the
    // object and Raw mode silently shows nothing (or falls through to index.html
    // if the route is wrong). See ChatView.refreshRawContent().
    if (typeof data === 'object' && data?.output) return data.output;
    return typeof data === 'string' ? data : '';
  },
  get_session_summary: (data: any) => {
    // REST returns {session, output}; callers that want the string get it here.
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
  is_update?: boolean;
  /** Present on "raw" events: character count of new content. */
  char_count?: number;
  /** Present on "raw" events: line count of new content. */
  line_count?: number;
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
