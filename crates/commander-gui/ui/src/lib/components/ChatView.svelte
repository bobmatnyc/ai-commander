<script lang="ts">
  import { get } from 'svelte/store';
  import { messages, currentSession, addMessageToSession, updateMessageContent, clearSessionMessages, sessionMessages, markSessionActive, markSessionDataReceived } from '../stores/app';
  import { onMount, onDestroy, afterUpdate, tick } from 'svelte';

  // ─── Pagination ────────────────────────────────────────────────────────────
  // Why: Long sessions accumulate hundreds of messages; rendering all of them
  // at once causes noticeable jank on scroll. Pagination shows only the most
  // recent PAGE_SIZE entries and prepends older ones as the user scrolls up.
  // What: visibleCount tracks how many messages to show from the END of the
  // full list. loadMore() increments it and restores scroll position so the
  // viewport doesn't jump.
  // Test: With >10 messages, assert only 10 render initially. Scroll to top,
  // assert 10 more are prepended and viewport position is preserved.
  const PAGE_SIZE = 10;
  let visibleCount = PAGE_SIZE;
  import { listen } from '@tauri-apps/api/event';
  import { invoke, subscribeSessionEvents, isDesktop, type SessionEventData } from '../transport';
  import { ArrowDown, Archive } from 'lucide-svelte';

  interface LogEntry {
    ts: number;
    text: string;
    hash: string;
  }

  let terminalEl: HTMLDivElement;
  let autoScroll = true;
  let showScrollButton = false;
  let isActionLoading = false;
  let connecting = false;
  let waitingForResponse = false;
  let waitingTimer: number;
  let streamingMessageId: string | null = null;
  let viewMode: 'summary' | 'raw' = 'summary';
  // Tracks sessions whose history has already been replayed to avoid
  // re-appending the same log entries on re-render.
  const loadedHistorySessions = new Set<string>();

  let isActive = false;
  let activityTimer: number;
  let lineCount = 0;

  // Why: Surface a subtle, always-visible metric of incoming data volume in
  // place of the old "Connected to session" chat bubble. Gives the user a
  // diagnostic signal that data is flowing without cluttering the message
  // stream. Resets on session switch / disconnect.
  // What: Running totals of characters and newline-delimited lines observed
  // since connecting to the current session.
  // Test: Send an SSE event with content "abc\ndef", assert charsReceived
  // becomes 7 and linesReceived becomes 2.
  let charsReceived = 0;
  let linesReceived = 0;

  // True when the polling loop has reported that both LLM backends (Ollama
  // and OpenRouter) have failed multiple times in a row. Surfaces a banner
  // so the user can take action instead of seeing silent no-ops.
  let llmUnavailable = false;

  // Raw-mode terminal state
  let rawContent = '';
  let rawError = '';
  let rawPollTimer: number | null = null;
  let rawPaneEl: HTMLPreElement;

  // Web-mode SSE subscription cleanup (no-op in Tauri mode)
  let sseCleanup: (() => void) | null = null;

  const ANSI_ESCAPE = /\x1b\[[0-9;]*[mGKHF]/g;

  /** Check if the last system message in a session has the same content (dedup). */
  function isDuplicateSystemMessage(sessionName: string, content: string): boolean {
    const msgs = get(sessionMessages).get(sessionName) || [];
    const last = msgs[msgs.length - 1];
    return !!(last && last.direction === 'system' && last.content === content);
  }

  function scrollToBottom(smooth = false) {
    if (terminalEl) {
      if (smooth) {
        terminalEl.scrollTo({ top: terminalEl.scrollHeight, behavior: 'smooth' });
      } else {
        terminalEl.scrollTop = terminalEl.scrollHeight;
      }
      autoScroll = true;
      showScrollButton = false;
    }
  }

  function scrollRawToBottom() {
    if (rawPaneEl) rawPaneEl.scrollTop = rawPaneEl.scrollHeight;
  }

  function handleScroll() {
    if (!terminalEl) return;
    const { scrollTop, scrollHeight, clientHeight } = terminalEl;
    const atBottom = scrollHeight - scrollTop - clientHeight < 50;
    autoScroll = atBottom;
    showScrollButton = !atBottom;
    // Load older messages when user scrolls near the top
    if (scrollTop < 50 && hasMore) {
      loadMore(terminalEl);
    }
  }

  // Why: Prepending older messages shifts content down; saving/restoring the
  // scroll offset relative to the bottom of the previous content keeps the
  // viewport anchored to the message the user was reading.
  // What: Captures scrollHeight before visibleCount grows, then after the DOM
  // updates sets scrollTop to the delta so content appears stationary.
  // Test: With 25 messages and visibleCount=10, trigger loadMore; assert
  // scrollTop after tick equals newScrollHeight - prevScrollHeight.
  function loadMore(el?: HTMLElement) {
    const prevScrollHeight = el?.scrollHeight ?? 0;
    visibleCount = Math.min(visibleCount + PAGE_SIZE, allMessages.length);
    tick().then(() => {
      if (el) {
        el.scrollTop = el.scrollHeight - prevScrollHeight;
      }
    });
  }

  function markActivity() {
    waitingForResponse = false;
    clearTimeout(waitingTimer);
  }

  function updateStreamingMessage(sessionName: string, text: string) {
    if (!streamingMessageId) {
      streamingMessageId = crypto.randomUUID();
      addMessageToSession(sessionName, {
        id: streamingMessageId,
        direction: 'received',
        content: text,
        timestamp: new Date(),
      });
    } else {
      updateMessageContent(sessionName, streamingMessageId, text);
    }
    if (autoScroll) setTimeout(scrollToBottom, 10);
  }

  function finalizeStreamingMessage(sessionName: string, text: string, cost?: number) {
    if (streamingMessageId) {
      updateMessageContent(sessionName, streamingMessageId, text);
      streamingMessageId = null;
    }
    if (cost) {
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Cost: $${cost.toFixed(4)}`,
        timestamp: new Date(),
      });
    }
    if (autoScroll) setTimeout(scrollToBottom, 10);
  }

  /** Extract sender name from "sender: content" pattern. */
  function extractSender(content: string): string {
    const match = content.match(/^(\w+):/);
    return match ? match[1] : 'system';
  }

  /** Strip sender prefix from message content. */
  function stripSender(content: string): string {
    return content.replace(/^\w+:\s*/, '');
  }

  function escapeHtml(text: string): string {
    return text
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');
  }

  /**
   * Why: LLM summaries frequently include markdown (tables, lists, bold, inline
   * code). Rendering raw markdown text hurts readability, so we normalize it
   * into sanitized HTML before handing it to Svelte's `{@html …}` sink.
   * What: Escapes all user content, then opts specific markdown patterns
   * (code blocks, tables, lists, bold/italic, inline code, headings) back into
   * structured HTML.
   * Test: Pass a string with a `| a | b |` table, `**bold**`, `\`code\``, and a
   * numbered list; assert the output contains `<table class="chat-table">`,
   * `<strong>bold</strong>`, `<code>code</code>`, and `<ol class="chat-list">`.
   */
  function renderInlineMarkdown(text: string): string {
    // Text must already be HTML-escaped. We re-inject tags for known patterns.
    // Order matters: inline code first so its contents aren't re-interpreted
    // as bold/italic.
    return text
      .replace(/`([^`\n]+)`/g, '<code class="inline-code">$1</code>')
      .replace(/\*\*([^*\n]+)\*\*/g, '<strong>$1</strong>')
      .replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<em>$2</em>');
  }

  function renderContent(content: string): string {
    if (!content) return '';

    // First handle code blocks (preserve them from table parsing)
    const codeBlocks: string[] = [];
    let processed = content.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, _lang, code) => {
      const idx = codeBlocks.length;
      codeBlocks.push(`<pre class="code-block"><code>${escapeHtml(code)}</code></pre>`);
      return `\x00CODEBLOCK${idx}\x00`;
    });

    // Now handle markdown tables
    const lines = processed.split('\n');
    let result = '';
    let inTable = false;
    let tableHtml = '';
    let headerDone = false;

    for (const line of lines) {
      const trimmed = line.trim();

      // Detect table rows: starts with | and ends with |
      if (trimmed.startsWith('|') && trimmed.endsWith('|')) {
        // Separator row (|---|---|)
        if (/^\|[\s\-:|]+\|$/.test(trimmed)) {
          headerDone = true;
          continue;
        }

        if (!inTable) {
          inTable = true;
          tableHtml = '<table class="chat-table"><thead>';
        }

        const cells = trimmed.split('|').filter(c => c.trim() !== '');

        if (!headerDone) {
          tableHtml += '<tr>' + cells.map(c => `<th>${renderInlineMarkdown(escapeHtml(c.trim()))}</th>`).join('') + '</tr></thead><tbody>';
        } else {
          tableHtml += '<tr>' + cells.map(c => `<td>${renderInlineMarkdown(escapeHtml(c.trim()))}</td>`).join('') + '</tr>';
        }
      } else {
        if (inTable) {
          tableHtml += '</tbody></table>';
          result += tableHtml;
          inTable = false;
          tableHtml = '';
          headerDone = false;
        }
        result += escapeHtml(line) + '\n';
      }
    }

    if (inTable) {
      tableHtml += '</tbody></table>';
      result += tableHtml;
    }

    // Headings (applied to escaped text)
    result = result.replace(/(^|\n)### (.+?)(?=\n|$)/g, '$1<h3 class="chat-h3">$2</h3>');
    result = result.replace(/(^|\n)## (.+?)(?=\n|$)/g, '$1<h2 class="chat-h2">$2</h2>');
    result = result.replace(/(^|\n)# (.+?)(?=\n|$)/g, '$1<h1 class="chat-h1">$2</h1>');

    // Detect numbered lists (2+ consecutive lines starting with N. or N) )
    result = result.replace(
      /(?:^|\n)((?:\d+[.)]\s+.+\n?){2,})/gm,
      (match) => {
        const items = match.trim().split('\n').map(line => {
          const m = line.match(/^\d+[.)]\s+(.+)/);
          return m ? `<li>${renderInlineMarkdown(m[1])}</li>` : '';
        }).join('');
        return `<ol class="chat-list">${items}</ol>`;
      }
    );

    // Detect bullet lists (2+ consecutive lines starting with - or *)
    result = result.replace(
      /(?:^|\n)((?:[-*]\s+.+\n?){2,})/gm,
      (match) => {
        const items = match.trim().split('\n').map(line => {
          const m = line.match(/^[-*]\s+(.+)/);
          return m ? `<li>${renderInlineMarkdown(m[1])}</li>` : '';
        }).join('');
        return `<ul class="chat-list">${items}</ul>`;
      }
    );

    // Detect selector lines (lines with ❯ or > or → prefix in a group)
    result = result.replace(
      /(?:^|\n)((?:[❯>→ ]\s+.+\n?){2,})/gm,
      (match) => {
        const items = match.trim().split('\n').map(line => {
          const isSelected = /^[❯→]/.test(line.trim());
          const text = line.trim().replace(/^[❯>→ ]\s+/, '');
          return `<div class="selector-item${isSelected ? ' selected' : ''}">${escapeHtml(text)}</div>`;
        }).join('');
        return `<div class="chat-selector">${items}</div>`;
      }
    );

    // Apply inline markdown (bold/italic/inline-code) to the remaining body,
    // avoiding the HTML we've already injected (tables, lists, code blocks).
    // Strategy: split on tag boundaries, transform only text segments.
    result = result.replace(
      /(<(?:table|ol|ul|pre|h[1-3])[\s\S]*?<\/(?:table|ol|ul|pre|h[1-3])>|\x00CODEBLOCK\d+\x00)|([^<\x00]+)/g,
      (_m, preserved, textChunk) => preserved ?? renderInlineMarkdown(textChunk)
    );

    // Restore code blocks
    result = result.replace(/\x00CODEBLOCK(\d+)\x00/g, (_match, idx) => codeBlocks[parseInt(idx)]);

    return result;
  }

  // Called by InputArea (via exported function) when a message is sent
  export function notifyMessageSent() {
    waitingForResponse = true;
    // Clear waiting state after 60s to avoid stale indicator
    clearTimeout(waitingTimer);
    waitingTimer = window.setTimeout(() => {
      waitingForResponse = false;
    }, 60_000);
  }

  // ─── Raw-mode terminal pane ────────────────────────────────────────────────

  async function refreshRawContent() {
    if (!$currentSession) return;
    const sessionName = $currentSession.name;
    try {
      const raw = await invoke<string>('capture_session_output', { name: sessionName });
      if ($currentSession?.name !== sessionName) return; // session switched during await
      rawContent = (raw || '').replace(ANSI_ESCAPE, '');
      rawError = '';
      setTimeout(scrollRawToBottom, 10);
    } catch (err) {
      // Surface the error in the raw pane so it's visible on mobile
      if ($currentSession?.name === sessionName) {
        rawError = `Failed to fetch terminal output: ${err}`;
      }
    }
  }

  function startRawPolling() {
    stopRawPolling();
    refreshRawContent();
    rawPollTimer = window.setInterval(refreshRawContent, 1000);
  }

  function stopRawPolling() {
    if (rawPollTimer !== null) {
      clearInterval(rawPollTimer);
      rawPollTimer = null;
    }
  }

  // ─── Web SSE event handling ──────────────────────────────────────────────
  // In Tauri desktop mode, `session-output` and `chat-event` fire via native
  // Tauri events (see onMount). In web mode those listeners are no-ops
  // (shimmed by vite.web.config.ts → tauri-event-shim.ts), so we bridge the
  // REST SSE endpoint `/api/sessions/:name/events` into the same UI updates.
  //
  // The backend emits `event_type: 'interpretation'` (primary poller) and
  // `event_type: 'update'` (shell-adapter path) events with LLM-generated
  // summaries of tmux screen changes. We render both as received Claude
  // messages in summary mode, or trigger a raw refresh in raw mode,
  // mirroring the Tauri `session-output` path.
  function handleSseEvent(data: SessionEventData) {
    connecting = false;
    markActivity();

    if (!$currentSession || $currentSession.name !== data.session_name) return;
    markSessionActive(data.session_name);

    // Increment the activity counter shown in the summary status bar so the
    // user gets a live diagnostic signal of incoming data volume.
    const rawChunk = data.content || '';
    if (rawChunk) {
      charsReceived += rawChunk.length;
      linesReceived += rawChunk.split('\n').length;
    }

    // Activity indicator
    isActive = true;
    clearTimeout(activityTimer);
    activityTimer = window.setTimeout(() => { isActive = false; }, 3000);

    // Handle lightweight raw-data events: update activity counters only,
    // no content to render. This fires even when the LLM filter strips
    // everything (pure tool-use chrome), keeping the counter alive.
    // Why: counters must update regardless of viewMode — incrementing them
    // after the raw-mode early return meant they were never reached in raw
    // view, leaving the chars/lines display frozen at zero.
    // Test: Switch to raw view, receive a 'raw' SSE event with char_count=10,
    // assert charsReceived increments by 10 even though viewMode === 'raw'.
    if (data.event_type === 'raw') {
      charsReceived += data.char_count ?? 0;
      linesReceived += data.line_count ?? 0;
      markSessionDataReceived(data.session_name);
      // Fall through to view-mode handling below (raw mode just needs a refresh).
    }

    if (viewMode === 'raw') {
      // Raw mode is driven by polling capture_session_output; just trigger a
      // refresh so the pane updates promptly when new content is detected.
      refreshRawContent();
      return;
    }

    // Summary mode: surface the interpretation text as a Claude message.
    // Skip empty/placeholder interpretations.
    const content = (data.content || '').trim();
    if (!content) return;

    // Why: The REST API emits both `interpretation` (from the primary poller
    // in handlers/web.rs:2014) and `update` (from the shell-adapter path at
    // handlers/web.rs:1903) as semantically identical LLM-summary events.
    // Accepting only `'interpretation'` here silently drops summaries for
    // shell-adapter sessions, leaving the Summary view blank while raw
    // output continues to accumulate — which was contributing to the
    // "summary never shows anything useful" symptom in web mode.
    // What: Treats both event_type values as a render-this-summary signal.
    // Unknown event_types are still ignored.
    // Test: Send an SSE event with event_type="update" and content="Hello",
    // assert a received message containing "Hello" appears in $messages.

    // 'raw' events carry no renderable content in summary mode — skip them.
    if (data.event_type === 'raw') return;

    // Both LLM backends (Ollama + OpenRouter) returned nothing — surface banner.
    if (data.event_type === 'llm_unavailable') {
      llmUnavailable = true;
      return;
    }

    if (data.event_type === 'interpretation' || data.event_type === 'update') {
      if (data.is_update && streamingMessageId) {
        // Replace the in-progress message with the newer interpretation
        updateMessageContent(data.session_name, streamingMessageId, content);
      } else {
        // Start a fresh received message for this interpretation
        streamingMessageId = crypto.randomUUID();
        addMessageToSession(data.session_name, {
          id: streamingMessageId,
          direction: 'received',
          content,
          timestamp: new Date(),
        });
      }
      lineCount += content.split('\n').length;
      if (autoScroll) setTimeout(scrollToBottom, 10);
    }
  }

  function startSseSubscription(sessionName: string) {
    stopSseSubscription();
    if (isDesktop()) return; // Tauri events handle this
    sseCleanup = subscribeSessionEvents(sessionName, handleSseEvent);
  }

  function stopSseSubscription() {
    if (sseCleanup) {
      sseCleanup();
      sseCleanup = null;
    }
  }

  // Toggle polling whenever viewMode or currentSession changes
  $: {
    if ($currentSession && viewMode === 'raw') {
      startRawPolling();
    } else {
      stopRawPolling();
      rawContent = '';
      rawError = '';
    }
  }

  // Pagination-derived values — recomputed whenever the message store or
  // visibleCount changes. allMessages follows $messages (the store already
  // scoped to the current session by the app store's derived logic).
  $: allMessages = $messages ?? [];
  $: visibleMessages = allMessages.slice(Math.max(0, allMessages.length - visibleCount));
  $: hasMore = allMessages.length > visibleCount;

  // ─── Session actions ───────────────────────────────────────────────────────

  async function handleStatus() {
    if (!$currentSession || isActionLoading) return;
    const sessionName = $currentSession.name;
    isActionLoading = true;

    try {
      const summary = await invoke('get_session_summary', { name: sessionName });
      let status = `📊 Session: ${sessionName}\n`;
      const s = summary as any;
      status += `Adapter: ${s.adapter}\n`;
      status += `State: ${s.is_idle ? '⏸ Idle' : '🔄 Active'}\n`;
      addMessageToSession(sessionName, {
        direction: 'system',
        content: status,
        timestamp: new Date(),
      });
    } catch (err) {
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Connected to "${sessionName}"`,
        timestamp: new Date(),
      });
    } finally {
      isActionLoading = false;
    }
  }

  async function handleStop() {
    if (!$currentSession || isActionLoading) return;
    const sessionName = $currentSession.name;

    const confirmed = confirm(`Are you sure you want to stop session "${sessionName}"? This will terminate the session.`);
    if (!confirmed) return;

    isActionLoading = true;

    try {
      await invoke('stop_session', { name: sessionName });
      clearSessionMessages(sessionName);
      currentSession.set(null);
    } catch (err) {
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Failed to stop session: ${err}`,
        timestamp: new Date(),
      });
    } finally {
      isActionLoading = false;
    }
  }

  async function handleDisconnect() {
    if (!$currentSession || isActionLoading) return;
    const sessionName = $currentSession.name;
    isActionLoading = true;

    try {
      await invoke('disconnect_session');
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Disconnected from session "${sessionName}".`,
        timestamp: new Date(),
      });
      currentSession.set(null);
    } catch (err) {
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Failed to disconnect: ${err}`,
        timestamp: new Date(),
      });
    } finally {
      isActionLoading = false;
    }
  }

  // ─── Summary log persistence ─────────────────────────────────────────────
  //
  // Why: Summaries emitted by the polling loop are ephemeral; when a session
  // is reopened the user sees an empty chat. We replay today's persisted log
  // as system messages so history survives restarts and session switches.
  // What: Fetches `get_session_log` for today's date and appends entries as
  // muted system messages prefixed with a timestamp.
  // Test: Seed a log file under a fake HOME, connect to the session, assert
  // the chat contains one system message per log entry before the connect
  // marker.
  /**
   * Why: Replaying N log entries as N separate bubbles clutters the chat with
   * identical-looking system blocks that each carry a timestamp in their body,
   * making the sender column meaningless and the history hard to scan.
   * What: Fetches today's log entries and emits a single consolidated system
   * message with sender "history" and one bullet per entry, mirroring the
   * live consolidation behaviour in addMessageToSession.
   * Test: Seed a log with 3 entries, open the session — assert exactly one
   * system message appears, its content starts with "history: ", and it
   * contains three "• " bullet lines.
   */
  async function loadLogHistory(sessionName: string) {
    if (loadedHistorySessions.has(sessionName)) return;
    loadedHistorySessions.add(sessionName);
    try {
      const today = new Date().toISOString().split('T')[0]; // YYYY-MM-DD
      const entries = (await invoke('get_session_log', {
        name: sessionName,
        date: today,
      })) as LogEntry[];
      if (!entries || entries.length === 0) return;

      // Deduplicate consecutive entries with identical text before rendering
      const deduped: LogEntry[] = [];
      for (const entry of entries) {
        if (deduped.length === 0 || deduped[deduped.length - 1].text !== entry.text) {
          deduped.push(entry);
        }
      }

      // Emit a single consolidated bubble: "history: • line1\n• line2\n..."
      const bullets = deduped.map(e => `• ${e.text}`).join('\n');
      const oldestTs = new Date(deduped[0].ts * 1000);
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `history: ${bullets}`,
        timestamp: oldestTs,
      });
    } catch (err) {
      // Non-fatal — absence of logs is normal.
      console.debug('loadLogHistory failed:', err);
    }
  }

  async function archiveLogs() {
    if (!$currentSession) return;
    const sessionName = $currentSession.name;
    try {
      const result = (await invoke('archive_session_logs', { name: sessionName })) as
        | string
        | { path: string };
      const path = typeof result === 'string' ? result : result?.path;
      addMessageToSession(sessionName, {
        direction: 'system',
        content: path ? `Logs archived to ${path}` : 'Logs archived',
        timestamp: new Date(),
      });
    } catch (err) {
      console.error('Archive failed:', err);
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Archive failed: ${err}`,
        timestamp: new Date(),
      });
    }
  }

  // ─── Lifecycle ─────────────────────────────────────────────────────────────

  // Why: After any DOM update that adds messages we scroll to the bottom when
  // the user hasn't manually scrolled up, using smooth scrolling to avoid a
  // jarring jump on new messages (initial load uses instant scroll below).
  // What: Fires after every Svelte DOM patch; checks autoScroll flag before
  // scrolling so the user can scroll up to read history without being yanked.
  // Test: Add a message while scrolled to bottom — assert smooth scroll fires.
  // Add a message while scrolled up — assert scroll position does NOT change.
  afterUpdate(() => {
    if (autoScroll && viewMode === 'summary') {
      tick().then(() => scrollToBottom(true));
    }
  });

  onMount(() => {
    connecting = true;
    // Instant scroll on initial mount — history may already be populated.
    tick().then(() => scrollToBottom(false));

    const unlistenSessionOutput = listen('session-output', (event: any) => {
      connecting = false;
      markActivity();

      const { content, full_content } = event.payload;
      if (!$currentSession) return;

      const sessionName = $currentSession.name;
      markSessionActive(sessionName);
      markSessionDataReceived(sessionName);
      const raw = content && content.length > 0 ? content : full_content;
      if (!raw) return;

      // Track activity indicator only (no content added in summary mode)
      lineCount += raw.split('\n').length;
      // Increment the diagnostic chars/lines counter that replaces the old
      // "Connected to session" message. Applies to both summary and raw mode.
      charsReceived += raw.length;
      linesReceived += raw.split('\n').length;
      isActive = true;
      clearTimeout(activityTimer);
      activityTimer = window.setTimeout(() => { isActive = false; }, 3000);

      // In raw mode, trigger an immediate refresh of the terminal pane
      if (viewMode === 'raw') {
        refreshRawContent();
      }
      // Summary mode: do nothing here. Content is driven purely by the
      // `chat-event` stream (user messages + Claude responses).
    });

    const unlistenChatEvent = listen('chat-event', (event: any) => {
      connecting = false;
      markActivity();

      const { type, content, accumulated, name, cost_usd, session } = event.payload;
      const sessionName = $currentSession?.name;
      if (!sessionName) return;

      // Ignore events that carry an explicit `session` field targeting a
      // different session. The polling loop only polls the current session,
      // so in practice this is a no-op guard, but it keeps things correct if
      // multiple sources ever fan in.
      if (session && session !== sessionName) return;

      markSessionActive(sessionName);

      switch (type) {
        case 'text':
          // Incremental text chunk (mpm-serve SSE path).
          updateStreamingMessage(sessionName, accumulated);
          break;
        case 'thinking':
          // Placeholder emitted by the polling loop while Claude is mid-
          // response.  Reuses the streaming-message slot so the final
          // LLM summary replaces it cleanly when `complete` fires.
          updateStreamingMessage(sessionName, content || 'Summarizing…');
          break;
        case 'update':
          // Running summary for the current block. Replaces the current
          // streaming bubble in place until a new block starts.
          updateStreamingMessage(sessionName, content || '');
          break;
        case 'new_block':
          // User sent new input — finalize the current summary bubble
          // so the next `update` creates a fresh one.
          if (streamingMessageId) {
            streamingMessageId = null;
          }
          break;
        case 'tool_use':
          addMessageToSession(sessionName, {
            direction: 'system',
            content: `Using tool: ${name}`,
            timestamp: new Date(),
          });
          break;
        case 'complete': {
          const finalText = (content || '').trim();
          if (!finalText) {
            // LLM returned nothing and the backend intentionally skipped
            // the fallback preview (summary mode never shows raw tmux
            // content). Discard any in-progress "Summarizing…" bubble
            // rather than leaving it blank.
            if (streamingMessageId) {
              updateMessageContent(sessionName, streamingMessageId, '');
              streamingMessageId = null;
            }
            break;
          }
          finalizeStreamingMessage(sessionName, finalText, cost_usd);
          break;
        }
        case 'system':
          // One-time degraded-mode notice (e.g. "LLM unavailable"). Dedup
          // so repeated emissions don't stack.
          if (content && !isDuplicateSystemMessage(sessionName, content)) {
            addMessageToSession(sessionName, {
              direction: 'system',
              content,
              timestamp: new Date(),
            });
          }
          break;
        case 'llm_unavailable':
          // Show a non-blocking banner; the user can configure OpenRouter
          // or dismiss. Reset on session switch (see reactive block below).
          llmUnavailable = true;
          break;
        case 'error':
          addMessageToSession(sessionName, {
            direction: 'system',
            content: `Error: ${content}`,
            timestamp: new Date(),
          });
          break;
      }
    });

    // Stop the "connecting" spinner after a short grace period even if
    // no output arrives immediately (avoids showing it on session switch).
    const connectingTimer = window.setTimeout(() => {
      connecting = false;
    }, 2000);

    return () => {
      unlistenSessionOutput.then(f => f());
      unlistenChatEvent.then(f => f());
      clearTimeout(connectingTimer);
      clearTimeout(waitingTimer);
      stopRawPolling();
      stopSseSubscription();
    };
  });

  onDestroy(() => {
    stopRawPolling();
    stopSseSubscription();
    clearTimeout(waitingTimer);
    clearTimeout(activityTimer);
  });

  // When the current session changes, just reset transient UI state.
  // The derived `messages` store already swaps to the cached history for the
  // new session (keyed by session name in `sessionMessages`), so we do NOT
  // clear anything and do NOT invoke the LLM summarizer.
  $: if ($currentSession) {
    const sessionName = $currentSession.name;
    connecting = true;
    waitingForResponse = false;
    lineCount = 0;
    // Reset the diagnostic chars/lines counter so it only reflects data
    // received during the current connection.
    charsReceived = 0;
    linesReceived = 0;
    isActive = false;
    streamingMessageId = null;
    llmUnavailable = false;
    // Reset pagination so the new session starts at the bottom showing the
    // most recent PAGE_SIZE messages rather than carrying over the previous
    // session's expanded count.
    visibleCount = PAGE_SIZE;

    // Web mode: (re)subscribe to the REST SSE event stream for this session.
    // No-op in Tauri mode (handled by native events).
    startSseSubscription(sessionName);

    // Drop "connecting" once we know whether history exists (avoid flash)
    const existing = get(sessionMessages).get(sessionName) || [];
    if (existing.length > 0) {
      connecting = false;
    } else {
      // In summary mode, replay persisted log history on open so users see
      // prior summaries without waiting for new activity.
      if (viewMode === 'summary') {
        loadLogHistory(sessionName);
      }
      // Why: Previously we injected a "Connected to session: X" system
      // message here. That bubble has been replaced by purely visual signals
      // — the green pulse dot on the session row (SessionList.svelte) and a
      // green tinge on the chat header (below) — so the chat stream stays
      // clean of redundant lifecycle chatter.
      // Test: Open a fresh session, assert $sessionMessages.get(name) does
      // NOT contain a "Connected to session" system entry.

      // Let the 2s onMount timer clear `connecting`
      setTimeout(() => { connecting = false; }, 500);
    }
  } else {
    connecting = false;
    waitingForResponse = false;
    streamingMessageId = null;
    charsReceived = 0;
    linesReceived = 0;
    stopSseSubscription();
  }


