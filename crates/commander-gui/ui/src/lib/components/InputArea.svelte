<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import {
    addMessageToSession,
    currentSession,
    sessions,
    clearSessionMessages,
    serverRebuilding,
    activityCounter
  } from '../stores/app';
  import { Send } from 'lucide-svelte';

  let input = '';
  let isSending = false;

  $: isDisabled = !$currentSession || isSending;
  $: canSend = !isDisabled && !$serverRebuilding;
  $: isConnected = !!$currentSession?.is_connected;
  $: isSlashCommand = input.trim().startsWith('/') && !input.trim().startsWith('/send ') && !input.trim().startsWith('/aic:send ');

  async function handleSlashCommand(command: string) {
    const sessionName = $currentSession?.name;
    if (!sessionName) return;

    const parts = command.split(' ');
    const cmd = parts[0].toLowerCase();

    try {
      switch (cmd) {
        case '/status':
          addMessageToSession(sessionName, {
            direction: 'system',
            content: `Session: ${sessionName}\nStatus: Connected\nAdapter: ${$currentSession?.name || 'unknown'}`,
            timestamp: new Date(),
          });
          break;

        case '/list':
          const sessionList = $sessions
            .map(s => `  ${s.name}${s.is_connected ? ' (connected)' : ''}`)
            .join('\n');
          addMessageToSession(sessionName, {
            direction: 'system',
            content: `Available sessions:\n${sessionList}`,
            timestamp: new Date(),
          });
          break;

        case '/disconnect':
          await invoke('disconnect_session');
          addMessageToSession(sessionName, {
            direction: 'system',
            content: 'Disconnected from session',
            timestamp: new Date(),
          });
          currentSession.set(null);
          break;

        case '/stop':
          if (confirm(`Stop session "${sessionName}"? This cannot be undone.`)) {
            await invoke('stop_session', { name: sessionName });
            clearSessionMessages(sessionName);
            currentSession.set(null);
          }
          break;

        case '/clear':
          clearSessionMessages(sessionName);
          addMessageToSession(sessionName, {
            direction: 'system',
            content: 'Messages cleared',
            timestamp: new Date(),
          });
          break;

        case '/help':
          addMessageToSession(sessionName, {
            direction: 'system',
            content: `Available commands:
  /send <text> - Send literal text to tmux (bypasses command handler)
  /status - Show session status
  /list - List all sessions
  /disconnect - Disconnect from session
  /stop - Stop this session
  /clear - Clear message history
  /iterm - Open session in iTerm2
  /help - Show this help`,
            timestamp: new Date(),
          });
          break;

        case '/iterm':
          try {
            await invoke('open_in_iterm', { sessionName });
            addMessageToSession(sessionName, {
              direction: 'system',
              content: 'Opening session in iTerm2...',
              timestamp: new Date(),
            });
          } catch (e) {
            addMessageToSession(sessionName, {
              direction: 'system',
              content: `Failed to open iTerm2: ${e}`,
              timestamp: new Date(),
            });
          }
          break;

        default:
          addMessageToSession(sessionName, {
            direction: 'system',
            content: `Unknown command: ${cmd}. Type /help for available commands.`,
            timestamp: new Date(),
          });
      }
    } catch (err) {
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Command failed: ${err}`,
        timestamp: new Date(),
      });
    }
  }

  async function sendMessage() {
    if (!input.trim() || !canSend) return;
    if (!$currentSession) return;

    const content = input.trim();
    const sessionName = $currentSession.name;
    input = '';
    isSending = true;

    try {
      if (content.startsWith('/')) {
        if (content.startsWith('/send ')) {
          const actualContent = content.substring(6);
          await invoke('send_message', { content: actualContent });
          addMessageToSession(sessionName, {
            direction: 'sent',
            content: actualContent,
            timestamp: new Date(),
          });
        } else if (content.startsWith('/aic:send ')) {
          // Legacy alias — kept for backwards compatibility
          const actualContent = content.substring(10);
          await invoke('send_message', { content: actualContent });
          addMessageToSession(sessionName, {
            direction: 'sent',
            content: actualContent,
            timestamp: new Date(),
          });
        } else {
          await handleSlashCommand(content);
        }
      } else {
        await invoke('send_message', { content });
        addMessageToSession(sessionName, {
          direction: 'sent',
          content,
          timestamp: new Date(),
        });
      }
    } catch (err) {
      addMessageToSession(sessionName, {
        direction: 'system',
        content: `Error: ${err}`,
        timestamp: new Date(),
      });
      input = content;
    } finally {
      isSending = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }
</script>

<div class="input-area-wrap">
  <!--
    Why: The chars/lines diagnostic counter previously lived in ChatView's
    session-actions toolbar where it was easy to miss. Moving it directly
    above the input field gives a peripheral signal of incoming traffic
    while the user is typing/sending.
    What: Renders running totals from the shared `activityCounter` store.
    Hidden when no session is connected or no data has arrived yet.
    Test: Connect a session, send/receive data, assert the line appears
    with chars/lines totals; disconnect and assert it disappears.
  -->
  {#if $currentSession && $activityCounter.chars > 0}
    <div class="activity-counter" title="Bytes / lines received since connect">
      ↓ {$activityCounter.chars.toLocaleString()} chars · {$activityCounter.lines.toLocaleString()} lines
    </div>
  {/if}
  <div class="input-area" class:connected={isConnected}>
    <input
    type="text"
    bind:value={input}
    on:keydown={handleKeydown}
    placeholder={
      !$currentSession
        ? 'Select a session first…'
        : $serverRebuilding
          ? 'Server rebuilding, please wait...'
          : isSending
            ? 'Sending…'
            : 'Type message or /help for commands…'
    }
    disabled={isDisabled}
    class="input-field"
    class:slash-command={isSlashCommand}
    class:sending={isSending}
  />
  <button
    on:click={sendMessage}
    disabled={!canSend || !input.trim()}
    class="send-button"
    class:loading={isSending}
    aria-label={isSending ? 'Sending…' : 'Send message'}
  >
    <Send size={18} />
  </button>
  </div>
</div>

<style>
  /*
   * Wrapper hosts the activity counter line above the actual input row.
   * Using a flex column lets the counter sit flush against the top border
   * without affecting the input row's existing layout / padding.
   */
  .input-area-wrap {
    display: flex;
    flex-direction: column;
    width: 100%;
    box-sizing: border-box;
  }

  /*
   * Diagnostic chars/lines counter — small, muted, monospace. Sits directly
   * above the input field so peripheral activity is visible while typing.
   */
  .activity-counter {
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', monospace;
    font-size: 0.75rem;
    color: #888;
    padding: 0.25rem 1rem 0;
    background-color: var(--bg-secondary);
    border-top: 1px solid var(--border);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: 0.01em;
    user-select: none;
  }

  /*
   * When the counter is visible, the input-area below it should not draw a
   * second top border (the counter already provides one). Use sibling combinator.
   */
  .activity-counter + .input-area {
    border-top: none;
  }

  @media (max-width: 768px) {
    .activity-counter {
      font-size: 0.68rem;
      padding: 0.2rem 0.6rem 0;
    }
  }

  .input-area {
    display: flex;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-top: 1px solid var(--border);
    background-color: var(--bg-secondary);
    /* Ensure nothing bleeds off-screen on any viewport */
    width: 100%;
    box-sizing: border-box;
    overflow: hidden;
  }

  @media (max-width: 768px) {
    .input-area {
      /* iPhone notch / home bar safe area. Stays in natural flex flow
         (parent .main-panel is flex column with overflow hidden), so the
         InputArea sits at the bottom without fixed positioning — which
         otherwise took it out of flow and pushed the send button off-screen. */
      padding-bottom: calc(0.75rem + env(safe-area-inset-bottom));
    }
  }

  /*
   * Connected state: subtle animated green ring around the entire input
   * container — same green (#22c55e) as the session list pulse dot.
   * The ring pulses between a tight 2 px glow and a slightly looser fade so
   * it's peripherally noticeable without being distracting.
   * Transitions in/out smoothly via box-shadow animation.
   * Test: Connect to a session, assert the input-area has box-shadow with
   * rgba(34,197,94,…); disconnect, assert it returns to `none`.
   */
  .input-area.connected {
    animation: input-ring-pulse 2s ease-in-out infinite;
  }

  @keyframes input-ring-pulse {
    0%, 100% { box-shadow: 0 0 0 2px rgba(34, 197, 94, 0.35); }
    50%       { box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.12); }
  }

  .input-field {
    flex: 1;
    padding: 0.625rem 0.875rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    font-size: 0.875rem;
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', monospace;
    background: var(--bg-primary);
    color: var(--text-primary);
    outline: none;
    transition: border-color 0.15s, background 0.15s;
  }

  .input-field::placeholder {
    color: var(--text-secondary);
  }

  .input-field:focus {
    border-color: var(--accent);
  }

  .input-field:disabled {
    background-color: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: not-allowed;
    opacity: 0.6;
  }

  .input-field.slash-command {
    border-color: var(--accent);
    background-color: rgba(99, 102, 241, 0.06);
    color: var(--accent);
  }

  .input-field.sending {
    opacity: 0.7;
    cursor: wait;
  }

  .send-button {
    padding: 0.625rem 0.875rem;
    border: none;
    border-radius: 0.375rem;
    background-color: var(--accent);
    color: white;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s;
    flex-shrink: 0;
  }

  .send-button:hover:not(:disabled) {
    filter: brightness(1.15);
  }

  .send-button:disabled {
    background-color: var(--bg-surface);
    color: var(--text-secondary);
    cursor: not-allowed;
  }

  .send-button.loading {
    opacity: 0.7;
    cursor: wait;
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 0.7; }
    50% { opacity: 1; }
  }
</style>
