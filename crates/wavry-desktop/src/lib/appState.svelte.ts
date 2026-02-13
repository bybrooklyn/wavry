import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

interface LinuxRuntimeDiagnostics {
    session_type: string;
    wayland_display: boolean;
    x11_display: boolean;
    xdg_current_desktop: string | null;
    expected_portal_backends: string[];
    expected_portal_descriptors: string[];
    available_portal_descriptors: string[];
    missing_expected_portal_descriptors: string[];
    required_video_source: string;
    required_video_source_available: boolean;
    available_audio_sources: string[];
    available_h264_encoders: string[];
    missing_gstreamer_elements: string[];
    recommendations: string[];
}

interface LinuxHostPreflight {
    requested_display_id: number | null;
    selected_display_id: number;
    selected_display_name: string;
    selected_resolution: { width: number; height: number };
    diagnostics: LinuxRuntimeDiagnostics;
}

export class AppState {
    displayName = $state("");
    connectivityMode = $state<"wavry" | "direct" | "custom">("wavry");
    isSetupCompleted = $state(false);
    isAuthenticated = $state(false);
    username = $state("");
    signalingToken = $state<string | null>(null);
    showLoginModal = $state(false);
    authModalMode = $state<"login" | "register">("login");

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
    isHostTransitioning = $state(false);
    connectionStatus = $state<"offline" | "ready" | "connecting" | "connected">("offline");
    hostStatusMessage = $state("");
    hostErrorMessage = $state("");
    pcvrStatus = $state("PCVR: Unknown");

    // Monitor state
    monitors = $state<{ id: number, name: string, resolution: { width: number, height: number } }[]>([]);
    selectedMonitorId = $state<number | null>(null);
    isLoadingMonitors = $state(false);
    linuxRuntimeDiagnostics = $state<LinuxRuntimeDiagnostics | null>(null);
    linuxPreflightSummary = $state("");

    // Resolution state
    resolutionMode = $state<"native" | "client" | "custom">("native");
    customResolution = $state<{ width: number, height: number }>({ width: 1920, height: 1080 });

    // Gamepad state
    gamepadEnabled = $state(true);
    gamepadDeadzone = $state(0.1);

    // Settings
    authServer = $state("https://auth.wavry.dev");
    hostPort = $state(0);
    upnpEnabled = $state(true);

    private parseStoredNumber(key: string, fallback: number): number {
        const raw = localStorage.getItem(key);
        if (raw == null) return fallback;
        const parsed = Number(raw);
        return Number.isFinite(parsed) ? parsed : fallback;
    }

    private normalizeError(error: unknown): string {
        if (error instanceof Error && error.message) return error.message;
        if (typeof error === "string") return error;
        if (error && typeof error === "object") {
            try {
                return JSON.stringify(error);
            } catch {
                return "Unexpected error";
            }
        }
        return "Unexpected error";
    }

    private clamp(value: number, min: number, max: number): number {
        return Math.min(max, Math.max(min, value));
    }

    private isLinuxOnlyCommandError(message: string): boolean {
        return message.toLowerCase().includes("only available on linux builds");
    }

    private sanitizeSettings() {
        this.hostPort = Math.trunc(this.clamp(this.hostPort, 0, 65535));
        this.gamepadDeadzone = this.clamp(this.gamepadDeadzone, 0, 0.5);
        this.customResolution = {
            width: Math.trunc(this.clamp(this.customResolution.width, 640, 8192)),
            height: Math.trunc(this.clamp(this.customResolution.height, 480, 8192)),
        };
    }

    validateSettingsInputs(): string | null {
        if (!Number.isInteger(this.hostPort) || this.hostPort < 0 || this.hostPort > 65535) {
            return "Host port must be an integer between 0 and 65535 (0 = random).";
        }

        if (this.connectivityMode === "custom") {
            try {
                const parsed = new URL(this.authServer);
                if (!["http:", "https:"].includes(parsed.protocol)) {
                    return "Custom gateway URL must start with http:// or https://.";
                }
            } catch {
                return "Custom gateway URL is invalid.";
            }
        }

        if (this.resolutionMode === "custom") {
            if (
                !Number.isInteger(this.customResolution.width) ||
                !Number.isInteger(this.customResolution.height)
            ) {
                return "Custom resolution width and height must be whole numbers.";
            }
            if (
                this.customResolution.width < 640 ||
                this.customResolution.width > 8192 ||
                this.customResolution.height < 480 ||
                this.customResolution.height > 8192
            ) {
                return "Custom resolution must be within 640x480 and 8192x8192.";
            }
        }

        if (this.gamepadDeadzone < 0 || this.gamepadDeadzone > 0.5) {
            return "Gamepad deadzone must be between 0.00 and 0.50.";
        }

        return null;
    }

