<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import {
    addMessageToSession,
    currentSession,
    sessions,
    clearSessionMessages
  } from '../stores/app';
  import { Send } from 'lucide-svelte';

  let input = '';
  let isSending = false;

  $: isDisabled = !$currentSession || isSending;
  $: isSlashCommand = input.trim().startsWith('/') && !input.trim().startsWith('/send ');

  async function handleSlashCommand(command: string) {
    const sessionName = $currentSession?.name;
    if (!sessionName) return;

    const parts = command.split(' ');
    const cmd = parts[0].toLowerCase();

    try {
      switch (cmd) {
        case '/status':
          await invoke('send_message', { content: '/status' });
          addMessageToSession(sessionName, {
            direction: 'system',
            content: 'Sent status command',
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
  /status - Send status command
  /list - List all sessions
  /disconnect - Disconnect from session
  /stop - Stop this session
  /clear - Clear message history
  /help - Show this help
  /send <text> - Send literal text (bypass interpreter)`,
            timestamp: new Date(),
          });
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
    if (!input.trim() || isDisabled) return;
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

<div class="input-area">
  <input
    type="text"
    bind:value={input}
    on:keydown={handleKeydown}
    placeholder={
      !$currentSession
        ? 'Select a session first…'
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
    disabled={isDisabled || !input.trim()}
    class="send-button"
    class:loading={isSending}
    aria-label={isSending ? 'Sending…' : 'Send message'}
  >
    <Send size={18} />
  </button>
</div>

<style>
  .input-area {
    display: flex;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-top: 1px solid #313244;
    background-color: #181825;
  }

  .input-field {
    flex: 1;
    padding: 0.625rem 0.875rem;
    border: 1px solid #45475a;
    border-radius: 0.375rem;
    font-size: 0.875rem;
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Consolas', monospace;
    background: #1e1e2e;
    color: #cdd6f4;
    outline: none;
    transition: border-color 0.15s, background 0.15s;
  }

  .input-field::placeholder {
    color: #6c7086;
  }

  .input-field:focus {
    border-color: #89b4fa;
  }

  .input-field:disabled {
    background-color: #181825;
    color: #585b70;
    cursor: not-allowed;
  }

  .input-field.slash-command {
    border-color: #cba6f7;
    background-color: rgba(203, 166, 247, 0.06);
    color: #cba6f7;
  }

  .input-field.sending {
    opacity: 0.7;
    cursor: wait;
  }

  .send-button {
    padding: 0.625rem 0.875rem;
    border: none;
    border-radius: 0.375rem;
    background-color: #89b4fa;
    color: #1e1e2e;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s;
    flex-shrink: 0;
  }

  .send-button:hover:not(:disabled) {
    background-color: #b4befe;
  }

  .send-button:disabled {
    background-color: #313244;
    color: #6c7086;
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
