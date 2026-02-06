<script lang="ts">
    import { appState } from "$lib/appState.svelte";

    let email = $state("");
    let username = $state("");
    let password = $state("");
    let showAdvanced = $state(false);
    let customServer = $state("https://auth.wavry.dev");
    let isRegistering = $state(false);
    let isLoading = $state(false);
    let errorMessage = $state("");

    function normalizeError(error: unknown): string {
        if (error instanceof Error && error.message) return error.message;
        if (typeof error === "string") return error;
        try {
            return JSON.stringify(error);
        } catch {
            return "Unexpected authentication error.";
        }
    }

    function close() {
        appState.showLoginModal = false;
    }

    async function performAuth() {
        isLoading = true;
        errorMessage = "";

        const server = showAdvanced ? customServer.trim() : "https://auth.wavry.dev";
        if (showAdvanced) {
            try {
                const parsed = new URL(server);
                if (!["http:", "https:"].includes(parsed.protocol)) {
                    errorMessage = "Server URL must start with http:// or https://";
                    isLoading = false;
                    return;
                }
            } catch {
                errorMessage = "Server URL is invalid.";
                isLoading = false;
                return;
            }
        }
        appState.authServer = server;

        try {
            if (isRegistering) {
                await appState.register({
                    email,
                    password,
                    display_name: appState.displayName || "Unknown", // Fallback
                    username,
                });
                isRegistering = false;
                errorMessage = "Account created! Please sign in.";
            } else {
                await appState.login({ email, password });
                close();
            }
        } catch (e: unknown) {
            errorMessage = normalizeError(e);
        } finally {
            isLoading = false;
        }
    }
</script>

<div
    class="modal-overlay"
    onclick={close}
    onkeydown={(e) => e.key === "Escape" && close()}
    role="button"
    tabindex="-1"
>
    <div
        class="modal-content"
        onclick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        tabindex="-1"
        onkeydown={(e) => e.stopPropagation()}
    >
        <button class="close-btn" onclick={close}>✕</button>

        <div class="header">
            <div class="logo">
                <svg width="60" height="60" viewBox="0 0 48 48" fill="none">
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
            <h2>{isRegistering ? "Create Account" : "Sign In"}</h2>
            <p class="subtitle">
                {isRegistering
                    ? "Join Wavry to connect via username"
                    : "Sign in to sync your devices"}
            </p>
        </div>

        <form
            onsubmit={(e) => {
                e.preventDefault();
                performAuth();
            }}
        >
            <input
                type="email"
                placeholder="Email"
                bind:value={email}
                disabled={isLoading}
            />

            {#if isRegistering}
                <input
                    type="text"
                    placeholder="Username"
                    bind:value={username}
                    disabled={isLoading}
                />
            {/if}

            <input
                type="password"
                placeholder="Password"
                bind:value={password}
                disabled={isLoading}
            />

            <details class="advanced" bind:open={showAdvanced}>
                <summary>Advanced</summary>
                <div class="advanced-content">
                    <label for="custom-server">Server URL</label>
                    <input
                        id="custom-server"
                        type="url"
                        placeholder="https://auth.wavry.dev"
                        bind:value={customServer}
                        disabled={isLoading}
                    />
                </div>
            </details>

            {#if errorMessage}
                <div
                    class="message"
                    class:success={errorMessage.includes("created")}
                    aria-live="polite"
                >
                    {errorMessage}
                </div>
            {/if}

            <button
                type="submit"
                class="primary"
                disabled={isLoading ||
                    !email ||
                    !password ||
                    (isRegistering && !username)}
            >
                {#if isLoading}
                    <span class="spinner"></span>
                {/if}
                {isRegistering ? "Create Account" : "Sign In"}
            </button>
        </form>

        <button
            class="toggle-mode"
            onclick={() => (isRegistering = !isRegistering)}
        >
            {isRegistering
                ? "Already have an account? Sign In"
                : "Don't have an account? Register"}
        </button>
    </div>
</div>

<style>
    .modal-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 100;
    }

    .modal-content {
        background: var(--colors-bg-elevation1);
        border-radius: var(--radius-xxl);
        padding: var(--spacing-xxl);
        width: 380px;
        position: relative;
        border: 1px solid var(--colors-border-subtle);
    }

    .close-btn {
        position: absolute;
        top: var(--spacing-lg);
        right: var(--spacing-lg);
        background: var(--colors-bg-elevation3);
        border-radius: 50%;
        width: 28px;
        height: 28px;
        color: var(--colors-text-secondary);
        font-size: 12px;
    }

    .close-btn:hover {
        color: var(--colors-text-primary);
    }

    .header {
        text-align: center;
        margin-bottom: var(--spacing-xl);
    }

    .logo {
        margin-bottom: var(--spacing-md);
    }

    h2 {
        font-size: var(--font-size-titleMd);
        font-weight: var(--font-weight-light);
        color: var(--colors-text-primary);
        margin: 0;
    }

    .subtitle {
        color: var(--colors-text-secondary);
        font-size: var(--font-size-body);
        margin: var(--spacing-sm) 0 0;
    }

    form {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-md);
    }

    input {
        padding: var(--spacing-md);
        background: var(--colors-bg-base);
        border: 1px solid var(--colors-border-input);
        border-radius: var(--radius-md);
        color: var(--colors-text-primary);
        font-size: var(--font-size-body);
    }

    input::placeholder {
        color: var(--colors-text-secondary);
    }

    input:disabled {
        opacity: 0.5;
    }

    .advanced {
        color: var(--colors-text-secondary);
        font-size: var(--font-size-caption);
    }

    .advanced summary {
        cursor: pointer;
        user-select: none;
    }

    .advanced-content {
        margin-top: var(--spacing-sm);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    .advanced-content label {
        font-size: var(--font-size-caption);
        color: var(--colors-text-secondary);
    }

    .message {
        padding: var(--spacing-sm);
        border-radius: var(--radius-md);
        font-size: var(--font-size-caption);
        background: rgba(255, 59, 48, 0.1);
        color: var(--colors-accent-danger);
    }

    .message.success {
        background: rgba(52, 199, 89, 0.1);
        color: var(--colors-accent-success);
    }

    button.primary {
        background: var(--colors-accent-primary);
        color: white;
        padding: var(--spacing-lg);
        border-radius: var(--radius-md);
        font-weight: var(--font-weight-bold);
        display: flex;
        align-items: center;
        justify-content: center;
        gap: var(--spacing-sm);
    }

    button.primary:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .spinner {
        width: 14px;
        height: 14px;
        border: 2px solid transparent;
        border-top-color: white;
        border-radius: 50%;
        animation: spin 0.8s linear infinite;
    }

    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }

    .toggle-mode {
        display: block;
        margin: var(--spacing-lg) auto 0;
        color: var(--colors-text-secondary);
        font-size: var(--font-size-caption);
    }

    .toggle-mode:hover {
        color: var(--colors-text-primary);
    }
</style>
