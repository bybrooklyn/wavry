import SwiftUI

struct ContentView: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        if !appState.hasPermissions {
            PermissionsView(appState: appState)
        } else {
            DashboardView(appState: appState)
        }
    }
}

struct DashboardView: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "wave.3.forward")
                .font(.system(size: 60))
                .foregroundStyle(appState.isConnected ? 
                    LinearGradient(colors: [.green, .blue], startPoint: .leading, endPoint: .trailing) :
                    LinearGradient(colors: [.gray, .gray], startPoint: .leading, endPoint: .trailing))
                
            Text("Wavry")
                .font(.largeTitle)
                .fontWeight(.bold)
            
            if appState.isConnected {
                HStack(spacing: 20) {
                    StatBox(label: "FPS", value: "\(appState.fps)")
                    StatBox(label: "RTT", value: String(format: "%.1f ms", appState.rtt))
                }
            } else {
                Text("Ready to Stream")
                    .foregroundColor(.secondary)
            }
            
            Button(appState.isConnected ? "Disconnect" : "Start Server") {
                if appState.isConnected {
                    appState.isConnected = false
                } else {
                    appState.connect()
                }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
        }
        .padding(50)
        .frame(minWidth: 500, minHeight: 400)
    }
}

struct StatBox: View {
    let label: String
    let value: String
    
    var body: some View {
        VStack {
            Text(label)
                .font(.caption)
                .foregroundColor(.secondary)
            Text(value)
                .font(.title2)
                .fontDesign(.monospaced)
                .fontWeight(.bold)
        }
        .padding()
        .background(.ultraThinMaterial)
        .cornerRadius(8)
    }
}
