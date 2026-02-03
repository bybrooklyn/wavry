import SwiftUI

struct PermissionsView: View {
    @ObservedObject var appState: AppState
    
    var body: some View {
        VStack(spacing: 30) {
            Image(systemName: "lock.shield")
                .font(.system(size: 50))
                .foregroundColor(.blue)
            
            Text("Permissions Required")
                .font(.title2)
                .fontWeight(.bold)
            
            Text("Wavry needs Screen Recording and Accessibility permissions to stream your desktop.")
                .multilineTextAlignment(.center)
                .foregroundColor(.secondary)
            
            VStack(alignment: .leading, spacing: 15) {
                PermissionRow(title: "Screen Recording", icon: "display", isGranted: false)
                PermissionRow(title: "Accessibility", icon: "hand.point.up.left", isGranted: false)
            }
            .padding()
            .background(Color.gray.opacity(0.1))
            .cornerRadius(10)
            
            Button("Grant Permissions") {
                // Mock granting
                withAnimation {
                    appState.hasPermissions = true
                }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
        }
        .padding(40)
        .frame(width: 500)
    }
}

struct PermissionRow: View {
    let title: String
    let icon: String
    let isGranted: Bool
    
    var body: some View {
        HStack {
            Image(systemName: icon)
                .frame(width: 24)
            Text(title)
            Spacer()
            Image(systemName: isGranted ? "checkmark.circle.fill" : "circle")
                .foregroundColor(isGranted ? .green : .gray)
        }
    }
}
