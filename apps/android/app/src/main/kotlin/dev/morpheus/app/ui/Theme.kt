package dev.morpheus.app.ui

import android.content.Context
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Composable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private fun rgb(v: UInt) = Color(0xFF000000.toInt() or v.toInt())
private fun Color.rgb(): UInt = (toArgb() and 0xFFFFFF).toUInt()

/** The Material 3 colour scheme of a palette the core made: every on-colour is the core's, picked by contrast. */
private fun schemeOf(m: uniffi.morpheus_ffi.MaterialItem, light: Boolean): ColorScheme =
    (if (light) lightColorScheme() else darkColorScheme()).copy(
        primary = rgb(m.primary), onPrimary = rgb(m.onPrimary),
        primaryContainer = rgb(m.primaryContainer), onPrimaryContainer = rgb(m.onPrimaryContainer),
        inversePrimary = rgb(m.inversePrimary),
        secondary = rgb(m.secondary), onSecondary = rgb(m.onSecondary),
        secondaryContainer = rgb(m.secondaryContainer), onSecondaryContainer = rgb(m.onSecondaryContainer),
        tertiary = rgb(m.tertiary), onTertiary = rgb(m.onTertiary),
        tertiaryContainer = rgb(m.tertiaryContainer), onTertiaryContainer = rgb(m.onTertiaryContainer),
        error = rgb(m.error), onError = rgb(m.onError),
        errorContainer = rgb(m.errorContainer), onErrorContainer = rgb(m.onErrorContainer),
        background = rgb(m.background), onBackground = rgb(m.onBackground),
        surface = rgb(m.surface), onSurface = rgb(m.onSurface),
        surfaceVariant = rgb(m.surfaceVariant), onSurfaceVariant = rgb(m.onSurfaceVariant),
        surfaceTint = rgb(m.primary),
        inverseSurface = rgb(m.inverseSurface), inverseOnSurface = rgb(m.inverseOnSurface),
        outline = rgb(m.outline), outlineVariant = rgb(m.outlineVariant), scrim = rgb(m.scrim),
        surfaceBright = rgb(m.surfaceContainerHighest), surfaceDim = rgb(m.background),
        surfaceContainerLowest = rgb(m.surfaceContainerLowest), surfaceContainerLow = rgb(m.surfaceContainerLow),
        surfaceContainer = rgb(m.surfaceContainer), surfaceContainerHigh = rgb(m.surfaceContainerHigh),
        surfaceContainerHighest = rgb(m.surfaceContainerHighest),
    )

/** A scheme the system made (Material You) with every on-colour put through the core's contrast helper: kept where it reads, else black or white. */
private fun contrasted(s: ColorScheme): ColorScheme {
    fun on(preferred: Color, fill: Color) = rgb(uniffi.morpheus_ffi.readableOn(preferred.rgb(), fill.rgb()))
    return s.copy(
        onPrimary = on(s.onPrimary, s.primary), onPrimaryContainer = on(s.onPrimaryContainer, s.primaryContainer),
        onSecondary = on(s.onSecondary, s.secondary), onSecondaryContainer = on(s.onSecondaryContainer, s.secondaryContainer),
        onTertiary = on(s.onTertiary, s.tertiary), onTertiaryContainer = on(s.onTertiaryContainer, s.tertiaryContainer),
        onError = on(s.onError, s.error), onErrorContainer = on(s.onErrorContainer, s.errorContainer),
        onBackground = on(s.onBackground, s.background), onSurface = on(s.onSurface, s.surface),
        onSurfaceVariant = on(s.onSurfaceVariant, s.surfaceVariant), inverseOnSurface = on(s.inverseOnSurface, s.inverseSurface),
    )
}

/** Every (role, fill, foreground drawn on it) of a scheme: what the contrast checks (and the debug bridge) read. */
fun colourPairs(s: ColorScheme): List<Triple<String, Int, Int>> = listOf(
    Triple("primary", s.primary.toArgb(), s.onPrimary.toArgb()),
    Triple("primary_container", s.primaryContainer.toArgb(), s.onPrimaryContainer.toArgb()),
    Triple("secondary", s.secondary.toArgb(), s.onSecondary.toArgb()),
    Triple("secondary_container", s.secondaryContainer.toArgb(), s.onSecondaryContainer.toArgb()),
    Triple("tertiary", s.tertiary.toArgb(), s.onTertiary.toArgb()),
    Triple("tertiary_container", s.tertiaryContainer.toArgb(), s.onTertiaryContainer.toArgb()),
    Triple("error", s.error.toArgb(), s.onError.toArgb()),
    Triple("error_container", s.errorContainer.toArgb(), s.onErrorContainer.toArgb()),
    Triple("background", s.background.toArgb(), s.onBackground.toArgb()),
    Triple("surface", s.surface.toArgb(), s.onSurface.toArgb()),
    Triple("surface_variant", s.surfaceVariant.toArgb(), s.onSurfaceVariant.toArgb()),
    Triple("surface_container_high", s.surfaceContainerHigh.toArgb(), s.onSurface.toArgb()),
    Triple("inverse_surface", s.inverseSurface.toArgb(), s.inverseOnSurface.toArgb()),
    Triple("composer_text", ComposerColours.fill(s).toArgb(), ComposerColours.text(s).toArgb()),
    Triple("composer_hint", ComposerColours.fill(s).toArgb(), ComposerColours.hint(s).toArgb()),
)

