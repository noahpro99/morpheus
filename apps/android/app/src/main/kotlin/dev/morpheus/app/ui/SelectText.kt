package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.border
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.PageRequest

private fun copyToClipboard(context: android.content.Context, text: String) {
    val clipboard = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as? android.content.ClipboardManager
    clipboard?.setPrimaryClip(android.content.ClipData.newPlainText("", text))
}

/**
 * Select text (#417): the message's words in a field you can select part of with the system's handles; Copy takes the selected
 * part, or the whole message when nothing is selected.
 */
@Composable
fun SelectTextPage(@Suppress("UNUSED_PARAMETER") app: AppModel, page: PageRequest) {
    val context = LocalContext.current
    var value by remember { mutableStateOf(TextFieldValue(page.extra)) }
    ScreenScaffold("page-select-text", { TopBar(t("Select text")) }) { inner ->
        Column(Modifier.fillMaxSize().padding(inner).padding(Space.lg), verticalArrangement = Arrangement.spacedBy(Space.md)) {
            androidx.compose.material3.OutlinedTextField(
                value, { value = it.copy(text = page.extra) },
                Modifier.fillMaxWidth().testTag("select-text-field"),
                readOnly = true,
                textStyle = MaterialTheme.typography.bodyLarge,
            )
            Button({
                val range = value.selection
                copyToClipboard(context, if (range.collapsed) page.extra else page.extra.substring(range.min, range.max))
                Toasts.say(t("Copied"), "copied")
            }, Modifier.testTag("select-copy")) { Text(t("Copy")) }
        }
    }
}

/** The messages picked to copy together (#417). */
class MessageSelection {
    var active by mutableStateOf(false)
        private set
    val keys = mutableStateListOf<String>()

    fun start(key: String) { active = true; keys.clear(); keys.add(key) }

    fun toggle(key: String) {
        if (!keys.remove(key)) keys.add(key)
        if (keys.isEmpty()) active = false
    }

    fun clear() { active = false; keys.clear() }
}

/** The selection the open chat is making, for its rows to read and change. */
val LocalSelection = compositionLocalOf<MessageSelection?> { null }

/** A round mark at the end of a row while messages are being picked: filled with a check when it is one of them. */
@Composable
fun SelectMark(selected: Boolean) {
    androidx.compose.foundation.layout.Box(
        Modifier
            .size(24.dp)
            .then(if (selected) Modifier.background(Palette.Accent, CircleShape) else Modifier.border(2.dp, Palette.Muted, CircleShape))
            .semantics { stateDescription = if (selected) "selected" else "unselected" }
            .testTag("select-mark"),
        contentAlignment = Alignment.Center,
    ) {
        if (selected) GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_check, size = 16.dp, tint = Palette.OnAccent)
    }
}

/** What the chat shows while messages are picked: how many, Copy (their transcript) and done. */
@Composable
fun SelectionBar(selection: MessageSelection, messages: List<uniffi.morpheus_ffi.MessageItem>) {
    val context = LocalContext.current
    Row(
        Modifier.fillMaxWidth().background(Palette.Raised).padding(horizontal = Space.lg, vertical = Space.xs).testTag("selection-bar"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.sm),
    ) {
        Text(t("%{n} selected", "n" to selection.keys.size.toString()), Modifier.weight(1f).testTag("selection-count"), color = Palette.Text, style = MaterialTheme.typography.titleMedium)
        Button({
            val picked = messages.filter { it.key in selection.keys }
            copyToClipboard(context, uniffi.morpheus_ffi.transcriptOf(picked))
            Toasts.say(t("Copied"), "copied")
            selection.clear()
        }, Modifier.testTag("select-copy-messages")) { Text(t("Copy")) }
        androidx.compose.material3.TextButton({ selection.clear() }, Modifier.testTag("select-cancel")) { Text(t("Cancel")) }
    }
}
