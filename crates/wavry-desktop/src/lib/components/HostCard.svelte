<script>
  let { appState } = $props();
</script>

<div class="host-card">
  <div class="preview">
    {#if appState.isConnected}
      <div class="video-placeholder">VIDEO</div>
      <div class="status-overlay">
        <span class="dot"></span>
        <span class="label">LIVE</span>
      </div>
    {:else}
      <span class="host-icon">💻</span>
    {/if}
  </div>

  <div class="info">
    <div class="meta">
      <h3>{appState.displayName || "Mac"}</h3>
      <p>{appState.connectivityMode || "Wavry Service"}</p>
    </div>

    <button
      class="action-btn"
      class:stop={appState.isHosting}
      onclick={() =>
        appState.isHosting ? appState.stopHosting() : appState.startHosting()}
    >
      {appState.isHosting ? "Stop Hosting" : "Start Hosting"}
    </button>
  </div>
</div>

<style>
  .host-card {
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
    overflow: hidden;
    background-color: var(--colors-bg-elevation1);
    border: 1px solid rgba(0, 0, 0, 0.5);
  }

  .preview {
    height: 200px;
    background-color: var(--colors-bg-elevation2);
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
  }

  .host-icon {
    font-size: 60px;
    opacity: 0.5;
  }

  .status-overlay {
    position: absolute;
    bottom: 8px;
    left: 8px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background-color: var(--colors-accent-success);
  }

  .label {
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-bold);
    color: var(--colors-accent-success);
  }

  .info {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px;
  }

  .meta h3 {
    font-size: var(
      --font-size-body
    ); /* headline in SwiftUI is approx body font size but bold */
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-primary);
    margin-bottom: 4px;
  }

  .meta p {
    font-size: var(--font-size-caption);
    color: var(--colors-text-secondary);
  }

  .action-btn {
    padding: 8px 20px;
    font-weight: var(--font-weight-bold);
    color: white;
    background-color: var(--colors-accent-primary);
    border-radius: var(--radius-sm);
    transition: filter 0.2s;
  }

  .action-btn.stop {
    background-color: var(--colors-accent-danger);
  }

  .action-btn:hover {
    filter: brightness(1.1);
  }
</style>
