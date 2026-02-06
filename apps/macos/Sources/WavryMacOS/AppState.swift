import SwiftUI
import Combine
import CoreGraphics
import AVFoundation
import AppKit
import Clibwavry

struct HostDisplayOption: Identifiable, Hashable {
    let id: UInt32
    let name: String
    let width: Int
    let height: Int

    var label: String {
        "\(name) (\(width)x\(height))"
    }
}

class AppState: ObservableObject {
    @Published var hasPermissions: Bool = false
    @Published var isConnected: Bool = false
    @Published var fps: Int = 0
    @Published var rtt: Double = 0.0

    @Published var isAuthenticated: Bool = UserDefaults.standard.string(forKey: "authToken") != nil
    @Published var authToken: String? = UserDefaults.standard.string(forKey: "authToken")

    @Published var isHost: Bool = false
    @Published var isStartingHost: Bool = false
    @Published var isConnectingClient: Bool = false

    @Published var statusMessage: String = ""
    @Published var errorMessage: String = ""

    @Published var hostDisplays: [HostDisplayOption] = []
    @Published var selectedDisplayID: UInt32 = {
        let value = UserDefaults.standard.integer(forKey: "selectedDisplayID")
        if value < 0 {
            return UInt32.max
        }
        return UInt32(value)
    }()

    private var permissionTimer: Timer?
    private var statsTimer: Timer?
    public let videoLayer = AVSampleBufferDisplayLayer()

    // MARK: - Onboarding & Config
    @Published var isSetupCompleted: Bool = UserDefaults.standard.bool(forKey: "isSetupCompleted")
    @Published var displayName: String = UserDefaults.standard.string(forKey: "displayName") ?? ""
    @Published var connectivityMode: ConnectivityMode = ConnectivityMode(rawValue: UserDefaults.standard.string(forKey: "connectivityMode") ?? "") ?? .wavry

    // Identity & Account
    @Published var isUsingHostname: Bool = UserDefaults.standard.bool(forKey: "isUsingHostname")
    @Published var authServer: String = UserDefaults.standard.string(forKey: "authServer") ?? "https://auth.wavry.dev"
    @Published var publicKey: String = "8a7b3c2d1e0f9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b"

    // Expanded Settings
    @Published var resolution: String = UserDefaults.standard.string(forKey: "resolution") ?? "1920x1080"
    @Published var fpsLimit: Int = UserDefaults.standard.integer(forKey: "fpsLimit") == 0 ? 60 : UserDefaults.standard.integer(forKey: "fpsLimit")
    @Published var hostFps: Int = UserDefaults.standard.integer(forKey: "hostFps") == 0 ? 60 : UserDefaults.standard.integer(forKey: "hostFps")
    @Published var bitrateMbps: Int = UserDefaults.standard.integer(forKey: "bitrateMbps") == 0 ? 25 : UserDefaults.standard.integer(forKey: "bitrateMbps")
    @Published var keyframeIntervalMs: Int = UserDefaults.standard.integer(forKey: "keyframeIntervalMs") == 0 ? 2000 : UserDefaults.standard.integer(forKey: "keyframeIntervalMs")
    @Published var clientPort: Int = UserDefaults.standard.integer(forKey: "clientPort") == 0 ? 0 : UserDefaults.standard.integer(forKey: "clientPort")
    @Published var hostStartPort: Int = UserDefaults.standard.integer(forKey: "hostStartPort") == 0 ? 0 : UserDefaults.standard.integer(forKey: "hostStartPort")
    @Published var upnpEnabled: Bool = UserDefaults.standard.object(forKey: "upnpEnabled") == nil ? true : UserDefaults.standard.bool(forKey: "upnpEnabled")
    @Published var showLoginSheet: Bool = false
    @Published var pcvrStatus: String = "PCVR: Not available on macOS"

