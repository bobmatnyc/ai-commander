import { writable, derived } from 'svelte/store';

export interface Session {
  name: string;
  created_at: string;
  is_connected: boolean;
}

export interface Message {
  id?: string;
  direction: 'sent' | 'received' | 'system';
  content: string;
  timestamp: Date;
  segmentType?: 'prompt' | 'output' | 'tool';
}

export interface BotStatus {
  running: boolean;
  pid: number | null;
}

// Maximum messages retained per session to prevent memory leaks
const MAX_MESSAGES_PER_SESSION = 500;

// Session-specific message history
export const sessionMessages = writable<Map<string, Message[]>>(new Map());

// Current active session
export const currentSession = writable<Session | null>(null);

// Derived store: messages for current session only
export const messages = derived(
  [sessionMessages, currentSession],
  ([$sessionMessages, $currentSession]) => {
    if (!$currentSession) return [];
    return $sessionMessages.get($currentSession.name) || [];
  }
);

// Helper to add message to specific session
export function addMessageToSession(sessionName: string, message: Message) {
  sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];
    const updated = [...msgs, message];
    map.set(sessionName, updated.length > MAX_MESSAGES_PER_SESSION
      ? updated.slice(updated.length - MAX_MESSAGES_PER_SESSION)
      : updated);
    return new Map(map);
  });
}

// Helper to update the content of a specific message by id
export function updateMessageContent(sessionName: string, messageId: string, content: string) {
  sessionMessages.update(map => {
    const msgs = map.get(sessionName);
    if (!msgs) return map;
    const updated = msgs.map(m => m.id === messageId ? { ...m, content } : m);
    map.set(sessionName, updated);
    return new Map(map);
  });
}

// Helper to update the last system message's content (for SSE is_update events)
export function updateLastSystemMessage(sessionName: string, content: string) {
  sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];
    // Find last system message and update its content
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].direction === 'system') {
        msgs[i] = { ...msgs[i], content };
        break;
      }
    }
    map.set(sessionName, [...msgs]);
    return new Map(map);
  });
}

// Helper to clear messages for specific session
export function clearSessionMessages(sessionName: string) {
  sessionMessages.update(map => {
    map.delete(sessionName);
    return new Map(map);
  });
}

export interface GitHubStats {
    open_issues: number;
    open_prs: number;
    repo: string;
}
export const githubStats = writable<Map<string, GitHubStats>>(new Map());

export const sessions = writable<Session[]>([]);
export const botRunning = writable(false);
export const botPid = writable<number | null>(null);
export const serverRebuilding = writable<boolean>(false);

// Track which sessions have recent activity (SSE or Tauri events)
export const activeSessions = writable<Set<string>>(new Set());

const activityTimers = new Map<string, ReturnType<typeof setTimeout>>();

export function markSessionActive(sessionName: string) {
  activeSessions.update(set => {
    const next = new Set(set);
    next.add(sessionName);
    return next;
  });

  // Clear previous timer for this session
  const prev = activityTimers.get(sessionName);
  if (prev) clearTimeout(prev);

  // Remove after 5 seconds of no activity
  const timer = setTimeout(() => {
    activeSessions.update(set => {
      const next = new Set(set);
      next.delete(sessionName);
      return next;
    });
    activityTimers.delete(sessionName);
  }, 5000);
  activityTimers.set(sessionName, timer);
}
