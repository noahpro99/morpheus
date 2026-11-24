package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.NotifyDefaults

/** Settings: the look, the language, what notifies by default, and the account. */
@Composable
fun SettingsScreen(app: AppModel) {
    BackHandler { app.closeSettings() }
    val page by app.settingsPage.collectAsState()
    val language by app.language.collectAsState()
    val look by app.appearance.collectAsState()
    val t: (String) -> String = remember(language) { { app.t(it) } }
    var problem by remember { mutableStateOf<String?>(null) }
    val account = app.morpheus.account()

    fun attempt(block: () -> Unit) {
        try {
            problem = null
            block()
        } catch (e: Exception) {
            problem = e.sentence()
        }
    }

    Column(Modifier.fillMaxSize().background(Palette.Surface).testTag("settings-screen")) {
        TopBar(t("Settings"), backTag = "settings-back", onBack = { app.closeSettings() }, titleTag = "settings-title")
        if (page == null) SettingsGroups(app, t, Modifier.weight(1f)) else Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (page == "Appearance") {
            Heading(t("Appearance"))
            Row(Modifier.testTag("setting-theme"), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                for (choice in app.morpheus.themes()) {
                    val chosen = look.theme == choice.key && look.colour == null
                    Column(
                        Modifier
                            .border(2.dp, if (chosen) Palette.Accent else Color.Transparent, RoundedCornerShape(10.dp))
                            .clickable { attempt { app.useAppearance(app.morpheus.setTheme(choice.key)) } }
                            .padding(8.dp)
                            .testTag("theme-${choice.key}"),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Row(Modifier.size(width = 64.dp, height = 36.dp)) {
                            Box(Modifier.weight(1f).fillMaxSize().background(argb(choice.sidebar)))
                            Box(Modifier.weight(2f).fillMaxSize().background(argb(choice.chat)))
                        }
                        Text(t(choice.label), fontSize = 13.sp)
                    }
                }
            }
            var colour by remember { mutableStateOf("") }
            Text(
                if (look.colour != null) "${t("Custom colour")} #${look.colour}" else t("Custom colour"),
                Modifier.testTag("custom-colour-label"), color = Palette.Muted, fontSize = 13.sp,
            )
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(colour, { colour = it }, Modifier.weight(1f).testTag("colour-input"), placeholder = { Text("2a4d8f") }, singleLine = true)
                Button({ attempt { app.useAppearance(app.morpheus.setCustomColour(colour)) } }, Modifier.testTag("colour-apply"), enabled = colour.isNotBlank()) { Text(t("Use")) }
            }
            val clipboard = LocalClipboardManager.current
            val code = remember(look) { app.morpheus.themeCode() }
            Text(t("Theme code"), color = Palette.Muted, fontSize = 13.sp)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(code, Modifier.weight(1f).testTag("theme-code"), fontSize = 13.sp)
                TextButton({ clipboard.setText(AnnotatedString(code)) }, Modifier.testTag("code-copy")) { Text(t("Copy")) }
            }
            var pasted by remember { mutableStateOf("") }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(pasted, { pasted = it }, Modifier.weight(1f).testTag("code-input"), placeholder = { Text(t("Paste a theme code.")) }, singleLine = true)
                Button({ attempt { app.useAppearance(app.morpheus.applyThemeCode(pasted)); pasted = "" } }, Modifier.testTag("code-apply"), enabled = pasted.isNotBlank()) { Text(t("Use")) }
            }

            Heading(t("Time format"))
            var clock24 by remember { mutableStateOf(app.morpheus.clock24Hour()) }
            Choice(t("24-hour time"), clock24, "clock-24-hour") {
                attempt { app.morpheus.setClock24Hour(!clock24); clock24 = app.morpheus.clock24Hour(); dev.morpheus.app.ui.Words.changed() }
            }

            Heading(t("Language"))
            for (choice in app.morpheus.languages()) {
                Choice(choice.name, app.morpheus.languageChoice() == choice.code, "language-${choice.code}") {
                    attempt { app.useLanguage(choice.code) }
                }
            }
            }

            if (page == "Notifications") {
            Heading(t("Notifications"))
            var defaults by remember { mutableStateOf<NotifyDefaults?>(null) }
            LaunchedEffect(Unit) { try { defaults = app.morpheus.notifyDefaults() } catch (_: Exception) {} }
            val scope = androidx.compose.runtime.rememberCoroutineScope()
            val shown = defaults
            if (shown != null) {
                for ((dm, label, current) in listOf(Triple(true, "Direct messages", shown.dms), Triple(false, "Group rooms", shown.rooms))) {
                    Text(t(label), color = Palette.Muted, fontSize = 13.sp)
                    for ((mode, words) in listOf("all" to "All messages", "mentions" to "Only mentions")) {
                        Choice(t(words), current == mode, "notify-${if (dm) "dms" else "rooms"}-$mode") {
                            scope.launch {
                                try {
                                    app.morpheus.setNotifyDefault(dm, mode)
                                    defaults = app.morpheus.notifyDefaults()
                                } catch (e: Exception) {
                                    problem = e.sentence()
                                }
                            }
                        }
                    }
                }
            }

            Heading(t("Link previews"))
            var previews by remember { mutableStateOf(app.morpheus.previewSettings()) }
            Choice(t("Show a card under messages with a link"), previews.enabled, "previews-enabled") {
                attempt { app.morpheus.setPreviewSettings(!previews.enabled, previews.inEncrypted); previews = app.morpheus.previewSettings() }
            }
            Choice(t("Also in encrypted rooms (the homeserver is told the link)"), previews.inEncrypted, "previews-encrypted") {
                attempt { app.morpheus.setPreviewSettings(previews.enabled, !previews.inEncrypted); previews = app.morpheus.previewSettings() }
            }
            }

            if (page == "Bridges") {
            Heading(t("Bridges"))
            Button({ app.openBridges() }, Modifier.testTag("settings-bridges")) { Text(t("Connect a network")) }
            }

            if (page == "Account") {
            Heading(t("Account"))
            Text(account?.userId.orEmpty(), Modifier.testTag("settings-account"), color = Palette.Muted)
            Button({ app.signOut() }, Modifier.testTag("settings-sign-out")) { Text(t("Sign out")) }
            }
            problem?.let { Text(it, Modifier.testTag("settings-error"), color = Palette.Danger, fontSize = 14.sp) }
        }
    }
}

