package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.SecurityModel
import uniffi.morpheus_ffi.ButtonKind
import uniffi.morpheus_ffi.PanelTone
import uniffi.morpheus_ffi.RecoveryKind
import uniffi.morpheus_ffi.VerificationPanel

/** Encryption on this device: verify it with another device, or unlock the key backup with the recovery key. */
@Composable
fun SecurityScreen(app: AppModel, model: SecurityModel) {
    BackHandler { app.closeSecurity() }
    val status by model.status.collectAsState()
    val panel by model.panel.collectAsState()
    val flow by model.flow.collectAsState()
    val error by model.error.collectAsState()
    val busy by model.busy.collectAsState()
    val newKey by model.newKey.collectAsState()
    var key by rememberSaveable { mutableStateOf("") }

    ScreenScaffold("security", { TopBar(t("Encryption"), backTag = "back", onBack = { app.closeSecurity() }) }) { inner ->
        Column(Modifier.padding(inner).verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            val current = status
            if (flow != null) {
                panel?.let { Panel(it) { action -> model.act(action) } }
                    ?: Text(t("Starting…"), color = Palette.Muted, modifier = Modifier.testTag("verify-starting"))
            } else if (newKey != null) {
                Text(t("Your recovery key. Keep it somewhere safe: it is shown only once."))
                Text(newKey.orEmpty(), Modifier.testTag("new-recovery-key"), fontSize = 16.sp)
                Button({ model.keySaved() }, Modifier.testTag("key-saved")) { Text(t("I have saved it")) }
            } else if (current == null) {
                Text(t("Loading…"), color = Palette.Muted)
            } else {
                // The state of this device, as a Material 3 card with a shield.
                androidx.compose.material3.Card(
                    Modifier.fillMaxWidth(),
                    colors = androidx.compose.material3.CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh, contentColor = ComposerColours.text(MaterialTheme.colorScheme)),
                ) {
                    Row(Modifier.padding(Space.lg), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
                        GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_shield, tint = ComposerColours.hint(MaterialTheme.colorScheme))
                        Text(
                            if (current.deviceVerified) "This device is verified." else "This device is not verified: encrypted messages from before you signed in cannot be read yet.",
                            Modifier.testTag("security-state"),
                        )
                    }
                }
                if (current.recovery == RecoveryKind.NEEDS_KEY) {
                    Text(t("Enter your recovery key (or passphrase) to unlock your encrypted history."), color = Palette.Muted)
                    OutlinedTextField(
                        key, { key = it },
                        Modifier.fillMaxWidth().testTag("recovery-key"),
                        label = { Text(t("Recovery key")) },
                        singleLine = true,
                    )
                    Button({ model.recover(key) }, Modifier.testTag("recover"), enabled = !busy && key.isNotBlank()) {
                        Text(if (busy) "Unlocking…" else "Unlock")
                    }
                }
                if (current.otherDevices > 0u && !current.deviceVerified) {
                    Text(t("Or verify with another device of yours that is signed in."), color = Palette.Muted)
                    OutlinedButton({ model.startVerification() }, Modifier.testTag("verify-device")) { Text(t("Verify with another device")) }
                }
                if (current.recovery == RecoveryKind.DISABLED) {
                    Text(t("Recovery is not set up: if you lose every device, your encrypted history is gone."), color = Palette.Muted)
                    OutlinedButton({ model.setUpRecovery() }, Modifier.testTag("set-up-recovery"), enabled = !busy) { Text(t("Set up recovery")) }
                }
            }
            error?.let { Text(it, color = Palette.Danger, modifier = Modifier.testTag("security-error")) }
        }
    }
}

@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun Panel(panel: VerificationPanel, onAction: (uniffi.morpheus_ffi.VerificationAction) -> Unit) {
    Column(Modifier.testTag("verify-panel"), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            panel.text,
            Modifier.testTag("verify-text"),
            color = when (panel.tone) {
                PanelTone.ERROR -> Palette.Danger
                else -> Palette.Text
            },
        )
        if (panel.emojis.isNotEmpty()) {
            FlowRow(Modifier.testTag("verify-emoji"), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (emoji in panel.emojis) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.padding(4.dp)) {
                        Text(emoji.symbol, fontSize = 32.sp)
                        Text(emoji.name, fontSize = 11.sp, color = Palette.Muted)
                    }
                }
            }
        }
        panel.qr?.let { code ->
            val size = code.size.toInt()
            Canvas(Modifier.size(220.dp).background(Color.White).testTag("verify-qr")) {
                val cell = this.size.width / size
                for (i in code.modules.indices) {
                    if (code.modules[i]) drawRect(Color.Black, Offset((i % size) * cell, (i / size) * cell), Size(cell + 0.5f, cell + 0.5f))
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (button in panel.buttons) {
                val tag = Modifier.testTag("verify-" + button.action.name.lowercase().replace('_', '-'))
                when (button.kind) {
                    ButtonKind.PRIMARY -> Button({ onAction(button.action) }, tag) { Text(button.label) }
                    else -> OutlinedButton({ onAction(button.action) }, tag) { Text(button.label) }
                }
            }
        }
    }
}
