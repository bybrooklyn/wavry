import SwiftUI

struct LoginView: View {
    @ObservedObject var appState: AppState
    
    @State private var email = ""
    @State private var username = ""
    @State private var password = ""
    @State private var showAdvanced = false
    @State private var customServer = "https://auth.wavry.dev"
    @State private var isRegistering = false
    @State private var isLoading = false
    @State private var errorMessage = ""
    
    var serverURL: String {
        showAdvanced ? customServer : "https://auth.wavry.dev"
    }
    
    var body: some View {
        VStack(spacing: 0) {
            // Close button
            HStack {
                Spacer()
                Button(action: { appState.showLoginSheet = false }) {
                    Image(systemName: "xmark")
                        .font(.title3)
                        .foregroundColor(.textSecondary)
                        .padding(8)
                        .background(Color.bgElevation2)
                        .cornerRadius(6)
                }
                .buttonStyle(.plain)
            }
            .padding()
            
            Spacer()
            
            // Logo / Header
            VStack(spacing: 10) {
                WavryIcon(name: .hostDefault, size: 60, color: .accentPrimary)
                Text(isRegistering ? "Create Account" : "Sign In")
                    .font(.title)
                    .fontWeight(.light)
                Text(isRegistering ? "Join Wavry to connect via username" : "Sign in to sync your devices")
                    .foregroundColor(.textSecondary)
            }
            .padding(.bottom, 30)
            
            // Form
            VStack(spacing: 15) {
                TextField("Email", text: $email)
                    .textFieldStyle(RoundedBorderTextFieldStyle())
                    .disabled(isLoading)
                
                if isRegistering {
                    TextField("Username", text: $username)
                        .textFieldStyle(RoundedBorderTextFieldStyle())
                        .disabled(isLoading)
                }
                
                SecureField("Password", text: $password)
                    .textFieldStyle(RoundedBorderTextFieldStyle())
                    .disabled(isLoading)
                
                // Advanced Toggle
                DisclosureGroup("Advanced", isExpanded: $showAdvanced) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Server URL")
                            .font(.caption)
                            .foregroundColor(.textSecondary)
                        TextField("https://auth.wavry.dev", text: $customServer)
                            .textFieldStyle(RoundedBorderTextFieldStyle())
                            .disabled(isLoading)
                    }
                    .padding(.top, 8)
                }
                .foregroundColor(.textSecondary)
                .font(.caption)
                
                if !errorMessage.isEmpty {
                    Text(errorMessage)
                        .foregroundColor(errorMessage.contains("created") ? .accentSuccess : .accentDanger)
                        .font(.caption)
                }
                
                Button(action: performAuth) {
                    HStack {
                        if isLoading {
                            ProgressView().controlSize(.small)
                        }
                        Text(isRegistering ? "Create Account" : "Sign In")
                    }
                    .frame(maxWidth: .infinity)
                    .padding()
                    .background(Color.accentPrimary)
                    .foregroundColor(.white)
                    .cornerRadius(8)
                }
                .buttonStyle(.plain)
                .disabled(isLoading || email.isEmpty || password.isEmpty || (isRegistering && username.isEmpty))
                
                Button(action: { isRegistering.toggle() }) {
                    Text(isRegistering ? "Already have an account? Sign In" : "Don't have an account? Register")
                        .font(.caption)
                        .foregroundColor(.textSecondary)
                }
                .buttonStyle(.plain)
                .disabled(isLoading)
            }
            .frame(width: 300)
            
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.bgBase)
    }
    
    func performAuth() {
        isLoading = true
        errorMessage = ""
        
        Task {
            do {
                if isRegistering {
                    guard let realPubKey = appState.getPublicKey() else {
                        errorMessage = "Failed to load Identity Key"
                        isLoading = false
                        return
                    }
                    try await AuthService.shared.register(server: serverURL, email: email, password: password, username: username, publicKey: realPubKey)
                    isRegistering = false
                    errorMessage = "Account created! Please sign in."
                } else {
                    let resp = try await AuthService.shared.login(server: serverURL, email: email, password: password)
                    
                    await MainActor.run {
                        appState.completeLogin(token: resp.token, email: resp.email)
                        appState.showLoginSheet = false
                    }
                }
            } catch {
                errorMessage = error.localizedDescription
            }
            isLoading = false
        }
    }
}
