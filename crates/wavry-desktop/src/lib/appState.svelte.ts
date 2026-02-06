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
    pcvrStatus = $state("PCVR: Unknown");

    // Monitor state
    monitors = $state<{ id: number, name: string, resolution: { width: number, height: number } }[]>([]);
    selectedMonitorId = $state<number | null>(null);

    // Resolution state
    resolutionMode = $state<"native" | "client" | "custom">("native");
    customResolution = $state<{ width: number, height: number }>({ width: 1920, height: 1080 });

    // Gamepad state
    gamepadEnabled = $state(true);
    gamepadDeadzone = $state(0.1);

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

    async refreshPcvrStatus() {
        try {
            const status = await invoke<string>("get_pcvr_status");
            this.pcvrStatus = status;
        } catch (e) {
            console.error("Failed to get PCVR status:", e);
            this.pcvrStatus = "PCVR: Status unavailable";
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

    async loadMonitors() {
        try {
            const list: any[] = await invoke("list_monitors");
            this.monitors = list;
            if (list.length > 0 && this.selectedMonitorId === null) {
                this.selectedMonitorId = list[0].id;
            }
        } catch (e) {
            console.error("Failed to list monitors:", e);
        }
    }

    async connect(ip: string) {
        if (!ip) throw new Error("IP address is required");
        this.connectionStatus = "connecting";

        let resolution = null;
        if (this.resolutionMode === "custom") {
            resolution = this.customResolution;
        } else if (this.resolutionMode === "client") {
            resolution = { width: window.innerWidth, height: window.innerHeight };
        }

        const result = await invoke("start_session", {
            addr: ip,
            resolution_mode: this.resolutionMode,
            width: resolution?.width,
            height: resolution?.height
        });
        this.connectionStatus = "connected";
        this.isConnected = true;
        this.startCCStatsPolling();
        return result;
    }

    async startHosting() {
        try {
            await invoke("start_host", { port: this.hostPort, display_id: this.selectedMonitorId });
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
