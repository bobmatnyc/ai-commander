import { writable, derived } from 'svelte/store';

export interface Session {
  name: string;
  created_at: string;
  is_connected: boolean;
}

export interface Message {
  direction: 'sent' | 'received' | 'system';
  content: string;
  timestamp: Date;
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

// Helper to clear messages for specific session
export function clearSessionMessages(sessionName: string) {
  sessionMessages.update(map => {
    map.delete(sessionName);
    return new Map(map);
  });
}

export const sessions = writable<Session[]>([]);
export const botRunning = writable(false);
export const botPid = writable<number | null>(null);
