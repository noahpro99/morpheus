package dev.morpheus.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** The dark surfaces the other Morpheus apps have (the Dioxus phone app's #0D0F14 first). */
object Palette {
    var Background by mutableStateOf(Color(0xFF0D0F14))
        private set
    var Surface by mutableStateOf(Color(0xFF151821))
        private set
    var Raised by mutableStateOf(Color(0xFF1D212D))
        private set
    var Accent by mutableStateOf(Color(0xFF7C8CF8))
        private set
    var OnAccent by mutableStateOf(Color.White)
        private set
    var Text by mutableStateOf(Color(0xFFE6E8EF))
        private set
    var Muted by mutableStateOf(Color(0xFF9AA0B2))
        private set
    var Danger by mutableStateOf(Color(0xFFF0616D))
        private set
    var IsLight by mutableStateOf(false)
        private set

    /** Draw with a palette the core made (a theme, or a custom colour). */
    fun use(p: uniffi.morpheus_ffi.PaletteItem) {
        fun c(v: UInt) = Color(0xFF000000.toInt() or v.toInt())
        Background = c(p.background)
        Surface = c(p.surface)
        Raised = c(p.raised)
        Accent = c(p.accent)
        OnAccent = c(p.onAccent)
        Text = c(p.text)
        Muted = c(p.muted)
        Danger = c(p.danger)
        IsLight = p.isLight
    }
}

@Composable
fun MorpheusTheme(content: @Composable () -> Unit) {
    val scheme = if (Palette.IsLight) {
        lightColorScheme(
            primary = Palette.Accent, onPrimary = Palette.OnAccent, background = Palette.Background,
            surface = Palette.Surface, surfaceVariant = Palette.Raised, onBackground = Palette.Text,
            onSurface = Palette.Text, onSurfaceVariant = Palette.Muted, error = Palette.Danger,
        )
    } else {
        darkColorScheme(
            primary = Palette.Accent, onPrimary = Palette.OnAccent, background = Palette.Background,
            surface = Palette.Surface, surfaceVariant = Palette.Raised, onBackground = Palette.Text,
            onSurface = Palette.Text, onSurfaceVariant = Palette.Muted, error = Palette.Danger,
        )
    }
    MaterialTheme(colorScheme = scheme, content = content)
}
