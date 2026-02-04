import SwiftUI

enum SettingsTab {
    case client, host, network, hotkeys, account
}

struct SettingsView: View {
    @ObservedObject var appState: AppState
    @State private var activeSettingsTab: SettingsTab = .client
    
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Settings Header
            VStack(alignment: .leading, spacing: 4) {
                Text("Settings")
                    .font(.system(size: 36, weight: .light))
                    .foregroundColor(.white)
                Text("Customize your Wavry experience.")
                    .font(.body)
                    .foregroundColor(.gray)
            }
            .padding(.horizontal, .themeSpacing.xxl)
            .padding(.top, .themeSpacing.xl)
            
            // Sub-tabs
            HStack(spacing: .themeSpacing.xl) {
                SettingsTabButton(title: "Client", active: activeSettingsTab == .client) { activeSettingsTab = .client }
                SettingsTabButton(title: "Host", active: activeSettingsTab == .host) { activeSettingsTab = .host }
                SettingsTabButton(title: "Network", active: activeSettingsTab == .network) { activeSettingsTab = .network }
                SettingsTabButton(title: "Hotkeys", active: activeSettingsTab == .hotkeys) { activeSettingsTab = .hotkeys }
                SettingsTabButton(title: "Account", active: activeSettingsTab == .account) { activeSettingsTab = .account }
                Spacer()
                Text("Version 0.1.0-native")
                    .font(.caption2)
                    .foregroundColor(.gray.opacity(0.5))
            }
            .padding(.horizontal, .themeSpacing.xxl)
            .padding(.top, .themeSpacing.xl)
            .padding(.bottom, .themeSpacing.sm)
            
            Divider().background(Color.borderSubtle).padding(.horizontal, .themeSpacing.xxl)
            
            // Content
            ScrollView {
                VStack(alignment: .leading, spacing: 30) {
                    switch activeSettingsTab {
                    case .client: clientSettings
                    case .host: hostSettings
                    case .network: networkSettings
                    case .hotkeys: hotkeySettings
                    case .account: accountSettings
                    }
                }
                .padding(.themeSpacing.xxl)
            }
            
