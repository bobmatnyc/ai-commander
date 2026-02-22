<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { botRunning, botPid } from '../stores/app';
  import { Power, Settings } from 'lucide-svelte';

  let interval: number;

  async function checkStatus() {
    try {
      const status: any = await invoke('get_bot_status');
      botRunning.set(status.running);
      botPid.set(status.pid);
    } catch (err) {
      console.error('Failed to check bot status:', err);
    }
  }

  async function startBot() {
    try {
      const info: any = await invoke('start_bot');
      botRunning.set(info.running);
      botPid.set(info.pid);
    } catch (err) {
      alert(`Failed to start bot: ${err}`);
    }
  }

  async function stopBot() {
    try {
      await invoke('stop_bot');
      botRunning.set(false);
      botPid.set(null);
    } catch (err) {
      alert(`Failed to stop bot: ${err}`);
    }
  }

  onMount(() => {
    checkStatus();
    interval = window.setInterval(checkStatus, 5000);

    return () => {
      clearInterval(interval);
    };
  });
</script>

<div class="bot-status">
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

  <div class="controls">
    <button
      on:click={startBot}
      disabled={$botRunning}
      class="control-button start"
    >
      Start
    </button>
    <button
      on:click={stopBot}
      disabled={!$botRunning}
      class="control-button stop"
    >
      Stop
    </button>
  </div>
</div>

<style>
  .bot-status {
    display: flex;
    align-items: center;
    gap: 1.5rem;
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

  .control-button:disabled {
    background-color: #d1d5db;
    color: #9ca3af;
    cursor: not-allowed;
  }
</style>
