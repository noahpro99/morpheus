package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.DeviceItem
import uniffi.morpheus_ffi.RecoveryKind
import uniffi.morpheus_ffi.SecurityStatus

private enum class Asking { NONE, CHANGE, RESET }

/**
 * Settings > Security & Privacy (the desktop's page, #444): this device's encryption and the recovery key (set up, change, reset),
 * the account's devices, and how long an unlocked chat stays open. Verifying and unlocking with a key stay on the Encryption screen.
 */
@Composable
fun SecurityPage(app: AppModel, t: (String) -> String) {
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf<SecurityStatus?>(null) }
    var devices by remember { mutableStateOf<List<DeviceItem>?>(null) }
    var newKey by remember { mutableStateOf<String?>(null) }
    var asking by remember { mutableStateOf(Asking.NONE) }
    var password by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var reload by remember { mutableIntStateOf(0) }
    var choices by remember { mutableStateOf(app.morpheus.lockTimeoutChoices()) }
    var customOpen by remember { mutableStateOf(false) }
    var number by remember { mutableStateOf("") }
    var unit by remember { mutableStateOf("minutes") }
    fun problem(e: Exception) = Toasts.say(e.sentence(), "settings-error", long = true)

    // The state of this device and its backup changes by itself (verification and the first key backup finish after sign-in): look again now and then.
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(4000)
            try { status = app.morpheus.securityStatus() } catch (_: Exception) {}
        }
    }
    LaunchedEffect(reload) {
        try { status = app.morpheus.securityStatus() } catch (_: Exception) {}
        try { devices = app.morpheus.ownDeviceList() } catch (_: Exception) { devices = emptyList() }
    }
    fun makeKey(call: suspend () -> String) {
        if (busy) return
        busy = true
        scope.launch {
            try { newKey = call(); asking = Asking.NONE; password = ""; reload++ } catch (e: Exception) { problem(e) }
            busy = false
        }
    }

    SmallHeading(t("Encryption"), Modifier.padding(horizontal = Space.lg))
    val current = status
    if (current == null) {
        Text(t("Loading…"), Modifier.padding(horizontal = Space.lg), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
    } else {
        Card(
            Modifier.padding(horizontal = Space.lg).fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh, contentColor = ComposerColours.text(MaterialTheme.colorScheme)),
        ) {
            Row(Modifier.padding(Space.lg), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
                GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_shield, tint = ComposerColours.hint(MaterialTheme.colorScheme))
                Column {
                    Text(if (current.deviceVerified) t("This device is verified") else t("This device is not verified"), Modifier.testTag("settings-security-state"), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        when (current.recovery) {
                            RecoveryKind.ENABLED -> t("Key backup connected")
                            RecoveryKind.NEEDS_KEY -> t("Key backup found; enter your recovery key to read older messages")
                            RecoveryKind.DISABLED -> t("No key backup on this account")
                            RecoveryKind.UNKNOWN -> t("Checking key backup…")
                        },
                        Modifier.testTag("settings-security-backup"), style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
        }
        if (!current.deviceVerified || current.recovery == RecoveryKind.NEEDS_KEY) {
            ListRow(t("Verify or unlock this device"), "security-open-encryption", leading = { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_verified_user) }, trailing = { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_chevron_right) }) { app.openSecurity() }
        }
        val key = newKey
        if (key != null) {
            Column(Modifier.padding(horizontal = Space.lg), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
                Text(t("Your recovery key"), style = MaterialTheme.typography.titleMedium)
                Text(t("Save it somewhere safe, like a password manager. With it, any new device can read your encrypted history; without it, history is lost if you lose all your signed-in devices."), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
                Text(key, Modifier.testTag("new-recovery-key"), style = MaterialTheme.typography.bodyLarge, fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace)
                Button({ newKey = null }, Modifier.testTag("key-saved")) { Text(t("I have saved it")) }
            }
        } else when (current.recovery) {
            RecoveryKind.DISABLED -> Button({ makeKey { app.morpheus.enableRecovery() } }, Modifier.padding(horizontal = Space.lg).testTag("set-up-recovery"), enabled = !busy) { Text(t("Set up recovery")) }
            RecoveryKind.ENABLED -> Column(Modifier.padding(horizontal = Space.lg), verticalArrangement = Arrangement.spacedBy(Space.xs)) {
                OutlinedButton({ asking = Asking.CHANGE }, Modifier.testTag("change-key"), enabled = !busy) { Text(t("Change recovery key")) }
                Text(t("Lost your recovery key? Make a new one here; the old one stops working."), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
            RecoveryKind.NEEDS_KEY -> Column(Modifier.padding(horizontal = Space.lg), verticalArrangement = Arrangement.spacedBy(Space.xs)) {
                OutlinedButton({ asking = Asking.RESET }, Modifier.testTag("reset-key"), enabled = !busy) { Text(t("Reset recovery key")) }
                Text(t("Lost your recovery key and have no other signed-in device? Reset it here."), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
            RecoveryKind.UNKNOWN -> {}
        }
    }
    if (asking != Asking.NONE) {
        val reset = asking == Asking.RESET
        ConfirmDialog(
            title = if (reset) t("Reset recovery key") else t("Change recovery key"),
            text = if (reset) t("Your old key stops working, and your encryption identity is reset: your other devices must be verified again, and history that only the old backup held may be unreadable. Enter your account password to continue.")
                else t("Your old key stops working: devices you sign in to later need the new key to read your encrypted history. Make a new key?"),
            confirm = t("Create new key"), confirmTag = "confirm-new-key",
            onConfirm = { makeKey { if (reset) app.morpheus.resetRecoveryKey(password) else app.morpheus.changeRecoveryKey() } },
            onDismiss = { asking = Asking.NONE; password = "" },
            danger = reset,
            confirmEnabled = !busy && (!reset || password.isNotEmpty()),
            content = if (reset) { { OutlinedTextField(password, { password = it }, Modifier.fillMaxWidth().testTag("reset-password"), singleLine = true, label = { Text(t("Password")) }, visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation()) } } else null,
        )
    }

    SmallHeading(t("Devices"), Modifier.padding(horizontal = Space.lg))
    Column(Modifier.testTag("device-list")) {
        val list = devices
        if (list == null) SkeletonRows(count = 2, avatars = false) else for (device in list) {
            ListRow(
                device.name.ifBlank { device.id }, "device-row-${device.id}",
                supporting = device.id + " · " + if (device.verified) t("verified") else t("unverified"),
                leading = { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_call) },
                trailing = if (device.isCurrent) { { Text(t("this device"), Modifier.testTag("device-this"), style = MaterialTheme.typography.labelMedium) } } else null,
            )
        }
    }

    SmallHeading(t("Locked chats"), Modifier.padding(horizontal = Space.lg))
    Text(
        t("A chat locked with a password (its menu, Lock with a password) opens again with the password only. Unlocked, it locks itself when you have not been looking at it for:"),
        Modifier.padding(horizontal = Space.lg), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium,
    )
    Column(Modifier.testTag("lock-timeouts")) {
        for (choice in choices) {
            val custom = choice.key == "custom"
            ChoiceRow(choice.label, choice.selected || (custom && customOpen), "lock-timeout-${choice.key}") {
                if (custom) customOpen = true else try {
                    app.morpheus.setLockTimeout(choice.key); choices = app.morpheus.lockTimeoutChoices(); customOpen = false
                } catch (e: Exception) { problem(e) }
            }
        }
    }
    if (customOpen || choices.any { it.key == "custom" && it.selected }) {
        Column(Modifier.padding(horizontal = Space.lg).testTag("lock-custom"), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
            OutlinedTextField(number, { number = it }, Modifier.width(120.dp).testTag("lock-custom-number"), singleLine = true, label = { Text(t("Time")) })
            for (label in listOf("seconds", "minutes", "hours")) ChoiceRow(t(label), unit == label, "lock-unit-$label") { unit = label }
            Button({
                try { app.morpheus.setLockTimeoutCustom(number, unit); choices = app.morpheus.lockTimeoutChoices() } catch (e: Exception) { problem(e) }
            }, Modifier.testTag("lock-custom-apply"), enabled = number.isNotBlank()) { Text(t("Use")) }
        }
    }
}