            // Footer Action
            HStack {
                Spacer()
                Button(action: {
                    appState.saveSettings()
                }) {
                    Text("Apply Changes")
                        .fontWeight(.bold)
                        .padding(.horizontal, .themeSpacing.xxl)
                        .padding(.vertical, .themeSpacing.sm)
                        .background(Color.accentPrimary)
                        .foregroundColor(.white)
                        .cornerRadius(.themeRadius.md)
                }
                .buttonStyle(.plain)
            }
            .padding(.themeSpacing.xl)
            .background(Color(white: 0.1))
        }
    }
    
    // MARK: - Tab Views
    
    var clientSettings: some View {
        VStack(alignment: .leading, spacing: 20) {
            SettingsSectionHeader(title: "Display")
            SettingsRow(label: "Overlay", sublabel: "Show Wavry overlay during session.", control: Toggle("", isOn: .constant(true)).labelsHidden())
            SettingsRow(label: "Window Mode", sublabel: "Start Wavry in fullscreen or windowed mode.", control: Picker("", selection: .constant(0)) {
                Text("Fullscreen").tag(0)
                Text("Windowed").tag(1)
            }.labelsHidden().pickerStyle(.menu).frame(width: 150))
            
            SettingsSectionHeader(title: "Performance")
            SettingsRow(label: "FPS Limit", sublabel: "Limit the client frame rate.", control: Picker("", selection: $appState.fpsLimit) {
                Text("30 FPS").tag(30)
                Text("60 FPS").tag(60)
                Text("120 FPS").tag(120)
            }.labelsHidden().pickerStyle(.menu).frame(width: 150))
            SettingsRow(label: "Decoder", sublabel: "Preferred video decoding method.", control: Text("Hardware (VideoToolbox)").foregroundColor(.gray))
        }
    }
    
    var hostSettings: some View {
        VStack(alignment: .leading, spacing: 20) {
            SettingsSectionHeader(title: "Hosting")
            SettingsRow(label: "Hosting Enabled", sublabel: "Allow connections to this computer.", control: Toggle("", isOn: .constant(true)).labelsHidden())
            SettingsRow(label: "Host Name", sublabel: "Identifies your computer to others.", control: TextField("Mac", text: $appState.displayName).textFieldStyle(.plain).padding(6).background(Color.bgElevation1).cornerRadius(.themeRadius.sm).frame(width: 200))
            
            SettingsSectionHeader(title: "Streaming")
            SettingsRow(label: "Resolution", sublabel: "Target resolution for host capture.", control: Picker("", selection: $appState.resolution) {
                Text("1280x720").tag("1280x720")
                Text("1920x1080").tag("1920x1080")
                Text("2560x1440").tag("2560x1440")
            }.labelsHidden().pickerStyle(.menu).frame(width: 150))
            SettingsRow(label: "Bandwidth Limit", sublabel: "Maximum bit rate used by the host.", control: Picker("", selection: $appState.bitrateMbps) {
                Text("10 Mbps").tag(10)
                Text("25 Mbps").tag(25)
                Text("50 Mbps").tag(50)
            }.labelsHidden().pickerStyle(.menu).frame(width: 150))
            
            SettingsSectionHeader(title: "Hardware")
            SettingsRow(label: "Display", sublabel: "Select which monitor to capture.", control: Picker("", selection: .constant(0)) {
                Text("Display 0 (Built-in)").tag(0)
            }.labelsHidden().pickerStyle(.menu).frame(width: 200))
        }
    }
    
    var networkSettings: some View {
        VStack(alignment: .leading, spacing: 20) {
            SettingsSectionHeader(title: "Connectivity Mode")
            SettingsRow(
                label: "Mode",
                sublabel: "LAN Only disables cloud features (no login, no relay).",
                control: Picker("", selection: $appState.connectivityMode) {
                    Text("Wavry Cloud").tag(ConnectivityMode.wavry)
                    Text("LAN Only").tag(ConnectivityMode.direct)
                    Text("Custom Server").tag(ConnectivityMode.custom)
                }.labelsHidden().pickerStyle(.menu).frame(width: 180)
            )
            
            if appState.connectivityMode == .custom {
                SettingsRow(
                    label: "Gateway URL",
                    sublabel: "Custom signaling server address.",
                    control: TextField("wss://...", text: $appState.authServer)
                        .textFieldStyle(.plain)
                        .padding(6)
                        .background(Color.bgElevation1)
                        .cornerRadius(.themeRadius.sm)
                        .frame(width: 250)
                )
            }
            
            SettingsSectionHeader(title: "Ports")
            SettingsRow(label: "Client Port", sublabel: "UDP port used for client traffic.", control: TextField("0", value: $appState.clientPort, formatter: NumberFormatter()).textFieldStyle(.plain).padding(6).background(Color.bgElevation1).cornerRadius(.themeRadius.sm).frame(width: 80))
            SettingsRow(label: "Host Start Port", sublabel: "Starting port for host listeners.", control: TextField("0", value: $appState.hostStartPort, formatter: NumberFormatter()).textFieldStyle(.plain).padding(6).background(Color.bgElevation1).cornerRadius(.themeRadius.sm).frame(width: 80))
            
            SettingsSectionHeader(title: "Discovery")
            SettingsRow(label: "UPnP", sublabel: "Enable automatic port forwarding.", control: Toggle("", isOn: $appState.upnpEnabled).labelsHidden())
        }
    }
    
    var hotkeySettings: some View {
        Text("No hotkeys configured yet.").foregroundColor(.gray)
    }
    
    var accountSettings: some View {
        VStack(alignment: .leading, spacing: 20) {
            SettingsSectionHeader(title: "Identity")
            SettingsRow(label: "Use Hostname", sublabel: "Automatically use MacBook's name as your identity.", control: Toggle("", isOn: $appState.isUsingHostname).labelsHidden())
            
            SettingsSectionHeader(title: "Security")
            SettingsRow(label: "Public Key", sublabel: "Cryptographic device fingerprint.", control: HostKeyView(key: appState.getPublicKey() ?? "Not Loaded"))
            
            SettingsSectionHeader(title: "Infrastructure")
            SettingsRow(label: "Auth Server", sublabel: "Wavry Master Server for signaling.", control: TextField("URL", text: $appState.authServer).textFieldStyle(.plain).padding(6).background(Color.bgElevation1).cornerRadius(.themeRadius.sm).frame(width: 250))
            
            Button("Hard Reset Application") {
                appState.resetSetup()
            }
            .buttonStyle(.plain)
            .foregroundColor(.red)
            .padding(.top, 20)
        }
    }
}

// MARK: - Components

struct SettingsSectionHeader: View {
    let title: String
    var body: some View {
        Text(title.uppercased())
            .font(.caption)
            .fontWeight(.bold)
            .foregroundColor(.accentPrimary.opacity(0.8))
            .padding(.top, .themeSpacing.sm)
    }
}

struct SettingsRow<Control: View>: View {
    let label: String
    let sublabel: String
    let control: Control
    
    var body: some View {
        HStack(alignment: .center) {
            VStack(alignment: .leading, spacing: 4) {
                Text(label)
                    .font(.body)
                    .foregroundColor(.textPrimary)
                Text(sublabel)
                    .font(.caption)
                    .foregroundColor(.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            
            control
        }
        .padding(.themeSpacing.md)
        .background(Color.white.opacity(0.02))
        .cornerRadius(.themeRadius.lg)
        .contentShape(Rectangle()) // Makes the whole area clickable for focus
    }
}

struct SettingsTabButton: View {
    let title: String
    let active: Bool
    let action: () -> Void
    
    var body: some View {
        Button(action: action) {
            VStack(spacing: 8) {
                Text(title)
                    .font(.subheadline)
                    .fontWeight(active ? .bold : .regular)
                    .foregroundColor(active ? .accentPrimary : .textSecondary)
                
                Rectangle()
                    .fill(active ? Color.accentPrimary : Color.clear)
                    .frame(height: 2)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

struct HostKeyView: View {
    let key: String
    var body: some View {
        HStack {
            Text(key.prefix(8) + "..." + key.suffix(8))
                .font(.system(.caption, design: .monospaced))
                .foregroundColor(.textSecondary)
            Button(action: {
                #if os(macOS)
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(key, forType: .string)
                #endif
            }) {
                Image(systemName: "doc.on.doc")
                    .font(.caption)
                    .foregroundColor(.accentPrimary)
            }
            .buttonStyle(.plain)
        }
    }
}
