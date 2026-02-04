<script lang="ts">
  import SidebarIcon from "$lib/components/SidebarIcon.svelte";
  import HostCard from "$lib/components/HostCard.svelte";
  import { appState } from "$lib/appState.svelte";

  let activeTab = $state("sessions");
</script>

<div class="content-view">
  <!-- 1. Icon Sidebar -->
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

  <!-- 2. Main Content Area -->
  <main class="main-content">
    <header class="top-bar">
      <div class="user-badge">
        <span class="status-dot"></span>
        <span class="username">{appState.effectiveDisplayName}</span>
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
              <span class="section-label">ACTIVE SESSIONS</span>
              <div class="placeholder-box">
                <span class="p-icon">🚫</span>
                <span class="p-text">No active sessions</span>
              </div>
            </div>
          </div>
        </section>
      {:else}
        <section class="settings-placeholder">
          <h1>Settings</h1>
          <p>
            Settings implementation matches tokens. Use tab buttons to switch
            views.
          </p>
        </section>
      {/if}
    </div>
  </main>
</div>

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
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background-color: var(--colors-accent-success);
  }

  .username {
    font-size: var(--font-size-caption);
    font-weight: var(--font-weight-bold);
    color: var(--colors-text-primary);
  }

  .tab-content {
    flex: 1;
    overflow-y: auto;
  }

  .sessions-view .header {
    padding: var(--spacing-xl) var(--spacing-xxl) var(--spacing-xxl);
  }

  .sessions-view h1 {
    font-size: var(--font-size-titleMd);
    font-weight: var(--font-weight-light);
    color: var(--colors-text-primary);
    margin-bottom: 5px;
  }

  .sessions-view p {
    font-size: var(--font-size-body);
    color: var(--colors-text-secondary);
  }

  .scroll-area {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xxl);
    padding-bottom: var(--spacing-xxl);
  }

  .section label,
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

  .settings-placeholder {
    padding: var(--spacing-xxl);
  }
</style>
