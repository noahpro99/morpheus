package dev.morpheus.app.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.AppState
import uniffi.morpheus_ffi.Appearance
import uniffi.morpheus_ffi.ThemeLook
import uniffi.morpheus_ffi.ThemeRowItemKind

private fun argb(v: UInt) = Color(0xFF000000.toInt() or v.toInt())

/**
 * The theme part of Settings > Appearance (#490, #446): one row that fits the width (Material You where offered, Dark, Midnight, Light, the
 * newest custom theme, Custom), the Custom panel (a colour or a pasted code, a preview, and the history of looks), and Share theme, which
 * opens Android's share sheet with the code. The rules (the row's order, the history) are Rust's (`view_model::theme_row`).
 */
@Composable
fun ThemeSettings(app: AppModel, t: (String) -> String, look: Appearance, attempt: (() -> Unit) -> Unit) {
    val context = LocalContext.current
    var version by remember { mutableIntStateOf(0) }
    var customOpen by remember { mutableStateOf(false) }
    val offered = MaterialYou.offered()
    val entries = remember(look, version, MaterialYou.enabled) { app.morpheus.themeRowNow(offered, offered && MaterialYou.enabled) }
    fun use(change: () -> Appearance) = attempt { MaterialYou.set(context, false); app.useAppearance(change()); version++ }
    Row(Modifier.fillMaxWidth().padding(horizontal = Space.lg).testTag("setting-theme"), horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
        for (entry in entries) {
            val tag = when (entry.kind) {
                ThemeRowItemKind.MATERIAL_YOU -> "theme-material-you"
                ThemeRowItemKind.BUILTIN -> "theme-${entry.theme}"
                ThemeRowItemKind.RECENT -> "theme-recent"
                ThemeRowItemKind.CUSTOM -> "theme-custom"
            }
            val label = when (entry.kind) {
                ThemeRowItemKind.MATERIAL_YOU -> t("Material You")
                ThemeRowItemKind.BUILTIN -> t(entry.theme.replaceFirstChar { it.uppercase() })
                ThemeRowItemKind.RECENT -> "#${entry.colour.orEmpty()}"
                ThemeRowItemKind.CUSTOM -> t("Custom")
            }
            OutlinedCard(
                Modifier.weight(1f).selectable(entry.selected, role = Role.RadioButton) {
                    when (entry.kind) {
                        ThemeRowItemKind.MATERIAL_YOU -> attempt { MaterialYou.set(context, true); version++ }
                        ThemeRowItemKind.BUILTIN -> use { app.morpheus.setTheme(entry.theme) }
                        ThemeRowItemKind.RECENT -> use { app.morpheus.useThemeLook(entry.theme, entry.colour) }
                        ThemeRowItemKind.CUSTOM -> customOpen = true
                    }
                }.semantics { contentDescription = label }.testTag(tag),
                border = BorderStroke(2.dp, if (entry.selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant),
            ) {
                Column(Modifier.padding(Space.sm), horizontalAlignment = Alignment.CenterHorizontally) {
                    Row(Modifier.fillMaxWidth().height(36.dp)) {
                        if (entry.sidebar != 0u || entry.chat != 0u) {
                            Box(Modifier.weight(1f).fillMaxSize().background(argb(entry.sidebar)))
                            Box(Modifier.weight(2f).fillMaxSize().background(argb(entry.chat)))
                        } else Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceVariant))
                    }
                    Text(label, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                }
            }
        }
    }
    // Share theme: Android's share sheet with the code (the code itself is not shown).
    OutlinedButton({
        val code = app.morpheus.themeCode()
        AppState.lastShare = code
        val send = android.content.Intent(android.content.Intent.ACTION_SEND).setType("text/plain").putExtra(android.content.Intent.EXTRA_TEXT, code)
        context.startActivity(android.content.Intent.createChooser(send, t("Share theme")).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
    }, Modifier.padding(horizontal = Space.lg).testTag("theme-share")) {
        GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_forward, size = 20.dp)
        Text(t("Share theme"), Modifier.padding(start = Space.sm))
    }
    if (customOpen) CustomThemeDialog(app, t, look, attempt, { version++ }) { customOpen = false }
}

