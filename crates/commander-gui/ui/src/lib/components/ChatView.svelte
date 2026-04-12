<script lang="ts">
  import { messages, currentSession, addMessageToSession, clearSessionMessages } from '../stores/app';
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowDown } from 'lucide-svelte';

  let chatContainer: HTMLDivElement;
  let autoScroll = true;
  let showScrollButton = false;
  let isActionLoading = false;

  function scrollToBottom() {
    if (chatContainer) {
      chatContainer.scrollTop = chatContainer.scrollHeight;
      autoScroll = true;
      showScrollButton = false;
    }
  }

  function handleScroll() {
    if (!chatContainer) return;
    const { scrollTop, scrollHeight, clientHeight } = chatContainer;
    const atBottom = scrollHeight - scrollTop - clientHeight < 50;
    autoScroll = atBottom;
    showScrollButton = !atBottom;
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
    const unlisten = listen('session-output', (event: any) => {
      const { output } = event.payload;
      if ($currentSession) {
        addMessageToSession($currentSession.name, {
          direction: 'received',
          content: output,
          timestamp: new Date(),
        });
      }

      if (autoScroll) {
        setTimeout(scrollToBottom, 10);
      }
    });

    return () => unlisten.then(f => f());
  });

  $: if ($messages.length && autoScroll) {
    setTimeout(scrollToBottom, 10);
  }
</script>

<div class="chat-view">
  {#if !$currentSession}
    <div class="empty-state">
      <p class="text-gray-500">Select a session to start chatting</p>
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
    </div>

    <div
      bind:this={chatContainer}
      on:scroll={handleScroll}
      class="messages"
    >
      {#each $messages as message}
        <div class="message {message.direction}">
          <div class="content">{message.content}</div>
          <div class="timestamp">
            {message.timestamp.toLocaleTimeString()}
          </div>
        </div>
      {:else}
        <div class="empty-state">
          <p class="text-gray-500">No messages yet</p>
        </div>
      {/each}
    </div>

    {#if showScrollButton}
      <button class="scroll-button" on:click={scrollToBottom}>
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
    background-color: white;
    overflow: hidden;
  }

  .session-actions {
    display: flex;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid #e5e7eb;
    background-color: #f9fafb;
  }

  .tab {
    padding: 0.5rem 1rem;
    border: 1px solid #d1d5db;
    border-radius: 0.375rem;
    background-color: white;
    color: #374151;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
  }

  .tab:hover:not(:disabled) {
    background-color: #f3f4f6;
    border-color: #9ca3af;
  }

  .tab:active:not(:disabled) {
    background-color: #e5e7eb;
  }

  .tab:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .message {
    display: flex;
    flex-direction: column;
    max-width: 70%;
    padding: 0.75rem 1rem;
    border-radius: 0.75rem;
    word-wrap: break-word;
  }

  .message.sent {
    align-self: flex-end;
    background-color: #3b82f6;
    color: white;
  }

  .message.received {
    align-self: flex-start;
    background-color: #f3f4f6;
    color: #1f2937;
  }

  .message.system {
    align-self: center;
    background-color: #fef3c7;
    color: #92400e;
    max-width: 80%;
  }

  .content {
    font-size: 0.875rem;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .timestamp {
    font-size: 0.75rem;
    opacity: 0.7;
    margin-top: 0.25rem;
  }

  .empty-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 2rem;
  }

  .scroll-button {
    position: absolute;
    bottom: 1rem;
    right: 1rem;
    width: 2.5rem;
    height: 2.5rem;
    border: none;
    border-radius: 50%;
    background-color: #3b82f6;
    color: white;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
    transition: all 0.2s;
  }

  .scroll-button:hover {
    background-color: #2563eb;
    box-shadow: 0 6px 8px rgba(0, 0, 0, 0.15);
  }
</style>
