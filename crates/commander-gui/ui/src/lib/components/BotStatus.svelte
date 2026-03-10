<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { botRunning, botPid } from '../stores/app';
  import { Power, Link, Check, X, Copy, CheckCheck } from 'lucide-svelte';

  interface TelegramConnection {
    connected: boolean;
    chat_ids: number[];
    count: number;
  }

  let interval: number;
  let connectionInterval: number;
  let showPairingModal = false;
  let pairingCode = '';
  let generatingCode = false;
  let telegramConnection: TelegramConnection | null = null;
  let checkingConnection = false;
  let copied = false;
  let starting = false;
  let stopping = false;

  async function checkStatus() {
    try {
      const status: any = await invoke('get_bot_status');
      botRunning.set(status.running);
      botPid.set(status.pid);
    } catch (err) {
      console.error('Failed to check bot status:', err);
    }
  }

  async function checkTelegramConnection() {
    if (!$botRunning) {
      telegramConnection = null;
      return;
    }

    checkingConnection = true;
    try {
      telegramConnection = await invoke('check_telegram_connection');
    } catch (err) {
      console.error('Failed to check telegram connection:', err);
      telegramConnection = null;
    } finally {
      checkingConnection = false;
    }
  }

  async function startBot() {
    starting = true;
    try {
      const info: any = await invoke('start_bot');
      botRunning.set(info.running);
      botPid.set(info.pid);

      // Check connection after starting
      setTimeout(checkTelegramConnection, 2000);
    } catch (err) {
      alert(`Failed to start bot: ${err}`);
    } finally {
      starting = false;
    }
  }

  async function stopBot() {
    stopping = true;
    try {
      await invoke('stop_bot');
      botRunning.set(false);
      botPid.set(null);
      telegramConnection = null;
    } catch (err) {
      alert(`Failed to stop bot: ${err}`);
    } finally {
      stopping = false;
    }
  }

  async function generatePairingCode() {
    if (!$botRunning) {
      alert('Please start the bot first');
      return;
    }

    generatingCode = true;
    try {
      pairingCode = await invoke('generate_pairing_code');
      showPairingModal = true;
    } catch (err) {
      alert(`Failed to generate pairing code: ${err}`);
    } finally {
      generatingCode = false;
    }
  }

  function closePairingModal() {
    showPairingModal = false;
    pairingCode = '';
    copied = false;

    // Check if user connected after closing modal
    setTimeout(checkTelegramConnection, 1000);
  }

  async function copyPairingCommand() {
    const command = `/pair ${pairingCode}`;
    try {
      await navigator.clipboard.writeText(command);
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
      alert('Failed to copy to clipboard');
    }
  }

  onMount(() => {
    checkStatus();
    checkTelegramConnection();

    interval = window.setInterval(checkStatus, 5000);
    connectionInterval = window.setInterval(checkTelegramConnection, 10000);

    return () => {
      clearInterval(interval);
      clearInterval(connectionInterval);
    };
  });
</script>

