import SwiftUI
import Combine
import CoreGraphics
import AVFoundation
import Clibwavry

class AppState: ObservableObject {
    @Published var hasPermissions: Bool = false
    @Published var isConnected: Bool = false
    @Published var fps: Int = 0
    @Published var rtt: Double = 0.0
    
    private var permissionTimer: Timer?
    public let videoLayer = AVSampleBufferDisplayLayer()
    
    init() {
        self.checkPermissions()
        
        // Initialize Renderer with Layer Pointer
        let layerPtr = Unmanaged.passUnretained(videoLayer).toOpaque()
        let res = wavry_init_renderer(layerPtr)
        if res != 0 {
            print("Failed to init renderer")
        }
        
        // Init Injector (Hardcoded size for verification, should be dynamic)
        wavry_init_injector(1920, 1080)
        
        // Poll for permissions in case user changes them in System Settings
        self.permissionTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            self?.checkPermissions()
        }
    }
    
    func checkPermissions() {
        // Real check
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
        // Request access (trigger system prompt)
        CGRequestScreenCaptureAccess()
        
        // Open System Settings deep link to Screen Recording
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
            NSWorkspace.shared.open(url)
        }
    }
    
    // Connect to Rust backend
    func connect() {
        wavry_connect()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            self.isConnected = true
            self.startMockStats()
        }
    }
    
    func disconnect() {
        self.isConnected = false
    }
    
    func startMockStats() {
        Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { _ in
            guard self.isConnected else { return }
            self.fps = Int.random(in: 58...60)
            self.rtt = Double.random(in: 2.0...5.0)
        }
    }
    
    func testInput() {
        wavry_test_input_injection()
    }
    
    // MARK: - Onboarding & Config
    @Published var isSetupCompleted: Bool = UserDefaults.standard.bool(forKey: "isSetupCompleted")
    @Published var displayName: String = UserDefaults.standard.string(forKey: "displayName") ?? ""
    @Published var connectivityMode: ConnectivityMode = ConnectivityMode(rawValue: UserDefaults.standard.string(forKey: "connectivityMode") ?? "") ?? .wavry
    
    // Identity & Account
    @Published var isUsingHostname: Bool = UserDefaults.standard.bool(forKey: "isUsingHostname")
    @Published var authServer: String = UserDefaults.standard.string(forKey: "authServer") ?? "https://auth.wavry.dev"
    @Published var publicKey: String = "8a7b3c2d1e0f9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b" // Mocked secure key
    
    // Expanded Settings
    @Published var resolution: String = UserDefaults.standard.string(forKey: "resolution") ?? "1920x1080"
    @Published var fpsLimit: Int = UserDefaults.standard.integer(forKey: "fpsLimit") == 0 ? 60 : UserDefaults.standard.integer(forKey: "fpsLimit")
    @Published var bitrateMbps: Int = UserDefaults.standard.integer(forKey: "bitrateMbps") == 0 ? 25 : UserDefaults.standard.integer(forKey: "bitrateMbps")
    @Published var clientPort: Int = UserDefaults.standard.integer(forKey: "clientPort") == 0 ? 0 : UserDefaults.standard.integer(forKey: "clientPort")
    @Published var hostStartPort: Int = UserDefaults.standard.integer(forKey: "hostStartPort") == 0 ? 0 : UserDefaults.standard.integer(forKey: "hostStartPort")
    @Published var upnpEnabled: Bool = UserDefaults.standard.object(forKey: "upnpEnabled") == nil ? true : UserDefaults.standard.bool(forKey: "upnpEnabled")
    
    var effectiveDisplayName: String {
        if isUsingHostname || displayName.isEmpty {
            return ProcessInfo.processInfo.hostName
        }
        return displayName
    }
    
    func completeSetup(name: String, mode: ConnectivityMode) {
        self.displayName = name
        self.connectivityMode = mode
        self.isSetupCompleted = true
        
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
        UserDefaults.standard.set(bitrateMbps, forKey: "bitrateMbps")
        UserDefaults.standard.set(clientPort, forKey: "clientPort")
        UserDefaults.standard.set(hostStartPort, forKey: "hostStartPort")
        UserDefaults.standard.set(upnpEnabled, forKey: "upnpEnabled")
    }
    
    func resetSetup() {
        self.isSetupCompleted = false
        UserDefaults.standard.removeObject(forKey: "isSetupCompleted")
    }
}

enum ConnectivityMode: String {
    case wavry = "wavry"
    case direct = "direct"
    case custom = "custom"
    
    var displayName: String {
        switch self {
        case .wavry: return "Wavry Service"
        case .direct: return "Direct (No Server)"
        case .custom: return "Custom Server"
        }
    }
}
