import SwiftUI

struct DesignTokens: Codable {
    struct Colors: Codable {
        struct Bg: Codable {
            let base: String
            let sidebar: String
            let elevation1: String
            let elevation2: String
            let elevation3: String
            let modal: String
            let rowHover: String
        }
        struct Accent: Codable {
            let primary: String
            let success: String
            let danger: String
        }
        struct Text: Codable {
            let primary: String
            let secondary: String
        }
        struct Border: Codable {
            let subtle: String
            let input: String
        }
        let bg: Bg
        let accent: Accent
        let text: Text
        let border: Border
    }
    struct Spacing: Codable {
        let xs: CGFloat
        let sm: CGFloat
        let md: CGFloat
        let lg: CGFloat
        let xl: CGFloat
        let xxl: CGFloat
        let xxxl: CGFloat
    }
    struct Radius: Codable {
        let sm: CGFloat
        let md: CGFloat
        let lg: CGFloat
        let xl: CGFloat
        let xxl: CGFloat
    }
    struct Typography: Codable {
        struct Size: Codable {
            let titleLg: CGFloat
            let titleMd: CGFloat
            let input: CGFloat
            let body: CGFloat
            let caption: CGFloat
        }
        struct Weight: Codable {
            let light: Int
            let regular: Int
            let semibold: Int
            let bold: Int
        }
        let size: Size
        let weight: Weight
    }
    
    let colors: Colors
    let spacing: Spacing
    let radius: Radius
    let typography: Typography
}

class Theme: ObservableObject {
    static let shared = Theme()
    
    let tokens: DesignTokens
    
    private init() {
        guard let url = Bundle.module.url(forResource: "tokens", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let decoded = try? JSONDecoder().decode(DesignTokens.self, from: data) else {
            fatalError("Failed to load design tokens")
        }
        self.tokens = decoded
    }
}

// Helper to convert hex to Color
extension Color {
    init(hex: String) {
        let hex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: hex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch hex.count {
        case 3: // RGB (12-bit)
            (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6: // RGB (24-bit)
            (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8: // ARGB (32-bit)
            (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default:
            (a, r, g, b) = (255, 0, 0, 0)
        }
        self.init(
            .sRGB,
            red: Double(r) / 255,
            green: Double(g) / 255,
            blue: Double(b) / 255,
            opacity: Double(a) / 255
        )
    }
    
    // Support "rgba(255, 255, 255, 0.5)" format if needed, but for now we'll stick to hex or simple rgba parsing
    // Adding a simple static method for the specific ones we have
    static func fromToken(_ str: String) -> Color {
        if str.hasPrefix("#") {
            return Color(hex: str)
        } else if str.hasPrefix("rgba") {
            // Very basic rgba parser for the specific tokens we have
            let components = str.replacingOccurrences(of: "rgba(", with: "")
                .replacingOccurrences(of: ")", with: "")
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces) }
            
            if components.count == 4 {
                let r = Double(components[0]) ?? 0
                let g = Double(components[1]) ?? 0
                let b = Double(components[2]) ?? 0
                let a = Double(components[3]) ?? 1
                return Color(.sRGB, red: r/255, green: g/255, blue: b/255, opacity: a)
            }
        }
        return .clear
    }
}

extension Font.Weight {
    static func fromToken(_ weight: Int) -> Font.Weight {
        switch weight {
        case 300: return .light
        case 400: return .regular
        case 600: return .semibold
        case 700: return .bold
        default: return .regular
        }
    }
}

extension Theme {
    var colors: DesignTokens.Colors { tokens.colors }
    var spacing: DesignTokens.Spacing { tokens.spacing }
    var radius: DesignTokens.Radius { tokens.radius }
    var typography: DesignTokens.Typography { tokens.typography }
}

extension Color {
    static let theme = Theme.shared.colors
    
    // Backgrounds
    static let bgBase = Color.fromToken(Theme.shared.colors.bg.base)
    static let bgSidebar = Color.fromToken(Theme.shared.colors.bg.sidebar)
    static let bgElevation1 = Color.fromToken(Theme.shared.colors.bg.elevation1)
    static let bgElevation2 = Color.fromToken(Theme.shared.colors.bg.elevation2)
    static let bgElevation3 = Color.fromToken(Theme.shared.colors.bg.elevation3)
    static let bgModal = Color.fromToken(Theme.shared.colors.bg.modal)
    static let bgRowHover = Color.fromToken(Theme.shared.colors.bg.rowHover)
    
    // Accents
    static let accentPrimary = Color.fromToken(Theme.shared.colors.accent.primary)
    static let accentSuccess = Color.fromToken(Theme.shared.colors.accent.success)
    static let accentDanger = Color.fromToken(Theme.shared.colors.accent.danger)
    
    // Text
    static let textPrimary = Color.fromToken(Theme.shared.colors.text.primary)
    static let textSecondary = Color.fromToken(Theme.shared.colors.text.secondary)
    
    // Border
    static let borderSubtle = Color.fromToken(Theme.shared.colors.border.subtle)
    static let borderInput = Color.fromToken(Theme.shared.colors.border.input)
}

extension CGFloat {
    static let themeSpacing = Theme.shared.spacing
    static let themeRadius = Theme.shared.radius
}
