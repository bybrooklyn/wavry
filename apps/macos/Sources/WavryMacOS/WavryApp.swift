import SwiftUI
import Clibwavry

@main
struct WavryApp: App {
    @StateObject var appState = AppState()
    
    init() {
        print("Initializing Wavry Model...")
        wavry_init() 
        
        #if os(macOS)
        // Ensure the app becomes a "regular" GUI app and takes focus
        NSApplication.shared.setActivationPolicy(.regular)
        DispatchQueue.main.async {
            NSApplication.shared.activate(ignoringOtherApps: true)
        }
        #endif
    }
    
    var body: some Scene {
        WindowGroup {
            Group {
                if appState.isSetupCompleted {
                    ContentView(appState: appState)
                } else {
                    SetupWizardView(appState: appState)
                }
            }
            .frame(minWidth: 900, minHeight: 650)
            .sheet(isPresented: $appState.showLoginSheet) {
                LoginView(appState: appState)
                    .frame(width: 400, height: 500)
            }
        }
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unified)
    }
}
