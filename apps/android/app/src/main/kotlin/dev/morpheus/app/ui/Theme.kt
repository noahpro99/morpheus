package dev.morpheus.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** The dark surfaces the other Morpheus apps have (the Dioxus phone app's #0D0F14 first). */
object Palette {
    val Background = Color(0xFF0D0F14)
    val Surface = Color(0xFF151821)
    val Raised = Color(0xFF1D212D)
    val Accent = Color(0xFF7C8CF8)
    val Text = Color(0xFFE6E8EF)
    val Muted = Color(0xFF9AA0B2)
    val Danger = Color(0xFFF0616D)
}

@Composable
fun MorpheusTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Palette.Accent,
            background = Palette.Background,
            surface = Palette.Surface,
            surfaceVariant = Palette.Raised,
            onBackground = Palette.Text,
            onSurface = Palette.Text,
            onSurfaceVariant = Palette.Muted,
            error = Palette.Danger,
        ),
        content = content,
    )
}
