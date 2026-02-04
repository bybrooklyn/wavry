import SwiftUI

enum SidebarTab {
    case sessions
    case settings
}

struct ContentView: View {
    @ObservedObject var appState: AppState
    @State private var showUserDetail = false
    @State private var activeTab: SidebarTab = .sessions
    @State private var joinPeerID: String = ""
    
    var body: some View {
        if !appState.hasPermissions {
            PermissionsView(appState: appState)
        } else {
            HStack(spacing: 0) {
                // 1. Icon Sidebar (Left)
                VStack(spacing: 20) {
                    Button(action: { activeTab = .sessions }) {
                        SidebarIcon(icon: "desktopcomputer", active: activeTab == .sessions)
                    }.buttonStyle(.plain)
                    
                    Spacer()
                    
                    // Settings at Bottom
                    Button(action: { activeTab = .settings }) {
                        SidebarIcon(icon: "gearshape.fill", active: activeTab == .settings)
                    }.buttonStyle(.plain)
                }
                .padding(.vertical, 20)
                .frame(width: 60)
                .background(Color(red: 0.1, green: 0.1, blue: 0.1))
                
                // 2. Main Content Area
                ZStack {
                    Color(red: 0.13, green: 0.13, blue: 0.13).ignoresSafeArea()
                    
                    VStack(alignment: .leading, spacing: 0) {
                        // Top Bar (User Identity)
                        HStack {
                            Spacer()
                            Button(action: { showUserDetail.toggle() }) {
                                HStack(spacing: 8) {
                                    Circle().fill(Color.green).frame(width: 8, height: 8)
                                    Text(appState.effectiveDisplayName)
                                        .font(.caption)
                                        .fontWeight(.bold)
                                        .foregroundColor(.white)
                                }
                                .padding(8)
                                .background(Color(white: 0.2))
                                .cornerRadius(6)
                            }
                            .buttonStyle(.plain)
                            .popover(isPresented: $showUserDetail, arrowEdge: .bottom) {
                                UserDetailView(appState: appState)
                            }
                        }
                        .padding(.horizontal, 30)
                        .padding(.top, 20)
                        
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
            .colorScheme(.dark)
        }
    }
}

struct SessionsView: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 5) {
                Text("Sessions")
                    .font(.system(size: 32, weight: .light))
                    .foregroundColor(.white)
                Text("Manage your local host and active connections.")
                    .font(.body)
                    .foregroundColor(.gray)
            }
            .padding(.horizontal, 30)
            .padding(.top, 20)
            .padding(.bottom, 30)
            
            // Scrollable Content
            ScrollView {
                VStack(alignment: .leading, spacing: 30) {
                    
                    // Local Host Section
                    VStack(alignment: .leading, spacing: 10) {
                        Text("LOCAL HOST")
                            .font(.caption)
                            .fontWeight(.bold)
                            .foregroundColor(.gray)
                            .padding(.horizontal, 30)
                        
                        HostCard(appState: appState)
                            .padding(.horizontal, 30)
                    }
                    
                    // Active Sessions Section
                    VStack(alignment: .leading, spacing: 10) {
                        Text("ACTIVE SESSIONS")
                            .font(.caption)
                            .fontWeight(.bold)
                            .foregroundColor(.gray)
                            .padding(.horizontal, 30)
                        
                        // Placeholder for Active Sessions
                        HStack {
                            Spacer()
                            VStack(spacing: 12) {
                                Image(systemName: "network.slash")
                                    .font(.system(size: 30))
                                    .foregroundColor(.gray.opacity(0.3))
                                Text("No active sessions")
                                    .font(.body)
                                    .foregroundColor(.gray.opacity(0.5))
                            }
                            Spacer()
                        }
                        .frame(height: 120)
                        .background(Color(white: 0.15))
                        .cornerRadius(6)
                        .padding(.horizontal, 30)
                    }
                    
                    Spacer()
                }
            }
        }
    }
}

struct SidebarIcon: View {
    let icon: String
    let active: Bool
    
    var body: some View {
        Image(systemName: icon)
            .font(.system(size: 20))
            .foregroundColor(active ? .blue : .gray)
            .frame(width: 40, height: 40)
            .background(active ? Color.blue.opacity(0.1) : Color.clear)
            .cornerRadius(8)
            .contentShape(Rectangle())
    }
}

struct HostCard: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        VStack(spacing: 0) {
            // Preview / Status Area
            ZStack {
                Color(white: 0.18)
                
                if appState.isConnected {
                    VideoView(layer: appState.videoLayer)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Image(systemName: "macpro.gen3.fill")
                        .font(.system(size: 60))
                        .foregroundColor(.gray.opacity(0.5))
                }
                
                if appState.isConnected {
                    VStack {
                        Spacer()
                        HStack {
                            Circle().fill(Color.green).frame(width: 6, height: 6)
                            Text("LIVE")
                                .font(.caption2)
                                .fontWeight(.bold)
                                .foregroundColor(.green)
                            Spacer()
                        }
                        .padding(8)
                    }
                }
            }
            .frame(height: 200)
            
            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(appState.effectiveDisplayName)
                        .font(.headline)
                        .foregroundColor(.white)
                    Text(appState.connectivityMode.displayName)
                        .font(.caption)
                        .foregroundColor(.gray)
                }
                
                Spacer()
                
                Button(action: {
                    if appState.isConnected { appState.disconnect() }
                    else { appState.connect() }
                }) {
                    Text(appState.isConnected ? "Stop Hosting" : "Start Session")
                        .fontWeight(.bold)
                        .padding(.horizontal, 20)
                        .padding(.vertical, 8)
                        .background(appState.isConnected ? Color.red : Color.blue)
                        .foregroundColor(.white)
                        .cornerRadius(4)
                }
                .buttonStyle(.plain)
            }
            .padding(16)
            .background(Color(white: 0.15))
        }
        .cornerRadius(6)
        .overlay(
            RoundedRectangle(cornerRadius: 6)
                .stroke(Color.black.opacity(0.5), lineWidth: 1)
        )
    }
}