/**
 * Material You: the system's dynamic colour (Android 12 and up) as an optional theme, picked in Settings, kept on this phone.
 * The look is the app's own theme otherwise.
 */
object MaterialYou {
    /** Dynamic colour needs Android 12 (API 31). */
    fun offered(sdk: Int = android.os.Build.VERSION.SDK_INT): Boolean = sdk >= 31

    private const val FILE = "look"
    private const val KEY = "material_you"

    /** Whether it is on (the person's choice; only in effect where it is offered). */
    var enabled by mutableStateOf(false)
        private set

    /** Read the choice and make the scheme it asks for (at start, and when the system's dark mode changes). */
    fun refresh(context: Context) {
        enabled = try { context.getSharedPreferences(FILE, Context.MODE_PRIVATE).getBoolean(KEY, false) } catch (_: Exception) { false }
        apply(context)
    }

    fun set(context: Context, on: Boolean) {
        try { context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit().putBoolean(KEY, on).apply() } catch (_: Exception) {}
        enabled = on
        apply(context)
    }

    private fun apply(context: Context) {
        Palette.dynamic = if (enabled && offered()) {
            val night = context.resources.configuration.uiMode and android.content.res.Configuration.UI_MODE_NIGHT_MASK == android.content.res.Configuration.UI_MODE_NIGHT_YES
            contrasted(if (night) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context))
        } else null
    }
}

/**
 * The colours every screen draws with: the core's palette of the look in use (a theme or a custom colour), or the system's
 * dynamic colours when Material You is on. All of it is one Material 3 [ColorScheme]; the names below are what the screens ask for.
 */
object Palette {
    private var fixed by mutableStateOf(darkColorScheme())
    private var fixedLight by mutableStateOf(false)

    /** Set while Material You is in use. */
    var dynamic by mutableStateOf<ColorScheme?>(null)

    /** The scheme in use. */
    val scheme: ColorScheme get() = dynamic ?: fixed

    val Background: Color get() = scheme.background
    val Surface: Color get() = scheme.surface
    val Raised: Color get() = scheme.surfaceContainerHigh
    val Accent: Color get() = scheme.primary
    val OnAccent: Color get() = scheme.onPrimary
    val Text: Color get() = scheme.onSurface
    val Muted: Color get() = scheme.onSurfaceVariant
    val Danger: Color get() = scheme.error
    val OnDanger: Color get() = scheme.onError
    val IsLight: Boolean get() = if (dynamic != null) scheme.background.luminance() > 0.5f else fixedLight

    /** A hairline between things: the outline colour, faint. */
    val Divider: Color get() = scheme.outlineVariant

    /** Draw with a palette the core made (a theme, or a custom colour). */
    fun use(p: uniffi.morpheus_ffi.PaletteItem) {
        fixed = schemeOf(p.material, p.isLight)
        fixedLight = p.isLight
    }
}

/** The type scale is Material 3's own (m3.material.io, Typography): display, headline, title, body and label roles at the spec's sizes. */
private val MorpheusTypography = Typography()

/** The corner radii as Material 3 shapes. */
private val MorpheusShapes = Shapes(
    extraSmall = RoundedCornerShape(4.dp), small = RoundedCornerShape(Radius.sm), medium = RoundedCornerShape(Radius.md),
    large = RoundedCornerShape(Radius.lg), extraLarge = RoundedCornerShape(Radius.sheet),
)

@Composable
fun MorpheusTheme(content: @Composable () -> Unit) {
    val scheme = Palette.scheme
    androidx.compose.runtime.SideEffect { dev.morpheus.app.AppState.colourPairs = colourPairs(scheme); dev.morpheus.app.AppState.colourSource = if (Palette.dynamic != null) "dynamic" else "palette" }
    MaterialTheme(colorScheme = scheme, typography = MorpheusTypography, shapes = MorpheusShapes, content = content)
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
    val rail = 80.dp
    /** A server on the rail: its 48 dp icon, with 16 dp between them (the rail starts level with the channel panel). */
    val railItem = 48.dp
    val avatarSm = 24.dp
    val avatar = 40.dp
    val avatarLg = 36.dp
    val avatarXl = 88.dp
}


/** Black or white, whichever reads on `fill` (the core's `on_colour`, the one rule every screen follows). */
fun onColour(fill: Color): Color {
    val rgb = (fill.toArgb() and 0xFFFFFF).toUInt()
    return Color(0xFF000000.toInt() or uniffi.morpheus_ffi.onColour(rgb).toInt())
}

/** [preferred] if it reads on [fill], else black or white: the core's contrast helper for anything drawn on a fill the scheme has no pair for. */
fun readable(preferred: Color, fill: Color): Color = rgb(uniffi.morpheus_ffi.readableOn(preferred.rgb(), fill.rgb()))

/** What the message box draws on its field (a surface container): the text, and the hint and icons, each read against the field's fill. */
object ComposerColours {
    fun fill(s: ColorScheme): Color = s.surfaceContainerHigh
    fun text(s: ColorScheme): Color = readable(s.onSurface, fill(s))
    fun hint(s: ColorScheme): Color = readable(s.onSurfaceVariant, fill(s))
}
