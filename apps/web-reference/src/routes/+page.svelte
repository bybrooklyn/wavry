<script lang="ts">
  import SidebarIcon from "$lib/components/SidebarIcon.svelte";
  import LoginModal from "$lib/components/LoginModal.svelte";
  import VideoPlayer from "$lib/components/VideoPlayer.svelte";
  import { appState } from "$lib/appState.svelte";
  import { onMount } from "svelte";

  let activeTab = $state("sessions");
  let remoteUsername = $state("");
  let isConnecting = $state(false);
  let connectError = $state("");

  onMount(async () => {
    await appState.initialize();
  });

  async function startSession() {
    isConnecting = true;
    connectError = "";
    appState.hostErrorMessage = "";
    appState.hostStatusMessage = "Starting direct session...";
    
    if (!remoteUsername.trim()) {
      connectError = "Username is required for web client connection.";
      isConnecting = false;
      return;
    }

    try {
      await appState.connect(remoteUsername.trim());
      appState.hostStatusMessage = `Connected to ${remoteUsername.trim()}`;
    } catch (e: any) {
      connectError = e.message || "Connection failed";
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
    } catch (e: any) {
      connectError = e.message || "Disconnection failed";
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

  function openLogin() {
    appState.openAuthModal("login");
  }

  function openRegister() {
    appState.openAuthModal("register");
  }
</script>

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
        {#if appState.ccBitrate > 0}
          <div
            class="performance-badge"
            class:warning={appState.ccState === "Congested"}
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
          class:connected={appState.isConnected}
          class:idle={!appState.isConnected}
        >
          {sessionStatusLabel()}
        </span>
      </div>

      <div
        class="user-badge"
        onclick={() => !appState.isAuthenticated && openLogin()}
        onkeydown={(e) =>
          (e.key === "Enter" || e.key === " ") &&
          !appState.isAuthenticated &&
          openLogin()}
        role="button"
        tabindex="0"
        aria-label={appState.isAuthenticated
          ? `Signed in as ${appState.username}`
          : "Sign in"}
      >
        <span class="status-dot" class:online={appState.isAuthenticated}></span>
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
      {#if appState.isConnected}
        <div class="fullscreen-video">
          <VideoPlayer stream={appState.remoteStream} />
          <div class="video-overlay">
            <button class="disconnect-overlay-btn" onclick={disconnectSession}>Disconnect</button>
          </div>
        </div>
      {:else}
        {#if activeTab === "sessions"}
          <section class="sessions-view">
            <div class="header">
              <h1>Web Client Sessions</h1>
              <p>Connect to a host via WebTransport and WebRTC.</p>
            </div>

            <div class="scroll-area session-grid">
              <section class="surface-card">
                <div class="section-head">
                  <span class="section-label">Connect to Host</span>
                </div>

                <div class="connect-panel">
                  {#if !appState.isAuthenticated}
                    <div class="auth-gate">
                      <strong>Cloud connect requires an account.</strong>
                      <p>Sign in or create one now to connect to hosts by username from any device.</p>
                      <div class="auth-actions">
                        <button class="primary-btn" onclick={openLogin}>
                          Sign In
                        </button>
                        <button class="ghost-btn auth-cta" onclick={openRegister}>
                          Create Account
                        </button>
                      </div>
                    </div>
                  {:else}
                    <p class="field-help">Enter the username of the host you want to connect to.</p>
                    <label class="field-label" for="remote-username">Host Username</label>
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
                          startSession()}
                      />
                      <button
                        class="primary-btn"
                        onclick={startSession}
                        disabled={isConnecting || !remoteUsername.trim()}
                      >
                        {#if isConnecting}Connecting...{:else}Connect{/if}
                      </button>
                    </div>
                  {/if}

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
            </div>
          </section>
        {:else if activeTab === "settings"}
          <section class="settings-view">
            <div class="header settings-header">
              <div>
                <h1>Settings</h1>
                <p>Configure web client settings.</p>
              </div>
              <div class="settings-actions">
                <button class="save-settings-btn" onclick={() => appState.saveToStorage()}>Save</button>
              </div>
            </div>
            <div class="settings-group">
              <span class="group-label">General</span>
              <div class="setting-row">
                <label for="setting-display-name">Client Name</label>
                <input
                  id="setting-display-name"
                  type="text"
                  bind:value={appState.displayName}
                  placeholder="My Web Client"
                />
              </div>
            </div>
            <div class="settings-group">
              <span class="group-label">Network</span>
              <div class="setting-row">
                <label for="setting-auth-server">Auth/Gateway Server</label>
                <input
                  id="setting-auth-server"
                  type="url"
                  bind:value={appState.authServer}
                  placeholder="http://localhost:3000"
                />
              </div>
            </div>
          </section>
        {/if}
      {/if}
    </div>
  </main>
</div>

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

  .field-help {
    margin: 0;
    font-size: 11px;
    color: var(--colors-text-secondary);
    line-height: 1.4;
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

  .primary-btn {
    padding: 10px 12px;
    background: var(--colors-accent-primary);
    color: white;
    font-weight: 600;
    white-space: nowrap;
    border-radius: 6px;
  }

  .auth-gate {
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 8px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .auth-actions {
    display: flex;
    gap: 8px;
  }

  .ghost-btn {
    padding: 10px 12px;
    border: 1px solid var(--colors-border-subtle);
    border-radius: 6px;
    color: var(--colors-text-primary);
  }

  .error {
    color: var(--colors-accent-danger);
    font-size: 12px;
  }

  .success {
    color: var(--colors-accent-success);
    font-size: 12px;
  }

  .active-session {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .session-line {
    display: flex;
    justify-content: space-between;
    padding: 8px 10px;
    background: rgba(0, 0, 0, 0.25);
    border-radius: 6px;
    font-size: 12px;
  }

  .danger-btn {
    padding: 10px;
    background: var(--colors-accent-danger);
    color: white;
    border-radius: 6px;
    font-weight: 600;
  }

  .placeholder-box {
    min-height: 110px;
    border: 1px dashed rgba(255, 255, 255, 0.16);
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--colors-text-secondary);
  }

  .settings-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-xl) var(--spacing-xxl);
  }

  .save-settings-btn {
    padding: 8px 16px;
    background: var(--colors-accent-primary);
    color: white;
    border-radius: 6px;
  }

  .settings-group {
    padding: 0 var(--spacing-xxl) var(--spacing-xl);
  }

  .group-label {
    display: block;
    font-size: 11px;
    text-transform: uppercase;
    font-weight: bold;
    color: var(--colors-text-secondary);
    margin-bottom: var(--spacing-md);
  }

  .setting-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px;
    background: rgba(0, 0, 0, 0.2);
    border-radius: 6px;
    margin-bottom: 8px;
  }

  .setting-row input {
    background: var(--colors-bg-base);
    border: 1px solid var(--colors-border-input);
    padding: 6px 10px;
    border-radius: 4px;
    width: 200px;
    text-align: right;
  }

  .fullscreen-video {
    width: 100%;
    height: 100%;
    position: relative;
  }

  .video-overlay {
    position: absolute;
    top: var(--spacing-md);
    right: var(--spacing-md);
    opacity: 0;
    transition: opacity 0.2s;
  }

  .fullscreen-video:hover .video-overlay {
    opacity: 1;
  }

  .disconnect-overlay-btn {
    background: rgba(255, 59, 48, 0.8);
    color: white;
    padding: 8px 16px;
    border-radius: var(--radius-md);
    font-weight: 600;
    backdrop-filter: blur(4px);
  }
</style>