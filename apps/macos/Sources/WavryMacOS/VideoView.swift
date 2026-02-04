import SwiftUI
import AVFoundation

struct VideoView: NSViewRepresentable {
    let layer: AVSampleBufferDisplayLayer
    
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.wantsLayer = true
        view.layer = layer
        
        // layer configuration
        layer.videoGravity = .resizeAspect
        
        return view
    }
    
    func updateNSView(_ nsView: NSView, context: Context) {}
}
