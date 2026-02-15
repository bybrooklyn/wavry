<script lang="ts">
  import Icon from "./Icon.svelte";

  let { appState } = $props();

  function modeLabel() {
    switch (appState.connectivityMode) {
      case "wavry":
        return "Wavry Service";
      case "direct":
        return "LAN";
      default:
        return "Custom";
    }
  }
</script>

<div class="host-card">
  <div class="preview" class:live={appState.isHosting}>
    {#if appState.isHosting}
      <div class="hosting-badge">
        <span class="dot"></span>
        <span>HOSTING</span>
      </div>
    {/if}

    <div class="host-icon">
      <Icon name="hostDefault" size={60} />
    </div>
  </div>

  <div class="info-row">
    <div class="identity">
      <h3>{appState.effectiveDisplayName}</h3>
      <p>{modeLabel()}</p>
    </div>

    {#if appState.isConnected && !appState.isHosting}
      <span class="client-state">Connected as Client</span>
    {:else}
      <button
        class="host-btn"
        class:stop={appState.isHosting}
        disabled={appState.isHostTransitioning || isNaN(appState.hostPort)}
        onclick={async () => {
          try {
            if (appState.isHosting) {
              await appState.stopHosting();
            } else {
              await appState.startHosting();
            }
          } catch {
            // User-facing errors are stored in appState.
          }
        }}
      >
        {#if appState.isHostTransitioning}
          {appState.isHosting ? "Stopping..." : "Starting..."}
        {:else}
          {appState.isHosting ? "Stop Hosting" : "Start Hosting"}
        {/if}
      </button>
    {/if}
  </div>
</div>

<style>
  .host-card {
    border-radius: var(--radius-md);
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(255, 255, 255, 0.03);
    backdrop-filter: blur(12px);
  }

  .preview {
    position: relative;
    height: 180px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.2);
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.34);
  }

  .preview.live {
    background: rgba(0, 0, 0, 0.26);
  }

  .host-icon {
    filter: drop-shadow(0 8px 18px rgba(0, 0, 0, 0.35));
  }

  .hosting-badge {
    position: absolute;
    left: 12px;
    bottom: 12px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(0, 0, 0, 0.32);
    color: var(--colors-accent-success);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
    line-height: 1;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: var(--colors-accent-success);
    box-shadow: 0 0 0 0 rgba(52, 199, 89, 0.42);
    animation: host-pulse 1.8s ease-out infinite;
  }

  .info-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px;
  }

  .identity {
    min-width: 0;
  }

  .identity h3 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    line-height: 1.1;
    color: var(--colors-text-primary);
  }

  .identity p {
    margin: 6px 0 0;
    font-size: 12px;
    line-height: 1.2;
    color: var(--colors-text-secondary);
  }

  .client-state {
    flex-shrink: 0;
    padding: 6px 12px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    color: var(--colors-text-secondary);
    font-size: 12px;
    line-height: 1;
  }

  .host-btn {
    flex-shrink: 0;
    padding: 10px 20px;
    border-radius: var(--radius-sm);
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: var(--colors-accent-primary);
    color: #fff;
    font-size: 13px;
    font-weight: 700;
    line-height: 1;
  }

  .host-btn.stop {
    background: var(--colors-accent-danger);
  }

  .host-btn:hover:enabled {
    filter: brightness(1.05);
  }

  .host-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  @keyframes host-pulse {
    0% {
      box-shadow: 0 0 0 0 rgba(52, 199, 89, 0.45);
    }
    70% {
      box-shadow: 0 0 0 10px rgba(52, 199, 89, 0);
    }
    100% {
      box-shadow: 0 0 0 0 rgba(52, 199, 89, 0);
    }
  }

  @media (max-width: 860px) {
    .info-row {
      flex-direction: column;
      align-items: stretch;
    }

    .host-btn,
    .client-state {
      width: 100%;
      text-align: center;
    }
  }
</style>
