<script lang="ts">
  import { onMount } from "svelte";

  let { appState } = $props();

  onMount(() => {
    if (!appState.monitors?.length) {
      appState.loadMonitors();
    }
  });

  function throughputLabel() {
    const mbps = appState.ccBitrate / 1000;
    if (!Number.isFinite(mbps)) return "0.0 Mbps";
    return `${mbps.toFixed(1)} Mbps`;
  }

  function modeLabel() {
    switch (appState.connectivityMode) {
      case "wavry":
        return "Cloud";
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
      <div class="status-overlay">
        <span class="dot"></span>
        <span class="label">HOSTING</span>
      </div>
    {/if}
    <span class="host-icon">{appState.isHosting ? "🟢" : "🖥️"}</span>
  </div>

  <div class="info">
    <div class="meta">
      <div class="meta-head">
        <h3>{appState.displayName || "Local Host"}</h3>
        <span class="mode-badge">{modeLabel()}</span>
      </div>
      <p class="subtle">Port {appState.hostPort === 0 ? "Random" : appState.hostPort} • {appState.isHosting ? "Ready for incoming peers" : "Idle"}</p>

      {#if appState.isHosting}
        <div class="stats-row">
          <span>Throughput: {throughputLabel()}</span>
          <span>State: {appState.ccState}</span>
        </div>
      {:else if appState.monitors.length > 0}
        <div class="monitor-row">
          <select
            bind:value={appState.selectedMonitorId}
            class="monitor-select"
            disabled={appState.isLoadingMonitors || appState.isHostTransitioning}
          >
            {#each appState.monitors as monitor}
              <option value={monitor.id}>
                {monitor.name} ({monitor.resolution.width}x{monitor.resolution.height})
              </option>
            {/each}
          </select>
          <button
            class="refresh-btn"
            onclick={() => appState.loadMonitors()}
            title="Refresh monitor list"
            disabled={appState.isLoadingMonitors || appState.isHostTransitioning}
          >
            {appState.isLoadingMonitors ? "Loading..." : "Refresh"}
          </button>
        </div>
      {/if}

      {#if appState.linuxRuntimeDiagnostics}
        <p class="linux-runtime-text">
          Linux runtime: {appState.linuxRuntimeDiagnostics.session_type.toUpperCase()} •
          {appState.linuxRuntimeDiagnostics.required_video_source}
        </p>
        {#if appState.linuxRuntimeDiagnostics.recommendations.length > 0}
          <p class="linux-runtime-warning">
            {appState.linuxRuntimeDiagnostics.recommendations[0]}
          </p>
        {/if}
      {/if}

      {#if appState.hostStatusMessage}
        <p class="status-text">{appState.hostStatusMessage}</p>
      {/if}
      {#if appState.hostErrorMessage}
        <p class="error-text">{appState.hostErrorMessage}</p>
      {/if}
    </div>

    <button
      class="action-btn"
      class:stop={appState.isHosting}
      disabled={appState.isHostTransitioning}
      onclick={async () => {
        try {
          if (appState.isHosting) {
            await appState.stopHosting();
          } else {
            await appState.startHosting();
          }
        } catch {
          // App state already stores a user-facing message.
        }
      }}
    >
      {#if appState.isHostTransitioning}
        {appState.isHosting ? "Stopping..." : "Starting..."}
      {:else}
        {appState.isHosting ? "Stop Hosting" : "Start Hosting"}
      {/if}
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
    border: 1px solid rgba(255, 255, 255, 0.06);
  }

  .preview {
    height: 184px;
    background: linear-gradient(135deg, rgba(25, 33, 45, 0.9), rgba(7, 11, 18, 0.95));
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
  }

  .preview.live {
    background: linear-gradient(135deg, rgba(11, 43, 36, 0.9), rgba(7, 21, 18, 0.95));
  }

  .host-icon {
    font-size: 52px;
    opacity: 0.85;
  }

  .status-overlay {
    position: absolute;
    top: 10px;
    left: 10px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.35);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background-color: var(--colors-accent-success);
    box-shadow: 0 0 0 rgba(16, 185, 129, 0.35);
    animation: host-pulse 1.8s ease-out infinite;
  }

  .label {
    font-size: 10px;
    letter-spacing: 0.06em;
    font-weight: var(--font-weight-bold);
    color: var(--colors-accent-success);
  }

  .info {
    display: flex;
    gap: 14px;
    align-items: flex-start;
    justify-content: space-between;
    padding: 14px 16px 16px;
  }

  .meta {
    min-width: 0;
    flex: 1;
  }

  .meta-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 2px;
  }

  .meta-head h3 {
    font-size: var(--font-size-body);
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-primary);
    margin: 0;
  }

  .mode-badge {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--colors-border-subtle);
    color: var(--colors-text-secondary);
  }

  .subtle {
    margin: 0;
    font-size: 12px;
    color: var(--colors-text-secondary);
  }

  .stats-row {
    margin-top: 8px;
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
    font-size: 11px;
    color: var(--colors-text-secondary);
  }

  .monitor-row {
    margin-top: 9px;
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .monitor-select {
    min-width: 240px;
    max-width: 100%;
    padding: 6px;
    background: var(--colors-bg-base);
    color: var(--colors-text-primary);
    border: 1px solid var(--colors-border-input);
    border-radius: var(--radius-sm);
    font-size: 11px;
  }

  .refresh-btn {
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--colors-border-subtle);
    background: var(--colors-bg-elevation2);
    color: var(--colors-text-secondary);
    font-size: 11px;
    cursor: pointer;
  }

  .refresh-btn:hover {
    color: var(--colors-text-primary);
  }

  .refresh-btn:disabled,
  .monitor-select:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .status-text {
    margin-top: 8px;
    font-size: 11px;
    color: var(--colors-accent-success);
  }

  .linux-runtime-text {
    margin-top: 8px;
    font-size: 11px;
    color: var(--colors-text-secondary);
  }

  .linux-runtime-warning {
    margin-top: 6px;
    font-size: 11px;
    color: #f59e0b;
  }

  .error-text {
    margin-top: 8px;
    font-size: 11px;
    color: var(--colors-accent-danger);
  }

  .action-btn {
    min-width: 128px;
    padding: 9px 14px;
    font-weight: var(--font-weight-bold);
    color: white;
    background-color: var(--colors-accent-primary);
    border-radius: var(--radius-sm);
    transition: filter 0.2s;
    cursor: pointer;
  }

  .action-btn.stop {
    background-color: var(--colors-accent-danger);
  }

  .action-btn:hover:enabled {
    filter: brightness(1.08);
  }

  .action-btn:focus-visible,
  .refresh-btn:focus-visible,
  .monitor-select:focus-visible {
    outline: 2px solid rgba(58, 130, 246, 0.75);
    outline-offset: 1px;
  }

  .action-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  @keyframes host-pulse {
    0% {
      box-shadow: 0 0 0 0 rgba(16, 185, 129, 0.45);
    }
    70% {
      box-shadow: 0 0 0 10px rgba(16, 185, 129, 0);
    }
    100% {
      box-shadow: 0 0 0 0 rgba(16, 185, 129, 0);
    }
  }

  @media (max-width: 860px) {
    .info {
      flex-direction: column;
      align-items: stretch;
    }

    .action-btn {
      width: 100%;
    }

    .monitor-select {
      min-width: 0;
      width: 100%;
    }
  }
</style>
