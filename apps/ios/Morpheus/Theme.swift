import SwiftUI

extension Color {
    /// A colour of the shared theme, `0xRRGGBB`.
    init(rgb: UInt32) {
        self.init(
            red: Double((rgb >> 16) & 0xFF) / 255,
            green: Double((rgb >> 8) & 0xFF) / 255,
            blue: Double(rgb & 0xFF) / 255
        )
    }
}

/// The colours a screen draws with: the core's palette (view-model `theme`), never made up here.
/// Text on a fill uses the core's `on_*` colours.
struct Theme {
    var background = Color(rgb: 0x1E1F22)
    var surface = Color(rgb: 0x313338)
    var raised = Color(rgb: 0x2B2D31)
    var text = Color.primary
    var muted = Color.secondary
    var accent = Color.accentColor
    var onAccent = Color.white
    var danger = Color.red
    var onDanger = Color.white

    init() {}

    init(_ palette: PaletteItem) {
        background = Color(rgb: palette.background)
        surface = Color(rgb: palette.surface)
        raised = Color(rgb: palette.raised)
        text = Color(rgb: palette.text)
        muted = Color(rgb: palette.muted)
        accent = Color(rgb: palette.accent)
        onAccent = Color(rgb: palette.onAccent)
        danger = Color(rgb: palette.danger)
        onDanger = Color(rgb: palette.onDanger)
    }
}
