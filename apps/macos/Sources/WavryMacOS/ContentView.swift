import SwiftUI

struct ContentView: View {
    @ObservedObject var appState: AppState
    @State private var activeTab: Tab = .sessions
    @State private var showingUserDetail = false
    
    enum Tab {
        case sessions
        case settings
    }
    
    var body: some View {
        if !appState.hasPermissions {
            PermissionsView(appState: appState)
        } else {
            HStack(spacing: 0) {
                // 1. Icon Sidebar (Left)
                VStack(spacing: .themeSpacing.xl) {
                    SidebarIcon(icon: .tabSessions, active: activeTab == .sessions) {
                        activeTab = .sessions
                    }
                    
                    Spacer()
                    
                    SidebarIcon(icon: .tabSettings, active: activeTab == .settings) {
                        activeTab = .settings
                    }
                }
                .padding(.vertical, .themeSpacing.xl)
                .frame(width: 60)
                .background(VisualEffectView(material: .sidebar, blendingMode: .behindWindow))
                
                // 2. Main Content Area
                ZStack {
                    VisualEffectView(material: .underWindowBackground, blendingMode: .behindWindow)
                        .ignoresSafeArea()
                    
                    VStack(alignment: .leading, spacing: 0) {
                        // Top Bar (User Identity)
                        HStack {
                            Spacer()
                            Button(action: { showingUserDetail.toggle() }) {
                                HStack(spacing: 8) {
                                    Circle()
                                        .fill(Color.accentSuccess)
                                        .frame(width: 8, height: 8)
                                    Text(appState.effectiveDisplayName)
                                        .font(.caption)
                                        .fontWeight(.bold)
                                        .foregroundColor(.white)
                                }
                                .padding(8)
                                .background(Color.bgElevation3)
                                .cornerRadius(.themeRadius.md)
                            }
                            .buttonStyle(.plain)
                            .popover(isPresented: $showingUserDetail, arrowEdge: .bottom) {
                                UserDetailView(appState: appState)
                            }
                        }
                        .padding(.horizontal, .themeSpacing.xxl)
                        .padding(.top, .themeSpacing.xl)
                        
                        // Dynamic Content
                        Group {
                            if activeTab == .sessions {
                                SessionsView(appState: appState)
                            } else {
                                SettingsView(appState: appState)
                            }
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                }
            }
            .frame(minWidth: 800, maxWidth: .infinity, minHeight: 600, maxHeight: .infinity)
            .preferredColorScheme(.dark)
        }
    }
}

struct SessionsView: View {
    @ObservedObject var appState: AppState
    @State private var remoteIP: String = "127.0.0.1"
    @State private var remoteUsername: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: .themeSpacing.xxl) {
            // Header
            VStack(alignment: .leading, spacing: 4) {
                Text("Sessions")
                    .font(.system(size: 32, weight: .light))
                    .foregroundColor(.textPrimary)
                Text("Manage your local host and active connections.")
                    .font(.body)
                    .foregroundColor(.textSecondary)
            }
            .padding(.horizontal, .themeSpacing.xxl)
            .padding(.top, .themeSpacing.xl)

            if !appState.errorMessage.isEmpty || !appState.statusMessage.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    if !appState.errorMessage.isEmpty {
                        Text(appState.errorMessage)
                            .font(.caption)
                            .foregroundColor(.accentDanger)
                    }
                    if !appState.statusMessage.isEmpty {
                        Text(appState.statusMessage)
                            .font(.caption)
                            .foregroundColor(.accentSuccess)
                    }
                }
                .padding()
                .background(.ultraThinMaterial)
                .cornerRadius(10)
                .padding(.horizontal, .themeSpacing.xxl)
            }
            
            ScrollView {
                VStack(alignment: .leading, spacing: .themeSpacing.xxl) {
                    // Local Host
                    VStack(alignment: .leading, spacing: .themeSpacing.sm) {
                        Text("LOCAL HOST")
                            .font(.caption)
                            .fontWeight(.bold)
                            .foregroundColor(.textSecondary)
                        HostCard(appState: appState)
                    }
                    .padding(.horizontal, .themeSpacing.xxl)
                    
                    // Remote Connection
                    VStack(alignment: .leading, spacing: .themeSpacing.sm) {
                        Text("REMOTE CONNECTION")
                            .font(.caption)
                            .fontWeight(.bold)
                            .foregroundColor(.textSecondary)
                        
                        VStack(spacing: 16) {
                            if appState.isConnected && !appState.isHost {
                                VideoPlayerView(layer: appState.videoLayer)
                                    .frame(height: 300)
                                    .cornerRadius(12)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 12)
                                            .stroke(Color.accentSuccess.opacity(0.5), lineWidth: 1)
                                    )
                                    
                                Button(action: { appState.stopSession() }) {
                                    Text("Disconnect")
                                        .frame(maxWidth: .infinity)
                                        .padding()
                                        .background(Color.accentDanger.opacity(0.8))
                                        .foregroundColor(.white)
                                        .cornerRadius(8)
                                }
                                .buttonStyle(.plain)
                            } else {
                                if appState.isHost {
                                    Text("Hosting is active. Stop hosting before starting a client connection.")
                                        .font(.caption)
                                        .foregroundColor(.textSecondary)
                                        .padding(.bottom, 4)
                                }

                                // Connect via ID (Cloud Mode)
                                if appState.connectivityMode != .direct {
                                    HStack {
                                        TextField("Username or ID", text: $remoteUsername)
                                            .textFieldStyle(PlainTextFieldStyle())
                                            .padding(10)
                                            .background(Color.white.opacity(0.05))
                                            .cornerRadius(8)
                                        
                                        Button(action: {
                                            appState.clearMessages()
                                            appState.connectViaId(username: remoteUsername)
                                        }) {
                                            Text("Connect")
                                                .fontWeight(.bold)
                                                .padding(.horizontal, 20)
                                                .padding(.vertical, 10)
                                                .background(Color.accentPrimary)
                                                .foregroundColor(.white)
                                                .cornerRadius(8)
                                        }
                                        .buttonStyle(.plain)
                                        .disabled(appState.isConnectingClient || remoteUsername.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || appState.isHost)
                                    }
                                    .padding()
                                    .background(.ultraThinMaterial)
                                    .cornerRadius(12)
                                    
                                    Text("OR").font(.caption).foregroundColor(.textSecondary)
                                }
                                
                                // Direct IP Connection (LAN Mode & Fallback)
                                HStack {
                                    TextField("Host IP", text: $remoteIP)
                                        .textFieldStyle(PlainTextFieldStyle())
                                        .padding(10)
                                        .background(Color.white.opacity(0.05))
                                        .cornerRadius(8)
                                    
                                    Button(action: {
                                        appState.clearMessages()
                                        appState.connectToHost(ip: remoteIP)
                                    }) {
                                        Text(appState.isConnectingClient ? "Connecting..." : "Connect Directly")
                                            .fontWeight(.bold)
                                            .padding(.horizontal, 20)
                                            .padding(.vertical, 10)
                                            .background(Color.accentPrimary.opacity(appState.connectivityMode == .direct ? 1.0 : 0.6))
                                            .foregroundColor(.white)
                                            .cornerRadius(8)
                                    }
                                    .buttonStyle(.plain)
                                    .disabled(appState.isConnectingClient || remoteIP.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || appState.isHost)
                                }
                                .padding()
                                .background(.ultraThinMaterial)
                                .cornerRadius(12)
                            }
                        }
                    }
                    .padding(.horizontal, .themeSpacing.xxl)

                }
            }
        }
    }
}

