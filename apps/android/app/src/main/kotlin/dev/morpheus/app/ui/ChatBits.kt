package dev.morpheus.app.ui

import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import uniffi.morpheus_ffi.ShieldItem
import uniffi.morpheus_ffi.StateLook
import uniffi.morpheus_ffi.StateTone

/** A room event's line (#410): the icon of its kind in its tone (green a join or a call, red a leave) and the words; any other line is quiet text. */
@Composable
fun StateOrText(summary: String, state: StateLook?, avatar: @Composable () -> Unit = {}) {
    if (state == null) {
        Text(summary, Modifier.testTag("msg-body"), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
        return
    }
    val drawable = when (state.glyph) {
        "phone" -> if (state.kind == "call-ended") dev.morpheus.app.R.drawable.ic_ms_call_end else dev.morpheus.app.R.drawable.ic_ms_call
        "log-in" -> dev.morpheus.app.R.drawable.ic_ms_login
        "log-out" -> dev.morpheus.app.R.drawable.ic_ms_logout
        else -> dev.morpheus.app.R.drawable.ic_ms_info
    }
    val tone = when (state.tone) {
        StateTone.GOOD -> Palette.Green
        StateTone.BAD -> Palette.Danger
        StateTone.QUIET -> Palette.Muted
    }
    val name = when (state.tone) { StateTone.GOOD -> "good"; StateTone.BAD -> "bad"; StateTone.QUIET -> "quiet" }
    Row(horizontalArrangement = Arrangement.spacedBy(Space.sm), verticalAlignment = Alignment.CenterVertically) {
        GlyphIcon(drawable, Modifier.semantics { stateDescription = "${state.kind} $name" }.testTag("state-icon"), size = 20.dp, tint = tone)
        avatar()
        Text(summary, Modifier.testTag("msg-body"), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
    }
}

/** The shield on a message from a device nobody verified (#409): grey for a weak reason, red for a real problem; a tap says why. */
@Composable
fun MessageShield(shield: ShieldItem, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val reason = reasonWords(shield.reason)
    GlyphIcon(
        dev.morpheus.app.R.drawable.ic_ms_shield,
        modifier
            .padding(top = 2.dp)
            .pointerInput(reason) { detectTapGestures { android.widget.Toast.makeText(context, reason, android.widget.Toast.LENGTH_LONG).show() } }
            .semantics { contentDescription = reason; stateDescription = if (shield.red) "red" else "grey" }
            .testTag("msg-shield"),
        size = 16.dp,
        tint = if (shield.red) Palette.Danger else Palette.Muted,
    )
}

/** The core's reason for a shield (its English words) in the language in use. */
private fun reasonWords(reason: String): String = when (reason) {
    "The authenticity of this encrypted message can't be guaranteed on this device." -> t("The authenticity of this encrypted message can't be guaranteed on this device.")
    "Encrypted by an unknown or deleted device." -> t("Encrypted by an unknown or deleted device.")
    "Encrypted by a device not verified by its owner." -> t("Encrypted by a device not verified by its owner.")
    "Encrypted by a user you haven't verified." -> t("Encrypted by a user you haven't verified.")
    "The sender's identity has changed since you verified them." -> t("The sender's identity has changed since you verified them.")
    "The sender doesn't match the device that encrypted this message." -> t("The sender doesn't match the device that encrypted this message.")
    "Not encrypted." -> t("Not encrypted.")
    else -> reason
}
