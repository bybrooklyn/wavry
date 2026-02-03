import SwiftUI
import Clibwavry

@main
struct WavryApp: App {
    @StateObject var appState = AppState()
    
    init() {
        print("Initializing Wavry Model...")
        wavry_init() 
    }
    
    var body: some Scene {
        WindowGroup {
            ContentView(appState: appState)
        }
        .windowStyle(.hiddenTitleBar)
    }
}