struct HostCard: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        VStack(spacing: 0) {
            // Preview / Thumbnail
            ZStack {
                Color.black.opacity(0.2)
                if appState.isHost {
                    // Live indicator
                    VStack {
                        Spacer()
                        HStack {
                            HStack(spacing: 6) {
                                Circle()
                                    .fill(Color.accentSuccess)
                                    .frame(width: 6, height: 6)
                                Text("HOSTING")
                                    .font(.caption2)
                                    .fontWeight(.bold)
                                    .foregroundColor(.accentSuccess)
                            }
                            .padding(8)
                            .background(.ultraThinMaterial)
                            .cornerRadius(6)
                            Spacer()
                        }
                        .padding(12)
                    }
                } else {
                    WavryIcon(name: .hostDefault, size: 60, color: .textSecondary.opacity(0.3))
                }
            }
            .frame(height: 180)
            
            // Info Row
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text(appState.effectiveDisplayName)
                        .font(.headline)
                        .foregroundColor(.textPrimary)
                    Text(appState.connectivityMode.displayName)
                        .font(.caption)
                        .foregroundColor(.textSecondary)
                }
                Spacer()
                
                if appState.isConnected && !appState.isHost {
                    Text("Connected as Client")
                        .font(.caption)
                        .foregroundColor(.textSecondary)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 6)
                        .background(Color.white.opacity(0.05))
                        .cornerRadius(6)
                } else {
                    Button(action: {
                        appState.clearMessages()
                        if appState.isHost {
                            appState.stopSession()
                        } else {
                            appState.startHosting()
                        }
                    }) {
                        Text(appState.isHost ? "Stop Hosting" : (appState.isStartingHost ? "Starting..." : "Start Hosting"))
                            .fontWeight(.bold)
                            .padding(.horizontal, .themeSpacing.xl)
                            .padding(.vertical, 10)
                            .background(appState.isHost ? Color.accentDanger.opacity(0.8) : Color.accentPrimary)
                            .foregroundColor(.white)
                            .cornerRadius(.themeRadius.sm)
                    }
                    .buttonStyle(.plain)
                    .disabled(appState.isStartingHost || appState.isConnectingClient)
                }
            }
            .padding(.themeSpacing.lg)
        }
        .background(.ultraThinMaterial)
        .cornerRadius(.themeRadius.md)
        .overlay(
            RoundedRectangle(cornerRadius: .themeRadius.md)
                .stroke(Color.white.opacity(0.1), lineWidth: 1)
        )
    }
}

struct SidebarIcon: View {
    let icon: WavryIconName
    let active: Bool
    let action: () -> Void
    
    var body: some View {
        Button(action: action) {
            WavryIcon(name: icon, size: 20, color: active ? .accentPrimary : .textSecondary)
                .frame(width: 40, height: 40)
                .background(active ? Color.accentPrimary.opacity(0.1) : Color.clear)
                .cornerRadius(.themeRadius.lg)
        }
        .buttonStyle(.plain)
    }
}

struct VideoPlayerView: NSViewRepresentable {
    var layer: CALayer
    
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.layer = layer
        view.wantsLayer = true
        // layer.backgroundColor = NSColor.black.cgColor
        return view
    }
    
    func updateNSView(_ nsView: NSView, context: Context) {
        if nsView.layer != layer {
            nsView.layer = layer
        }
    }
}

struct VisualEffectView: NSViewRepresentable {
    var material: NSVisualEffectView.Material
    var blendingMode: NSVisualEffectView.BlendingMode
    
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = material
        view.blendingMode = blendingMode
        view.state = .active
        return view
    }
    
    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {
        nsView.material = material
        nsView.blendingMode = blendingMode
    }
}