    validateConnectTarget(target: string): string | null {
        const value = target.trim();
        if (!value) return "Host address is required.";

        if (value.includes("://")) {
            return "Use host or host:port only (no URL scheme).";
        }

        let port: string | null = null;
        if (value.startsWith("[")) {
            const closing = value.indexOf("]");
            if (closing <= 1) return "Invalid bracketed IPv6 host format.";
            if (value.length > closing + 1) {
                if (value[closing + 1] !== ":") return "Invalid host format.";
                port = value.slice(closing + 2);
            }
        } else {
            const colonCount = (value.match(/:/g) ?? []).length;
            if (colonCount === 1) {
                const idx = value.lastIndexOf(":");
                if (idx === 0 || idx === value.length - 1) {
                    return "Host and port must both be provided.";
                }
                port = value.slice(idx + 1);
            } else if (colonCount > 1 && value.includes(".")) {
                return "Invalid host format.";
            }
        }

        if (port != null) {
            if (!/^\d+$/.test(port)) return "Port must be numeric.";
            const parsed = Number(port);
            if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) {
                return "Port must be between 1 and 65535.";
            }
        }

        return null;
    }

    saveToStorage() {
        this.sanitizeSettings();
        localStorage.setItem("isSetupCompleted", this.isSetupCompleted ? "true" : "false");
        localStorage.setItem("displayName", this.displayName);
        localStorage.setItem("connectivityMode", this.connectivityMode);
        localStorage.setItem("authServer", this.authServer);
        localStorage.setItem("hostPort", String(this.hostPort));
        localStorage.setItem("upnpEnabled", this.upnpEnabled ? "true" : "false");
        localStorage.setItem("resolutionMode", this.resolutionMode);
        localStorage.setItem("customResolutionWidth", String(this.customResolution.width));
        localStorage.setItem("customResolutionHeight", String(this.customResolution.height));
        localStorage.setItem("gamepadEnabled", this.gamepadEnabled ? "true" : "false");
        localStorage.setItem("gamepadDeadzone", String(this.gamepadDeadzone));
        if (this.selectedMonitorId != null) {
            localStorage.setItem("selectedMonitorId", String(this.selectedMonitorId));
        } else {
            localStorage.removeItem("selectedMonitorId");
        }
    }

    resetSettingsToDefaults() {
        this.connectivityMode = "wavry";
        this.authServer = "https://auth.wavry.dev";
        this.hostPort = 0;
        this.upnpEnabled = true;
        this.resolutionMode = "native";
        this.customResolution = { width: 1920, height: 1080 };
        this.gamepadEnabled = true;
        this.gamepadDeadzone = 0.1;
        this.selectedMonitorId = this.monitors.length > 0 ? this.monitors[0].id : null;
        this.hostStatusMessage = "Settings reset to defaults. Save to keep them.";
        this.hostErrorMessage = "";
    }

    completeSetup(name: string, mode: "wavry" | "direct" | "custom") {
        this.displayName = name;
        this.connectivityMode = mode;
        this.isSetupCompleted = true;
        this.saveToStorage();
    }

    openAuthModal(mode: "login" | "register" = "login") {
        this.authModalMode = mode;
        this.showLoginModal = true;
    }

    closeAuthModal() {
        this.showLoginModal = false;
    }

    async initialize() {
        this.isSetupCompleted = localStorage.getItem("isSetupCompleted") === "true";
        this.displayName = localStorage.getItem("displayName") || "";
        const mode = localStorage.getItem("connectivityMode");
        this.connectivityMode = mode === "wavry" || mode === "direct" || mode === "custom" ? mode : "wavry";
        
        try {
            this.username = await invoke<string | null>("load_secure_data", { key: "username" }) || "";
            this.signalingToken = await invoke<string | null>("load_secure_token");
            this.isAuthenticated = !!this.username && !!this.signalingToken;
            if (this.signalingToken) {
                await invoke("set_signaling_token", { token: this.signalingToken, server: this.authServer });
            }
        } catch (e) {
            console.error("Failed to load secure state:", e);
            this.isAuthenticated = false;
        }

        this.authServer = localStorage.getItem("authServer") || "https://auth.wavry.dev";
        this.hostPort = this.parseStoredNumber("hostPort", 0);
        this.upnpEnabled = localStorage.getItem("upnpEnabled") !== "false";
        const resolutionMode = localStorage.getItem("resolutionMode");
        this.resolutionMode =
            resolutionMode === "native" || resolutionMode === "client" || resolutionMode === "custom"
                ? resolutionMode
                : "native";
        this.customResolution = {
            width: this.parseStoredNumber("customResolutionWidth", 1920),
            height: this.parseStoredNumber("customResolutionHeight", 1080),
        };
        this.gamepadEnabled = localStorage.getItem("gamepadEnabled") !== "false";
        this.gamepadDeadzone = this.parseStoredNumber("gamepadDeadzone", 0.1);
        const storedMonitor = localStorage.getItem("selectedMonitorId");
        const parsedMonitor = storedMonitor == null ? null : Number(storedMonitor);
        this.selectedMonitorId = parsedMonitor != null && Number.isFinite(parsedMonitor) ? parsedMonitor : null;
        this.sanitizeSettings();
        await this.refreshLinuxRuntimeHealth();

        listen("host-error", (event: any) => {
            const payload = event.payload;
            console.error("Host error received:", payload);
            this.hostErrorMessage = `Host error: ${payload.message}`;
            if (!payload.can_retry) {
                this.isHosting = false;
                this.isConnected = false;
                this.connectionStatus = "offline";
                this.stopCCStatsPolling();
            } else {
                this.hostStatusMessage = "Host error occurred. Retrying automatically...";
            }
        });
    }

    async register(details: any) {
        try {
            const res = await invoke("register", details);
            return res;
        } catch (e: any) {
            console.error("Registration failed:", e);
            throw new Error(this.normalizeError(e));
        }
    }

    async login(details: any) {
        try {
            const res = await invoke<any>("login_full", details);
            this.username = res.username;
            this.signalingToken = res.token;
            this.isAuthenticated = true;
            // Note: login_full already saves the token and username to secure storage on the Rust side
            this.authModalMode = "login";
            this.showLoginModal = false;
            return res.username;
        } catch (e: any) {
            console.error("Login failed:", e);
            throw new Error(this.normalizeError(e));
        }
    }

    async logout() {
        this.username = "";
        this.signalingToken = null;
        this.isAuthenticated = false;
        await invoke("delete_secure_token");
        await invoke("delete_secure_data", { key: "username" });
        await invoke("set_signaling_token", { token: null, server: this.authServer });
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
            if (!this.isHosting && !this.isConnected) {
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
        this.isLoadingMonitors = true;
        try {
            const list: any[] = await invoke("list_monitors");
            this.monitors = list;
            if (list.length === 0) {
                this.selectedMonitorId = null;
                localStorage.removeItem("selectedMonitorId");
                return;
            }

            if (
                this.selectedMonitorId === null ||
                !list.some((monitor) => monitor.id === this.selectedMonitorId)
            ) {
                this.selectedMonitorId = list[0].id;
            }
            this.saveToStorage();
            await this.refreshLinuxRuntimeHealth();
        } catch (e: unknown) {
            console.error("Failed to list monitors:", e);
            this.hostErrorMessage = `Failed to list monitors: ${this.normalizeError(e)}`;
        } finally {
            this.isLoadingMonitors = false;
        }
    }

    async refreshLinuxRuntimeHealth() {
        try {
            const diagnostics = await invoke<LinuxRuntimeDiagnostics>("linux_runtime_health");
            this.linuxRuntimeDiagnostics = diagnostics;
        } catch (e: unknown) {
            const message = this.normalizeError(e);
            if (this.isLinuxOnlyCommandError(message)) return;
            console.warn("Failed to load Linux runtime diagnostics:", message);
        }
    }

    private async runLinuxHostPreflight(): Promise<LinuxHostPreflight | null> {
        try {
            const preflight = await invoke<LinuxHostPreflight>("linux_host_preflight", {
                display_id: this.selectedMonitorId,
            });
            this.linuxRuntimeDiagnostics = preflight.diagnostics;
            this.linuxPreflightSummary = `${preflight.selected_display_name} (${preflight.selected_resolution.width}x${preflight.selected_resolution.height})`;
            return preflight;
        } catch (e: unknown) {
            const message = this.normalizeError(e);
            if (this.isLinuxOnlyCommandError(message)) return null;
            throw new Error(message);
        }
    }

    async connect(ip: string) {
        const target = ip.trim();
        const addressError = this.validateConnectTarget(target);
        if (addressError) throw new Error(addressError);
        const settingsError = this.validateSettingsInputs();
        if (settingsError) throw new Error(settingsError);

        this.hostErrorMessage = "";
        this.hostStatusMessage = `Connecting to ${target}...`;
        this.connectionStatus = "connecting";
        this.isConnected = false;

        let resolution = null;
        if (this.resolutionMode === "custom") {
            resolution = this.customResolution;
        } else if (this.resolutionMode === "client") {
            resolution = { width: window.innerWidth, height: window.innerHeight };
        }

        try {
            const result = await invoke("start_session", {
                addr: target,
                resolution_mode: this.resolutionMode,
                width: resolution?.width,
                height: resolution?.height,
                gamepad_enabled: this.gamepadEnabled,
                gamepad_deadzone: this.gamepadDeadzone,
            });
            this.connectionStatus = "connected";
            this.isConnected = true;
            this.hostStatusMessage = `Session started with ${target}`;
            this.startCCStatsPolling();
            return result;
        } catch (e: unknown) {
            this.connectionStatus = "offline";
            this.isConnected = false;
            const message = this.normalizeError(e);
            this.hostStatusMessage = "";
            this.hostErrorMessage = `Connection failed: ${message}`;
            throw new Error(message);
        }
    }

    async startHosting() {
        if (this.isHostTransitioning) return;
        const settingsError = this.validateSettingsInputs();
        if (settingsError) {
            const message = settingsError;
            this.hostErrorMessage = message;
            throw new Error(message);
        }

        this.hostErrorMessage = "";
        this.hostStatusMessage = "Starting host...";
        this.isHostTransitioning = true;
        try {
            const preflight = await this.runLinuxHostPreflight();
            if (preflight) {
                this.hostStatusMessage = `Linux preflight OK: ${this.linuxPreflightSummary}. Starting host...`;
            }

            const backendMessage = await invoke<string>("start_host", {
                port: this.hostPort,
                display_id: this.selectedMonitorId,
            });
            this.isHosting = true;
            this.isConnected = true;
            this.connectionStatus = "connected";
            const normalized = typeof backendMessage === "string" && backendMessage.trim()
                ? backendMessage.trim()
                : (this.hostPort === 0 ? "Hosting on random UDP port" : `Hosting on UDP ${this.hostPort}`);
            this.hostStatusMessage = normalized;
            this.saveToStorage();
            this.startCCStatsPolling();
        } catch (e: unknown) {
            console.error("Failed to start host:", e);
            const message = this.normalizeError(e);
            this.hostStatusMessage = "";
            this.hostErrorMessage = `Failed to start host: ${message}`;
            throw new Error(message);
        } finally {
            this.isHostTransitioning = false;
        }
    }

    async stopHosting() {
        if (this.isHostTransitioning) return;
        this.isHostTransitioning = true;
        this.hostErrorMessage = "";
        this.hostStatusMessage = "Stopping host...";
        try {
            await invoke("stop_host");
            this.isHosting = false;
            this.isConnected = false;
            this.connectionStatus = "offline";
            this.hostStatusMessage = "Hosting stopped";
            this.stopCCStatsPolling();
        } catch (e: unknown) {
            console.error("Failed to stop host:", e);
            const message = this.normalizeError(e);
            this.hostStatusMessage = "";
            this.hostErrorMessage = `Failed to stop host: ${message}`;
            throw new Error(message);
        } finally {
            this.isHostTransitioning = false;
        }
    }


    async disconnect() {
        if (this.isHosting) {
            await this.stopHosting();
        } else {
            let stopErrorMessage: string | null = null;
            try {
                await invoke("stop_session");
            } catch (e: unknown) {
                const message = this.normalizeError(e);
                if (!message.includes("No active client session")) {
                    stopErrorMessage = message;
                }
            }
            this.isConnected = false;
            this.connectionStatus = "offline";
            this.stopCCStatsPolling();
            if (stopErrorMessage) {
                this.hostStatusMessage = "";
                this.hostErrorMessage = `Failed to stop session: ${stopErrorMessage}`;
                throw new Error(stopErrorMessage);
            }
            this.hostStatusMessage = "Session disconnected.";
            this.hostErrorMessage = "";
        }
    }

    async sendFileTransferCommand(fileId: number, action: "pause" | "resume" | "cancel" | "retry") {
        if (!Number.isInteger(fileId) || fileId <= 0) {
            throw new Error("File ID must be a positive integer.");
        }
        const response = await invoke<string>("send_file_transfer_command", {
            file_id: fileId,
            action,
        });
        return response;
    }
}

export const appState = new AppState();
