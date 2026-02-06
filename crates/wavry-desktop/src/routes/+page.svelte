<script lang="ts">
  import SidebarIcon from "$lib/components/SidebarIcon.svelte";
  import HostCard from "$lib/components/HostCard.svelte";
  import SetupWizard from "$lib/components/SetupWizard.svelte";
  import LoginModal from "$lib/components/LoginModal.svelte";
  import DeltaTuning from "$lib/components/DeltaTuning.svelte";
  import { appState } from "$lib/appState.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let activeTab = $state("sessions");
  let connectIp = $state("");
  let remoteUsername = $state("");
  let isConnecting = $state(false);
  let connectError = $state("");
  let setupStep = $state(0);
  let saveFeedback = $state("");
  let saveFeedbackType = $state<"success" | "error">("success");
  let baselineSettingsFingerprint = $state("");

  let showAdvancedSettings = $state(false);

  function normalizeError(err: unknown): string {
    if (err instanceof Error) return err.message;
    if (typeof err === "string") return err;
    return JSON.stringify(err);
  }

  function settingsFingerprint(): string {
    return JSON.stringify({
      displayName: appState.displayName,
      connectivityMode: appState.connectivityMode,
      authServer: appState.authServer,
      hostPort: appState.hostPort,
      upnpEnabled: appState.upnpEnabled,
      resolutionMode: appState.resolutionMode,
      customResolution: appState.customResolution,
      gamepadEnabled: appState.gamepadEnabled,
      gamepadDeadzone: appState.gamepadDeadzone,
      selectedMonitorId: appState.selectedMonitorId,
    });
  }

  let currentSettingsFingerprint = $derived(settingsFingerprint());
  let hasUnsavedSettings = $derived(
    baselineSettingsFingerprint.length > 0 &&
      currentSettingsFingerprint !== baselineSettingsFingerprint,
  );

  function captureSettingsBaseline() {
    baselineSettingsFingerprint = settingsFingerprint();
  }

  onMount(() => {
    appState.loadFromStorage();
    appState.refreshPcvrStatus();
    appState.loadMonitors();
    captureSettingsBaseline();
  });

  async function startSession() {
    isConnecting = true;
    connectError = "";
    appState.hostErrorMessage = "";
    const addressError = appState.validateConnectTarget(connectIp);
    if (addressError) {
      connectError = addressError;
      isConnecting = false;
      return;
    }

    try {
      await appState.connect(connectIp.trim());
    } catch (e) {
      connectError = normalizeError(e);
    } finally {
      isConnecting = false;
    }
  }

  async function connectViaId() {
    isConnecting = true;
    connectError = "";
    appState.hostErrorMessage = "";
    const username = remoteUsername.trim();

    if (!appState.isAuthenticated) {
      isConnecting = false;
      appState.showLoginModal = true;
      connectError = "Sign in first to connect via username.";
      return;
    }
    if (!username) {
      isConnecting = false;
      connectError = "Username is required.";
      return;
    }

    try {
      await invoke("connect_via_id", { targetUsername: username });
      appState.hostStatusMessage = `Connection request sent to ${username}`;
    } catch (e) {
      connectError = normalizeError(e);
    } finally {
      isConnecting = false;
    }
  }

  async function disconnectSession() {
    try {
      await appState.disconnect();
      connectError = "";
      appState.hostErrorMessage = "";
      appState.hostStatusMessage = "Session disconnected.";
    } catch (e) {
      connectError = normalizeError(e);
    }
  }

  function modeLabel(mode: "wavry" | "direct" | "custom") {
    if (mode === "wavry") return "Cloud";
    if (mode === "direct") return "LAN";
    return "Custom";
  }

  function sessionStatusLabel() {
    if (appState.isHostTransitioning) return "Transitioning";
    if (appState.isHosting) return "Hosting";
    if (appState.isConnected) return "Connected";
    return "Idle";
  }

  function networkModeHint(mode: "wavry" | "direct" | "custom") {
    if (mode === "wavry") return "Cloud signaling with username lookup.";
    if (mode === "direct") return "LAN-only direct host discovery/connect.";
    return "Custom gateway for self-hosted routing.";
  }

  function handleSetupComplete(
    name: string,
    mode: "wavry" | "direct" | "custom",
  ) {
    appState.completeSetup(name, mode);
    captureSettingsBaseline();
  }

  function saveSettings() {
    const validationError = appState.validateSettingsInputs();
    if (validationError) {
      saveFeedbackType = "error";
      saveFeedback = validationError;
      return;
    }
    appState.saveToStorage();
    captureSettingsBaseline();
    saveFeedbackType = "success";
    saveFeedback = "Settings saved";
    setTimeout(() => {
      if (saveFeedback === "Settings saved") saveFeedback = "";
    }, 1600);
  }

  function resetSettings() {
    appState.resetSettingsToDefaults();
    saveFeedbackType = "success";
    saveFeedback = "Defaults restored";
  }
