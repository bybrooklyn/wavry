import SwiftUI
import Combine

import Clibwavry

class AppState: ObservableObject {
    @Published var hasPermissions: Bool = false
    @Published var isConnected: Bool = false
    @Published var fps: Int = 0
    @Published var rtt: Double = 0.0
    
    // Mock connection
    func connect() {
        // Connect to Rust backend here
        wavry_connect()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            self.isConnected = true
            self.startMockStats()
        }
    }
    
    func startMockStats() {
        Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { _ in
            self.fps = Int.random(in: 58...60)
            self.rtt = Double.random(in: 2.0...5.0)
        }
    }
}
