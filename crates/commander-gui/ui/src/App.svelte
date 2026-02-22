<script lang="ts">
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import BotStatus from './lib/components/BotStatus.svelte';
</script>

<main class="app">
  <header>
    <h1>AI Commander</h1>
    <BotStatus />
  </header>

  <div class="content">
    <aside>
      <SessionList />
    </aside>

    <section class="main-panel">
      <ChatView />
      <InputArea />
    </section>
  </div>
</main>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background-color: white;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1rem 1.5rem;
    border-bottom: 1px solid #e5e7eb;
    background-color: white;
  }

  h1 {
    font-size: 1.5rem;
    font-weight: 700;
    color: #1f2937;
  }

  .content {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  aside {
    width: 250px;
    border-right: 1px solid #e5e7eb;
    overflow-y: auto;
  }

  .main-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
</style>
