<script lang="ts">
    import { appState } from "$lib/appState.svelte";

    let email = $state("");
    let username = $state("");
    let password = $state("");
    let showAdvanced = $state(false);
    let customServer = $state("https://auth.wavry.dev"); // Default for web client
    let isRegistering = $state(appState.authModalMode === "register");
    let isLoading = $state(false);
    let feedbackMessage = $state("");
    let feedbackType = $state<"error" | "success">("error");
    let emailValue = $derived(email.trim());
    let usernameValue = $derived(username.trim());
    const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    const USERNAME_PATTERN = /^[a-zA-Z0-9_.-]{3,32}$/;

    function normalizeError(error: unknown): string {
        const raw =
            error instanceof Error && error.message
                ? error.message
                : typeof error === "string"
                    ? error
                    : "";
        const lowered = raw.toLowerCase();
        if (lowered.includes("timed out")) {
            return "The server took too long to respond. Please try again.";
        }
        if (lowered.includes("invalid credentials") || lowered.includes("invalid email or password")) {
            return "Incorrect email or password.";
        }
        if (lowered.includes("already exists") || lowered.includes("already registered")) {
            return "This account already exists. Sign in instead.";
        }
        if (raw) return raw;
        try {
            return JSON.stringify(error);
        } catch {
            return "Unexpected authentication error.";
        }
    }

    function setMode(mode: "login" | "register") {
        isRegistering = mode === "register";
        appState.authModalMode = mode;
        feedbackMessage = "";
    }

    function validateInputs(): string | null {
        if (!emailValue) return "Email is required.";
        if (!EMAIL_PATTERN.test(emailValue)) return "Enter a valid email address.";
        if (!password) return "Password is required.";

        if (isRegistering) {
            if (!usernameValue) return "Username is required.";
            if (!USERNAME_PATTERN.test(usernameValue)) {
                return "Username must be 3-32 characters and use letters, numbers, ., _, or -.";
            }
            if (password.length < 8) return "Password must be at least 8 characters.";
        }

        return null;
    }

    function resolveServer(): string | null {
        const server = showAdvanced ? customServer.trim() : "https://auth.wavry.dev"; // Default for web client
        if (!showAdvanced) return server;
        try {
            const parsed = new URL(server);
            if (!["http:", "https:"].includes(parsed.protocol)) {
                return null;
            }
            return server;
        } catch {
            return null;
        }
    }

    function close() {
        appState.closeAuthModal();
    }

    async function performAuth() {
        feedbackMessage = "";
        feedbackType = "error";
        const validationError = validateInputs();
        if (validationError) {
            feedbackMessage = validationError;
            return;
        }

        const server = resolveServer();
        if (!server) {
            feedbackMessage = "Server URL is invalid. Use http:// or https://.";
            return;
        }

        isLoading = true;
        appState.authServer = server;

        try {
            if (isRegistering) {
                // Register logic (using fetch for web)
                const res = await fetch(`${server}/auth/register`, {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ email: emailValue, password, username: usernameValue }),
                });
                if (!res.ok) throw new Error(await res.text());
                setMode("login");
                password = "";
                feedbackType = "success";
                feedbackMessage = "Account created. Sign in with the same email and password.";
            } else {
                // Login logic (using appState.login for web)
                await appState.login({ email: emailValue, password });
                close();
            }
        } catch (e: any) {
            feedbackType = "error";
            feedbackMessage = normalizeError(e);
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
        onkeydown={(e) => e.key === "Escape" && close()}
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
                    ? "Create one account to connect all hosts by username."
                    : "Use the email from registration to access cloud connect."}
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
                autocomplete="email"
            />

            {#if isRegistering}
                <input
                    type="text"
                    placeholder="Username (3-32 chars)"
                    bind:value={username}
                    disabled={isLoading}
                    autocomplete="username"
                />
            {/if}

            <input
                type="password"
                placeholder="Password"
                bind:value={password}
                disabled={isLoading}
                autocomplete={isRegistering ? "new-password" : "current-password"}
            />

            {#if isRegistering}
                <div class="hint">Password must be at least 8 characters.</div>
            {/if}

            <details class="advanced" bind:open={showAdvanced}>
                <summary>Advanced</summary>
                <div class="advanced-content">
                    <label for="custom-server">Server URL</label>
                    <input
                        id="custom-server"
                        type="url"
                        bind:value={customServer}
                        placeholder="http://localhost:3000"
                        disabled={isLoading}
                    />
                </div>
            </details>

            {#if feedbackMessage}
                <div
                    class="message"
                    class:success={feedbackType === "success"}
                    aria-live="polite"
                >
                    {feedbackMessage}
                </div>
            {/if}

            <button
                type="submit"
                class="primary"
                disabled={isLoading ||
                    !emailValue ||
                    !password ||
                    (isRegistering && !usernameValue)}
            >
                {#if isLoading}
                    <span class="spinner"></span>
                {/if}
                {isRegistering ? "Create Account" : "Sign In"}
            </button>
        </form>

        <button
            class="toggle-mode"
            onclick={() => setMode(isRegistering ? "login" : "register")}
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

    .hint {
        color: var(--colors-text-secondary);
        font-size: var(--font-size-caption);
        margin-top: calc(var(--spacing-xs) * -1);
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