</script>

<div class="chat-view">
  {#if !$currentSession}
    <div class="empty-state">
      <p>Select a session to start chatting</p>
    </div>
  {:else}
    <!--
      `connected` class adds a subtle green tinge to the header bar as a
      persistent (non-noisy) signal that the chat is wired up to a live
      session. Replaces the old "Connected to session" chat bubble.
    -->
    <div class="session-actions" class:connected={$currentSession?.is_connected}>
      <button
        class="tab"
        on:click={handleStatus}
        disabled={isActionLoading}
        title="Show session status"
      >
        Status
      </button>
      <button
        class="tab"
        on:click={handleStop}
        disabled={isActionLoading}
        title="Stop and destroy this session"
      >
        Stop
      </button>
      <button
        class="tab"
        on:click={handleDisconnect}
        disabled={isActionLoading}
        title="Disconnect from this session"
      >
        Disconnect
      </button>

      <div class="view-mode-group">
        <button
          class="tab"
          class:active={viewMode === 'summary'}
          on:click={() => viewMode = 'summary'}
          title="Show summarized output"
        >
          Summary
        </button>
        <button
          class="tab"
          class:active={viewMode === 'raw'}
          on:click={() => viewMode = 'raw'}
          title="Show raw terminal output"
        >
          Raw
        </button>
        <button
          class="tab archive-btn"
          on:click={archiveLogs}
          title="Archive session logs to a zip file"
          aria-label="Archive session logs"
        >
          <Archive size={14} />
        </button>
      </div>

      {#if isActive}
        <span class="status-badge active">
          <span class="activity-dot"></span>
          Active
        </span>
      {/if}

      {#if connecting}
        <span class="status-badge connecting">
          <span class="spinner"></span>
          Connecting…
        </span>
      {:else if waitingForResponse}
        <span class="status-badge waiting">
          <span class="spinner"></span>
          Waiting for response…
        </span>
      {/if}

      <!--
        Activity counter — small, muted, monospace diagnostic metric showing
        volume of data received from the backend since connect. Replaces the
        old "Connected to session" chat bubble with a continuously-updated
        signal. Hidden when disconnected.
        Test: Connect to a session, assert this span renders; disconnect,
        assert it disappears.
      -->
      {#if $currentSession && (charsReceived > 0 || linesReceived > 0)}
        <span class="activity-counter" title="Bytes / lines received since connect">
          ↓ {charsReceived.toLocaleString()} chars · {linesReceived.toLocaleString()} lines
        </span>
      {/if}
    </div>

    {#if llmUnavailable && viewMode === 'summary'}
      <div class="llm-banner">
        <span>⚠ LLM summarization unavailable.</span>
        <button on:click={() => { llmUnavailable = false; }}>Configure OpenRouter</button>
        <button on:click={() => llmUnavailable = false}>Dismiss</button>
      </div>
    {/if}

    {#if viewMode === 'raw'}
      <pre bind:this={rawPaneEl} class="raw-pane" class:raw-error={rawError}>{rawError || rawContent || 'Loading terminal…'}</pre>
    {:else}
      <div
        bind:this={terminalEl}
        on:scroll={handleScroll}
        class="terminal-output"
      >
        {#if hasMore}
          <div
            class="load-more-indicator"
            on:click={() => loadMore(terminalEl)}
            role="button"
            tabindex="0"
            on:keydown={(e) => e.key === 'Enter' && loadMore(terminalEl)}
            aria-label="Load {allMessages.length - visibleCount} older messages"
          >
            ↑ {allMessages.length - visibleCount} older messages
          </div>
        {/if}
        {#each visibleMessages as message}
          {#if message.direction === 'sent'}
            <div class="message sent">
              <span class="message-sender">you</span>
              <span class="line-content sent-text">{@html renderContent(message.content)}</span>
            </div>
          {:else if message.direction === 'system'}
            <div class="message system">
              <span class="message-sender">{extractSender(message.content)}</span>
              <span class="line-content system-text">{@html renderContent(stripSender(message.content))}</span>
            </div>
          {:else}
            <div class="message received">
              <span class="message-sender">claude</span>
              <span class="line-content">{@html renderContent(message.content)}</span>
            </div>
          {/if}
        {:else}
          <!--
            Why: Summary mode intentionally renders only LLM-interpreted
            messages, never raw terminal output. On initial connect the
            summary buffer is empty while the first LLM call is throttled
            (see events.rs: `LLM_THROTTLE_STARTUP_MS`). Without this
            placeholder the pane looks dead and users toggle to Raw to
            "see what's happening", which is the symptom that was being
            reported as "raw text bleeding into Summary view".
            What: Shows a subtle waiting indicator tied to `lineCount` —
            once activity has been observed from the polling loop we know
            a summary is imminent rather than the session being empty.
            Test: Open a fresh session, assert the pane shows
            "Waiting for summary…" while `lineCount > 0` and no messages
            are in the store yet.
          -->
          <div class="terminal-empty">
            {#if lineCount > 0}
              <span class="waiting-summary">
                <span class="spinner"></span>
                Waiting for summary…
              </span>
            {:else}
              <span>No messages yet — send a message to start the conversation…</span>
            {/if}
          </div>
        {/each}
      </div>

      {#if showScrollButton}
        <button class="scroll-button" on:click={() => scrollToBottom(false)} aria-label="Scroll to bottom">
          <ArrowDown size={20} />
        </button>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .chat-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    position: relative;
    background-color: var(--bg-primary);
    overflow: hidden;
    min-height: 0;
  }

  .session-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
    background-color: var(--bg-secondary);
    flex-shrink: 0;
    flex-wrap: wrap;
    overflow-x: auto;
    -webkit-overflow-scrolling: touch;
    /* Transition both tint and border so connect/disconnect is a smooth fade */
    border-left: 4px solid transparent;
    transition: background-color 0.3s ease, border-left-color 0.3s ease;
  }

  /*
   * Connected-state visual: subtle green left-border + background tint.
   * Intentionally low-contrast — meant to be peripherally noticed, never
   * distracting. Replaces the old "Connected to session" system message.
   */
  .session-actions.connected {
    background-color: rgba(34, 197, 94, 0.08);
    border-left-color: #22c55e;
  }

  /*
   * Diagnostic activity counter — small, muted, monospace. Sits in the chat
   * toolbar alongside status badges. Visually distinct from chat content so
   * the user reads it as a metric, not a message.
   */
  .activity-counter {
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', monospace;
    font-size: 0.7rem;
    color: var(--text-secondary);
    opacity: 0.75;
    padding: 0.2rem 0.5rem;
    white-space: nowrap;
    letter-spacing: 0.01em;
    user-select: none;
  }

  @media (max-width: 768px) {
    .activity-counter {
      display: none;
    }
  }

  .tab {
    padding: 0.3rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.25rem;
    background-color: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tab:hover:not(:disabled) {
    background-color: var(--bg-surface);
    border-color: var(--text-secondary);
  }

  .tab:active:not(:disabled) {
    background-color: var(--bg-surface);
  }

  .tab:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .status-badge {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 0.7rem;
    padding: 0.2rem 0.6rem;
    border-radius: 9999px;
    margin-left: auto;
  }

  .status-badge.connecting {
    color: var(--color-connecting);
    background: rgba(137, 180, 250, 0.12);
  }

  .status-badge.waiting {
    color: var(--color-waiting);
    background: rgba(249, 226, 175, 0.12);
  }

  .spinner {
    display: inline-block;
    width: 8px;
    height: 8px;
    border: 1.5px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
    flex-shrink: 0;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Terminal output area */
  .terminal-output {
    flex: 1;
    overflow-y: auto;
    padding: 0.75rem 1rem;
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', monospace;
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-primary);
    background: var(--bg-primary);
  }

  @media (max-width: 768px) {
    /* Leave room for the fixed InputArea bar (~4rem input + safe area) */
    .terminal-output {
      padding-bottom: calc(4rem + env(safe-area-inset-bottom));
    }
    .raw-pane {
      padding-bottom: calc(4rem + env(safe-area-inset-bottom));
    }
  }

  .terminal-output::-webkit-scrollbar {
    width: 6px;
  }

  .terminal-output::-webkit-scrollbar-track {
    background: var(--bg-primary);
  }

  .terminal-output::-webkit-scrollbar-thumb {
    background: var(--border);
    border-radius: 3px;
  }

  /* Raw-mode terminal pane: shows live tmux capture verbatim */
  .raw-pane {
    flex: 1;
    margin: 0;
    padding: 0.75rem 1rem;
    overflow-y: auto;
    overflow-x: auto;
    /* iOS Safari momentum scrolling — required for touch scroll on <pre> */
    -webkit-overflow-scrolling: touch;
    background: #0d1117;
    color: #e6edf3;
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', monospace;
    font-size: 12.5px;
    line-height: 1.45;
    white-space: pre;
    /* Critical for flex children — without this the pane collapses to 0 height */
    min-height: 0;
  }

  .raw-pane.raw-error {
    color: #f38ba8;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .raw-pane::-webkit-scrollbar {
    width: 8px;
    height: 8px;
  }
  .raw-pane::-webkit-scrollbar-track {
    background: #0d1117;
  }
  .raw-pane::-webkit-scrollbar-thumb {
    background: #30363d;
    border-radius: 3px;
  }

  .message {
    white-space: pre-wrap;
    word-break: break-word;
    padding: 0.5rem 0.75rem;
    border-radius: 0.5rem;
    margin-bottom: 0.5rem;
    max-width: 85%;
  }

  .message.sent {
    margin-left: auto;
    background: var(--chat-user-bg, rgba(34, 197, 94, 0.08));
  }

  .message.received {
    background: var(--chat-ai-bg, rgba(59, 130, 246, 0.08));
  }

  .message.system {
    background: var(--chat-system-bg, rgba(107, 114, 128, 0.08));
  }

  .message-sender {
    display: block;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-secondary);
    margin-bottom: 0.25rem;
  }

  .line-content {
    color: var(--text-primary);
  }

  .sent-text {
    color: var(--color-sent);
    font-weight: 500;
  }

  .system-text {
    color: var(--color-system);
    font-style: normal;
    line-height: 1.6;
  }

  .view-mode-group {
    display: flex;
    gap: 0.25rem;
    margin-left: 0.5rem;
    padding-left: 0.5rem;
    border-left: 1px solid var(--border);
  }

  .archive-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0.3rem 0.5rem;
  }

  .tab.active {
    background-color: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }

  .tab.active:hover:not(:disabled) {
    background-color: var(--accent);
    border-color: var(--accent);
    opacity: 0.9;
  }

  .load-more-indicator {
    text-align: center;
    font-size: 0.72rem;
    color: var(--text-secondary);
    opacity: 0.6;
    padding: 0.4rem 0;
    margin-bottom: 0.5rem;
    cursor: pointer;
    user-select: none;
    letter-spacing: 0.02em;
    transition: opacity 0.15s;
  }

  .load-more-indicator:hover {
    opacity: 1;
  }

  .terminal-empty {
    color: var(--text-secondary);
    font-style: italic;
    padding: 0.5rem 0;
  }

  .waiting-summary {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font-style: normal;
    font-size: 0.8rem;
    color: var(--text-secondary);
    animation: waiting-pulse 2.5s ease-in-out infinite;
  }

  @keyframes waiting-pulse {
    0%, 100% { opacity: 0.85; }
    50% { opacity: 0.45; }
  }

  .empty-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-primary);
    color: var(--text-secondary);
    font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
    font-size: 0.875rem;
  }

  .scroll-button {
    position: absolute;
    bottom: 1rem;
    right: 1rem;
    width: 2.25rem;
    height: 2.25rem;
    border: none;
    border-radius: 50%;
    background-color: var(--color-scroll-btn);
    color: var(--color-scroll-btn-text);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3);
    transition: all 0.2s;
  }

  .scroll-button:hover {
    background-color: var(--color-scroll-btn-hover);
    box-shadow: 0 6px 8px rgba(0, 0, 0, 0.4);
  }

  :global(.code-block) {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 0.25rem;
    padding: 0.5rem 0.75rem;
    margin: 0.375rem 0;
    overflow-x: auto;
    white-space: pre;
  }

  :global(.code-block code) {
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', monospace;
    font-size: 12px;
    color: var(--text-primary);
  }

  :global(.chat-table) {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
    margin: 0.5rem 0;
  }

  :global(.chat-table th),
  :global(.chat-table td) {
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--border, #ddd);
    text-align: left;
  }

  :global(.chat-table th) {
    background: var(--bg-surface, rgba(0,0,0,0.05));
    font-weight: 600;
  }

  :global(.chat-table tr:nth-child(even)) {
    background: var(--bg-surface, rgba(0,0,0,0.02));
  }

  :global(.chat-list) {
    margin: 0.25rem 0;
    padding-left: 1.5rem;
  }
  :global(.chat-list li) {
    margin: 0.15rem 0;
  }
  :global(.inline-code) {
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', monospace;
    font-size: 0.85em;
    padding: 0.1rem 0.3rem;
    background: var(--bg-secondary, rgba(0,0,0,0.05));
    border-radius: 0.2rem;
  }
  :global(.chat-h1),
  :global(.chat-h2),
  :global(.chat-h3) {
    margin: 0.5rem 0 0.25rem 0;
    font-weight: 600;
    line-height: 1.3;
  }
  :global(.chat-h1) { font-size: 1.15rem; }
  :global(.chat-h2) { font-size: 1.05rem; }
  :global(.chat-h3) { font-size: 0.95rem; }
  :global(.chat-selector) {
    margin: 0.25rem 0;
    font-family: monospace;
  }
  :global(.selector-item) {
    padding: 0.2rem 0.5rem;
    border-radius: 0.25rem;
  }
  :global(.selector-item.selected) {
    background: rgba(59, 130, 246, 0.15);
    font-weight: 600;
  }

  .activity-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #10b981;
    animation: pulse 1.5s ease-in-out infinite;
    flex-shrink: 0;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .status-badge.active {
    color: #10b981;
    background: rgba(16, 185, 129, 0.1);
  }

  .llm-banner {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    background: #fef3c7;
    color: #92400e;
    font-size: 0.8rem;
    border-bottom: 1px solid #fcd34d;
    flex-wrap: wrap;
  }

  .llm-banner button {
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
    border: 1px solid #d97706;
    background: white;
    color: #92400e;
    cursor: pointer;
    font-size: 0.75rem;
  }
</style>
