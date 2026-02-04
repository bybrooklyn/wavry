import SwiftUI

struct SetupWizardView: View {
    @ObservedObject var appState: AppState
    @State private var step = 1
    @State private var displayName: String = ""
    @State private var selectedMode: ConnectivityMode = .wavry
    
    var body: some View {
        ZStack {
            // Background - matching ContentView exactly
            Color(red: 0.13, green: 0.13, blue: 0.13).ignoresSafeArea()
            
            VStack {
                Spacer()
                
                VStack(spacing: 40) {
                    if step == 1 {
                        identityView
                    } else {
                        connectivityView
                    }
                }
                .frame(maxWidth: 600) // Constrain content for better scaling
                .padding(40)
                .background(Color(white: 0.1).opacity(0.5))
                .cornerRadius(24)
                .overlay(
                    RoundedRectangle(cornerRadius: 24)
                        .stroke(Color.white.opacity(0.05), lineWidth: 1)
                )
                
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(40)
        }
        .preferredColorScheme(.dark)
    }
    
    // Step 1: Identity
    var identityView: some View {
        VStack(spacing: 32) {
            VStack(spacing: 12) {
                Image(systemName: "person.circle.fill")
                    .font(.system(size: 64))
                    .foregroundColor(.blue)
                
                Text("Set Your Local Host Name")
                    .font(.system(size: 32, weight: .light))
                    .foregroundColor(.white)
                
                Text("This name identifies your computer when hosting sessions or connecting to others.")
                    .font(.body)
                    .foregroundColor(.gray)
                    .multilineTextAlignment(.center)
            }
            
            VStack(alignment: .leading, spacing: 8) {
                TextField("e.g. My MacPro", text: $displayName)
                    .textFieldStyle(PlainTextFieldStyle())
                    .padding(16)
                    .background(Color(white: 0.15))
                    .cornerRadius(12)
                    .font(.system(size: 18))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12)
                            .stroke(Color.white.opacity(0.1), lineWidth: 1)
                    )
            }
            
            Button(action: {
                withAnimation { step = 2 }
            }) {
                Text("Continue")
                    .fontWeight(.bold)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 16)
                    .background(displayName.isEmpty ? Color.gray.opacity(0.2) : Color.blue)
                    .foregroundColor(displayName.isEmpty ? .gray : .white)
                    .cornerRadius(12)
            }
            .buttonStyle(.plain)
            .disabled(displayName.isEmpty)
        }
    }
    
    // Step 2: Connectivity Mode
    var connectivityView: some View {
        VStack(spacing: 32) {
            VStack(spacing: 12) {
                Text("Choose Connectivity")
                    .font(.system(size: 32, weight: .light))
                    .foregroundColor(.white)
                
                Text("Select how you want to discover and connect to peers.")
                    .font(.body)
                    .foregroundColor(.gray)
            }
            
            VStack(spacing: 16) {
                ModeOptionCard(
                    title: "Wavry Service",
                    description: "Global discovery via Wavry's secure signaling network.",
                    icon: "cloud.fill",
                    isSelected: selectedMode == .wavry,
                    onSelect: { selectedMode = .wavry }
                )
                
                ModeOptionCard(
                    title: "Direct Connection",
                    description: "Manual IP/Port connection. Best for LAN and power users.",
                    icon: "network",
                    isSelected: selectedMode == .direct,
                    onSelect: { selectedMode = .direct }
                )
            }
            
            HStack(spacing: 16) {
                Button(action: {
                    withAnimation { step = 1 }
                }) {
                    Text("Back")
                        .fontWeight(.semibold)
                        .padding(.vertical, 14)
                        .padding(.horizontal, 30)
                        .background(Color(white: 0.2))
                        .cornerRadius(12)
                }
                .buttonStyle(.plain)
                
                Button(action: {
                    appState.completeSetup(name: displayName, mode: selectedMode)
                }) {
                    Text("Ready to Start")
                        .fontWeight(.bold)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                        .background(Color.blue)
                        .foregroundColor(.white)
                        .cornerRadius(12)
                }
                .buttonStyle(.plain)
            }
        }
    }
}

struct ModeOptionCard: View {
    let title: String
    let description: String
    let icon: String
    let isSelected: Bool
    let onSelect: () -> Void
    
    var body: some View {
        Button(action: onSelect) {
            HStack(spacing: 20) {
                Image(systemName: icon)
                    .font(.system(size: 24))
                    .foregroundColor(isSelected ? .blue : .gray)
                    .frame(width: 50, height: 50)
                    .background(isSelected ? Color.blue.opacity(0.1) : Color(white: 0.15))
                    .cornerRadius(10)
                
                VStack(alignment: .leading, spacing: 4) {
                    Text(title)
                        .font(.headline)
                        .foregroundColor(.white)
                    Text(description)
                        .font(.caption)
                        .foregroundColor(.gray)
                        .lineLimit(2)
                }
                
                Spacer()
                
                if isSelected {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundColor(.blue)
                }
            }
            .padding(16)
            .background(Color(white: 0.15).opacity(isSelected ? 1.0 : 0.5))
            .cornerRadius(12)
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(isSelected ? Color.blue : Color.clear, lineWidth: 1)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
