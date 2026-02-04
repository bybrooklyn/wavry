import AppKit
import Foundation
import CoreText

func getSVGPath(symbolName: String) -> String? {
    let config = NSImage.SymbolConfiguration(pointSize: 24, weight: .regular)
    guard let image = NSImage(systemSymbolName: symbolName, accessibilityDescription: nil)?.withSymbolConfiguration(config) else {
        return nil
    }
    
    // SF Symbols are stored as glyphs in the system font.
    // We can't easily get the glyph index for a "symbol name" directly via public CoreText API,
    // because the mapping is private.
    // HOWEVER, we can render the image into a CGContext that records paths!
    
    let path = CGMutablePath()
    let rect = CGRect(x: 0, y: 0, width: 24, height: 24)
    
    // We'll use a PDF context but instead of parsing text, we'll use a more reliable way.
    // Actually, on macOS, we can use `NSImage` to get the `CGImage` and then trace it? No, that's raster.
    
    // Let's try the PDF approach again but with a better parser.
    // To ensure the PDF has the path data, we need to make sure it's vector.
    
    let pdfData = image.dataWithPDF(inside: NSRect(x: 0, y: 0, width: 24, height: 24))
    let pdfString = String(data: pdfData, encoding: .latin1) ?? ""
    
    // PDF commands can be compacted: "1.2 3.4 m" or "1.2 3.4 m 5.6 7.8 l"
    // We need a tokenizer.
    
    var svgPath = ""
    let tokens = pdfString.components(separatedBy: .whitespacesAndNewlines)
    var buffer: [String] = []
    
    for token in tokens {
        if let op = ["m", "l", "c", "h"].first(where: { token == $0 }) {
            switch op {
            case "m":
                if buffer.count >= 2 {
                    let y = Double(buffer.removeLast()) ?? 0
                    let x = Double(buffer.removeLast()) ?? 0
                    svgPath += "M \(x) \(24 - y) "
                }
            case "l":
                if buffer.count >= 2 {
                    let y = Double(buffer.removeLast()) ?? 0
                    let x = Double(buffer.removeLast()) ?? 0
                    svgPath += "L \(x) \(24 - y) "
                }
            case "c":
                if buffer.count >= 6 {
                    let y3 = Double(buffer.removeLast()) ?? 0
                    let x3 = Double(buffer.removeLast()) ?? 0
                    let y2 = Double(buffer.removeLast()) ?? 0
                    let x2 = Double(buffer.removeLast()) ?? 0
                    let y1 = Double(buffer.removeLast()) ?? 0
                    let x1 = Double(buffer.removeLast()) ?? 0
                    svgPath += "C \(x1) \(24 - y1) \(x2) \(24 - y2) \(x3) \(24 - y3) "
                }
            case "h":
                svgPath += "Z "
            default: break
            }
            buffer.removeAll()
        } else if Double(token) != nil {
            buffer.append(token)
        } else {
            buffer.removeAll()
        }
    }
    
    return svgPath.isEmpty ? nil : svgPath
}

// Main logic same as before...
let manifestPath = "design/icons.json"
guard let manifestData = try? Data(contentsOf: URL(fileURLWithPath: manifestPath)),
      let json = try? JSONSerialization.jsonObject(with: manifestData) as? [String: [String: String]],
      let iconMap = json["icons"] else {
    print("Error loading manifest")
    exit(1)
}

let outputDir = URL(fileURLWithPath: "crates/wavry-desktop/src/assets/icons")
try? FileManager.default.createDirectory(at: outputDir, withIntermediateDirectories: true)

for (semanticName, symbolName) in iconMap {
    print("Exporting \(semanticName) (\(symbolName))...")
    if let path = getSVGPath(symbolName: symbolName) {
        let svg = "<svg width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"\(path)\" fill=\"currentColor\"/></svg>"
        try? svg.write(to: outputDir.appendingPathComponent("\(semanticName).svg"), atomically: true, encoding: .utf8)
    } else {
        print("Failed to export \(symbolName)")
        // Don't exit, just skip or use a fallback
    }
}
print("Done!")
