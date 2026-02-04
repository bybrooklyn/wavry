import SwiftUI

struct UserDetailView: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Header
            HStack {
                Circle()
                    .fill(Color.green)
                    .frame(width: 10, height: 10)
                Text(appState.displayName)
                    .font(.headline)
                    .foregroundColor(.white)
                Spacer()
            }
            .padding(.bottom, 4)
            
            Divider().background(Color.white.opacity(0.2))
            
            // Identity Info
            VStack(alignment: .leading, spacing: 4) {
                Text("IDENTITY FINGERPRINT")
                    .font(.caption2)
                    .fontWeight(.bold)
                    .foregroundColor(.gray)
                Text("SHA256: 8a7b...3c2d") // Placeholder for now
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.white.opacity(0.8))
            }
            
            // Mode Info
            VStack(alignment: .leading, spacing: 4) {
                Text("CONNECTIVITY MODE")
                    .font(.caption2)
                    .fontWeight(.bold)
                    .foregroundColor(.gray)
                
                HStack {
                    Image(systemName: modeIcon)
                    Text(appState.connectivityMode.displayName)
                }
                .font(.caption)
                .foregroundColor(.blue)
            }
            
            Divider().background(Color.white.opacity(0.2))
            
            // Actions
            Button("Sign Out (Reset Setup)") {
                // For demo purposes, allow resetting setup
                appState.resetSetup()
            }
            .buttonStyle(.plain)
            .font(.caption)
            .foregroundColor(.red)
        }
        .padding()
        .frame(width: 250)
        .background(Color(white: 0.15))
    }
    
    var modeIcon: String {
        switch appState.connectivityMode {
        case .wavry: return "cloud.fill"
        case .direct: return "network"
        case .custom: return "server.rack"
        }
    }
}
