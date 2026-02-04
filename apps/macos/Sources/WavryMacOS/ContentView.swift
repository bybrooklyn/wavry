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
                .background(Color.bgSidebar)
                
                // 2. Main Content Area
                ZStack {
                    Color.bgBase.ignoresSafeArea()
                    
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
                    
                    // Active Sessions
                    VStack(alignment: .leading, spacing: .themeSpacing.sm) {
                        Text("ACTIVE SESSIONS")
                            .font(.caption)
                            .fontWeight(.bold)
                            .foregroundColor(.textSecondary)
                        
                        VStack {
                            WavryIcon(name: .noSessions, size: 30, color: .textSecondary.opacity(0.3))
                            Text("No active sessions")
                                .font(.body)
                                .foregroundColor(.textSecondary)
                                .opacity(0.5)
                        }
                        .frame(maxWidth: .infinity)
                        .frame(height: 120)
                        .background(Color.bgElevation1)
                        .cornerRadius(.themeRadius.md)
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
                Color.bgElevation2
                if appState.isConnected {
                    // Live indicator
                    VStack {
                        Spacer()
                        HStack {
                            HStack(spacing: 6) {
                                Circle()
                                    .fill(Color.accentSuccess)
                                    .frame(width: 6, height: 6)
                                Text("LIVE")
                                    .font(.caption2)
                                    .fontWeight(.bold)
                                    .foregroundColor(.accentSuccess)
                            }
                            .padding(8)
                            Spacer()
                        }
                    }
                } else {
                    WavryIcon(name: .hostDefault, size: 60, color: .textSecondary.opacity(0.5))
                }
                
                if appState.isConnected {
                    VStack {
                        Spacer()
                        HStack {
                            HStack(spacing: 6) {
                                Circle()
                                    .fill(Color.accentSuccess)
                                    .frame(width: 6, height: 6)
                                Text("LIVE")
                                    .font(.caption2)
                                    .fontWeight(.bold)
                                    .foregroundColor(.accentSuccess)
                            }
                            .padding(8)
                            Spacer()
                        }
                    }
                }
            }
            .frame(height: 200)
            
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
                Button(action: {
                    if appState.isConnected {
                        appState.disconnect()
                    } else {
                        appState.connect()
                    }
                }) {
                    Text(appState.isConnected ? "Stop Hosting" : "Start Session")
                        .fontWeight(.bold)
                        .padding(.horizontal, .themeSpacing.xl)
                        .padding(.vertical, 8)
                        .background(appState.isConnected ? Color.accentDanger : Color.accentPrimary)
                        .foregroundColor(.white)
                        .cornerRadius(.themeRadius.sm)
                }
                .buttonStyle(.plain)
            }
            .padding(.themeSpacing.lg)
        }
        .background(Color.bgElevation1)
        .cornerRadius(.themeRadius.md)
        .overlay(
            RoundedRectangle(cornerRadius: .themeRadius.md)
                .stroke(Color.black.opacity(0.5), lineWidth: 1)
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

