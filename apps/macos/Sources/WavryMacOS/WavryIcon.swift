import SwiftUI

/// Semantic icon names used across the Wavry application.
/// This enum is the single source of truth for icon choice.
enum WavryIconName: String, CaseIterable {
    case tabSessions = "desktopcomputer"
    case tabSettings = "gearshape.fill"
    case copy = "doc.on.doc"
    case identity = "person.circle.fill"
    case success = "checkmark.circle.fill"
    case noSessions = "network.slash"
    case hostDefault = "macpro.gen3.fill"
    case connectivityService = "cloud.fill"
    case connectivityDirect = "network"
    case connectivityCustom = "server.rack"
    case permissions = "lock.shield.fill"
    case screenRecording = "display"
    case info = "info.circle"
}

/// A view that renders an icon based on a semantic WavryIconName.
struct WavryIcon: View {
    let name: WavryIconName
    var size: CGFloat? = nil
    var color: Color? = nil
    
    var body: some View {
        Image(systemName: name.rawValue)
            .resizable()
            .aspectRatio(contentMode: .fit)
            .frame(width: size, height: size)
            .foregroundColor(color)
    }
}

extension View {
    /// Helper to render a WavryIcon inline with SwiftUI modifier-like syntax.
    func wavryIcon(_ name: WavryIconName, size: CGFloat? = nil, color: Color? = nil) -> some View {
        WavryIcon(name: name, size: size, color: color)
    }
}
