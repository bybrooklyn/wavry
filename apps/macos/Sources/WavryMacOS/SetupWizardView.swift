import SwiftUI

struct SetupWizardView: View {
    @ObservedObject var appState: AppState
    @State private var step = 1
    @State private var displayName: String = ""
    @State private var selectedMode: ConnectivityMode = .wavry
    
    var body: some View {
        ZStack {
            // Background - matching ContentView exactly
            Color.bgBase.ignoresSafeArea()
            
            VStack {
                Spacer()
                
                VStack(spacing: .themeSpacing.xxxl) {
                    if step == 1 {
                        identityView
                    } else {
                        connectivityView
                    }
                }
                .frame(maxWidth: 600) // Constrain content for better scaling
                .padding(.themeSpacing.xxxl)
                .background(Color.bgModal)
                .cornerRadius(.themeRadius.xxl)
                .overlay(
                    RoundedRectangle(cornerRadius: .themeRadius.xxl)
                        .stroke(Color.white.opacity(0.05), lineWidth: 1)
                )
                
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.themeSpacing.xxxl)
        }
        .preferredColorScheme(.dark)
    }
    
    // Step 1: Identity
    var identityView: some View {
        VStack(spacing: .themeSpacing.xxl) {
            VStack(spacing: .themeSpacing.md) {
                Image(systemName: "person.circle.fill")
                    .font(.system(size: 64))
                    .foregroundColor(.accentPrimary)
                
                Text("Set Your Local Host Name")
                    .font(.system(size: 32, weight: .light))
                    .foregroundColor(.white)
                
                Text("This name identifies your computer when hosting sessions or connecting to others.")
                    .font(.body)
                    .foregroundColor(.gray)
                    .multilineTextAlignment(.center)
            }
            
            VStack(alignment: .leading, spacing: .themeSpacing.sm) {
                TextField("e.g. My MacPro", text: $displayName)
                    .textFieldStyle(PlainTextFieldStyle())
                    .padding(.themeSpacing.lg)
                    .background(Color.bgElevation1)
                    .cornerRadius(.themeRadius.xl)
                    .font(.system(size: .themeSpacing.xl)) // Using 20 as roughly 18
                    .overlay(
                        RoundedRectangle(cornerRadius: .themeRadius.xl)
                            .stroke(Color.borderInput, lineWidth: 1)
                    )
            }
            
            Button(action: {
                withAnimation { step = 2 }
            }) {
                Text("Continue")
                    .fontWeight(.bold)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, .themeSpacing.lg)
                    .background(displayName.isEmpty ? Color.textSecondary.opacity(0.2) : Color.accentPrimary)
                    .foregroundColor(displayName.isEmpty ? .textSecondary : .white)
                    .cornerRadius(.themeRadius.xl)
            }
            .buttonStyle(.plain)
            .disabled(displayName.isEmpty)
        }
    }
    
    // Step 2: Connectivity Mode
    var connectivityView: some View {
        VStack(spacing: .themeSpacing.xxl) {
            VStack(spacing: .themeSpacing.md) {
                Text("Choose Connectivity")
                    .font(.system(size: 32, weight: .light))
                    .foregroundColor(.white)
                
                Text("Select how you want to discover and connect to peers.")
                    .font(.body)
                    .foregroundColor(.gray)
            }
            
            VStack(spacing: .themeSpacing.lg) {
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
            
            HStack(spacing: .themeSpacing.lg) {
                Button(action: {
                    withAnimation { step = 1 }
                }) {
                    Text("Back")
                        .fontWeight(.semibold)
                        .padding(.vertical, 14)
                        .padding(.horizontal, .themeSpacing.xxl)
                        .background(Color.bgElevation3)
                        .cornerRadius(.themeRadius.xl)
                }
                .buttonStyle(.plain)
                
                Button(action: {
                    appState.completeSetup(name: displayName, mode: selectedMode)
                }) {
                    Text("Ready to Start")
                        .fontWeight(.bold)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                        .background(Color.accentPrimary)
                        .foregroundColor(.white)
                        .cornerRadius(.themeRadius.xl)
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
            HStack(spacing: .themeSpacing.xl) {
                Image(systemName: icon)
                    .font(.system(size: 24))
                    .foregroundColor(isSelected ? .accentPrimary : .textSecondary)
                    .frame(width: 50, height: 50)
                    .background(isSelected ? Color.accentPrimary.opacity(0.1) : Color.bgElevation1)
                    .cornerRadius(.themeRadius.md)
                
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
                        .foregroundColor(.accentPrimary)
                }
            }
            .padding(.themeSpacing.lg)
            .background(Color.bgElevation1.opacity(isSelected ? 1.0 : 0.5))
            .cornerRadius(.themeRadius.xl)
            .overlay(
                RoundedRectangle(cornerRadius: .themeRadius.xl)
                    .stroke(isSelected ? Color.accentPrimary : Color.clear, lineWidth: 1)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
