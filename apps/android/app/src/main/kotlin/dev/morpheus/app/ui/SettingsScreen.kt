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
import androidx.compose.foundation.BorderStroke
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedCard
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

    // What went wrong is said in a toast.
    LaunchedEffect(problem) { problem?.let { Toasts.say(it, "settings-error", long = true) } }
    ScreenScaffold("settings-screen", { TopBar(t("Settings"), backTag = "settings-back", onBack = { app.closeSettings() }, titleTag = "settings-title") }) { inner ->
        if (page == null) SettingsGroups(app, t, Modifier.padding(inner)) else Column(
            Modifier.fillMaxSize().padding(inner).verticalScroll(rememberScrollState()).padding(vertical = Space.lg),
            verticalArrangement = Arrangement.spacedBy(Space.md),
        ) {
            if (page == "Appearance") {
            Heading(t("Appearance"))
            Row(Modifier.padding(horizontal = Space.lg).testTag("setting-theme"), horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                for (choice in app.morpheus.themes()) {
                    val chosen = look.theme == choice.key && look.colour == null
                    OutlinedCard(
                        { attempt { app.useAppearance(app.morpheus.setTheme(choice.key)) } },
                        Modifier.testTag("theme-${choice.key}"),
                        border = BorderStroke(2.dp, if (chosen) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant),
                    ) { Column(Modifier.padding(8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Row(Modifier.size(width = 64.dp, height = 36.dp)) {
                            Box(Modifier.weight(1f).fillMaxSize().background(argb(choice.sidebar)))
                            Box(Modifier.weight(2f).fillMaxSize().background(argb(choice.chat)))
                        }
                        Text(t(choice.label), style = MaterialTheme.typography.bodySmall)
                    } }
                }
            }
            if (MaterialYou.offered()) {
                val context = androidx.compose.ui.platform.LocalContext.current
                ToggleRow(t("Material You"), MaterialYou.enabled, "theme-material-you", supporting = t("Use the colours of your wallpaper (Android 12 and up)")) { MaterialYou.set(context, it) }
            }
            var colour by remember { mutableStateOf("") }
            Text(
                if (look.colour != null) "${t("Custom colour")} #${look.colour}" else t("Custom colour"),
                Modifier.padding(horizontal = Space.lg).testTag("custom-colour-label"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium,
            )
            Row(Modifier.padding(horizontal = Space.lg), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                OutlinedTextField(colour, { colour = it }, Modifier.weight(1f).testTag("colour-input"), placeholder = { Text("2a4d8f") }, singleLine = true)
                Button({ attempt { app.useAppearance(app.morpheus.setCustomColour(colour)) } }, Modifier.testTag("colour-apply"), enabled = colour.isNotBlank()) { Text(t("Use")) }
            }
            val clipboard = LocalClipboardManager.current
            val code = remember(look) { app.morpheus.themeCode() }
            Text(t("Theme code"), Modifier.padding(horizontal = Space.lg), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            Row(Modifier.padding(horizontal = Space.lg), verticalAlignment = Alignment.CenterVertically) {
                Text(code, Modifier.weight(1f).testTag("theme-code"), style = MaterialTheme.typography.bodyMedium)
                TextButton({ clipboard.setText(AnnotatedString(code)) }, Modifier.testTag("code-copy")) { Text(t("Copy")) }
            }
            // A message that carried a theme code opened this page with the code ready to use.
            val offered by app.themeCodeToTry.collectAsState()
            var pasted by remember { mutableStateOf(offered ?: "") }
            LaunchedEffect(offered) { offered?.let { pasted = it; app.themeCodeTried() } }
            Row(Modifier.padding(horizontal = Space.lg), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                OutlinedTextField(pasted, { pasted = it }, Modifier.weight(1f).testTag("code-input"), placeholder = { Text(t("Paste a theme code.")) }, singleLine = true)
                Button({ attempt { app.useAppearance(app.morpheus.applyThemeCode(pasted)); pasted = "" } }, Modifier.testTag("code-apply"), enabled = pasted.isNotBlank()) { Text(t("Use")) }
            }

            Heading(t("Time format"))
            var clock24 by remember { mutableStateOf(app.morpheus.clock24Hour()) }
            ToggleRow(t("24-hour time"), clock24, "clock-24-hour") { _ ->
                attempt { app.morpheus.setClock24Hour(!clock24); clock24 = app.morpheus.clock24Hour(); dev.morpheus.app.ui.Words.changed() }
            }

            Heading(t("Language"))
            for (choice in app.morpheus.languages()) {
                ChoiceRow(choice.name, app.morpheus.languageChoice() == choice.code, "language-${choice.code}") {
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
                    Text(t(label), Modifier.padding(horizontal = Space.lg), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
                    for ((mode, words) in listOf("all" to "All messages", "mentions" to "Only mentions")) {
                        ChoiceRow(t(words), current == mode, "notify-${if (dm) "dms" else "rooms"}-$mode") {
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
            ToggleRow(t("Show a card under messages with a link"), previews.enabled, "previews-enabled") { _ ->
                attempt { app.morpheus.setPreviewSettings(!previews.enabled, previews.inEncrypted); previews = app.morpheus.previewSettings() }
            }
            ToggleRow(t("Also in encrypted rooms (the homeserver is told the link)"), previews.inEncrypted, "previews-encrypted") { _ ->
                attempt { app.morpheus.setPreviewSettings(previews.enabled, !previews.inEncrypted); previews = app.morpheus.previewSettings() }
            }
            }

            if (page == "Bridges") {
            Heading(t("Bridges"))
            Button({ app.openBridges() }, Modifier.padding(horizontal = Space.lg).testTag("settings-bridges")) { Text(t("Connect a network")) }
            }

            if (page == "Account") {
            Heading(t("Account"))
            Text(account?.userId.orEmpty(), Modifier.padding(horizontal = Space.lg).testTag("settings-account"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
            Button({ app.signOut() }, Modifier.padding(horizontal = Space.lg).testTag("settings-sign-out")) { Text(t("Sign out")) }
            }
        }
    }
}

private fun argb(v: UInt) = Color(0xFF000000.toInt() or v.toInt())

@Composable
private fun Heading(text: String) {
    SmallHeading(text, Modifier.padding(start = Space.lg, end = Space.lg, top = Space.lg))
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
                if (hits.isEmpty()) Text(t("No settings match."), Modifier.padding(Space.lg).testTag("settings-no-hits"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
                for (hit in hits) {
                    ListRow(t(hit.label), "settings-hit-${hit.anchor}", trailing = { Text(t(groups.firstOrNull { it.key == hit.group }?.label ?: hit.group), style = MaterialTheme.typography.bodyMedium) }) { app.openSettingsPage(hit.group, hit.anchor) }
                }
            } else {
                for (group in groups) {
                    val icon = glyphDrawable(group.glyph)
                    ListRow(
                        t(group.label), "settings-group-${group.key.lowercase()}",
                        leading = icon?.let { { GlyphIcon(it) } },
                        trailing = { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_right) },
                    ) { app.openSettingsPage(group.key) }
                }
            }
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(Palette.Divider))
        SearchField(query, { query = it }, t("Search settings"), "settings-search", Modifier.padding(Space.md))
    }
}

/** The generated drawable for a desktop icon name (`user`, `bell`), if there is one. */
@Composable
private fun glyphDrawable(name: String): Int? {
    val context = androidx.compose.ui.platform.LocalContext.current
    val id = context.resources.getIdentifier("ic_lucide_" + name.replace('-', '_'), "drawable", context.packageName)
    return id.takeIf { it != 0 }
}
