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
  let isDisabled = false;

  $: isDisabled = !$currentSession;
  $: isSlashCommand = input.trim().startsWith('/') && !input.trim().startsWith('/send ');

  async function handleSlashCommand(command: string) {
    const sessionName = $currentSession?.name;
    if (!sessionName) return;

    const parts = command.split(' ');
    const cmd = parts[0].toLowerCase();
    const args = parts.slice(1).join(' ');

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
            .map(s => `  ${s.name.replace(/^commander-/, '')}${s.is_connected ? ' (connected)' : ''}`)
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
          if (confirm(`Stop session "${sessionName.replace(/^commander-/, '')}"? This cannot be undone.`)) {
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

    if (!$currentSession) {
      return;
    }

    const content = input.trim();
    const sessionName = $currentSession.name;
    input = '';

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
    placeholder={isDisabled ? "Select a session first..." : "Type message or /help for commands..."}
    disabled={isDisabled}
    class="input-field"
    class:slash-command={isSlashCommand}
  />
  <button
    on:click={sendMessage}
    disabled={isDisabled || !input.trim()}
    class="send-button"
  >
    <Send size={20} />
  </button>
</div>

<style>
  .input-area {
    display: flex;
    gap: 0.75rem;
    padding: 1rem;
    border-top: 1px solid #e5e7eb;
    background-color: white;
  }

  .input-field {
    flex: 1;
    padding: 0.75rem 1rem;
    border: 1px solid #d1d5db;
    border-radius: 0.5rem;
    font-size: 0.875rem;
    outline: none;
    transition: border-color 0.2s;
  }

  .input-field:focus {
    border-color: #3b82f6;
  }

  .input-field:disabled {
    background-color: #f9fafb;
    color: #9ca3af;
    cursor: not-allowed;
  }

  .input-field.slash-command {
    border-color: #8b5cf6;
    background-color: #faf5ff;
  }

  .send-button {
    padding: 0.75rem 1rem;
    border: none;
    border-radius: 0.5rem;
    background-color: #3b82f6;
    color: white;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.2s;
  }

  .send-button:hover:not(:disabled) {
    background-color: #2563eb;
  }

  .send-button:disabled {
    background-color: #d1d5db;
    cursor: not-allowed;
  }
</style>
