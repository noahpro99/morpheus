package dev.morpheus.app.ui

import android.graphics.BitmapFactory
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.BridgeBot
import uniffi.morpheus_ffi.BridgeListener
import uniffi.morpheus_ffi.BridgePhase
import uniffi.morpheus_ffi.BridgeSnapshot

/** The localpart of a bot's ID, for test tags (`@discordbot:server` -> `discordbot`). */
private fun local(bot: String) = bot.trimStart('@').substringBefore(':')

/**
 * Settings > Bridges (#107): connect Discord, WhatsApp, Telegram, Signal and the other
 * networks through the mautrix bridge bots of the server. The conversation with each bot runs
 * in Rust (`BridgeHandle`, the same login steps as the desktop's page); this draws what it
 * pushes: the status, the QR code (the bot's image), the question to answer.
 */
@Composable
fun BridgesScreen(app: AppModel) {
    BackHandler { app.closeBridges() }
    val language by app.language.collectAsState()
    var bots by remember { mutableStateOf<List<BridgeBot>?>(null) }
    var note by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    fun find() {
        scope.launch {
            try { bots = app.morpheus.bridgeBots() } catch (e: Exception) { note = e.sentence(); bots = bots ?: emptyList() }
        }
    }
    LaunchedEffect(Unit) { find() }

    Column(Modifier.fillMaxSize().background(Palette.Surface).testTag("bridges-screen")) {
        TopBar(t("Bridges"), backTag = "bridges-back", onBack = { app.closeBridges() })
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(
                t("Connect chats from other networks. Morpheus messages the bridge's bot for you and shows what to scan or type."),
                color = Palette.Muted, fontSize = 13.sp,
            )
            val shown = bots
            if (shown == null) {
                Text(t("Looking for bridges..."), Modifier.testTag("bridges-looking"), color = Palette.Muted)
            } else if (shown.isEmpty()) {
                Text(
                    t("No bridge bots were found on your server. If your bridge's bot has another name, add it below."),
                    Modifier.testTag("bridges-none"), color = Palette.Muted,
                )
            }
            for (bot in shown.orEmpty()) BridgeRow(app, bot, language)

            Text(t("Another bridge bot").uppercase(), Modifier.padding(top = 12.dp), color = Palette.Muted, fontSize = 12.sp, fontWeight = FontWeight.Bold)
            Text(t("Bridges are run by your server's administrator. If the bot of yours has another name, type it here."), color = Palette.Muted, fontSize = 13.sp)
            var typed by remember { mutableStateOf("") }
            fun add() {
                val name = typed
                if (name.isBlank()) return
                scope.launch {
                    try {
                        app.morpheus.addBridgeBot(name)
                        typed = ""
                        note = null
                        find()
                    } catch (e: Exception) {
                        note = e.sentence()
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    typed, { typed = it }, Modifier.weight(1f).testTag("bridge-bot-input"),
                    placeholder = { Text(t("Bot name, e.g. discordbot or @bot:example.org")) }, singleLine = true,
                )
                Button({ add() }, Modifier.testTag("bridge-bot-add"), enabled = typed.isNotBlank()) { Text(t("Add")) }
            }
            note?.let { Text(it, Modifier.testTag("bridges-note"), color = Palette.Danger, fontSize = 14.sp) }
        }
    }
}

@Composable
private fun BridgeRow(app: AppModel, bot: BridgeBot, language: String) {
    val id = local(bot.bot)
    var snapshot by remember(bot.bot) { mutableStateOf(BridgeSnapshot(BridgePhase.UNKNOWN, "Not connected", null, null)) }
    val handle = remember(bot.bot) {
        app.morpheus.openBridge(bot.bot, object : BridgeListener {
            override fun onBridge(snapshot2: BridgeSnapshot) {
                snapshot = snapshot2
            }
        })
    }
    DisposableEffect(handle) { onDispose { handle.stop() } }
    val phase = snapshot.phase
    val connecting = phase == BridgePhase.STARTING || phase == BridgePhase.QR || phase == BridgePhase.PROMPT
    val status = t(snapshot.label) + if (phase == BridgePhase.LOGGED_IN && snapshot.detail != null) " " + t("as %{who}", "who" to snapshot.detail!!) else ""

    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(Palette.Background).padding(12.dp).testTag("bridge-row-$id"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(bot.name, fontWeight = FontWeight.SemiBold, modifier = Modifier.testTag("bridge-name-$id"))
                Text(bot.bot, color = Palette.Muted, fontSize = 12.sp)
            }
            Text(
                status, Modifier.testTag("bridge-status-$id"),
                color = when (phase) {
                    BridgePhase.LOGGED_IN -> Palette.Accent
                    BridgePhase.ERROR, BridgePhase.NEEDS_LOGIN -> Palette.Danger
                    else -> Palette.Muted
                },
            )
        }
        if (connecting) Text(t(bot.steps), color = Palette.Muted, fontSize = 13.sp, modifier = Modifier.testTag("bridge-steps-$id"))
        val picture = remember(snapshot.qr) {
            snapshot.qr?.let { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }
        }
        if (phase == BridgePhase.QR && picture != null) {
            Image(picture, null, Modifier.size(220.dp).testTag("bridge-qr-$id"))
        }
        if (phase == BridgePhase.PROMPT) {
            Text(snapshot.detail.orEmpty(), Modifier.testTag("bridge-question-$id"))
            var answer by remember(snapshot.detail) { mutableStateOf("") }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    answer, { answer = it }, Modifier.weight(1f).testTag("bridge-answer-$id"),
                    placeholder = { Text(t("Type your answer")) }, singleLine = true,
                )
                Button({ handle.answer(answer); answer = "" }, Modifier.testTag("bridge-send-$id"), enabled = answer.isNotBlank()) { Text(t("Send")) }
            }
        }
        if ((phase == BridgePhase.ERROR || phase == BridgePhase.NEEDS_LOGIN) && snapshot.detail != null) {
            Text(snapshot.detail.orEmpty(), Modifier.testTag("bridge-error-$id"), color = Palette.Danger, fontSize = 13.sp)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (phase == BridgePhase.LOGGED_IN) {
                Button({ handle.logOut() }, Modifier.testTag("bridge-logout-$id")) { Text(t("Log out")) }
            } else if (connecting) {
                Button({ handle.cancel() }, Modifier.testTag("bridge-cancel-$id")) { Text(t("Cancel")) }
            } else {
                Button({ handle.connect() }, Modifier.testTag("bridge-connect-$id")) {
                    Text(t(if (phase == BridgePhase.NEEDS_LOGIN) "Log in again" else "Connect"))
                }
            }
            if (!connecting) TextButton({ handle.check() }, Modifier.testTag("bridge-check-$id")) { Text(t("Check status")) }
        }
    }
}
