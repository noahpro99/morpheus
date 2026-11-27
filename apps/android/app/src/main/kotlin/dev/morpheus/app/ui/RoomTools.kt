package dev.morpheus.app.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import dev.morpheus.app.AppModel
import dev.morpheus.app.RoomHooks
import dev.morpheus.app.read
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.EmojiPackInfo
import uniffi.morpheus_ffi.PermissionsInfo

/** The Edit room page's Permissions section (#422): who may send, react, pin and start calls; a choice is written to the room at once. */
@Composable
fun PermissionsSection(app: AppModel, roomId: String, onOk: (String) -> Unit, onError: (String) -> Unit) {
    var info by remember { mutableStateOf<PermissionsInfo?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(reload) { try { info = app.morpheus.roomPermissions(roomId) } catch (e: Exception) { onError(e.sentence()) } }
    val shown = info ?: return
    Column(Modifier.testTag("perm-section")) {
        SectionHeader(t("Permissions"))
        if (!shown.canChange) {
            Text(shown.note, Modifier.padding(horizontal = Space.lg).testTag("perm-note"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
            return@Column
        }
        for (row in shown.rows) {
            Text(row.label, Modifier.padding(horizontal = Space.lg, vertical = Space.xs), style = MaterialTheme.typography.titleSmall)
            for (option in row.options) {
                val chosen = option.power == row.level
                ChoiceRow(
                    option.label, chosen, "perm-${row.action}-${option.power}",
                    Modifier.semantics { stateDescription = if (chosen) "selected" else "" },
                ) {
                    scope.launch {
                        try { app.morpheus.setRoomPermission(roomId, row.action, option.power); reload++ } catch (e: Exception) { onError(e.sentence()) }
                    }
                }
            }
        }
    }
}

/** The Edit room page's Emoji section (#423): the room's (or server's) own emoji, and for those who may change them adding, renaming and removing. */
@Composable
fun EmojiPackSection(app: AppModel, roomId: String, onOk: (String) -> Unit, onError: (String) -> Unit) {
    var pack by remember { mutableStateOf<EmojiPackInfo?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var typed by remember { mutableStateOf("") }
    var renaming by remember { mutableStateOf<String?>(null) }
    var newName by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    LaunchedEffect(reload) { try { pack = app.morpheus.roomEmojiPack(roomId) } catch (e: Exception) { onError(e.sentence()) } }
    val add: (String, ByteArray) -> Unit = { file, bytes ->
        scope.launch {
            try { app.morpheus.addRoomEmoji(roomId, typed, file, bytes); typed = ""; reload++ } catch (e: Exception) { onError(e.sentence()) }
        }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch { context.read(uri)?.let { add(it.name, it.bytes) } }
    }
    DisposableEffect(typed) {
        RoomHooks.emoji = { file, bytes -> add(file, bytes) }
        onDispose { RoomHooks.emoji = null }
    }
    val shown = pack ?: return
    Column(Modifier.testTag("emoji-section")) {
        SectionHeader(t("Emoji"))
        if (shown.emotes.isEmpty()) Text(t("No custom emoji yet."), Modifier.padding(horizontal = Space.lg).testTag("emoji-empty"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        for (emote in shown.emotes) {
            if (renaming == emote.shortcode) {
                Row(Modifier.fillMaxWidth().padding(horizontal = Space.lg), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                    CustomEmojiImage(emote.media, 32)
                    OutlinedTextField(newName, { newName = it }, Modifier.weight(1f).testTag("emoji-rename-input"), singleLine = true)
                    TextButton({
                        scope.launch {
                            try { app.morpheus.renameRoomEmoji(roomId, emote.shortcode, newName); renaming = null; reload++ } catch (e: Exception) { onError(e.sentence()) }
                        }
                    }, Modifier.testTag("emoji-rename-save")) { Text(t("Save")) }
                }
            } else ListRow(
                ":${emote.shortcode}:", "emoji-item-${emote.shortcode}",
                leading = { CustomEmojiImage(emote.media, 32) },
                trailing = if (shown.editable) ({
                    Row {
                        IconAction(dev.morpheus.app.R.drawable.ic_lucide_pencil, t("Rename %{name}", "name" to emote.shortcode), "emoji-rename-${emote.shortcode}", { renaming = emote.shortcode; newName = emote.shortcode })
                        IconAction(dev.morpheus.app.R.drawable.ic_lucide_x, t("Remove %{name}", "name" to emote.shortcode), "emoji-remove-${emote.shortcode}", {
                            scope.launch { try { app.morpheus.removeRoomEmoji(roomId, emote.shortcode); reload++ } catch (e: Exception) { onError(e.sentence()) } }
                        })
                    }
                }) else null,
            )
        }
        if (shown.editable) {
            OutlinedTextField(typed, { typed = it }, Modifier.fillMaxWidth().padding(horizontal = Space.lg).testTag("emoji-name"), singleLine = true, label = { Text(t("Name")) }, placeholder = { Text("wave") })
            TextButton({ picker.launch("image/*") }, Modifier.padding(horizontal = Space.sm).testTag("emoji-add")) { Text(t("Add emoji…")) }
        } else Text(t("Only people who may change this room can add emoji."), Modifier.padding(horizontal = Space.lg), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
    }
}
