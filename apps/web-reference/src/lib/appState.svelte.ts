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
    signalingToken = $state<string | null>(null);
    showLoginModal = $state(false);
    authModalMode = $state<"login" | "register">("login");

    // CC Stats
    ccBitrate = $state(0);
    ccState = $state("Stable");
    
    // Session state
    isHosting = $state(false);
    isConnected = $state(false);
    isHostTransitioning = $state(false);
    connectionStatus = $state<"offline" | "ready" | "connecting" | "connected">("offline");
    hostStatusMessage = $state("");
    hostErrorMessage = $state("");

    // Settings
    authServer = $state("https://auth.wavry.dev"); // Default local gateway
    
    // WebRTC for media
    private pc: RTCPeerConnection | null = null;
    remoteStream = $state<MediaStream | null>(null);

    // Transport and signaling
    private ws = $state<WebSocket | null>(null);
    private transport = $state<any>(null);
    private controlWriter: WritableStreamDefaultWriter | null = null;

    constructor() {
        if (typeof window !== "undefined") {
            this.loadFromStorage();
        }
    }

    private loadFromStorage() {
        this.isSetupCompleted = localStorage.getItem("isSetupCompleted") === "true";
        this.displayName = localStorage.getItem("displayName") || "";
        const mode = localStorage.getItem("connectivityMode");
        this.connectivityMode = mode === "wavry" || mode === "direct" || mode === "custom" ? mode : "wavry";
        this.authServer = localStorage.getItem("authServer") || "https://auth.wavry.dev";
        this.username = localStorage.getItem("username") || "";
        this.signalingToken = localStorage.getItem("signalingToken") || null;
        this.isAuthenticated = !!this.username && !!this.signalingToken;
    }

    saveToStorage() {
        localStorage.setItem("isSetupCompleted", this.isSetupCompleted ? "true" : "false");
        localStorage.setItem("displayName", this.displayName);
        localStorage.setItem("connectivityMode", this.connectivityMode);
        localStorage.setItem("authServer", this.authServer);
        localStorage.setItem("username", this.username);
        if (this.signalingToken) {
            localStorage.setItem("signalingToken", this.signalingToken);
        } else {
            localStorage.removeItem("signalingToken");
        }
    }

    async initialize() {
        this.loadFromStorage();
        if (this.isAuthenticated) {
            await this.connectSignaling();
        }
    }

    private get wsUrl() {
        const base = this.authServer.trim().replace(/\/+$/, "");
        if (base.startsWith("https://")) {
            return `wss://${base.slice("https://".length)}/ws`;
        }
        if (base.startsWith("http://")) {
            return `ws://${base.slice("http://".length)}/ws`;
        }
        return `ws://${base}/ws`;
    }

    private get wtUrl() {
        const base = this.authServer.trim().replace(/\/+$/, "");
        // WebTransport usually runs on a different port or path, assuming /wt for now
        if (base.startsWith("https://")) {
            return `https://${base.slice("https://".length)}/wt`;
        }
        return `https://${base.replace(/^http:\/\//, "")}/wt`;
    }

    async connectSignaling() {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) return;
        if (!this.signalingToken) return;

        return new Promise<void>((resolve, reject) => {
            const url = this.wsUrl;
            this.ws = new WebSocket(url);

            this.ws.onopen = () => {
                console.log("Signaling WS connected");
                this.sendBind();
                resolve();
            };

            this.ws.onmessage = (evt: MessageEvent) => {
                try {
                    const msg = JSON.parse(evt.data);
                    this.handleSignalingMessage(msg);
                } catch (e) {
                    console.error("Failed to parse WS message:", evt.data);
                }
            };

            this.ws.onclose = () => {
                console.log("Signaling WS closed");
                this.ws = null;
            };

            this.ws.onerror = (err: Event) => {
                console.error("Signaling WS error:", err);
                reject(err);
            };
        });
    }

    async connectTransport() {
        if (typeof (window as any).WebTransport === "undefined") {
            throw new Error("WebTransport is not supported in this browser.");
        }

        const url = this.wtUrl;
        console.log("Connecting WebTransport to", url);
        
        try {
            this.transport = new (window as any).WebTransport(url);
            await this.transport.ready;
            console.log("WebTransport ready");

            const stream = await this.transport.createBidirectionalStream();
            this.controlWriter = stream.writable.getWriter();
            
            this.readControlResponses(stream.readable);
            this.readDatagrams();

            // Send initial connect message
            await this.sendControl({
                type: "control",
                control: {
                    type: "connect",
                    session_token: this.signalingToken,
                    client_name: this.effectiveDisplayName,
                    capabilities: {
                        max_width: window.screen.width,
                        max_height: window.screen.height,
                        max_fps: 60,
                        supports_gamepad: true,
                        supports_touch: "ontouchstart" in window
                    }
                }
            });

        } catch (e) {
            console.error("WebTransport connection failed:", e);
            this.transport = null;
            throw e;
        }
    }

    private async readControlResponses(readable: ReadableStream) {
        const reader = readable.getReader();
        try {
            while (true) {
                const { value, done } = await reader.read();
                if (done) break;
                const text = new TextDecoder().decode(value);
                const msg = JSON.parse(text);
                console.log("Received control response:", msg);
                this.handleControlResponse(msg);
            }
        } catch (e) {
            console.error("Error reading control stream:", e);
        } finally {
            reader.releaseLock();
        }
    }

    private async readDatagrams() {
        if (!this.transport) return;
        const reader = this.transport.datagrams.readable.getReader();
        try {
            while (true) {
                const { value, done } = await reader.read();
                if (done) break;
                // console.log("Received datagram:", value);
            }
        } catch (e) {
            console.error("Error reading datagrams:", e);
        } finally {
            reader.releaseLock();
        }
    }

    private async handleControlResponse(msg: any) {
        if (msg.type === "response") {
            const resp = msg.response;
            if (resp.type === "connected") {
                this.isConnected = true;
                this.connectionStatus = "connected";
                this.hostStatusMessage = `Connected to ${resp.server_name}`;
            } else if (resp.type === "error") {
                this.hostErrorMessage = resp.message;
                this.disconnect();
            } else if (resp.type === "web_rtc_offer") {
                await this.handleWebRtcOffer(resp.from_username, resp.sdp);
            } else if (resp.type === "web_rtc_answer") {
                await this.handleWebRtcAnswer(resp.from_username, resp.sdp);
            } else if (resp.type === "web_rtc_candidate") {
                await this.handleWebRtcCandidate(resp.from_username, resp.candidate);
            }
        }
    }

    private async handleWebRtcOffer(from: string, sdp: string) {
        console.log("Received WebRTC offer from", from);
        if (!this.pc) this.initWebRtc(from);
        await this.pc!.setRemoteDescription({ type: "offer", sdp });
        const answer = await this.pc!.createAnswer();
        await this.pc!.setLocalDescription(answer);
        
        await this.sendControl({
            type: "control",
            control: {
                type: "web_rtc_answer",
                target_username: from,
                sdp: answer.sdp
            }
        });
    }

    private async handleWebRtcAnswer(from: string, sdp: string) {
        console.log("Received WebRTC answer from", from);
        if (!this.pc) return;
        await this.pc.setRemoteDescription({ type: "answer", sdp });
    }

    private async handleWebRtcCandidate(from: string, candidate: string) {
        console.log("Received WebRTC candidate from", from);
        if (!this.pc) return;
        try {
            await this.pc.addIceCandidate(JSON.parse(candidate));
        } catch (e) {
            console.error("Error adding ICE candidate:", e);
        }
    }

    private activeTarget: string | null = null;

    initWebRtc(targetUsername: string) {
        this.activeTarget = targetUsername;
        if (this.pc) {
            this.pc.close();
        }

        this.pc = new RTCPeerConnection({
            iceServers: [
                { urls: "stun:stun.l.google.com:19302" },
                { urls: "stun:stun1.l.google.com:19302" }
            ]
        });

        this.pc.onicecandidate = (evt) => {
            if (evt.candidate && this.ws && this.ws.readyState === WebSocket.OPEN) {
                this.ws.send(JSON.stringify({
                    type: "CANDIDATE",
                    target_username: this.activeTarget,
                    candidate: JSON.stringify(evt.candidate)
                }));
            }
        };

        this.pc.ontrack = (evt) => {
            console.log("Received remote track:", evt.streams[0]);
            this.remoteStream = evt.streams[0];
            this.startStatsLoop();
        };

        this.pc.oniceconnectionstatechange = () => {
            console.log("ICE state:", this.pc?.iceConnectionState);
            if (this.pc?.iceConnectionState === "failed") {
                this.hostErrorMessage = "WebRTC ICE connection failed.";
            }
        };
    }

    private statsInterval: any = null;
    private startStatsLoop() {
        if (this.statsInterval) clearInterval(this.statsInterval);
        this.statsInterval = setInterval(async () => {
            if (!this.pc || this.pc.connectionState !== "connected") return;
            
            try {
                const stats = await this.pc.getStats();
                stats.forEach(report => {
                    if (report.type === "inbound-rtp" && report.kind === "video") {
                        // Estimate bitrate from bytesReceived
                        if (this.lastBytesReceived && this.lastTimestamp) {
                            const deltaBytes = report.bytesReceived - this.lastBytesReceived;
                            const deltaTime = (report.timestamp - this.lastTimestamp) / 1000; // s
                            this.ccBitrate = Math.floor((deltaBytes * 8) / deltaTime / 1000); // kbps
                        }
                        this.lastBytesReceived = report.bytesReceived;
                        this.lastTimestamp = report.timestamp;
                        
                        if (report.jitter) {
                            // Convert jitter to ms
                            this.ccState = report.jitter > 0.02 ? "Congested" : "Stable";
                        }
                    }
                });
            } catch (e) {
                console.error("Error getting WebRTC stats:", e);
            }
        }, 1000);
    }

    private lastBytesReceived = 0;
    private lastTimestamp = 0;

    async sendControl(msg: any) {
        if (!this.controlWriter) return;
        const text = JSON.stringify(msg);
        const data = new TextEncoder().encode(text);
        await this.controlWriter.write(data);
    }

    async sendInput(kind: number, payload: any) {
        if (!this.transport || !this.transport.datagrams.writable) return;
        
        const writer = this.transport.datagrams.writable.getWriter();
        const buf = new ArrayBuffer(32);
        const view = new DataView(buf);
        const timestamp = Math.floor(performance.now() * 1000); // us

        view.setUint8(0, 1); // Version
        view.setUint8(1, kind);
        view.setBigUint64(2, BigInt(timestamp), true);

        let offset = 10;
        if (kind === 1) { // MouseMove
            view.setInt16(offset, payload.dx, true);
            view.setInt16(offset + 2, payload.dy, true);
            offset += 4;
        } else if (kind === 2) { // Scroll
            view.setInt16(offset, payload.dx, true);
            view.setInt16(offset + 2, payload.dy, true);
            offset += 4;
        } else if (kind === 3) { // Analog
            view.setUint8(offset, payload.axis);
            view.setFloat32(offset + 1, payload.value, true);
            offset += 5;
        } else if (kind === 4) { // Gamepad
            view.setUint8(offset, payload.gamepad_id);
            view.setUint16(offset + 1, payload.buttons, true);
            view.setInt16(offset + 3, payload.axes[0], true);
            view.setInt16(offset + 5, payload.axes[1], true);
            view.setInt16(offset + 7, payload.axes[2], true);
            view.setInt16(offset + 9, payload.axes[3], true);
            offset += 11;
        }

        await writer.write(new Uint8Array(buf, 0, offset));
        writer.releaseLock();
    }

    private sendBind() {
        if (!this.ws || !this.signalingToken) return;
        const bindMsg = {
            type: "Bind",
            payload: { token: this.signalingToken },
        };
        this.ws.send(JSON.stringify(bindMsg));
    }

    private handleSignalingMessage(msg: any) {
        console.log("Signaling message received:", msg);
        switch (msg.type) {
            case "OFFER":
                this.handleWebRtcOffer(msg.target_username, msg.sdp);
                break;
            case "ANSWER":
                this.handleWebRtcAnswer(msg.target_username, msg.sdp);
                break;
            case "CANDIDATE":
                this.handleWebRtcCandidate(msg.target_username, msg.candidate);
                break;
            case "ERROR":
                console.error("Signaling error:", msg.message);
                this.hostErrorMessage = msg.message;
                break;
        }
    }

    async login(details: any) {
        try {
            const res = await fetch(`${this.authServer}/auth/login`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(details),
            });
            if (!res.ok) throw new Error(await res.text());
            const data = await res.json();
            this.username = data.username;
            this.signalingToken = data.token;
            this.isAuthenticated = true;
            this.saveToStorage();
            await this.connectSignaling();
            this.showLoginModal = false;
            return data.username;
        } catch (e: any) {
            console.error("Login failed:", e);
            throw e;
        }
    }

    async logout() {
        this.username = "";
        this.signalingToken = null;
        this.isAuthenticated = false;
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
        if (this.transport) {
            this.transport.close();
            this.transport = null;
        }
        this.saveToStorage();
    }

    openAuthModal(mode: "login" | "register" = "login") {
        this.authModalMode = mode;
        this.showLoginModal = true;
    }

    closeAuthModal() {
        this.showLoginModal = false;
    }

    get effectiveDisplayName() {
        return this.username || this.displayName || "Web Client";
    }

    async connect(targetUsername: string) {
        this.hostStatusMessage = `Requesting connection to ${targetUsername}...`;
        this.connectionStatus = "connecting";
        
        try {
            // try {
            //     await this.connectTransport();
            // } catch (e) {
            //     console.warn("WebTransport failed, falling back to pure WebRTC/WS control", e);
            // }

            this.initWebRtc(targetUsername);
            
            // Send OFFER request via Signaling WS
            if (this.ws && this.ws.readyState === WebSocket.OPEN) {
                this.ws.send(JSON.stringify({
                    type: "REQUEST_RELAY", // This triggers the master to coordinate
                    target_username: targetUsername
                }));
            } else {
                throw new Error("Signaling connection is not ready.");
            }

        } catch (e: any) {
            this.hostErrorMessage = `Failed to connect: ${e.message}`;
            this.connectionStatus = "offline";
        }
    }

    async disconnect() {
        this.isConnected = false;
        this.connectionStatus = "offline";
        this.hostStatusMessage = "Disconnected";
        if (this.transport) {
            this.transport.close();
            this.transport = null;
        }
        if (this.pc) {
            this.pc.close();
            this.pc = null;
        }
        this.remoteStream = null;
    }
}

export const appState = new AppState();
