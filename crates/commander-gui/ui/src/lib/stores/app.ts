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

const CONSOLIDATION_WINDOW_MS = 5 * 60 * 1000; // 5 minutes

/**
 * Why: System/history messages that arrive within 5 minutes of each other
 * should be grouped into a single bullet-block rather than cluttering the
 * chat with many small system bubbles. This covers both rapid-succession
 * messages and messages that arrive a few minutes apart.
 * What: Scans back through the message list for the most recent system
 * message from the same sender that is within the 5-minute window. If found,
 * appends the new body as a bullet line. If the existing block does not yet
 * use bullet format it is normalised first.
 * Test: Add two 'system' messages 2 minutes apart with the same sender prefix
 * — assert only one Message exists in the store and its content contains two
 * bullet lines. Add a third message 6 minutes later — assert a second Message
 * is created.
 *
 * @param msgs   Mutable array of messages for the session (will be mutated in-place).
 * @param message The incoming system message to consolidate or append.
 * @returns true if the message was consolidated into an existing block; false
 *          if a new message should be pushed.
 */
function tryConsolidateSystemMessage(msgs: Message[], message: Message): boolean {
  const newSender = extractSenderPrefix(message.content);
  if (!newSender) return false;

  const now = message.timestamp instanceof Date ? message.timestamp.getTime() : Date.now();
  const newBody = extractMessageBody(message.content).trim();

  // Walk backwards to find the most recent system message from the same sender
  // within the consolidation window.
  for (let i = msgs.length - 1; i >= 0; i--) {
    const candidate = msgs[i];
    if (candidate.direction !== 'system') continue;

    const candidateSender = extractSenderPrefix(candidate.content);
    if (candidateSender !== newSender) continue;

    const candidateTime = candidate.timestamp instanceof Date
      ? candidate.timestamp.getTime()
      : 0;
    if (now - candidateTime > CONSOLIDATION_WINDOW_MS) break; // too old

    // Found a match within the window. Normalise the existing block to bullet
    // format if it isn't already, then append the new body.
    const existingBody = extractMessageBody(candidate.content);
    const existingLines = existingBody
      .split('\n')
      .map(l => l.trim())
      .filter(l => l.length > 0);

    // Dedup: skip if exact body already present in the block.
    const plainLines = existingLines.map(l => l.replace(/^• /, ''));
    if (plainLines.includes(newBody)) return true;

    // Normalise existing lines to bullet format.
    const normalisedLines = existingLines.map(l => l.startsWith('• ') ? l : `• ${l}`);
    const newBullet = `• ${newBody}`;
    const updatedBody = [...normalisedLines, newBullet].join('\n');
    const updatedContent = `${newSender}: ${updatedBody}`;

    msgs[i] = { ...candidate, content: updatedContent, timestamp: message.timestamp };
    return true;
  }

  return false;
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

    // Try 5-minute window consolidation for system messages.
    if (message.direction === 'system') {
      const mutableMsgs = [...msgs];
      if (tryConsolidateSystemMessage(mutableMsgs, message)) {
        map.set(sessionName, mutableMsgs);
        return new Map(map);
      }
      // No match in window — push new message (bullet-prefixed body).
      const newSender = extractSenderPrefix(message.content);
      const newBody = extractMessageBody(message.content).trim();
      const formatted: Message = newSender
        ? { ...message, content: `${newSender}: • ${newBody}` }
        : message;
      const updated = [...mutableMsgs, formatted];
      map.set(sessionName, updated.length > MAX_MESSAGES_PER_SESSION
        ? updated.slice(updated.length - MAX_MESSAGES_PER_SESSION)
        : updated);
      return new Map(map);
    }

    // For 'received' messages (LLM summaries): consolidate rapid-fire updates
    // into the existing bubble rather than appending a new one every 2s.
    // Why: The poller fires every ~2s and each poll may emit a new interpretation
    // event with is_update=false (first message of a new poll cycle). Without
    // consolidation each cycle adds a fresh bubble, producing a wall of near-
    // identical 'received' bubbles. We update in-place when the last received
    // message is within a 30-second window.
    // What: Scans back for the most recent 'received' message; if within 30s,
    // calls updateMessageContent on its id (or mutates in-place). Otherwise
    // pushes a new message.
    // Test: Add two 'received' messages 5s apart — assert only one Message entry
    // exists in the store. Add a third 40s later — assert a second entry is created.
    if (message.direction === 'received') {
      const RECEIVED_WINDOW_MS = 30_000;
      const now = message.timestamp instanceof Date ? message.timestamp.getTime() : Date.now();

      // Walk backwards for the most recent 'received' message.
      for (let i = msgs.length - 1; i >= 0; i--) {
        const candidate = msgs[i];
        if (candidate.direction !== 'received') continue;

        const candidateTime = candidate.timestamp instanceof Date
          ? candidate.timestamp.getTime()
          : 0;

        if (now - candidateTime > RECEIVED_WINDOW_MS) break; // too old — stop looking

        // Found a recent received message — update in place.
        if (candidate.id) {
          // Mutate a copy so the store update triggers reactivity.
          const mutableMsgs = [...msgs];
          mutableMsgs[i] = { ...candidate, content: message.content, timestamp: message.timestamp };
          map.set(sessionName, mutableMsgs);
          return new Map(map);
        }
        break; // found but no id — fall through to push
      }

      // No recent received message found — push as new.
      const updated = [...msgs, message];
      map.set(sessionName, updated.length > MAX_MESSAGES_PER_SESSION
        ? updated.slice(updated.length - MAX_MESSAGES_PER_SESSION)
        : updated);
      return new Map(map);
    }

    // All other non-system directions: push directly.
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

// Why: The session list needs a subtle pulse on any row that just received
// data — not just the currently-selected session. We record the timestamp of
// the most recent `session-output` / SSE event per session name; a ticker
// in SessionList re-evaluates "recent" every 200 ms.
// What: Reactive map of `sessionName -> lastActivityAt (ms since epoch)`.
// Test: Call `markSessionDataReceived('foo')`, assert `$lastActivityAt.get('foo')`
// is within the last 100 ms.
export const lastActivityAt = writable<Map<string, number>>(new Map());

/**
 * Why: Funnels both Tauri `session-output` events and REST SSE events through a
 * single helper so the pulse-dot logic has one source of truth.
 * What: Sets lastActivityAt for the given session to the current timestamp.
 * Test: Call this with 'foo', read $lastActivityAt.get('foo') — should be close
 * to Date.now() and `isRecentlyActive('foo')` should return true for 3s.
 */
export function markSessionDataReceived(sessionName: string) {
  lastActivityAt.update(map => {
    const next = new Map(map);
    next.set(sessionName, Date.now());
    return next;
  });
}

// Current view for navigation (e.g. 'sessions', 'chat', 'dashboard')
export const currentView = writable<string>('sessions');

// Why: CreateSessionModal is rendered at the top level of WebApp.svelte to
// avoid stacking-context clipping by the <aside> element in web mode.
// SessionList sets this to true when the user clicks the New button.
// What: Shared boolean that controls CreateSessionModal visibility from
// SessionList (writer) and WebApp (renderer).
// Test: Click New in SessionList, assert $showCreateSessionModal === true;
// close the modal, assert it resets to false.
export const showCreateSessionModal = writable(false);

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