private fun argb(v: UInt) = Color(0xFF000000.toInt() or v.toInt())

@Composable
private fun Heading(text: String) {
    Text(text.uppercase(), Modifier.padding(top = Space.lg), color = Palette.Muted, style = Type.label)
}

@Composable
private fun Choice(label: String, chosen: Boolean, tag: String, onPick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Size.target).clip(RoundedCornerShape(Radius.md)).clickable { onPick() }.padding(vertical = Space.sm).testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.md),
    ) {
        Text(if (chosen) "●" else "○", Modifier.width(24.dp), color = if (chosen) Palette.Accent else Palette.Muted, style = Type.heading)
        Text(label, style = Type.body)
    }
}

/** What the phone has under each of the desktop's groups. */
private val supported = listOf("Account", "Notifications", "Appearance", "Bridges")

/** The groups, each opening its page, with the search bar at the bottom (the desktop's index, through the core). */
@Composable
private fun SettingsGroups(app: AppModel, t: (String) -> String, modifier: Modifier) {
    var query by remember { mutableStateOf("") }
    val hits = remember(query) { if (query.isBlank()) emptyList() else uniffi.morpheus_ffi.searchSettings(query).filter { it.group in supported } }
    val groups = remember { uniffi.morpheus_ffi.settingsGroups().filter { it.key in supported } }
    Column(modifier.fillMaxWidth().testTag("settings-groups")) {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            if (query.isNotBlank()) {
                if (hits.isEmpty()) Text(t("No settings match."), Modifier.padding(Space.lg).testTag("settings-no-hits"), color = Palette.Muted)
                for (hit in hits) {
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = Size.row).clickable { app.openSettingsPage(hit.group, hit.anchor) }.padding(horizontal = Space.lg).testTag("settings-hit-${hit.anchor}"),
                        verticalAlignment = Alignment.CenterVertically,
                    ) { Text(t(hit.label), Modifier.weight(1f), style = Type.body); Text(t(groups.firstOrNull { it.key == hit.group }?.label ?: hit.group), color = Palette.Muted, style = Type.secondary) }
                }
            } else {
                for (group in groups) {
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = Size.row).clickable { app.openSettingsPage(group.key) }.padding(horizontal = Space.lg).testTag("settings-group-${group.key.lowercase()}"),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.lg),
                    ) {
                        val icon = glyphDrawable(group.glyph)
                        if (icon != null) GlyphIcon(icon, tint = Palette.Muted)
                        Text(t(group.label), Modifier.weight(1f), style = Type.body)
                        GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_right, tint = Palette.Muted)
                    }
                }
            }
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(Palette.Divider))
        androidx.compose.material3.OutlinedTextField(
            query, { query = it }, Modifier.fillMaxWidth().padding(Space.md).testTag("settings-search"),
            placeholder = { Text(t("Search settings")) }, singleLine = true,
            leadingIcon = { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_search, tint = Palette.Muted) },
        )
    }
}

/** The generated drawable for a desktop icon name (`user`, `bell`), if there is one. */
@Composable
private fun glyphDrawable(name: String): Int? {
    val context = androidx.compose.ui.platform.LocalContext.current
    val id = context.resources.getIdentifier("ic_lucide_" + name.replace('-', '_'), "drawable", context.packageName)
    return id.takeIf { it != 0 }
}
