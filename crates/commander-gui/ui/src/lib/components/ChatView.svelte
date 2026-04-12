<script lang="ts">
  import { messages, currentSession, addMessageToSession, clearSessionMessages } from '../stores/app';
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowDown } from 'lucide-svelte';

  let terminalEl: HTMLDivElement;
  let autoScroll = true;
  let showScrollButton = false;
  let isActionLoading = false;
  let connecting = false;
  let waitingForResponse = false;
  let waitingTimer: number;

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

  onMount(() => {
    connecting = true;

    const unlistenPromise = listen('session-output', (event: any) => {
      connecting = false;
      markActivity();

      const { content, full_content } = event.payload;
      if (!$currentSession) return;

      const sessionName = $currentSession.name;

      if (content && content.length > 0) {
        addMessageToSession(sessionName, {
          direction: 'received',
          content,
          timestamp: new Date(),
        });
      } else if (full_content) {
        addMessageToSession(sessionName, {
          direction: 'received',
          content: full_content,
          timestamp: new Date(),
        });
      }

      if (autoScroll) {
        setTimeout(scrollToBottom, 10);
      }
    });

    // Stop the "connecting" spinner after a short grace period even if
    // no output arrives immediately (avoids showing it on session switch).
    const connectingTimer = window.setTimeout(() => {
      connecting = false;
    }, 2000);

    return () => {
      unlistenPromise.then(f => f());
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
        {:else}
          <div class="terminal-line received">
            <span class="line-content">{message.content}</span>
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
    background-color: #1e1e2e;
    overflow: hidden;
  }

  .session-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #313244;
    background-color: #181825;
    flex-shrink: 0;
  }

  .tab {
    padding: 0.3rem 0.75rem;
    border: 1px solid #45475a;
    border-radius: 0.25rem;
    background-color: #181825;
    color: #cdd6f4;
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tab:hover:not(:disabled) {
    background-color: #313244;
    border-color: #6c7086;
  }

  .tab:active:not(:disabled) {
    background-color: #45475a;
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
    color: #89b4fa;
    background: rgba(137, 180, 250, 0.12);
  }

  .status-badge.waiting {
    color: #f9e2af;
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
    color: #cdd6f4;
    background: #1e1e2e;
  }

  .terminal-output::-webkit-scrollbar {
    width: 6px;
  }

  .terminal-output::-webkit-scrollbar-track {
    background: #1e1e2e;
  }

  .terminal-output::-webkit-scrollbar-thumb {
    background: #45475a;
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
    color: #6c7086;
    user-select: none;
  }

  .line-content {
    color: #cdd6f4;
  }

  .sent-text {
    color: #89dceb;
    font-weight: 500;
  }

  .system-text {
    color: #a6e3a1;
    font-style: italic;
  }

  .terminal-empty {
    color: #6c7086;
    font-style: italic;
    padding: 0.5rem 0;
  }

  .empty-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #1e1e2e;
    color: #6c7086;
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
    background-color: #89b4fa;
    color: #1e1e2e;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3);
    transition: all 0.2s;
  }

  .scroll-button:hover {
    background-color: #b4befe;
    box-shadow: 0 6px 8px rgba(0, 0, 0, 0.4);
  }
</style>
