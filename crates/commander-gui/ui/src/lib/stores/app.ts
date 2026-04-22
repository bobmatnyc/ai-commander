import { writable, derived } from 'svelte/store';

export interface Session {
  name: string;
  created_at: string;
  is_connected: boolean;
  path?: string;
  is_active?: boolean;
  status_line?: string;
  nickname?: string;
  /**
   * Tri-state lifecycle label returned by the backend:
   * - "connected"    — tmux session exists AND is actively monitored
   * - "disconnected" — tmux session exists but not currently monitored
   * - "registered"   — only a project registration exists (no tmux)
   *
   * Optional for defensive compatibility with older backends; the session
   * list UI falls back to "disconnected" when absent.
   */
  session_state?: 'connected' | 'disconnected' | 'registered';
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

/**
 * Extract the "sender:" prefix from a message content string.
 * e.g., "claude: Running hooks" → "claude"
 */
function extractSenderPrefix(content: string): string | null {
  const match = content.match(/^(\w+):\s/);
  return match ? match[1] : null;
}

/**
 * Extract the text after the "sender:" prefix.
 * e.g., "claude: Running hooks" → "Running hooks"
 */
function extractMessageBody(content: string): string {
  const idx = content.indexOf(': ');
  return idx >= 0 ? content.substring(idx + 2) : content;
}

// Helper to add message to specific session.
// Consolidates consecutive messages from the same sender into one block.
// Skips system messages that echo recent user input.
export function addMessageToSession(sessionName: string, message: Message) {
  sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];

    // Skip system interpretations that echo a recent user message.
    // Search all sent messages in the last 30 entries (system messages can
    // push the user's message out of a small window).
    if (message.direction === 'system') {
      const body = extractMessageBody(message.content).trim().toLowerCase();
      if (body.length > 0) {
        const recentSent = msgs.slice(-30).filter(m => m.direction === 'sent');
        for (const sent of recentSent) {
          const sentText = sent.content.trim().toLowerCase();
          if (sentText.length > 3 && (body.includes(sentText) || sentText.includes(body))) {
            return map; // Skip — this is just echoing user input
          }
        }
      }
    }

    const lastMsg = msgs[msgs.length - 1];

    // Try to consolidate with previous message from same sender + direction
    if (lastMsg && lastMsg.direction === message.direction && message.direction === 'system') {
      const lastSender = extractSenderPrefix(lastMsg.content);
      const newSender = extractSenderPrefix(message.content);

      if (lastSender && newSender && lastSender === newSender) {
        const newBody = extractMessageBody(message.content).trim();
        const existingBodies = extractMessageBody(lastMsg.content)
          .split('\n')
          .map(l => l.trim())
          .filter(l => l.length > 0);

        // Skip if this exact line already exists (dedup)
        if (existingBodies.includes(newBody)) {
          return map; // No change needed
        }

        // Append new line to existing block
        const updatedContent = lastMsg.content + '\n' + newBody;
        msgs[msgs.length - 1] = { ...lastMsg, content: updatedContent, timestamp: message.timestamp };
        map.set(sessionName, [...msgs]);
        return new Map(map);
      }
    }

    // Otherwise add as new message
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

// Helper to update the last system message's content (for SSE is_update events).
// Replaces the last line in the consolidated block if same sender, or replaces entirely.
export function updateLastSystemMessage(sessionName: string, content: string) {
  sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].direction === 'system') {
        const lastSender = extractSenderPrefix(msgs[i].content);
        const newSender = extractSenderPrefix(content);

        if (lastSender && newSender && lastSender === newSender) {
          // Same sender: replace the last line in the block
          const lines = msgs[i].content.split('\n');
          const newBody = extractMessageBody(content).trim();
          if (lines.length > 1) {
            // Replace last line only
            lines[lines.length - 1] = newBody;
            msgs[i] = { ...msgs[i], content: lines.join('\n') };
          } else {
            // Single line — replace entirely
            msgs[i] = { ...msgs[i], content };
          }
        } else {
          // Different sender — replace the whole message
          msgs[i] = { ...msgs[i], content };
        }
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

// Current view for navigation (e.g. 'sessions', 'chat', 'dashboard')
export const currentView = writable<string>('sessions');

// Sessions explicitly hidden from the dashboard
export const hiddenSessions = writable<Set<string>>(new Set());

export function hideSession(name: string) {
  hiddenSessions.update(set => {
    const next = new Set(set);
    next.add(name);
    return next;
  });
}

export function unhideAll() {
  hiddenSessions.set(new Set());
}

// No-op stub — hydration from localStorage can be wired up later
export async function hydrateSessionMessages(_sessionName: string): Promise<void> {}

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
