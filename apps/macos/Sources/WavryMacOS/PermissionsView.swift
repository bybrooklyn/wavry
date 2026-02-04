import SwiftUI

struct PermissionsView: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        VStack(spacing: 0) {
            // Icon
            Image(systemName: "lock.shield.fill")
                .font(.system(size: 72))
                .foregroundStyle(.blue)
                .padding(.bottom, 24)
                .padding(.top, 60)
            
            Text("Permissions Required")
                .font(.largeTitle)
                .fontWeight(.bold)
                .padding(.bottom, 12)
            
            Text("Wavry needs access to screen recording to capture your desktop.\nGranting permission allows sending video to your other devices.")
                .multilineTextAlignment(.center)
                .foregroundColor(.secondary)
                .font(.body)
                .padding(.horizontal, 60)
                .padding(.bottom, 40)
            
            // Permission Status Item
            HStack {
                Image(systemName: "display")
                    .font(.title2)
                    .frame(width: 40)
                    .foregroundColor(.secondary)
                
                VStack(alignment: .leading) {
                    Text("Screen Recording")
                        .font(.headline)
                    Text(appState.hasPermissions ? "Access Granted" : "Access Denied / Not Determined")
                        .font(.caption)
                        .foregroundColor(appState.hasPermissions ? .green : .secondary)
                }
                Spacer()
                
                if appState.hasPermissions {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundColor(.green)
                        .font(.title2)
                } else {
                    Button("Open Settings") {
                        appState.requestPermissions()
                    }
                }
            }
            .padding()
            .background(Color(nsColor: .controlBackgroundColor))
            .cornerRadius(12)
            .padding(.horizontal, 50)
            
            Spacer()
            
            if !appState.hasPermissions {
                HStack {
                    Image(systemName: "info.circle")
                    Text("You may need to restart the app after granting permissions.")
                }
                .font(.caption)
                .foregroundColor(.secondary)
                .padding(.bottom, 40)
            }
        }
        .frame(width: 600, height: 500)
    }
}
