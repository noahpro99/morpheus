package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import dev.morpheus.app.AppModel
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.EmojiPackInfo
import uniffi.morpheus_ffi.PermissionsInfo

/** Edit room > Permissions (#422): for sending, reacting, pinning and calling, who may: everyone, moderators and admins, or admins only. A change is kept at once. */
@Composable
fun PermissionsSection(app: AppModel, roomId: String) {
    var info by remember { mutableStateOf<PermissionsInfo?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(roomId) { try { info = app.morpheus.roomPermissions(roomId) } catch (_: Exception) {} }
    val shown = info ?: return
    SectionHeader(t("Permissions"), Modifier.testTag("perm-section"))
    shown.note?.let { Text(it, Modifier.testTag("perm-note"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium) }
    for (row in shown.rows) {
        Text(row.label, style = MaterialTheme.typography.titleSmall)
        for (choice in row.choices) {
            ChoiceRow(choice.label, choice.selected, "perm-${row.id}-${choice.power}") {
                if (!shown.canChange) return@ChoiceRow
                scope.launch {
                    try { info = app.morpheus.setRoomPermission(roomId, row.id, choice.power); error = null } catch (e: Exception) { error = e.sentence(); info = try { app.morpheus.roomPermissions(roomId) } catch (_: Exception) { info } }
                }
            }
        }
    }
    Note(null, error)
}

/** Edit room > custom emoji (#423): the room's (or server's) pack: add a picture under a name, rename, remove; only where the room's levels allow. */
@Composable
fun EmojiSection(app: AppModel, roomId: String) {
    var pack by remember { mutableStateOf<EmojiPackInfo?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var name by remember { mutableStateOf("") }
    var renaming by remember { mutableStateOf<String?>(null) }
    var renamed by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    fun run(done: String, call: suspend () -> Unit) {
        scope.launch {
            try { call(); ok = done; error = null; reload++ } catch (e: Exception) { error = e.sentence(); ok = null }
        }
    }
    val picker = rememberPicturePicker { square -> run(t("Emoji added.")) { app.morpheus.addRoomEmoji(roomId, name, "emoji.png", square); name = "" } }
    LaunchedEffect(roomId, reload) { try { pack = app.morpheus.roomEmojiPack(roomId) } catch (_: Exception) {} }
    val shown = pack ?: return
    SectionHeader(t("Custom emoji"), Modifier.testTag("emoji-section"))
    if (!shown.editable) Text(t("Only people who may change this room's settings can change its emoji."), Modifier.testTag("emoji-note"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
    for (emoji in shown.emotes) {
        Row(Modifier.fillMaxWidth().testTag("emoji-row-${emoji.shortcode}"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
            Avatar(emoji.media, emoji.shortcode, Size.avatarSm)
            Text(":${emoji.shortcode}:", Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
            if (shown.editable) {
                TextButton({ renaming = emoji.shortcode; renamed = emoji.shortcode }, Modifier.testTag("emoji-rename-${emoji.shortcode}")) { Text(t("Rename")) }
                TextButton({ run(t("Emoji removed.")) { app.morpheus.removeRoomEmoji(roomId, emoji.shortcode) } }, Modifier.testTag("emoji-remove-${emoji.shortcode}")) { Text(t("Remove")) }
            }
        }
    }
    renaming?.let { old ->
        OutlinedTextField(renamed, { renamed = it }, Modifier.fillMaxWidth().testTag("emoji-rename-input"), singleLine = true, label = { Text(t("New name")) })
        Row(horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
            Button({ run(t("Emoji renamed.")) { app.morpheus.renameRoomEmoji(roomId, old, renamed); renaming = null } }, Modifier.testTag("emoji-rename-save")) { Text(t("Save")) }
            OutlinedButton({ renaming = null }) { Text(t("Cancel")) }
        }
    }
    if (shown.editable) {
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("emoji-name"), singleLine = true, label = { Text(t("Name for the new emoji")) })
        OutlinedButton({ picker.pick() }, Modifier.testTag("emoji-add")) { Text(t("Add emoji…")) }
        picker.Show()
    }
    Note(ok, error)
}
