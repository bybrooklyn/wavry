import AppKit
import Foundation

struct IconManifest: Codable {
    let icons: [String: String]
}

func getSVGPath(from cgPath: CGPath) -> String {
    var svgPath = ""
    cgPath.applyWithBlock { element in
        let points = element.pointee.points
        switch element.pointee.type {
        case .moveToPoint:
            svgPath += "M \(points[0].x) \(points[0].y) "
        case .addLineToPoint:
            svgPath += "L \(points[0].x) \(points[0].y) "
        case .addQuadCurveToPoint:
            svgPath += "Q \(points[0].x) \(points[0].y) \(points[1].x) \(points[1].y) "
        case .addCurveToPoint:
            svgPath += "C \(points[0].x) \(points[0].y) \(points[1].x) \(points[1].y) \(points[2].x) \(points[2].y) "
        case .closeSubpath:
            svgPath += "Z "
        @unknown default:
            break
        }
    }
    return svgPath
}

func exportSymbol(name: String, to url: URL) {
    let pointSize: CGFloat = 24.0
    let config = NSImage.SymbolConfiguration(pointSize: pointSize, weight: .regular)
    guard let image = NSImage(systemSymbolName: name, accessibilityDescription: nil)?.withSymbolConfiguration(config) else {
        print("Error: Could not find symbol \(name)")
        exit(1)
    }
    
    // SF Symbols are rendered as multiple layers. 
    // For simplicity and matching the "monochrome/outline" constraint, 
    // we'll try to get the path by rendering into a context that captures it, 
    // or by finding the glyph.
    
    // Actually, the easiest way to get the path of a system symbol is to use 
    // the internal _vibrantRepresentation if available, but that's private.
    // Instead, we can use the fact that they are available as glyphs.
    
    // Rendering to a PDF and extracting path is more reliable for multi-layer symbols.
    // But for now, let's try a simpler approach: use NSImage path extraction if possible.
    
    // Since we can't easily get the path from NSImage directly in a public way,
    // let's use the core graphics approach to render to a PDF data and then 
    // a very limited PDF-to-SVG conversion for these specific files.
    
    let rect = NSRect(x: 0, y: 0, width: pointSize * 2, height: pointSize * 2) // Extra padding
    let pdfData = image.dataWithPDF(inside: rect)
    
    // Instead of parsing PDF, let's use a hidden trick:
    // Rendering the image to a CGContext which is a custom one.
    // Wait, I will use a SwiftUI-based approach if I can run it from CLI.
    
    // Actually, I'll use a Python script that uses AppKit to save as PDF 
    // and then I'll use a small python snippet to extract paths from the PDF.
}

// For now, I'll provide a script that uses the known glyph names if possible.
// Actually, I'll just use a Swift script that renders the symbol to a PNG 
// and I'll notify the user that for TRUE SVG we need a specific tool 
// or I can try one more trick.
