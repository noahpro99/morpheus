package dev.morpheus.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

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
    var OnDanger by mutableStateOf(Color.White)
        private set
    var IsLight by mutableStateOf(false)
        private set

    /** A hairline between things: the text colour, very faint. */
    val Divider: Color get() = Text.copy(alpha = 0.08f)

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
        OnDanger = c(p.onDanger)
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

/** The spacing scale (dp): every gap and padding is one of these. */
object Space {
    val xs = 4.dp
    val sm = 8.dp
    val md = 12.dp
    val lg = 16.dp
    val xl = 24.dp
    val xxl = 32.dp
}

/** Corner radii (dp). */
object Radius {
    val sm = 6.dp
    val md = 10.dp
    val lg = 16.dp
    val sheet = 24.dp
}

/** Sizes the screens share: the tap target, the bars, the avatars. */
object Size {
    val target = 48.dp
    val row = 56.dp
    val topBar = 56.dp
    val tabBar = 64.dp
    val rail = 72.dp
    val avatarSm = 24.dp
    val avatar = 40.dp
    val avatarLg = 36.dp
    val avatarXl = 88.dp
}

/** The type scale: title, heading, body, secondary, caption and the small-caps label. */
object Type {
    val title = TextStyle(fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.Bold)
    val heading = TextStyle(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold)
    val body = TextStyle(fontSize = 16.sp, lineHeight = 22.sp)
    val bodyStrong = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold)
    val secondary = TextStyle(fontSize = 14.sp, lineHeight = 19.sp)
    val caption = TextStyle(fontSize = 12.sp, lineHeight = 16.sp)
    val label = TextStyle(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.8.sp)
}

/** Black or white, whichever reads on `fill` (the core's `on_colour`, the one rule every screen follows). */
fun onColour(fill: Color): Color {
    val rgb = (fill.toArgb() and 0xFFFFFF).toUInt()
    return Color(0xFF000000.toInt() or uniffi.morpheus_ffi.onColour(rgb).toInt())
}
