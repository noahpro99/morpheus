package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
        Row(Modifier.fillMaxWidth().background(Palette.Background), verticalAlignment = Alignment.CenterVertically) {
            TextButton({ app.closeSettings() }, Modifier.testTag("settings-back")) { Text("‹", fontSize = 28.sp) }
            Text(t("Settings"), Modifier.testTag("settings-title"), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        }
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Heading(t("Appearance"))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
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

            Heading(t("Language"))
            for (choice in app.morpheus.languages()) {
                Choice(choice.name, app.morpheus.languageChoice() == choice.code, "language-${choice.code}") {
                    attempt { app.useLanguage(choice.code) }
                }
            }

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

            Heading(t("Account"))
            Text(account?.userId.orEmpty(), Modifier.testTag("settings-account"), color = Palette.Muted)
            Button({ app.signOut() }, Modifier.testTag("settings-sign-out")) { Text(t("Sign out")) }
            problem?.let { Text(it, Modifier.testTag("settings-error"), color = Palette.Danger, fontSize = 14.sp) }
        }
    }
}

private fun argb(v: UInt) = Color(0xFF000000.toInt() or v.toInt())

@Composable
private fun Heading(text: String) {
    Text(text.uppercase(), Modifier.padding(top = 12.dp), color = Palette.Muted, fontSize = 12.sp, fontWeight = FontWeight.Bold)
}

@Composable
private fun Choice(label: String, chosen: Boolean, tag: String, onPick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onPick() }.padding(vertical = 10.dp).testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(if (chosen) "●" else "○", color = if (chosen) Palette.Accent else Palette.Muted)
        Text(label)
    }
}