<div class="bot-status">
  <div class="status-row">
    <div class="status-indicator">
      <Power
        size={16}
        class={$botRunning ? 'text-green-500' : 'text-gray-400'}
      />
      <span class="status-text">
        Bot {$botRunning ? 'Running' : 'Stopped'}
      </span>
      {#if $botPid}
        <span class="pid">(PID: {$botPid})</span>
      {/if}
    </div>

    <div class="connection-status" class:visible={$botRunning && telegramConnection}>
      {#if $botRunning && telegramConnection}
        {#if telegramConnection.connected}
          <Check size={14} class="text-green-500" />
          <span class="connected">Connected</span>
          <span class="count">({telegramConnection.count} chat{telegramConnection.count !== 1 ? 's' : ''})</span>
        {:else}
          <X size={14} class="text-gray-400" />
          <span class="not-connected">Not connected</span>
        {/if}
      {/if}
    </div>
  </div>

  <div class="controls">
    <button
      on:click={startBot}
      disabled={$botRunning || starting}
      class="control-button start"
      class:loading={starting}
    >
      {starting ? 'Starting...' : 'Start'}
    </button>
    <button
      on:click={stopBot}
      disabled={!$botRunning || stopping}
      class="control-button stop"
      class:loading={stopping}
    >
      {stopping ? 'Stopping...' : 'Stop'}
    </button>
    <button
      on:click={generatePairingCode}
      disabled={!$botRunning || generatingCode}
      class="control-button pair"
    >
      <Link size={14} />
      {generatingCode ? 'Generating...' : 'Pair'}
    </button>
  </div>
</div>

{#if showPairingModal}
  <div class="modal-overlay" on:click={closePairingModal} role="presentation">
    <div class="modal-content" on:click|stopPropagation role="dialog" aria-modal="true" aria-labelledby="pairing-modal-title">
      <div class="modal-header">
        <h2 id="pairing-modal-title">Telegram Pairing Code</h2>
        <button class="close-btn" on:click={closePairingModal}>&times;</button>
      </div>

      <div class="modal-body">
        <p class="instructions">
          Send this code to the bot in Telegram to connect:
        </p>

        <div class="pairing-code">
          {pairingCode}
        </div>

        <div class="help-text">
          <span>Open Telegram and send: <code>/pair {pairingCode}</code></span>
          <button
            class="copy-btn"
            on:click={copyPairingCommand}
            title={copied ? 'Copied!' : 'Copy to clipboard'}
          >
            {#if copied}
              <CheckCheck size={16} class="text-green-500" />
            {:else}
              <Copy size={16} />
            {/if}
          </button>
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn-primary" on:click={closePairingModal}>
          Done
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .bot-status {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .status-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .status-text {
    font-size: 0.875rem;
    font-weight: 500;
    color: #1f2937;
  }

  .pid {
    font-size: 0.75rem;
    color: #6b7280;
  }

  .connection-status {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 0.75rem;
    padding: 0.25rem 0.625rem;
    background: #f9fafb;
    border-radius: 0.375rem;
    border: 1px solid #e5e7eb;
    opacity: 0;
    visibility: hidden;
    transition: opacity 0.3s ease, visibility 0.3s ease;
    min-width: 120px; /* Reserve space to prevent layout shift */
  }

  .connection-status.visible {
    opacity: 1;
    visibility: visible;
  }

  .connected {
    color: #059669;
    font-weight: 500;
  }

  .not-connected {
    color: #6b7280;
  }

  .count {
    color: #6b7280;
    font-size: 0.7rem;
  }

  .controls {
    display: flex;
    gap: 0.5rem;
  }

  .control-button {
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 0.375rem;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    gap: 0.375rem;
  }

  .control-button.start {
    background-color: #10b981;
    color: white;
  }

  .control-button.start:hover:not(:disabled) {
    background-color: #059669;
  }

  .control-button.stop {
    background-color: #ef4444;
    color: white;
  }

  .control-button.stop:hover:not(:disabled) {
    background-color: #dc2626;
  }

  .control-button.pair {
    background-color: #3b82f6;
    color: white;
  }

  .control-button.pair:hover:not(:disabled) {
    background-color: #2563eb;
  }

  .control-button:disabled {
    background-color: #d1d5db;
    color: #9ca3af;
    cursor: not-allowed;
  }

  .control-button.loading {
    opacity: 0.7;
    cursor: wait;
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% {
      opacity: 0.7;
    }
    50% {
      opacity: 1;
    }
  }

  .modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-content {
    background: white;
    border-radius: 0.5rem;
    width: 90%;
    max-width: 500px;
    box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1),
      0 10px 10px -5px rgba(0, 0, 0, 0.04);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.5rem;
    border-bottom: 1px solid #e5e7eb;
  }

  .modal-header h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
    color: #1f2937;
  }

  .close-btn {
    background: none;
    border: none;
    font-size: 1.75rem;
    cursor: pointer;
    padding: 0;
    width: 2rem;
    height: 2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #6b7280;
    line-height: 1;
  }

  .close-btn:hover {
    color: #374151;
  }

  .modal-body {
    padding: 1.5rem;
  }

  .instructions {
    margin: 0 0 1rem 0;
    color: #374151;
    font-size: 0.875rem;
  }

  .pairing-code {
    font-size: 2rem;
    font-weight: 700;
    text-align: center;
    padding: 1.5rem;
    background: #f3f4f6;
    border-radius: 0.5rem;
    letter-spacing: 0.5rem;
    margin-bottom: 1rem;
    font-family: 'Monaco', 'Courier New', monospace;
    color: #1f2937;
    user-select: all;
  }

  .help-text {
    font-size: 0.875rem;
    color: #6b7280;
    text-align: center;
    margin: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
  }

  .help-text code {
    background: #f3f4f6;
    padding: 0.25rem 0.5rem;
    border-radius: 0.25rem;
    font-family: 'Monaco', 'Courier New', monospace;
    color: #1f2937;
  }

  .copy-btn {
    background: transparent;
    border: 1px solid #d1d5db;
    padding: 0.375rem;
    border-radius: 0.375rem;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.2s;
    color: #6b7280;
  }

  .copy-btn:hover {
    background: #f3f4f6;
    border-color: #9ca3af;
  }

  .copy-btn:active {
    transform: scale(0.95);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    padding: 1.5rem;
    border-top: 1px solid #e5e7eb;
  }

  .btn-primary {
    background: #3b82f6;
    color: white;
    border: none;
    padding: 0.5rem 1.5rem;
    border-radius: 0.375rem;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
  }

  .btn-primary:hover {
    background: #2563eb;
  }
</style>