    init() {
        self.checkPermissions()
        self.refreshDisplays()

        let layerPtr = Unmanaged.passUnretained(videoLayer).toOpaque()
        let rendererInit = wavry_init_renderer(layerPtr)
        if rendererInit != 0 {
            setError("Renderer initialization failed (\(rendererInit)).")
        }

        if let supportDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            let path = supportDir.path
            path.withCString { ptr in
                let identityInit = wavry_init_identity(ptr)
                if identityInit != 0 {
                    setError("Identity initialization failed (\(identityInit)).")
                }
            }
        }

        wavry_init_injector(1920, 1080)

        self.permissionTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            self?.checkPermissions()
        }
    }

    deinit {
        permissionTimer?.invalidate()
        statsTimer?.invalidate()
    }

    var effectiveDisplayName: String {
        if isUsingHostname || displayName.isEmpty {
            return ProcessInfo.processInfo.hostName
        }
        return displayName
    }

    // MARK: - UI Messaging

    func clearMessages() {
        errorMessage = ""
        statusMessage = ""
    }

    private func setStatus(_ message: String) {
        DispatchQueue.main.async {
            self.statusMessage = message
            self.errorMessage = ""
        }
    }

    private func setError(_ message: String) {
        DispatchQueue.main.async {
            self.errorMessage = message
            self.statusMessage = ""
        }
    }

    // MARK: - Permissions

    func checkPermissions() {
        let granted = CGPreflightScreenCaptureAccess()
        DispatchQueue.main.async {
            if self.hasPermissions != granted {
                withAnimation {
                    self.hasPermissions = granted
                }
            }
        }
    }

    func requestPermissions() {
        _ = CGRequestScreenCaptureAccess()
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
            NSWorkspace.shared.open(url)
        }
    }

    // MARK: - Display Enumeration

    func refreshDisplays() {
        let maxDisplays: UInt32 = 16
        var displayIDs = [CGDirectDisplayID](repeating: 0, count: Int(maxDisplays))
        var count: UInt32 = 0

        let result = CGGetActiveDisplayList(maxDisplays, &displayIDs, &count)
        guard result == .success else {
            setError("Failed to enumerate displays (\(result.rawValue)).")
            return
        }

        let refreshed: [HostDisplayOption] = displayIDs
            .prefix(Int(count))
            .enumerated()
            .map { index, id in
                HostDisplayOption(
                    id: id,
                    name: "Display \(index + 1)",
                    width: CGDisplayPixelsWide(id),
                    height: CGDisplayPixelsHigh(id)
                )
            }

        DispatchQueue.main.async {
            self.hostDisplays = refreshed
            if refreshed.isEmpty {
                self.selectedDisplayID = UInt32.max
                return
            }

            if self.selectedDisplayID == UInt32.max || !refreshed.contains(where: { $0.id == self.selectedDisplayID }) {
                self.selectedDisplayID = refreshed[0].id
            }
        }
    }

    // MARK: - Session Management

    private func resolvedHostPort() throws -> UInt16 {
        if hostStartPort == 0 {
            return 4444
        }
        guard hostStartPort > 0 && hostStartPort <= Int(UInt16.max) else {
            throw AppStateError.invalidPort("Host port must be between 1 and 65535")
        }
        return UInt16(hostStartPort)
    }

    private func resolvedClientTarget(from input: String) throws -> (host: String, port: UInt16) {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw AppStateError.invalidAddress("Host IP is required")
        }

        var host = trimmed
        var explicitPort: UInt16?

        if let range = trimmed.range(of: ":", options: .backwards) {
            let candidateHost = String(trimmed[..<range.lowerBound])
            let candidatePort = String(trimmed[range.upperBound...])
            if let parsedPort = UInt16(candidatePort), !candidateHost.isEmpty {
                host = candidateHost
                explicitPort = parsedPort
            }
        }

        if let explicitPort {
            return (host, explicitPort)
        }

        if clientPort == 0 {
            return (host, 4444)
        }

        guard clientPort > 0 && clientPort <= Int(UInt16.max) else {
            throw AppStateError.invalidPort("Client port must be between 1 and 65535")
        }

        return (host, UInt16(clientPort))
    }

    private func parsedResolution() -> (UInt16, UInt16) {
        let parts = resolution
            .lowercased()
            .split(separator: "x", maxSplits: 1)
            .map { String($0) }

        guard parts.count == 2,
              let w = UInt16(parts[0]),
              let h = UInt16(parts[1]),
              w >= 320,
              h >= 240
        else {
            return (1920, 1080)
        }

        return (w, h)
    }

    func startHosting() {
        guard !isStartingHost else { return }
        guard hasPermissions else {
            setError("Screen Recording permission is required before hosting.")
            return
        }

        do {
            let port = try resolvedHostPort()
            let (width, height) = parsedResolution()
            let chosenDisplay = hostDisplays.contains(where: { $0.id == selectedDisplayID }) ? selectedDisplayID : UInt32.max

            var config = WavryHostConfig(
                width: width,
                height: height,
                fps: UInt16(max(15, min(hostFps, 240))),
                bitrate_kbps: UInt32(max(1, bitrateMbps) * 1000),
                keyframe_interval_ms: UInt32(max(250, min(keyframeIntervalMs, 10000))),
                display_id: chosenDisplay
            )

            isStartingHost = true
            let res = withUnsafePointer(to: &config) { ptr in
                wavry_start_host_with_config(port, ptr)
            }
            isStartingHost = false

            if res == 0 {
                isHost = true
                startPollingStats()
                setStatus("Host started on port \(port). Waiting for a client.")
            } else {
                setError(hostStartErrorMessage(code: res))
            }
        } catch {
            setError(error.localizedDescription)
        }
    }

    private func hostStartErrorMessage(code: Int32) -> String {
        switch code {
        case -1:
            return "A host/client session is already running. Stop it first."
        case -2:
            return "Failed to start host runtime. Check permissions and selected display."
        case -3:
            return "Host startup was interrupted before initialization completed."
        case -4:
            return "Invalid host configuration provided by the app."
        default:
            return "Failed to start host (error \(code))."
        }
    }

    func connectToHost(ip: String) {
        guard !isConnectingClient else { return }

        do {
            let target = try resolvedClientTarget(from: ip)
            isConnectingClient = true

            target.host.withCString { ipPtr in
                let res = wavry_start_client(ipPtr, target.port)
                DispatchQueue.main.async {
                    self.isConnectingClient = false
                    if res == 0 {
                        self.isHost = false
                        self.startPollingStats()
                        self.setStatus("Connecting to \(target.host):\(target.port)...")
                    } else {
                        self.setError(self.clientStartErrorMessage(code: res))
                    }
                }
            }
        } catch {
            setError(error.localizedDescription)
        }
    }

    private func clientStartErrorMessage(code: Int32) -> String {
        switch code {
        case -1:
            return "A host/client session is already running. Stop it first."
        case -2:
            return "Missing host address."
        case -3:
            return "Host address contains invalid characters."
        case -4:
            return "Client session failed to initialize."
        case -5:
            return "Client startup channel closed unexpectedly."
        default:
            return "Failed to connect to host (error \(code))."
        }
    }

    func connectViaId(username: String) {
        let trimmed = username.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            setError("Username is required.")
            return
        }
        guard connectivityMode != .direct else {
            setError("Connect by ID is unavailable in LAN mode.")
            return
        }

        trimmed.withCString { usernamePtr in
            let res = wavry_send_connect_request(usernamePtr)
            if res != 0 {
                setError("Failed to send connection request (error \(res)).")
            } else {
                setStatus("Connection request sent to \(trimmed).")
            }
        }
    }

    func stopSession() {
        let res = wavry_stop()
        statsTimer?.invalidate()
        statsTimer = nil
        isConnected = false
        isHost = false
        isStartingHost = false
        isConnectingClient = false

        if res == 0 {
            setStatus("Session stopped.")
        } else {
            setError("No active session to stop.")
        }
    }

    func startPollingStats() {
        statsTimer?.invalidate()
        statsTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            guard let self else { return }
            var stats = WavryStats()
            let res = wavry_get_stats(&stats)
            if res == 0 {
                DispatchQueue.main.async {
                    self.isConnected = stats.connected != 0
                    self.fps = Int(stats.fps)
                    self.rtt = Double(stats.rtt_ms)
                }
            }
        }
    }

    func testInput() {
        _ = wavry_test_input_injection()
    }

    // MARK: - Settings Persistence

    func completeSetup(name: String, mode: ConnectivityMode) {
        displayName = name
        connectivityMode = mode
        isSetupCompleted = true

        UserDefaults.standard.set(true, forKey: "isSetupCompleted")
        UserDefaults.standard.set(name, forKey: "displayName")
        UserDefaults.standard.set(mode.rawValue, forKey: "connectivityMode")
    }

    func saveSettings() {
        UserDefaults.standard.set(displayName, forKey: "displayName")
        UserDefaults.standard.set(connectivityMode.rawValue, forKey: "connectivityMode")
        UserDefaults.standard.set(isUsingHostname, forKey: "isUsingHostname")
        UserDefaults.standard.set(authServer, forKey: "authServer")
        UserDefaults.standard.set(resolution, forKey: "resolution")
        UserDefaults.standard.set(fpsLimit, forKey: "fpsLimit")
        UserDefaults.standard.set(hostFps, forKey: "hostFps")
        UserDefaults.standard.set(bitrateMbps, forKey: "bitrateMbps")
        UserDefaults.standard.set(keyframeIntervalMs, forKey: "keyframeIntervalMs")
        UserDefaults.standard.set(clientPort, forKey: "clientPort")
        UserDefaults.standard.set(hostStartPort, forKey: "hostStartPort")
        UserDefaults.standard.set(upnpEnabled, forKey: "upnpEnabled")

        let selectedDisplayToPersist: Int
        if selectedDisplayID == UInt32.max {
            selectedDisplayToPersist = -1
        } else {
            selectedDisplayToPersist = Int(selectedDisplayID)
        }
        UserDefaults.standard.set(selectedDisplayToPersist, forKey: "selectedDisplayID")

        setStatus("Settings saved.")
    }

    func resetSetup() {
        isSetupCompleted = false
        UserDefaults.standard.removeObject(forKey: "isSetupCompleted")
    }

    // MARK: - Auth

    func completeLogin(token: String, email: String) {
        authToken = token
        UserDefaults.standard.set(token, forKey: "authToken")
        displayName = email
        isAuthenticated = true

        if connectivityMode != .direct {
            token.withCString { tokenPtr in
                var result: Int32 = 0
                if connectivityMode == .custom {
                    authServer.withCString { urlPtr in
                        result = wavry_connect_signaling_with_url(urlPtr, tokenPtr)
                    }
                } else {
                    result = wavry_connect_signaling(tokenPtr)
                }

                if result != 0 {
                    setError("Signaling connection failed (\(result)).")
                }
            }
        }
    }

    func logout() {
        authToken = nil
        isAuthenticated = false
        UserDefaults.standard.removeObject(forKey: "authToken")
    }

    func getPublicKey() -> String? {
        var buffer = [UInt8](repeating: 0, count: 32)
        let res = wavry_get_public_key(&buffer)
        if res == 0 {
            return buffer.map { String(format: "%02x", $0) }.joined()
        }
        return nil
    }
}

enum AppStateError: LocalizedError {
    case invalidPort(String)
    case invalidAddress(String)

    var errorDescription: String? {
        switch self {
        case .invalidPort(let message):
            return message
        case .invalidAddress(let message):
            return message
        }
    }
}

enum ConnectivityMode: String {
    case wavry = "wavry"
    case direct = "direct"
    case custom = "custom"

    var displayName: String {
        switch self {
        case .wavry:
            return "Wavry Service"
        case .direct:
            return "Direct (No Server)"
        case .custom:
            return "Custom Server"
        }
    }
}
