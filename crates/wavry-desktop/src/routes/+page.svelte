<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  import { appState } from "$lib/appState.svelte";
  import HostCard from "$lib/components/HostCard.svelte";
  import LoginModal from "$lib/components/LoginModal.svelte";
  import SetupWizard from "$lib/components/SetupWizard.svelte";
  import SidebarIcon from "$lib/components/SidebarIcon.svelte";

  let activeTab = $state<"sessions" | "settings">("sessions");
  let activeSettingsTab = $state<
    "client" | "host" | "network" | "hotkeys" | "account"
  >("client");

  let connectIp = $state("127.0.0.1");
  let remoteUsername = $state("");
  let isConnecting = $state(false);
  let connectError = $state("");
  let isMacOS = $state(false);

  let setupStep = $state(0);
  let saveFeedback = $state("");
  let saveFeedbackType = $state<"success" | "error">("success");
  let baselineSettingsFingerprint = $state("");
  let overlayEnabled = $state(true);
  let windowMode = $state("Fullscreen");
  let fpsLimit = $state("60 FPS");

  const USERNAME_PATTERN = /^[a-zA-Z0-9_.-]{3,32}$/;

  function normalizeError(err: unknown): string {
    if (err instanceof Error) return err.message;
    if (typeof err === "string") return err;
    return JSON.stringify(err);
  }

  function normalizeConnectError(err: unknown): string {
    const raw = normalizeError(err);
    const lowered = raw.toLowerCase();

    if (lowered.includes("timed out")) {
      return "Connection timed out. Check that the remote host is online and reachable.";
    }
    if (lowered.includes("connection rejected")) {
      return "The host rejected this request.";
    }
    if (lowered.includes("not logged in")) {
      return "Sign in to use username-based cloud connect.";
    }
    if (lowered.includes("client session already active")) {
      return "A client session is already running. Disconnect first, then retry.";
    }

    return raw;
  }

  function openAuth(mode: "login" | "register" = "login") {
    appState.openAuthModal(mode);
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

  onMount(async () => {
    isMacOS = /mac/i.test(navigator.userAgent);
    await appState.initialize();
    appState.refreshPcvrStatus();
    appState.loadMonitors();
    captureSettingsBaseline();
  });

  async function startSession() {
    isConnecting = true;
    connectError = "";
    appState.hostErrorMessage = "";
    appState.hostStatusMessage = "Starting direct session...";

    const addressError = appState.validateConnectTarget(connectIp);
    if (addressError) {
      connectError = addressError;
      isConnecting = false;
      return;
    }

    try {
      await appState.connect(connectIp.trim());
    } catch (e) {
      connectError = normalizeConnectError(e);
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
      openAuth("login");
      connectError = "Sign in first to connect via username.";
      return;
    }
    if (!username) {
      isConnecting = false;
      connectError = "Username is required.";
      return;
    }
    if (!USERNAME_PATTERN.test(username)) {
      isConnecting = false;
      connectError = "Username must be 3-32 characters and use letters, numbers, ., _, or -.";
      return;
    }

    try {
      appState.hostStatusMessage = `Sending cloud request to ${username}...`;
      await invoke("connect_via_id", { targetUsername: username });
      appState.hostStatusMessage = `Connected to ${username}`;
    } catch (e) {
      connectError = normalizeConnectError(e);
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
    if (mode === "wavry") return "Wavry Cloud";
    if (mode === "direct") return "LAN Only";
    return "Custom Server";
  }

  function handleSetupComplete(
    name: string,
    mode: "wavry" | "direct" | "custom",
  ) {
    appState.completeSetup(name, mode);
    if (mode === "wavry" && !appState.isAuthenticated) {
      appState.hostStatusMessage = "Create an account to connect by username.";
      openAuth("register");
    }
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
</script>

{#if !appState.isSetupCompleted}
  <SetupWizard step={setupStep} onComplete={handleSetupComplete} onOpenAuth={openAuth} />
{:else}
  <div class="content-view" class:macos={isMacOS}>
    <div class="window-drag-strip" data-tauri-drag-region></div>

    <aside class="sidebar">
      <SidebarIcon
        icon="tabSessions"
        active={activeTab === "sessions"}
        onclick={() => (activeTab = "sessions")}
      />

      <div class="spacer"></div>

      <SidebarIcon
        icon="tabSettings"
        active={activeTab === "settings"}
        onclick={() => (activeTab = "settings")}
      />
    </aside>

    <main class="main-content">
      <header class="top-bar">
        <div
          class="user-badge"
          onclick={() => !appState.isAuthenticated && openAuth("login")}
          onkeydown={(e) =>
            (e.key === "Enter" || e.key === " ") &&
            !appState.isAuthenticated &&
            openAuth("login")}
          role="button"
          tabindex="0"
          aria-label={appState.isAuthenticated
            ? `Signed in as ${appState.username}`
            : "Sign in"}
        >
          <span class="status-dot" class:online={appState.isAuthenticated}></span>
          <span class="username">{appState.effectiveDisplayName}</span>
        </div>
      </header>

      {#if activeTab === "sessions"}
        <section class="sessions-view">
          <div class="view-header">
            <h1>Sessions</h1>
            <p>Manage your local host and active connections.</p>
          </div>

          {#if connectError || appState.hostErrorMessage || appState.hostStatusMessage}
            <div class="message-box">
              {#if connectError}
                <div class="error-message">{connectError}</div>
              {/if}
              {#if appState.hostErrorMessage}
                <div class="error-message">{appState.hostErrorMessage}</div>
              {/if}
              {#if appState.hostStatusMessage}
                <div class="success-message">{appState.hostStatusMessage}</div>
              {/if}
            </div>
          {/if}

          <div class="scroll-panel">
            <div class="session-section">
              <h2>LOCAL HOST</h2>
              <HostCard {appState} />
            </div>

            <div class="session-section">
              <h2>REMOTE CONNECTION</h2>

              <div class="remote-content">
                {#if appState.isConnected && !appState.isHosting}
                  <div class="video-placeholder">Remote session connected</div>
                  <button class="danger-btn" onclick={disconnectSession}>Disconnect</button>
                {:else}
                  {#if appState.isHosting}
                    <p class="helper-text">
                      Hosting is active. Stop hosting before starting a client connection.
                    </p>
                  {/if}

                  {#if appState.connectivityMode !== "direct"}
                    <div class="connect-row">
                      <input
                        type="text"
                        placeholder="Username or ID"
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
                        {#if isConnecting}Connecting...{:else}Connect{/if}
                      </button>
                    </div>

                    <div class="or-text">OR</div>
                  {/if}

                  <div class="connect-row">
                    <input
                      type="text"
                      placeholder="Host IP"
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
                      {#if isConnecting}Connecting...{:else}Connect Directly{/if}
                    </button>
                  </div>
                {/if}
              </div>
            </div>
          </div>
        </section>
      {:else}
        <section class="settings-view">
          <div class="view-header settings-header">
            <h1>Settings</h1>
            <p>Customize your Wavry experience.</p>
          </div>

          <div class="settings-tabs" role="tablist" aria-label="Settings tabs">
            <button class="settings-tab" class:active={activeSettingsTab === "client"} onclick={() => (activeSettingsTab = "client")}>Client</button>
            <button class="settings-tab" class:active={activeSettingsTab === "host"} onclick={() => (activeSettingsTab = "host")}>Host</button>
            <button class="settings-tab" class:active={activeSettingsTab === "network"} onclick={() => (activeSettingsTab = "network")}>Network</button>
            <button class="settings-tab" class:active={activeSettingsTab === "hotkeys"} onclick={() => (activeSettingsTab = "hotkeys")}>Hotkeys</button>
            <button class="settings-tab" class:active={activeSettingsTab === "account"} onclick={() => (activeSettingsTab = "account")}>Account</button>
            <span class="settings-version">Version 0.1.0-native</span>
          </div>

          <div class="settings-divider"></div>

          {#if saveFeedback}
            <div class="settings-message">
              <div class={saveFeedbackType === "error" ? "error-message" : "success-message"}>
                {saveFeedback}
              </div>
            </div>
          {/if}

          <div class="settings-scroll">
            {#if activeSettingsTab === "client"}
              <div class="settings-group">
                <h3>DISPLAY</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Overlay</div>
                    <div class="setting-sub">Show Wavry overlay during session.</div>
                  </div>
                  <input type="checkbox" bind:checked={overlayEnabled} />
                </div>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Window Mode</div>
                    <div class="setting-sub">Start Wavry in fullscreen or windowed mode.</div>
                  </div>
                  <select bind:value={windowMode}>
                    <option value="Fullscreen">Fullscreen</option>
                    <option value="Windowed">Windowed</option>
                  </select>
                </div>
              </div>

              <div class="settings-group">
                <h3>PERFORMANCE</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">FPS Limit</div>
                    <div class="setting-sub">Limit the client frame rate.</div>
                  </div>
                  <select bind:value={fpsLimit}>
                    <option value="30 FPS">30 FPS</option>
                    <option value="60 FPS">60 FPS</option>
                    <option value="120 FPS">120 FPS</option>
                  </select>
                </div>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Decoder</div>
                    <div class="setting-sub">Preferred video decoding method.</div>
                  </div>
                  <span class="setting-value">Hardware (VideoToolbox)</span>
                </div>
              </div>

              <div class="settings-group">
                <h3>VR / PCVR</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">PCVR Adapter</div>
                    <div class="setting-sub">
                      Linux/Windows clients use OpenXR via ALVR adapter. Wayland via Vulkan; X11 via OpenGL.
                    </div>
                  </div>
                  <span class="setting-value">Info</span>
                </div>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">PCVR Status</div>
                    <div class="setting-sub">Runtime path on this machine.</div>
                  </div>
                  <span class="setting-value">{appState.pcvrStatus}</span>
                </div>
              </div>
            {:else if activeSettingsTab === "host"}
              <div class="settings-group">
                <h3>HOSTING</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Host Name</div>
                    <div class="setting-sub">Identifies your computer to others.</div>
                  </div>
                  <input type="text" bind:value={appState.displayName} placeholder="My Desktop" />
                </div>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Host Start Port</div>
                    <div class="setting-sub">Starting port for host listeners.</div>
                  </div>
                  <input type="number" min="0" max="65535" bind:value={appState.hostPort} />
                </div>
              </div>

              <div class="settings-group">
                <h3>HARDWARE</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Display</div>
                    <div class="setting-sub">Select which monitor to capture.</div>
                  </div>
                  <div class="monitor-picker-wrap">
                    <select bind:value={appState.selectedMonitorId}>
                      {#if appState.monitors.length === 0}
                        <option value={null}>No displays detected</option>
                      {:else}
                        {#each appState.monitors as monitor}
                          <option value={monitor.id}>
                            {monitor.name} ({monitor.resolution.width}x{monitor.resolution.height})
                          </option>
                        {/each}
                      {/if}
                    </select>
                    <button class="ghost-btn" onclick={() => appState.loadMonitors()} disabled={appState.isLoadingMonitors}>
                      {appState.isLoadingMonitors ? "Loading..." : "Refresh"}
                    </button>
                  </div>
                </div>
              </div>
            {:else if activeSettingsTab === "network"}
              <div class="settings-group">
                <h3>CONNECTIVITY MODE</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Mode</div>
                    <div class="setting-sub">LAN Only disables cloud features (no login, no relay).</div>
                  </div>
                  <select bind:value={appState.connectivityMode}>
                    <option value="wavry">Wavry Cloud</option>
                    <option value="direct">LAN Only</option>
                    <option value="custom">Custom Server</option>
                  </select>
                </div>
                <p class="setting-help">{modeLabel(appState.connectivityMode)}</p>

                {#if appState.connectivityMode === "custom"}
                  <div class="setting-row">
                    <div class="setting-copy">
                      <div class="setting-label">Gateway URL</div>
                      <div class="setting-sub">Custom signaling server address.</div>
                    </div>
                    <input type="url" bind:value={appState.authServer} placeholder="https://auth.wavry.dev" />
                  </div>
                {/if}

                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">UPnP</div>
                    <div class="setting-sub">Enable automatic port forwarding.</div>
                  </div>
                  <input type="checkbox" bind:checked={appState.upnpEnabled} />
                </div>
              </div>
            {:else if activeSettingsTab === "hotkeys"}
              <div class="settings-group">
                <h3>HOTKEYS</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-sub">No hotkeys configured yet.</div>
                  </div>
                </div>
              </div>
            {:else}
              <div class="settings-group">
                <h3>IDENTITY</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Account</div>
                    <div class="setting-sub">Current signed-in identity.</div>
                  </div>
                  <span class="setting-value">{appState.isAuthenticated ? appState.username : "Not signed in"}</span>
                </div>
              </div>

              <div class="settings-group">
                <h3>INFRASTRUCTURE</h3>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Auth Server</div>
                    <div class="setting-sub">Wavry signaling server for cloud connect.</div>
                  </div>
                  <input type="url" bind:value={appState.authServer} placeholder="https://auth.wavry.dev" />
                </div>
                <div class="setting-row">
                  <div class="setting-copy">
                    <div class="setting-label">Session</div>
                    <div class="setting-sub">Manage authentication state.</div>
                  </div>
                  {#if appState.isAuthenticated}
                    <button class="ghost-btn" onclick={() => appState.logout()}>Sign Out</button>
                  {:else}
                    <button class="primary-btn" onclick={() => openAuth("login")}>Sign In</button>
                  {/if}
                </div>
              </div>
            {/if}
          </div>

          <footer class="settings-footer">
            <button class="ghost-btn" onclick={() => appState.loadMonitors()} disabled={appState.isLoadingMonitors}>
              {appState.isLoadingMonitors ? "Refreshing..." : "Refresh Displays"}
            </button>
            <button class="primary-btn" onclick={saveSettings} disabled={!hasUnsavedSettings}>Apply Changes</button>
          </footer>
        </section>
      {/if}
    </main>
  </div>
{/if}

{#if appState.showLoginModal}
  <LoginModal />
{/if}

<style>
  .content-view {
    display: flex;
    width: 100vw;
    height: 100vh;
    position: relative;
    background:
      radial-gradient(circle at 18% -24%, rgba(71, 103, 185, 0.24), transparent 46%),
      radial-gradient(circle at 90% 5%, rgba(39, 145, 121, 0.2), transparent 42%),
      linear-gradient(145deg, rgba(33, 35, 43, 0.82), rgba(27, 29, 36, 0.76));
    backdrop-filter: blur(18px) saturate(1.08);
    -webkit-backdrop-filter: blur(18px) saturate(1.08);
  }

  .window-drag-strip {
    position: absolute;
    inset: 0 0 auto 0;
    height: 38px;
    z-index: 0;
  }

  .content-view.macos .window-drag-strip {
    height: 56px;
  }

  .sidebar {
    width: 60px;
    padding: 20px 0;
    background: rgba(10, 12, 15, 0.72);
    border-right: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
    backdrop-filter: blur(16px) saturate(1.08);
    -webkit-backdrop-filter: blur(16px) saturate(1.08);
    position: relative;
    z-index: 1;
  }

  .content-view.macos .sidebar {
    padding-top: 58px;
  }

  .spacer {
    flex: 1;
  }

  .main-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    position: relative;
    z-index: 1;
  }

  .top-bar {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    padding: 20px 32px 0;
  }

  .user-badge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    background: var(--colors-bg-elevation3);
    border-radius: var(--radius-md);
    border: 1px solid rgba(255, 255, 255, 0.08);
    cursor: pointer;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.2);
  }

  .status-dot.online {
    background: var(--colors-accent-success);
    box-shadow: 0 0 10px rgba(52, 199, 89, 0.6);
  }

  .username {
    font-size: 12px;
    line-height: 1;
    font-weight: 700;
    color: var(--colors-text-primary);
  }

  .sessions-view,
  .settings-view {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .view-header {
    padding: 20px 32px 0;
  }

  .view-header h1 {
    margin: 0;
    font-size: 32px;
    font-weight: 300;
    color: var(--colors-text-primary);
  }

  .settings-header h1 {
    font-size: 36px;
  }

  .view-header p {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--colors-text-secondary);
  }

  .message-box,
  .settings-message {
    margin: 12px 32px 0;
    padding: 12px;
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.04);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .error-message,
  .success-message {
    font-size: 11px;
    line-height: 1.35;
  }

  .error-message {
    color: var(--colors-accent-danger);
  }

  .success-message {
    color: var(--colors-accent-success);
  }

  .scroll-panel,
  .settings-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 32px;
    display: flex;
    flex-direction: column;
    gap: 32px;
  }

  .session-section,
  .settings-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .session-section h2,
  .settings-group h3 {
    margin: 0;
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    font-weight: 700;
    color: rgba(0, 122, 255, 0.82);
  }

  .remote-content {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    border-radius: 12px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(255, 255, 255, 0.03);
  }

  .video-placeholder {
    height: 240px;
    border-radius: 12px;
    border: 1px solid rgba(52, 199, 89, 0.5);
    background: rgba(0, 0, 0, 0.3);
    color: rgba(255, 255, 255, 0.65);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
  }

  .helper-text,
  .or-text,
  .setting-help {
    margin: 0;
    font-size: 11px;
    color: var(--colors-text-secondary);
  }

  .or-text {
    text-align: center;
    letter-spacing: 0.08em;
  }

  .connect-row {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.05);
  }

  .connect-row input {
    flex: 1;
    min-width: 0;
    padding: 10px;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    background: rgba(0, 0, 0, 0.22);
    color: var(--colors-text-primary);
    font-size: 14px;
  }

  .settings-tabs {
    display: flex;
    align-items: flex-end;
    gap: 20px;
    padding: 20px 32px 8px;
  }

  .settings-tab {
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--colors-text-secondary);
    font-size: 14px;
    line-height: 1;
    padding: 0 0 8px;
    border-bottom: 2px solid transparent;
  }

  .settings-tab.active {
    color: var(--colors-accent-primary);
    border-bottom-color: var(--colors-accent-primary);
    font-weight: 700;
  }

  .settings-version {
    margin-left: auto;
    padding-bottom: 8px;
    color: rgba(255, 255, 255, 0.36);
    font-size: 10px;
    line-height: 1;
  }

  .settings-divider {
    height: 1px;
    margin: 0 32px;
    background: rgba(255, 255, 255, 0.08);
  }

  .setting-row {
    display: flex;
    gap: 14px;
    align-items: center;
    justify-content: space-between;
    padding: 12px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.025);
  }

  .setting-copy {
    min-width: 0;
    flex: 1;
  }

  .setting-label {
    color: var(--colors-text-primary);
    font-size: 13px;
    font-weight: 600;
    line-height: 1.25;
  }

  .setting-sub {
    margin-top: 4px;
    color: var(--colors-text-secondary);
    font-size: 11px;
    line-height: 1.35;
  }

  .setting-value {
    color: var(--colors-text-secondary);
    font-size: 12px;
    text-align: right;
    max-width: 340px;
  }

  .setting-row input[type="text"],
  .setting-row input[type="url"],
  .setting-row input[type="number"],
  .setting-row select {
    width: 240px;
    max-width: 100%;
    padding: 8px;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: rgba(0, 0, 0, 0.22);
    color: var(--colors-text-primary);
    text-align: right;
    font-size: 12px;
  }

  .setting-row input[type="checkbox"] {
    width: 18px;
    height: 18px;
    accent-color: var(--colors-accent-primary);
  }

  .monitor-picker-wrap {
    width: 320px;
    max-width: 100%;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }

  .monitor-picker-wrap select {
    flex: 1;
    min-width: 0;
  }

  .settings-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 20px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(0, 0, 0, 0.24);
  }

  .settings-footer .primary-btn {
    margin-left: auto;
    padding-left: 32px;
    padding-right: 32px;
  }

  .primary-btn,
  .ghost-btn,
  .danger-btn {
    border-radius: 8px;
    padding: 10px 14px;
    font-size: 12px;
    font-weight: 700;
    line-height: 1;
    cursor: pointer;
  }

  .primary-btn {
    border: 1px solid rgba(255, 255, 255, 0.16);
    background: var(--colors-accent-primary);
    color: #fff;
  }

  .ghost-btn {
    border: 1px solid rgba(255, 255, 255, 0.16);
    background: rgba(255, 255, 255, 0.06);
    color: var(--colors-text-primary);
  }

  .danger-btn {
    border: 1px solid rgba(255, 255, 255, 0.16);
    background: var(--colors-accent-danger);
    color: #fff;
  }

  .primary-btn:hover:enabled,
  .ghost-btn:hover:enabled,
  .danger-btn:hover:enabled {
    filter: brightness(1.05);
  }

  .primary-btn:disabled,
  .ghost-btn:disabled,
  .danger-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  @media (max-width: 1040px) {
    .top-bar,
    .view-header,
    .message-box,
    .settings-message,
    .settings-tabs,
    .scroll-panel,
    .settings-scroll {
      padding-left: 20px;
      padding-right: 20px;
    }

    .settings-divider {
      margin-left: 20px;
      margin-right: 20px;
    }
  }

  @media (max-width: 780px) {
    .connect-row,
    .setting-row,
    .monitor-picker-wrap,
    .settings-footer {
      flex-direction: column;
      align-items: stretch;
    }

    .primary-btn,
    .ghost-btn,
    .danger-btn,
    .settings-footer .primary-btn {
      width: 100%;
      margin-left: 0;
    }

    .setting-row input[type="text"],
    .setting-row input[type="url"],
    .setting-row input[type="number"],
    .setting-row select,
    .monitor-picker-wrap,
    .setting-value {
      width: 100%;
      max-width: 100%;
      text-align: left;
      justify-content: flex-start;
    }

    .settings-tabs {
      flex-wrap: wrap;
      gap: 12px;
    }

    .settings-version {
      width: 100%;
      margin-left: 0;
    }
  }

  @media (max-width: 760px) {
    .content-view {
      flex-direction: column;
    }

    .sidebar {
      width: 100%;
      height: 64px;
      padding: 0 12px;
      flex-direction: row;
      justify-content: center;
      border-right: none;
      border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    }

    .spacer {
      display: none;
    }

    .top-bar {
      justify-content: flex-end;
      padding-top: 14px;
    }

    .view-header h1 {
      font-size: 30px;
    }

    .settings-header h1 {
      font-size: 32px;
    }
  }
</style>
