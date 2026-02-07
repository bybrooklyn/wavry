<script lang="ts">
    let { step = $bindable(0), onComplete, onOpenAuth } = $props<{
        step: number;
        onComplete: (name: string, mode: "wavry" | "direct" | "custom") => void;
        onOpenAuth?: (mode: "login" | "register") => void;
    }>();

    let displayName = $state("");
    let selectedMode = $state<"wavry" | "direct" | "custom">("wavry");

    function openAuth(mode: "login" | "register") {
        onOpenAuth?.(mode);
    }
</script>

<div class="wizard-container">
    <div class="wizard-card">
        {#if step === 0}
            <!-- Welcome Screen -->
            <div class="step welcome">
                <div class="logo">
                    <svg width="80" height="80" viewBox="0 0 48 48" fill="none">
                        <circle
                            cx="24"
                            cy="24"
                            r="22"
                            stroke="var(--colors-accent-primary)"
                            stroke-width="2.5"
                            fill="none"
                        />
                        <path
                            d="M16 20 L24 32 L32 20"
                            stroke="var(--colors-accent-primary)"
                            stroke-width="2.5"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        />
                    </svg>
                </div>

                <h1>Welcome to Wavry!</h1>
                <p class="subtitle">
                    Ultra-low latency remote desktop streaming.<br />Fast,
                    secure, and private.
                </p>

                <div class="actions">
                    <button class="primary" onclick={() => (step = 1)}
                        >Get Started</button
                    >
                    <button class="link" onclick={() => openAuth("login")}
                        >I already have an account</button
                    >
                </div>
            </div>
        {:else if step === 1}
            <!-- Identity Screen -->
            <div class="step identity">
                <div class="icon">
                    <svg
                        width="64"
                        height="64"
                        viewBox="0 0 24 24"
                        fill="var(--colors-accent-primary)"
                    >
                        <circle cx="12" cy="8" r="4" fill="currentColor" />
                        <path
                            d="M12 14c-4.42 0-8 1.79-8 4v2h16v-2c0-2.21-3.58-4-8-4z"
                            fill="currentColor"
                        />
                    </svg>
                </div>

                <h1>Set Your Host Name</h1>
                <p class="subtitle">
                    This name identifies your computer on the network.
                </p>

                <input
                    type="text"
                    placeholder="e.g. My Desktop"
                    bind:value={displayName}
                />

                <div class="button-row">
                    <button class="secondary" onclick={() => (step = 0)}
                        >Back</button
                    >
                    <button
                        class="primary"
                        disabled={!displayName.trim()}
                        onclick={() => (step = 2)}>Continue</button
                    >
                </div>
            </div>
        {:else}
            <!-- Connectivity Mode Screen -->
            <div class="step connectivity">
                <h1>Choose Connectivity</h1>
                <p class="subtitle">
                    How do you want to connect to other devices?
                </p>

                <div class="mode-options">
                    <button
                        class="mode-card"
                        class:selected={selectedMode === "wavry"}
                        onclick={() => (selectedMode = "wavry")}
                    >
                        <div class="mode-icon">☁️</div>
                        <div class="mode-info">
                            <span class="mode-title">Wavry Cloud</span>
                            <span class="mode-desc"
                                >Connect via username. Requires account.</span
                            >
                        </div>
                        {#if selectedMode === "wavry"}
                            <div class="check">✓</div>
                        {/if}
                    </button>

                    <button
                        class="mode-card"
                        class:selected={selectedMode === "direct"}
                        onclick={() => (selectedMode = "direct")}
                    >
                        <div class="mode-icon">🖧</div>
                        <div class="mode-info">
                            <span class="mode-title">LAN Only</span>
                            <span class="mode-desc"
                                >Direct IP connection. Fully offline.</span
                            >
                        </div>
                        {#if selectedMode === "direct"}
                            <div class="check">✓</div>
                        {/if}
                    </button>
                </div>

                {#if selectedMode === "wavry"}
                    <div class="cloud-note">
                        <strong>Cloud mode works best with an account.</strong>
                        <p>Create one now or sign in after setup to connect by username.</p>
                        <div class="cloud-actions">
                            <button class="link-chip" onclick={() => openAuth("register")}>
                                Create Account
                            </button>
                            <button class="link-chip" onclick={() => openAuth("login")}>
                                Sign In
                            </button>
                        </div>
                    </div>
                {/if}

                <div class="button-row">
                    <button class="secondary" onclick={() => (step = 1)}
                        >Back</button
                    >
                    <button
                        class="primary"
                        onclick={() => onComplete(displayName, selectedMode)}
                    >
                        Finish Setup
                    </button>
                </div>
            </div>
        {/if}
    </div>
</div>

<style>
    .wizard-container {
        display: flex;
        align-items: center;
        justify-content: center;
        height: 100vh;
        width: 100vw;
        background: var(--colors-bg-base);
    }

    .wizard-card {
        background: var(--colors-bg-elevation1);
        border-radius: var(--radius-xxl);
        padding: var(--spacing-xxxl);
        max-width: 500px;
        width: 90%;
        border: 1px solid var(--colors-border-subtle);
    }

    .step {
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: var(--spacing-xl);
    }

    .logo,
    .icon {
        margin-bottom: var(--spacing-md);
    }

    h1 {
        font-size: var(--font-size-titleMd);
        font-weight: var(--font-weight-light);
        color: var(--colors-text-primary);
        margin: 0;
    }

    .subtitle {
        color: var(--colors-text-secondary);
        font-size: var(--font-size-body);
        line-height: 1.5;
        margin: 0;
    }

    .actions {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
        width: 100%;
        margin-top: var(--spacing-lg);
    }

    button.primary {
        background: var(--colors-accent-primary);
        color: white;
        padding: var(--spacing-lg) var(--spacing-xxl);
        border-radius: var(--radius-xl);
        font-weight: var(--font-weight-bold);
        font-size: var(--font-size-body);
        width: 100%;
        transition: opacity 0.2s;
    }

    button.primary:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    button.primary:hover:not(:disabled) {
        opacity: 0.9;
    }

    button.secondary {
        background: var(--colors-bg-elevation3);
        color: var(--colors-text-primary);
        padding: var(--spacing-lg) var(--spacing-xxl);
        border-radius: var(--radius-xl);
        font-weight: var(--font-weight-semibold);
    }

    button.link {
        background: transparent;
        color: var(--colors-text-secondary);
        font-size: var(--font-size-body);
    }

    button.link:hover {
        color: var(--colors-text-primary);
    }

    .button-row {
        display: flex;
        gap: var(--spacing-lg);
        width: 100%;
        margin-top: var(--spacing-lg);
    }

    .button-row button.primary {
        flex: 1;
    }

    input {
        width: 100%;
        padding: var(--spacing-lg);
        background: var(--colors-bg-base);
        border: 1px solid var(--colors-border-input);
        border-radius: var(--radius-xl);
        font-size: var(--font-size-input);
        color: var(--colors-text-primary);
    }

    input::placeholder {
        color: var(--colors-text-secondary);
    }

    .mode-options {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-lg);
        width: 100%;
    }

    .mode-card {
        display: flex;
        align-items: center;
        gap: var(--spacing-xl);
        padding: var(--spacing-lg);
        background: var(--colors-bg-base);
        border-radius: var(--radius-xl);
        border: 1px solid transparent;
        text-align: left;
        transition: all 0.2s;
    }

    .mode-card.selected {
        border-color: var(--colors-accent-primary);
        background: rgba(0, 122, 255, 0.1);
    }

    .mode-icon {
        font-size: 24px;
        width: 50px;
        height: 50px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--colors-bg-elevation2);
        border-radius: var(--radius-md);
    }

    .mode-info {
        flex: 1;
        display: flex;
        flex-direction: column;
        gap: 4px;
    }

    .mode-title {
        font-weight: var(--font-weight-semibold);
        color: var(--colors-text-primary);
    }

    .mode-desc {
        font-size: var(--font-size-caption);
        color: var(--colors-text-secondary);
    }

    .check {
        color: var(--colors-accent-primary);
        font-size: 18px;
    }

    .cloud-note {
        width: 100%;
        padding: var(--spacing-md);
        border-radius: var(--radius-xl);
        border: 1px solid rgba(58, 130, 246, 0.35);
        background: rgba(58, 130, 246, 0.1);
        text-align: left;
    }

    .cloud-note strong {
        display: block;
        color: var(--colors-text-primary);
        font-size: var(--font-size-caption);
    }

    .cloud-note p {
        margin: var(--spacing-xs) 0 0;
        color: var(--colors-text-secondary);
        font-size: var(--font-size-caption);
        line-height: 1.4;
    }

    .cloud-actions {
        margin-top: var(--spacing-sm);
        display: flex;
        gap: var(--spacing-sm);
    }

    .link-chip {
        padding: 6px 10px;
        border-radius: 999px;
        background: rgba(0, 0, 0, 0.24);
        border: 1px solid rgba(255, 255, 255, 0.12);
        color: var(--colors-text-primary);
        font-size: var(--font-size-caption);
    }

    .link-chip:hover {
        border-color: rgba(255, 255, 255, 0.24);
    }
</style>
