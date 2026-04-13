<script lang="ts">
  import { messages, currentSession, addMessageToSession, updateMessageContent, clearSessionMessages } from '../stores/app';
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowDown, Terminal } from 'lucide-svelte';

  let terminalEl: HTMLDivElement;
  let autoScroll = true;
  let showScrollButton = false;
  let isActionLoading = false;
  let connecting = false;
  let waitingForResponse = false;
  let waitingTimer: number;
  let streamingMessageId: string | null = null;

  function scrollToBottom() {
    if (terminalEl) {
      terminalEl.scrollTop = terminalEl.scrollHeight;
      autoScroll = true;
      showScrollButton = false;
    }
  }

  function handleScroll() {
    if (!terminalEl) return;
    const { scrollTop, scrollHeight, clientHeight } = terminalEl;
    const atBottom = scrollHeight - scrollTop - clientHeight < 50;
    autoScroll = atBottom;
    showScrollButton = !atBottom;
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

  function renderContent(content: string): string {
    return content.replace(/```(\w*)\n([\s\S]*?)```/g,
      '<pre class="code-block"><code>$2</code></pre>');
  }

  type Segment = { type: 'prompt' | 'output' | 'tool'; content: string };

  function isUiChrome(line: string): boolean {
    // Box drawing characters, status bars, empty decorations
    return /^[─│╭╮╰╯┌┐└┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬]+$/.test(line)
      || /^\s*$/.test(line)
      || line.includes('bypass permissions')
      || line.includes('[r@')  // status bar fragment
      || /^\s*⏵/.test(line);  // mode indicator
  }

  function parseTerminalOutput(raw: string): Segment[] {
    const lines = raw.split('\n');
    const segments: Segment[] = [];
    let currentBlock: string[] = [];

    for (const line of lines) {
      const trimmed = line.trim();

      // Skip empty lines and UI chrome
      if (!trimmed) continue;
      if (isUiChrome(trimmed)) continue;

      // Detect Claude Code prompt markers
      if (trimmed.startsWith('❯') || trimmed.startsWith('>') || trimmed.match(/^\$\s/)) {
        if (currentBlock.length) {
          segments.push({ type: 'output', content: currentBlock.join('\n') });
          currentBlock = [];
        }
        segments.push({ type: 'prompt', content: trimmed });
      }
      // Detect tool use markers
      else if (trimmed.startsWith('⏺') || trimmed.includes('Tool:') || trimmed.match(/^\s*(Read|Write|Edit|Bash|Glob|Grep)\(/)) {
        if (currentBlock.length) {
          segments.push({ type: 'output', content: currentBlock.join('\n') });
          currentBlock = [];
        }
        segments.push({ type: 'tool', content: trimmed });
      }
      else {
        currentBlock.push(trimmed);
      }
    }

    if (currentBlock.length) {
      segments.push({ type: 'output', content: currentBlock.join('\n') });
    }

    return segments;
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

  async function handleStatus() {
    if (!$currentSession || isActionLoading) return;
    const sessionName = $currentSession.name;
    isActionLoading = true;

    try {
      await invoke('send_message', { content: '/status' });
      addMessageToSession(sessionName, {
        direction: 'sent',
        content: '/status',
        timestamp: new Date(),
      });
    } catch (err) {
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Failed to send status command: ${err}`,
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

  async function handleOpenIterm() {
    if (!$currentSession) return;
    try {
      await invoke('open_in_iterm', { sessionName: $currentSession.name });
    } catch (err) {
      addMessageToSession($currentSession.name, {
        direction: 'system',
        content: `Failed to open iTerm2: ${err}`,
        timestamp: new Date(),
      });
    }
  }

  onMount(() => {
    connecting = true;

    const unlistenSessionOutput = listen('session-output', (event: any) => {
      connecting = false;
      markActivity();

      const { content, full_content } = event.payload;
      if (!$currentSession) return;

      const sessionName = $currentSession.name;
      const raw = content && content.length > 0 ? content : full_content;
      if (!raw) return;

      const segments = parseTerminalOutput(raw);
      for (const seg of segments) {
        addMessageToSession(sessionName, {
          direction: 'received',
          content: seg.content,
          timestamp: new Date(),
          segmentType: seg.type,
        });
      }

      if (autoScroll) {
        setTimeout(scrollToBottom, 10);
      }
    });

    const unlistenChatEvent = listen('chat-event', (event: any) => {
      connecting = false;
      markActivity();

      const { type, content, accumulated, name, cost_usd, input } = event.payload;
      const sessionName = $currentSession?.name;
      if (!sessionName) return;

      switch (type) {
        case 'text':
          updateStreamingMessage(sessionName, accumulated);
          break;
        case 'tool_use':
          addMessageToSession(sessionName, {
            direction: 'system',
            content: `Using tool: ${name}`,
            timestamp: new Date(),
          });
          break;
        case 'complete':
          finalizeStreamingMessage(sessionName, content, cost_usd);
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
    };
  });

  // Reset connecting state when session changes
  $: if ($currentSession) {
    connecting = true;
    waitingForResponse = false;
    window.setTimeout(() => { connecting = false; }, 2000);
  } else {
    connecting = false;
    waitingForResponse = false;
  }

  $: if ($messages.length && autoScroll) {
    setTimeout(scrollToBottom, 10);
  }
</script>

<div class="chat-view">
  {#if !$currentSession}
    <div class="empty-state">
      <p>Select a session to start chatting</p>
    </div>
  {:else}
    <div class="session-actions">
      <button
        class="tab"
        on:click={handleStatus}
        disabled={isActionLoading}
        title="Send /status command"
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
    </div>

    <div
      bind:this={terminalEl}
      on:scroll={handleScroll}
      class="terminal-output"
    >
      {#each $messages as message}
        {#if message.direction === 'sent'}
          <div class="terminal-line sent">
            <span class="line-prefix">&gt; </span><span class="line-content sent-text">{message.content}</span>
          </div>
        {:else if message.direction === 'system'}
          <div class="terminal-line system">
            <span class="line-prefix">[ </span><span class="line-content system-text">{message.content}</span><span class="line-prefix"> ]</span>
          </div>
        {:else if message.segmentType === 'prompt'}
          <div class="terminal-line seg-prompt">
            <span class="seg-prompt-prefix">❯</span>
            <span class="line-content seg-prompt-text">{message.content.replace(/^[❯>]\s*/, '')}</span>
          </div>
        {:else if message.segmentType === 'tool'}
          <div class="terminal-line seg-tool">
            <span class="seg-tool-prefix">⏺</span>
            <span class="line-content seg-tool-text">{message.content.replace(/^⏺\s*/, '')}</span>
          </div>
        {:else}
          <div class="terminal-line received">
            <span class="line-content">{@html renderContent(message.content)}</span>
          </div>
        {/if}
      {:else}
        <div class="terminal-empty">
          <span>No output yet — send a message or wait for session output…</span>
        </div>
      {/each}
    </div>

    {#if showScrollButton}
      <button class="scroll-button" on:click={scrollToBottom} aria-label="Scroll to bottom">
        <ArrowDown size={20} />
      </button>
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
  }

  .session-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
    background-color: var(--bg-secondary);
    flex-shrink: 0;
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

  .iterm-tab {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    margin-left: auto;
    color: var(--accent);
    border-color: var(--accent);
  }

  .iterm-tab:hover {
    background: var(--accent);
    color: white;
    border-color: var(--accent);
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

  .terminal-line {
    white-space: pre-wrap;
    word-break: break-word;
    margin: 0;
    padding: 0;
  }

  .terminal-line + .terminal-line {
    margin-top: 0.125rem;
  }

  .line-prefix {
    color: var(--text-secondary);
    user-select: none;
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
    font-style: italic;
  }

  .terminal-empty {
    color: var(--text-secondary);
    font-style: italic;
    padding: 0.5rem 0;
  }

  /* Segment: prompt line (cyan, ❯ prefix) */
  .seg-prompt {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    margin-top: 0.5rem;
  }

  .seg-prompt-prefix {
    color: var(--color-prompt, #89dceb);
    user-select: none;
    flex-shrink: 0;
  }

  .seg-prompt-text {
    color: var(--color-prompt, #89dceb);
    font-weight: 500;
  }

  /* Segment: tool use line (indigo/accent, ⏺ prefix) */
  .seg-tool {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    margin-top: 0.25rem;
    opacity: 0.85;
  }

  .seg-tool-prefix {
    color: var(--accent, #6366f1);
    user-select: none;
    flex-shrink: 0;
  }

  .seg-tool-text {
    color: var(--accent, #6366f1);
    font-size: 0.8rem;
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
</style>