</script>

{#if !appState.isSetupCompleted}
  <SetupWizard step={setupStep} onComplete={handleSetupComplete} />
{:else}
  <div class="content-view">
    <aside class="sidebar">
      <div class="top-icons">
        <SidebarIcon
          icon="tabSessions"
          active={activeTab === "sessions"}
          onclick={() => (activeTab = "sessions")}
        />
      </div>

      <div class="spacer"></div>

      <SidebarIcon
        icon="tabSettings"
        active={activeTab === "settings"}
        onclick={() => (activeTab = "settings")}
      />
    </aside>

    <main class="main-content">
      <header class="top-bar">
        <div class="status-indicators">
          {#if appState.isHosting || appState.isConnected}
            <div
              class="performance-badge"
              class:warning={appState.ccState === "Congested"}
              class:rising={appState.ccState === "Rising"}
            >
              <span class="label">{appState.ccState.toUpperCase()}</span>
              <span class="value"
                >{(appState.ccBitrate / 1000).toFixed(1)} Mbps</span
              >
            </div>
          {/if}

          <span class="mode-pill">{modeLabel(appState.connectivityMode)}</span>
          <span
            class="session-pill"
            class:hosting={appState.isHosting}
            class:connected={!appState.isHosting && appState.isConnected}
            class:idle={!appState.isHosting && !appState.isConnected}
          >
            {sessionStatusLabel()}
          </span>
        </div>

        <div
          class="user-badge"
          onclick={() =>
            !appState.isAuthenticated && (appState.showLoginModal = true)}
          onkeydown={(e) =>
            (e.key === "Enter" || e.key === " ") &&
            !appState.isAuthenticated &&
            (appState.showLoginModal = true)}
          role="button"
          tabindex="0"
          aria-label={appState.isAuthenticated
            ? `Signed in as ${appState.username}`
            : "Sign in"}
        >
          <span class="status-dot" class:online={appState.isAuthenticated}
          ></span>
          <span class="username">{appState.effectiveDisplayName}</span>
          {#if appState.isAuthenticated}
            <button
              class="logout-btn"
              onclick={(e) => {
                e.stopPropagation();
                appState.logout();
              }}>Logout</button
            >
          {/if}
        </div>
      </header>

      <div class="tab-content">
        {#if activeTab === "sessions"}
          <section class="sessions-view">
            <div class="header">
              <h1>Sessions</h1>
              <p>Host your desktop or connect to another machine.</p>
            </div>

            <div class="scroll-area session-grid">
              <section class="surface-card">
                <div class="section-head">
                  <span class="section-label">Local Host</span>
                </div>
                <HostCard {appState} />
              </section>

              <section class="surface-card">
                <div class="section-head">
                  <span class="section-label">Connect to Host</span>
                </div>

                <div class="connect-panel">
                  {#if appState.connectivityMode === "wavry"}
                    {#if !appState.isAuthenticated}
                      <div class="auth-gate">
                        <strong>Cloud connect needs sign-in.</strong>
                        <p>Sign in once, then connect by username from any trusted device.</p>
                        <button class="primary-btn" onclick={() => (appState.showLoginModal = true)}>
                          Sign In
                        </button>
                      </div>
                    {:else}
                      <label class="field-label" for="remote-username">Username</label>
                      <div class="field-row">
                        <input
                          id="remote-username"
                          type="text"
                          placeholder="e.g. brooklyn"
                          bind:value={remoteUsername}
                          onkeydown={(e) =>
                            e.key === "Enter" &&
                            !isConnecting &&
                            remoteUsername.trim() &&
                            !appState.isHosting &&
                            !appState.isHostTransitioning &&
                            connectViaId()}
                        />
                        <button
                          class="primary-btn"
                          onclick={connectViaId}
                          disabled={isConnecting || !remoteUsername.trim() || appState.isHosting || appState.isHostTransitioning}
                        >
                          {#if isConnecting}Connecting...{:else}Connect by ID{/if}
                        </button>
                      </div>
                    {/if}

                    <div class="divider"><span>or</span></div>
                  {/if}

                  <label class="field-label" for="direct-host">Direct Host (IP or IP:PORT)</label>
                  <div class="field-row">
                    <input
                      id="direct-host"
                      type="text"
                      placeholder="192.168.1.20:8000"
                      bind:value={connectIp}
                      onkeydown={(e) =>
                        e.key === "Enter" &&
                        !isConnecting &&
                        connectIp.trim() &&
                        !appState.isHosting &&
                        !appState.isHostTransitioning &&
                        startSession()}
                    />
                    <button
                      class="primary-btn"
                      onclick={startSession}
                      disabled={isConnecting || !connectIp.trim() || appState.isHosting || appState.isHostTransitioning}
                    >
                      {#if isConnecting}Connecting...{:else}Connect{/if}
                    </button>
                  </div>

                  {#if connectError}
                    <div class="error">{connectError}</div>
                  {/if}
                  {#if appState.hostStatusMessage}
                    <div class="success">{appState.hostStatusMessage}</div>
                  {/if}
                  {#if appState.hostErrorMessage}
                    <div class="error">{appState.hostErrorMessage}</div>
                  {/if}
                </div>
              </section>

              <section class="surface-card">
                <div class="section-head">
                  <span class="section-label">Active Session</span>
                </div>

                {#if appState.isConnected}
                  <div class="active-session">
                    <div class="session-line">
                      <span>Status</span>
                      <strong>{appState.isHosting ? "Hosting" : "Connected"}</strong>
                    </div>
                    <div class="session-line">
                      <span>Link State</span>
                      <strong>{appState.ccState}</strong>
                    </div>
                    <div class="session-line">
                      <span>Estimated Throughput</span>
                      <strong>{(appState.ccBitrate / 1000).toFixed(1)} Mbps</strong>
                    </div>
                    <button class="danger-btn" onclick={disconnectSession}>Disconnect</button>
                  </div>
                {:else}
                  <div class="placeholder-box">
                    <span class="p-icon">⏳</span>
                    <span class="p-text">No active session</span>
                  </div>
                {/if}
              </section>
            </div>
          </section>
        {:else}
          <section class="settings-view">
            <div class="header settings-header">
              <div>
                <h1>Settings</h1>
                <p>Tune networking, display behavior, and input handling.</p>
              </div>

              <div class="settings-actions">
                <button class="ghost-btn" onclick={resetSettings}>Reset</button>
                <button class="save-settings-btn" onclick={saveSettings} disabled={!hasUnsavedSettings}>Save</button>
              </div>
            </div>

            <div class="settings-feedback-row">
              <div class="pcvr-banner">{appState.pcvrStatus}</div>
              {#if hasUnsavedSettings}
                <div class="unsaved-pill">Unsaved changes</div>
              {/if}
              {#if saveFeedback}
                <div class="saved-pill" class:error={saveFeedbackType === "error"}>{saveFeedback}</div>
              {/if}
            </div>

            <div class="scroll-area settings-grid">
              <div class="settings-group">
                <span class="group-label">Identity</span>
                <div class="setting-row">
                  <label for="setting-host-name">Host Name</label>
                  <input
                    id="setting-host-name"
                    type="text"
                    bind:value={appState.displayName}
                    placeholder="My Desktop"
                  />
                </div>
              </div>

              <div class="settings-group">
                <span class="group-label">Network</span>
                <div class="setting-row">
                  <label for="setting-mode">Connectivity Mode</label>
                  <select id="setting-mode" bind:value={appState.connectivityMode}>
                    <option value="wavry">Wavry Cloud</option>
                    <option value="direct">LAN Only</option>
                    <option value="custom">Custom Server</option>
                  </select>
                </div>
                <p class="setting-hint">{networkModeHint(appState.connectivityMode)}</p>

                <div class="setting-row">
                  <label for="setting-port">Host Port</label>
                  <input id="setting-port" type="number" min="1" max="65535" bind:value={appState.hostPort} />
                </div>

                <div class="setting-row checkbox">
                  <label for="setting-upnp">Enable UPnP</label>
                  <input id="setting-upnp" type="checkbox" bind:checked={appState.upnpEnabled} />
                </div>
              </div>

              <div class="settings-group">
                <span class="group-label">Display</span>
                <div class="setting-row">
                  <label for="setting-resolution-mode">Client Resolution</label>
                  <select id="setting-resolution-mode" bind:value={appState.resolutionMode}>
                    <option value="native">Use Host Native</option>
                    <option value="client">Match This Client</option>
                    <option value="custom">Custom Fixed</option>
                  </select>
                </div>

                {#if appState.resolutionMode === "custom"}
                  <div class="setting-row">
                    <label for="setting-width">Width</label>
                    <input id="setting-width" type="number" min="640" bind:value={appState.customResolution.width} />
                  </div>
                  <div class="setting-row">
                    <label for="setting-height">Height</label>
                    <input id="setting-height" type="number" min="480" bind:value={appState.customResolution.height} />
                  </div>
                {/if}

                <div class="setting-row">
                  <label for="setting-monitor">Host Monitor</label>
                  <div class="monitor-picker-wrap">
                    <select id="setting-monitor" bind:value={appState.selectedMonitorId}>
                      {#if appState.monitors.length === 0}
                        <option value={null}>No displays detected</option>
                      {:else}
                        {#each appState.monitors as monitor}
                          <option value={monitor.id}>{monitor.name} ({monitor.resolution.width}x{monitor.resolution.height})</option>
                        {/each}
                      {/if}
                    </select>
                    <button
                      class="mini-btn"
                      onclick={() => appState.loadMonitors()}
                      disabled={appState.isLoadingMonitors}
                    >
                      {appState.isLoadingMonitors ? "Loading..." : "Refresh"}
                    </button>
                  </div>
                </div>
              </div>

              <div class="settings-group">
                <span class="group-label">Input</span>
                <div class="setting-row checkbox">
                  <label for="setting-gamepad">Gamepad Passthrough</label>
                  <input id="setting-gamepad" type="checkbox" bind:checked={appState.gamepadEnabled} />
                </div>

                {#if appState.gamepadEnabled}
                  <div class="setting-row">
                    <label for="setting-deadzone">Deadzone</label>
                    <div class="deadzone-wrap">
                      <input
                        id="setting-deadzone"
                        type="range"
                        min="0"
                        max="0.5"
                        step="0.05"
                        bind:value={appState.gamepadDeadzone}
                      />
                      <span>{appState.gamepadDeadzone.toFixed(2)}</span>
                    </div>
                  </div>
                {/if}
              </div>

              <div class="settings-group">
                <span class="group-label">VR / PCVR</span>
                <div class="setting-row">
                  <div class="setting-info full">
                    Linux/Windows OpenXR client. Wayland supported via Vulkan, X11 via OpenGL.
                    Transport stays in Wavry/RIFT (no ALVR networking).
                  </div>
                </div>
              </div>

              <details class="advanced-group" bind:open={showAdvancedSettings}>
                <summary>Advanced</summary>
                <div class="settings-group advanced-inner">
                  <div class="setting-row">
                    <label for="setting-gateway">Gateway URL</label>
                    <input
                      id="setting-gateway"
                      type="url"
                      bind:value={appState.authServer}
                      placeholder="https://auth.wavry.dev"
                    />
                  </div>
                </div>
              </details>

              <div class="settings-group">
                <span class="group-label">Tuning</span>
                <DeltaTuning />
              </div>
            </div>
          </section>
        {/if}
      </div>
    </main>
  </div>
{/if}

{#if appState.showLoginModal}
  <LoginModal />
{/if}

<style>
  .content-view {
    display: flex;
    height: 100vh;
    width: 100vw;
    background: radial-gradient(circle at 20% -10%, rgba(58, 84, 118, 0.3), transparent 40%),
      radial-gradient(circle at 90% 10%, rgba(21, 57, 50, 0.25), transparent 38%),
      var(--colors-bg-base);
  }

  .sidebar {
    width: 60px;
    background-color: var(--colors-bg-sidebar);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--spacing-xl) 0;
    gap: var(--spacing-xl);
    border-right: 1px solid rgba(255, 255, 255, 0.05);
  }

  .spacer {
    flex: 1;
  }

  .main-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  button,
  input,
  select,
  summary {
    transition: border-color 0.2s, box-shadow 0.2s, background-color 0.2s, transform 0.15s;
  }

  button:focus-visible,
  input:focus-visible,
  select:focus-visible,
  summary:focus-visible {
    outline: 2px solid rgba(58, 130, 246, 0.75);
    outline-offset: 1px;
  }

  .top-bar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-xl) var(--spacing-xxl) 0;
    gap: var(--spacing-md);
  }

  .status-indicators {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .performance-badge {
    display: flex;
    align-items: center;
    gap: 8px;
    background: rgba(0, 0, 0, 0.35);
    padding: 6px 12px;
    border-radius: 999px;
    border: 1px solid rgba(255, 255, 255, 0.06);
    font-size: 11px;
    font-family: monospace;
    backdrop-filter: blur(10px);
  }

  .performance-badge .label {
    color: #10b981;
    font-weight: bold;
  }

  .performance-badge.rising .label {
    color: #f59e0b;
  }

  .performance-badge.warning .label {
    color: #ef4444;
  }

  .performance-badge .value {
    color: rgba(255, 255, 255, 0.7);
  }

  .mode-pill {
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--colors-text-secondary);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 999px;
    padding: 5px 10px;
  }

  .session-pill {
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    border-radius: 999px;
    padding: 5px 10px;
    border: 1px solid rgba(255, 255, 255, 0.12);
  }

  .session-pill.hosting {
    color: #10b981;
    border-color: rgba(16, 185, 129, 0.55);
    background: rgba(16, 185, 129, 0.12);
  }

  .session-pill.connected {
    color: #3b82f6;
    border-color: rgba(59, 130, 246, 0.55);
    background: rgba(59, 130, 246, 0.12);
  }

  .session-pill.idle {
    color: var(--colors-text-secondary);
    background: rgba(255, 255, 255, 0.03);
  }

  .user-badge {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    background-color: var(--colors-bg-elevation3);
    border-radius: var(--radius-md);
    cursor: pointer;
    border: 1px solid rgba(255, 255, 255, 0.06);
  }

  .status-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background-color: var(--colors-bg-elevation3);
    border: 2px solid var(--colors-border-subtle);
    transition: background-color 0.3s;
  }

  .status-dot.online {
    background-color: var(--colors-accent-success);
    box-shadow: 0 0 10px var(--colors-accent-success);
  }

  .username {
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-primary);
  }

  .logout-btn {
    padding: 4px 8px;
    background: var(--colors-bg-elevation1);
    border: 1px solid var(--colors-border-subtle);
    border-radius: var(--radius-sm);
    color: var(--colors-text-secondary);
    font-size: 10px;
    cursor: pointer;
    margin-left: 8px;
  }

  .logout-btn:hover {
    color: var(--colors-text-primary);
    background: var(--colors-bg-elevation2);
  }

  .tab-content {
    flex: 1;
    overflow-y: auto;
    padding-bottom: var(--spacing-xxl);
  }

  .sessions-view .header,
  .settings-view .header {
    padding: var(--spacing-xl) var(--spacing-xxl) var(--spacing-xxl);
  }

  .sessions-view h1,
  .settings-view h1 {
    font-size: var(--font-size-titleMd);
    font-weight: var(--font-weight-light);
    color: var(--colors-text-primary);
    margin: 0 0 5px;
  }

  .sessions-view p,
  .settings-view p {
    font-size: var(--font-size-body);
    color: var(--colors-text-secondary);
    margin: 0;
  }

  .scroll-area {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xl);
  }

  .session-grid,
  .settings-grid {
    padding: 0 var(--spacing-xxl);
  }

  .surface-card {
    background: linear-gradient(180deg, rgba(23, 31, 44, 0.8), rgba(15, 21, 32, 0.8));
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: var(--radius-md);
    padding: var(--spacing-lg);
    box-shadow: 0 16px 28px rgba(0, 0, 0, 0.15);
    backdrop-filter: blur(6px);
  }

  .surface-card:hover {
    border-color: rgba(255, 255, 255, 0.14);
    transform: translateY(-1px);
  }

  .section-head {
    margin-bottom: var(--spacing-md);
  }

  .section-label {
    display: block;
    font-size: 11px;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-secondary);
  }

  .connect-panel {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .field-label {
    font-size: 12px;
    color: var(--colors-text-secondary);
    margin-top: 2px;
  }

  .field-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .field-row input {
    flex: 1;
    padding: 10px;
    border-radius: 6px;
    border: 1px solid var(--colors-border-input);
    background: var(--colors-bg-base);
    color: var(--colors-text-primary);
    min-width: 0;
  }

  .primary-btn,
  .danger-btn,
  .ghost-btn,
  .save-settings-btn,
  .mini-btn {
    border: none;
    border-radius: 6px;
    cursor: pointer;
  }

  .primary-btn {
    padding: 10px 12px;
    background: var(--colors-accent-primary);
    color: white;
    font-weight: 600;
    white-space: nowrap;
  }

  .primary-btn:hover:enabled {
    transform: translateY(-1px);
    filter: brightness(1.05);
  }

  .primary-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .divider {
    position: relative;
    text-align: center;
    margin: 4px 0;
  }

  .divider::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    border-top: 1px solid rgba(255, 255, 255, 0.09);
  }

  .divider span {
    position: relative;
    background: rgba(16, 22, 33, 0.95);
    padding: 0 8px;
    color: var(--colors-text-secondary);
    font-size: 11px;
    text-transform: uppercase;
  }

  .auth-gate {
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 8px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .auth-gate strong {
    color: var(--colors-text-primary);
    font-size: 13px;
  }

  .auth-gate p {
    margin: 0;
    font-size: 12px;
    color: var(--colors-text-secondary);
  }

  .active-session {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .session-line {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 10px;
    background: rgba(0, 0, 0, 0.25);
    border-radius: 6px;
    color: var(--colors-text-secondary);
    font-size: 12px;
  }

  .session-line strong {
    color: var(--colors-text-primary);
    font-weight: 600;
  }

  .danger-btn {
    margin-top: 4px;
    padding: 10px;
    color: #fff;
    font-weight: 600;
    background: var(--colors-accent-danger);
  }

  .danger-btn:hover {
    filter: brightness(1.05);
  }

  .placeholder-box {
    min-height: 110px;
    border-radius: 8px;
    border: 1px dashed rgba(255, 255, 255, 0.16);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .p-icon {
    font-size: 22px;
    opacity: 0.8;
  }

  .p-text {
    font-size: 12px;
    color: var(--colors-text-secondary);
  }

  .error {
    color: var(--colors-accent-danger);
    font-size: 12px;
  }

  .success {
    color: var(--colors-accent-success);
    font-size: 12px;
  }

  .settings-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .settings-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .ghost-btn,
  .save-settings-btn,
  .mini-btn {
    border: 1px solid var(--colors-border-subtle);
    background: var(--colors-bg-elevation2);
    color: var(--colors-text-primary);
  }

  .ghost-btn,
  .save-settings-btn {
    padding: 8px 14px;
    font-size: 12px;
  }

  .save-settings-btn {
    background: rgba(58, 130, 246, 0.18);
    border-color: rgba(58, 130, 246, 0.55);
  }

  .settings-feedback-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 var(--spacing-xxl) var(--spacing-xl);
    flex-wrap: wrap;
  }

  .pcvr-banner {
    padding: 8px 12px;
    border-radius: var(--radius-md);
    background: var(--colors-bg-elevation2);
    border: 1px solid var(--colors-border-subtle);
    color: var(--colors-text-primary);
    font-size: 12px;
  }

  .unsaved-pill,
  .saved-pill {
    padding: 6px 10px;
    border-radius: 999px;
    font-size: 11px;
    border: 1px solid;
  }

  .unsaved-pill {
    color: #f59e0b;
    border-color: rgba(245, 158, 11, 0.55);
    background: rgba(245, 158, 11, 0.12);
  }

  .saved-pill {
    color: #10b981;
    border-color: rgba(16, 185, 129, 0.55);
    background: rgba(16, 185, 129, 0.12);
  }

  .saved-pill.error {
    color: #ef4444;
    border-color: rgba(239, 68, 68, 0.55);
    background: rgba(239, 68, 68, 0.12);
  }

  .settings-group {
    background: linear-gradient(180deg, rgba(23, 31, 44, 0.8), rgba(15, 21, 32, 0.8));
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: var(--radius-md);
    padding: var(--spacing-lg);
  }

  .group-label {
    display: block;
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-secondary);
    margin-bottom: var(--spacing-md);
  }

  .setting-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 12px;
    background: rgba(0, 0, 0, 0.24);
    border-radius: var(--radius-sm);
    margin-bottom: var(--spacing-sm);
  }

  .setting-row label {
    color: var(--colors-text-primary);
    font-size: var(--font-size-body);
  }

  .setting-row input[type="text"],
  .setting-row input[type="url"],
  .setting-row input[type="number"],
  .setting-row select {
    width: 220px;
    max-width: 100%;
    padding: 8px;
    border: 1px solid var(--colors-border-input);
    border-radius: var(--radius-sm);
    background: var(--colors-bg-base);
    color: var(--colors-text-primary);
    text-align: right;
  }

  .setting-row.checkbox input {
    width: 18px;
    height: 18px;
  }

  .setting-hint {
    margin: 0 0 var(--spacing-sm);
    font-size: 11px;
    color: var(--colors-text-secondary);
  }

  .deadzone-wrap {
    width: 220px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
  }

  .deadzone-wrap input {
    width: 150px;
  }

  .monitor-picker-wrap {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    width: 320px;
    max-width: 100%;
  }

  .monitor-picker-wrap select {
    flex: 1;
    min-width: 0;
  }

  .mini-btn {
    padding: 7px 10px;
    font-size: 11px;
    white-space: nowrap;
  }

  .ghost-btn:disabled,
  .save-settings-btn:disabled,
  .mini-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .setting-info {
    max-width: 420px;
    font-size: 12px;
    color: var(--colors-text-secondary);
    text-align: right;
    line-height: 1.4;
  }

  .setting-info.full {
    text-align: left;
    max-width: none;
  }

  .advanced-group {
    color: var(--colors-text-secondary);
  }

  .advanced-group summary {
    cursor: pointer;
    font-size: 12px;
    font-weight: var(--font-weight-bold);
    margin: 0 var(--spacing-sm);
  }

  .advanced-inner {
    margin-top: var(--spacing-sm);
  }

  @media (max-width: 1040px) {
    .field-row {
      flex-direction: column;
      align-items: stretch;
    }

    .primary-btn {
      width: 100%;
    }

    .settings-header {
      flex-direction: column;
      align-items: stretch;
    }

    .settings-actions {
      justify-content: flex-end;
    }

    .setting-row {
      flex-direction: column;
      align-items: stretch;
    }

    .setting-row input[type="text"],
    .setting-row input[type="url"],
    .setting-row input[type="number"],
    .setting-row select,
    .deadzone-wrap,
    .monitor-picker-wrap {
      width: 100%;
      max-width: 100%;
      text-align: left;
      justify-content: flex-start;
    }
  }
</style>
