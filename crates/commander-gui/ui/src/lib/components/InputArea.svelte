<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { messages, currentSession } from '../stores/app';
  import { Send } from 'lucide-svelte';

  let input = '';
  let isDisabled = false;

  $: isDisabled = !$currentSession;

  async function sendMessage() {
    if (!input.trim() || isDisabled) return;

    if (!$currentSession) {
      messages.update(m => [...m, {
        direction: 'system',
        content: 'Error: Not connected to a session. Please select a session first.',
        timestamp: new Date(),
      }]);
      return;
    }

    const content = input.trim();
    input = '';

    try {
      await invoke('send_message', { content });
      messages.update(m => [...m, {
        direction: 'sent',
        content,
        timestamp: new Date(),
      }]);
    } catch (err) {
      messages.update(m => [...m, {
        direction: 'system',
        content: `Failed to send message: ${err}`,
        timestamp: new Date(),
      }]);
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
    placeholder={isDisabled ? "Select a session first..." : "Type message..."}
    disabled={isDisabled}
    class="input-field"
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