/** The Custom panel: a colour (or a pasted theme code, previewed first), what it would look like, and the looks used before, newest first. */
@Composable
private fun CustomThemeDialog(app: AppModel, t: (String) -> String, look: Appearance, attempt: (() -> Unit) -> Unit, changed: () -> Unit, close: () -> Unit) {
    val context = LocalContext.current
    var colour by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var fromCode by remember { mutableStateOf<ThemeLook?>(null) }
    var version by remember { mutableIntStateOf(0) }
    // What would be used: the previewed code's look, else the typed colour on the theme in use.
    val typed = remember(colour) { if (colour.isBlank()) null else try { uniffi.morpheus_ffi.lookSwatch(look.theme, colour).let { ThemeLook(look.theme, colour.trim().trimStart('#')) } } catch (_: Exception) { null } }
    val pending = if (colour.isNotBlank()) typed else fromCode
    val swatch = remember(pending, look) { try { uniffi.morpheus_ffi.lookSwatch(pending?.theme ?: look.theme, pending?.colour ?: look.colour) } catch (_: Exception) { null } }
    val history = remember(look, version, MaterialYou.enabled) { app.morpheus.themeHistoryNow(MaterialYou.enabled && MaterialYou.offered()) }
    ConfirmDialog(
        title = t("Custom theme"), confirm = t("Use"), confirmTag = "colour-apply", confirmEnabled = pending != null,
        dismiss = t("Close"),
        onConfirm = { pending?.let { p -> attempt { MaterialYou.set(context, false); app.useAppearance(app.morpheus.useThemeLook(p.theme, p.colour)); p.colour?.let { app.morpheus.noteRecentColour(it) }; version++; changed(); colour = ""; fromCode = null } } },
        onDismiss = close,
        content = {
            Column(Modifier.heightIn(max = 480.dp).verticalScroll(androidx.compose.foundation.rememberScrollState()).testTag("custom-theme"), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
                Row(Modifier.fillMaxWidth().height(48.dp).testTag("theme-preview").semantics { stateDescription = "#" + String.format("%06x", (swatch?.chat ?: 0u).toInt() and 0xFFFFFF) }) {
                    Box(Modifier.weight(1f).fillMaxSize().background(argb(swatch?.sidebar ?: 0u)))
                    Box(Modifier.weight(2f).fillMaxSize().background(argb(swatch?.chat ?: 0u)))
                }
                ColourPicker(app.morpheus, colour, { colour = it; fromCode = null }, if (look.colour != null) "${t("Custom colour")} #${look.colour}" else t("Custom colour"), refreshKey = version, showPreview = false, showUse = false) {}
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                    OutlinedTextField(code, { code = it }, Modifier.weight(1f).testTag("code-input"), placeholder = { Text(t("Paste a theme code.")) }, singleLine = true)
                    TextButton({ attempt { fromCode = app.morpheus.themeLookOfCode(code); colour = "" } }, Modifier.testTag("code-preview"), enabled = code.isNotBlank()) { Text(t("Preview")) }
                }
                for (entry in history) {
                    val hex = entry.colour.orEmpty()
                    Row(
                        Modifier.fillMaxWidth().selectable(entry.selected, role = Role.RadioButton) {
                            attempt { MaterialYou.set(context, false); app.useAppearance(app.morpheus.useThemeLook(entry.theme, entry.colour)); version++; changed() }
                        }.padding(vertical = Space.xs).testTag("history-$hex"),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm),
                    ) {
                        Box(Modifier.height(24.dp).weight(1f).background(argb(entry.sidebar)))
                        Box(Modifier.height(24.dp).weight(2f).background(argb(entry.chat)))
                        Text("#$hex", Modifier.weight(2f), style = MaterialTheme.typography.bodyMedium, color = if (entry.selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
                    }
                }
            }
        },
    )
}
