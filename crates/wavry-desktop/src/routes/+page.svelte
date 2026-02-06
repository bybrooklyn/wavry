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

  // Settings state
  let showAdvancedSettings = $state(false);

  onMount(() => {
    appState.loadFromStorage();
    appState.refreshPcvrStatus();
  });

  async function startSession() {
    isConnecting = true;
    connectError = "";
    try {
      await appState.connect(connectIp);
    } catch (e: any) {
      connectError = e.toString();
    } finally {
      isConnecting = false;
    }
  }

  async function connectViaId() {
    isConnecting = true;
    connectError = "";
    try {
      await invoke("connect_via_id", { targetUsername: remoteUsername });
    } catch (e: any) {
      connectError = e.toString();
    } finally {
      isConnecting = false;
    }
  }

  function handleSetupComplete(
    name: string,
    mode: "wavry" | "direct" | "custom",
  ) {
    appState.completeSetup(name, mode);
  }
</script>

{#if !appState.isSetupCompleted}
  <SetupWizard step={setupStep} onComplete={handleSetupComplete} />
{:else}
  <div class="content-view">
    <!-- Sidebar -->
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

    <!-- Main Content -->
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
              <p>Manage your local host and active connections.</p>
            </div>

            <div class="scroll-area">
              <div class="section">
                <span class="section-label">LOCAL HOST</span>
                <div class="card-container">
                  <HostCard {appState} />
                </div>
              </div>

              <div class="section">
                <span class="section-label">CONNECT TO HOST</span>
                <div class="card-container">
                  <div class="connect-box">
                    {#if appState.connectivityMode === "wavry"}
                      <input
                        type="text"
                        placeholder="Username (e.g. john)"
                        bind:value={remoteUsername}
                      />
                      <button
                        onclick={connectViaId}
                        disabled={isConnecting || !remoteUsername}
                      >
                        {#if isConnecting}Connecting...{:else}Connect via ID{/if}
                      </button>
                    {:else}
                      <input
                        type="text"
                        placeholder="Host IP (e.g. 192.168.1.5:8000)"
                        bind:value={connectIp}
                      />
                      <button
                        onclick={startSession}
                        disabled={isConnecting || !connectIp}
                      >
                        {#if isConnecting}Connecting...{:else}Connect{/if}
                      </button>
                    {/if}
                    {#if connectError}
                      <div class="error">{connectError}</div>
                    {/if}
                  </div>
                </div>
              </div>

              <div class="section">
                <span class="section-label">ACTIVE SESSIONS</span>
                <div class="placeholder-box">
                  <span class="p-icon">🚫</span>
                  <span class="p-text">No active sessions</span>
                </div>
              </div>
            </div>
          </section>
        {:else}
          <section class="settings-view">
            <div class="header">
              <h1>Settings</h1>
              <p>Configure your Wavry experience.</p>
            </div>

            <div class="pcvr-banner">{appState.pcvrStatus}</div>

            <div class="scroll-area">
              <div class="settings-group">
                <span class="group-label">IDENTITY</span>
                <div class="setting-row">
                  <label>Host Name</label>
                  <input
                    type="text"
                    bind:value={appState.displayName}
                    placeholder="My Desktop"
                  />
                </div>
              </div>

              <div class="settings-group">
                <span class="group-label">NETWORK</span>
                <div class="setting-row">
                  <label>Connectivity Mode</label>
                  <select bind:value={appState.connectivityMode}>
                    <option value="wavry">Wavry Cloud</option>
                    <option value="direct">LAN Only</option>
                    <option value="custom">Custom Server</option>
                  </select>
                </div>

                <div class="setting-row">
                  <label>Host Port</label>
                  <input type="number" bind:value={appState.hostPort} />
                </div>

                <div class="setting-row checkbox">
                  <label>Enable UPnP</label>
                  <input type="checkbox" bind:checked={appState.upnpEnabled} />
                </div>
              </div>

              <div class="settings-group">
                <span class="group-label">DISPLAY</span>
                <div class="setting-row">
                  <label>Client Resolution</label>
                  <select bind:value={appState.resolutionMode}>
                    <option value="native">Use Host Native</option>
                    <option value="client">Match This Client</option>
                    <option value="custom">Custom Fixed</option>
                  </select>
                </div>

                {#if appState.resolutionMode === "custom"}
                  <div class="setting-row">
                    <label>Width</label>
                    <input
                      type="number"
                      bind:value={appState.customResolution.width}
                    />
                  </div>
                  <div class="setting-row">
                    <label>Height</label>
                    <input
                      type="number"
                      bind:value={appState.customResolution.height}
                    />
                  </div>
                {/if}
              </div>

              <div class="settings-group">
                <span class="group-label">INPUT</span>
                <div class="setting-row">
                  <label>Gamepad Passthrough</label>
                  <input
                    type="checkbox"
                    bind:checked={appState.gamepadEnabled}
                  />
                </div>
                {#if appState.gamepadEnabled}
                  <div class="setting-row">
                    <label>Deadzone</label>
                    <input
                      type="range"
                      min="0"
                      max="0.5"
                      step="0.05"
                      bind:value={appState.gamepadDeadzone}
                    />
                    <span>{appState.gamepadDeadzone}</span>
                  </div>
                {/if}
              </div>

              <div class="settings-group">
                <span class="group-label">VR / PCVR</span>
                <div class="setting-row">
                  <label>PCVR Adapter</label>
                  <div class="setting-info">
                    Linux/Windows OpenXR client. Wayland supported via Vulkan, X11 via OpenGL.
                    Transport stays in Wavry/RIFT (no ALVR networking).
                  </div>
                </div>
              </div>

              <details class="advanced-group" bind:open={showAdvancedSettings}>
                <summary>Advanced</summary>
                <div class="settings-group">
                  <div class="setting-row">
                    <label>Gateway URL</label>
                    <input
                      type="url"
                      bind:value={appState.authServer}
                      placeholder="https://auth.wavry.dev"
                    />
                  </div>
                </div>
              </details>

              <div class="settings-group">
                <span class="group-label">TUNING</span>
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
  }

  .sidebar {
    width: 60px;
    background-color: var(--colors-bg-sidebar);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--spacing-xl) 0;
    gap: var(--spacing-xl);
  }

  .spacer {
    flex: 1;
  }

  .main-content {
    flex: 1;
    background-color: var(--colors-bg-base);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .top-bar {
    display: flex;
    justify-content: flex-end;
    padding: var(--spacing-xl) var(--spacing-xxl) 0;
  }

  .user-badge {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    background-color: var(--colors-bg-elevation3);
    border-radius: var(--radius-md);
    cursor: pointer;
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
    margin-bottom: 5px;
  }

  .sessions-view p,
  .settings-view p {
    font-size: var(--font-size-body);
    color: var(--colors-text-secondary);
  }

  .pcvr-banner {
    margin: 0 var(--spacing-xxl) var(--spacing-xxl);
    padding: var(--spacing-md) var(--spacing-lg);
    border-radius: var(--radius-md);
    background: var(--colors-bg-elevation2);
    border: 1px solid var(--colors-border-subtle);
    color: var(--colors-text-primary);
    font-size: var(--font-size-caption);
  }

  .scroll-area {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xxl);
    padding-bottom: var(--spacing-xxl);
  }

  .section .section-label {
    display: block;
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-secondary);
    padding: 0 var(--spacing-xxl);
    margin-bottom: 10px;
  }

  .card-container {
    padding: 0 var(--spacing-xxl);
  }

  .placeholder-box {
    margin: 0 var(--spacing-xxl);
    height: 120px;
    background-color: var(--colors-bg-elevation1);
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
  }

  .p-icon {
    font-size: 30px;
    opacity: 0.3;
  }

  .p-text {
    font-size: var(--font-size-body);
    color: var(--colors-text-secondary);
    opacity: 0.5;
  }

  .connect-box {
    display: flex;
    gap: 10px;
    flex-direction: column;
    background: var(--colors-bg-elevation1);
    padding: 15px;
    border-radius: var(--radius-md);
  }

  .connect-box input {
    padding: 10px;
    border-radius: 4px;
    border: 1px solid var(--colors-border-input);
    background: var(--colors-bg-base);
    color: white;
  }

  .connect-box button {
    padding: 10px;
    background: var(--colors-accent-primary);
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .connect-box button:disabled {
    opacity: 0.5;
  }

  .error {
    color: var(--colors-accent-danger);
    font-size: 12px;
  }

  /* Settings Styles */
  .settings-group {
    padding: 0 var(--spacing-xxl);
  }

  .group-label {
    display: block;
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-secondary);
    margin-bottom: var(--spacing-md);
  }

  .setting-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-md) var(--spacing-lg);
    background: var(--colors-bg-elevation1);
    border-radius: var(--radius-md);
    margin-bottom: var(--spacing-sm);
  }

  .setting-row label {
    color: var(--colors-text-primary);
    font-size: var(--font-size-body);
  }

  .setting-info {
    max-width: 360px;
    font-size: var(--font-size-caption);
    color: var(--colors-text-secondary);
    text-align: right;
    line-height: 1.4;
  }

  .setting-row input[type="text"],
  .setting-row input[type="url"],
  .setting-row input[type="number"] {
    width: 200px;
    padding: var(--spacing-sm);
    border: 1px solid var(--colors-border-input);
    border-radius: var(--radius-sm);
    background: var(--colors-bg-base);
    color: var(--colors-text-primary);
    text-align: right;
  }

  .setting-row select {
    padding: var(--spacing-sm);
    border: 1px solid var(--colors-border-input);
    border-radius: var(--radius-sm);
    background: var(--colors-bg-base);
    color: var(--colors-text-primary);
  }

  .setting-row.checkbox input {
    width: 18px;
    height: 18px;
  }

  .advanced-group {
    padding: 0 var(--spacing-xxl);
    color: var(--colors-text-secondary);
  }

  .advanced-group summary {
    cursor: pointer;
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-bold);
    margin-bottom: var(--spacing-md);
  }

  /* Performance Badge */
  .status-indicators {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .performance-badge {
    display: flex;
    align-items: center;
    gap: 8px;
    background: rgba(0, 0, 0, 0.4);
    padding: 6px 12px;
    border-radius: 20px;
    border: 1px solid rgba(255, 255, 255, 0.05);
    font-size: 11px;
    font-family: monospace;
    backdrop-filter: blur(10px);
  }

  .performance-badge .label {
    color: #10b981; /* emerald-500 */
    font-weight: bold;
  }

  .performance-badge.rising .label {
    color: #f59e0b; /* amber-500 */
  }

  .performance-badge.warning .label {
    color: #ef4444; /* red-500 */
  }

  .performance-badge .value {
    color: rgba(255, 255, 255, 0.7);
  }
</style>
