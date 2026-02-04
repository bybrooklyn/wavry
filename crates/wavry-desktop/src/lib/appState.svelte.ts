import { invoke } from "@tauri-apps/api/core";

export interface DeltaConfig {
    target_delay_us: number;
    alpha: number;
    beta: number;
    increase_kbps: number;
    min_bitrate_kbps: number;
    max_bitrate_kbps: number;
    k_persistence: number;
    epsilon_us: number;
}

export class AppState {
    displayName = $state("");
    connectivityMode = $state<"wavry" | "direct" | "custom">("wavry");
    isSetupCompleted = $state(false);
    isAuthenticated = $state(false);
    username = $state("");
    showLoginModal = $state(false);

    // CC Stats
    ccBitrate = $state(0);
    ccState = $state("Stable");
    ccConfig = $state<DeltaConfig>({
        target_delay_us: 15000,
        alpha: 0.125,
        beta: 0.85,
        increase_kbps: 500,
        min_bitrate_kbps: 2000,
        max_bitrate_kbps: 50000,
        k_persistence: 3,
        epsilon_us: 100.0,
    });

    // Session state
    isHosting = $state(false);
    isConnected = $state(false);
    connectionStatus = $state<"offline" | "ready" | "connecting" | "connected">("offline");

    // Settings
    authServer = $state("https://auth.wavry.dev");
    hostPort = $state(8000);
    upnpEnabled = $state(true);

    completeSetup(name: string, mode: "wavry" | "direct" | "custom") {
        this.displayName = name;
        this.connectivityMode = mode;
        this.isSetupCompleted = true;
        localStorage.setItem("isSetupCompleted", "true");
        localStorage.setItem("displayName", name);
        localStorage.setItem("connectivityMode", mode);
    }

    loadFromStorage() {
        this.isSetupCompleted = localStorage.getItem("isSetupCompleted") === "true";
        this.displayName = localStorage.getItem("displayName") || "";
        this.connectivityMode = (localStorage.getItem("connectivityMode") as "wavry" | "direct" | "custom") || "wavry";
        this.username = localStorage.getItem("username") || "";
        this.isAuthenticated = !!this.username;

        // Recover token and sync with backend
        const token = localStorage.getItem("signaling_token");
        if (token) {
            invoke("set_signaling_token", { token });
        }
    }

    async register(details: any) {
        try {
            const res = await invoke("register", details);
            return res;
        } catch (e: any) {
            console.error("Registration failed:", e);
            throw e;
        }
    }

    async login(details: any) {
        try {
            const res = await invoke<any>("login_full", details);
            this.username = res.username;
            this.isAuthenticated = true;
            localStorage.setItem("username", res.username);
            localStorage.setItem("signaling_token", res.token);
            this.showLoginModal = false;
            return res.username;
        } catch (e: any) {
            console.error("Login failed:", e);
            throw e;
        }
    }

    logout() {
        this.username = "";
        this.isAuthenticated = false;
        localStorage.removeItem("username");
        localStorage.removeItem("signaling_token");
        invoke("set_signaling_token", { token: null });
    }

    get effectiveDisplayName() {
        return this.username || this.displayName || "Local Host";
    }

    async updateCCConfig() {
        try {
            await invoke("set_cc_config", { config: this.ccConfig });
        } catch (e) {
            console.error("Failed to update CC config:", e);
        }
    }

    private ccStatsInterval: any = null;
    startCCStatsPolling() {
        if (this.ccStatsInterval) return;
        this.ccStatsInterval = setInterval(async () => {
            if (!this.isHosting) {
                this.stopCCStatsPolling();
                return;
            }
            try {
                const stats: any = await invoke("get_cc_stats");
                this.ccBitrate = stats.bitrate_kbps;
                this.ccState = stats.state;
            } catch (e) {
                // Silently fail if session ended
            }
        }, 500);
    }

    stopCCStatsPolling() {
        if (this.ccStatsInterval) {
            clearInterval(this.ccStatsInterval);
            this.ccStatsInterval = null;
        }
    }

    async startHosting() {
        try {
            await invoke("start_host", { port: this.hostPort });
            this.isHosting = true;
            this.isConnected = true;
            this.startCCStatsPolling();
        } catch (e: any) {
            console.error("Failed to start host:", e);
        }
    }

    async stopHosting() {
        try {
            await invoke("stop_host");
            this.isHosting = false;
            this.isConnected = false;
            this.stopCCStatsPolling();
        } catch (e: any) {
            console.error("Failed to stop host:", e);
        }
    }

    // Client connection (invoked from +page.svelte)
    async connect(ip: string) {
        try {
            await invoke("start_session", { addr: ip, name: this.displayName });
            this.isConnected = true;
            this.isHosting = false;
        } catch (e: any) {
            console.error("Failed to connect:", e);
            throw e;
        }
    }

    async disconnect() {
        if (this.isHosting) {
            await this.stopHosting();
        } else {
            // TODO: stop client session command if it exists
            this.isConnected = false;
        }
    }
}

export const appState = new AppState();
