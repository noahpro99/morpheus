package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import dev.morpheus.app.AppModel
import dev.morpheus.app.PageRequest
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch

/** A room a Matrix link named that I am not in (#454): its name, topic and size, with Join, or Ask to join when it takes knocks. */
@Composable
fun LinkPreviewPage(app: AppModel, @Suppress("UNUSED_PARAMETER") page: PageRequest) {
    val room by app.linkPreview.collectAsState()
    val target = room ?: return
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var asked by remember { mutableStateOf(false) }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    PageFrame(app, "Room", "page-link-preview") {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
            Avatar(target.avatar, target.name, Size.avatarLg)
            Column {
                Text(target.name, Modifier.testTag("link-name"), style = MaterialTheme.typography.titleLarge)
                Text(target.members, Modifier.testTag("link-members"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
            }
        }
        target.topic?.let { Text(it, Modifier.testTag("link-topic"), style = MaterialTheme.typography.bodyLarge) }
        Note(ok, error)
        if (target.knockable) {
            Button({
                busy = true
                scope.launch {
                    try { app.morpheus.knockLink(target.address, target.via); asked = true; ok = t("Asked to join.") } catch (e: Exception) { error = e.sentence() }
                    busy = false
                }
            }, Modifier.fillMaxWidth().testTag("link-knock"), enabled = !busy && !asked) { Text(t(if (asked) "Asked" else "Ask to join")) }
        } else {
            Button({
                busy = true
                scope.launch {
                    try {
                        app.morpheus.joinChannel(target.id, target.via)
                        app.closePage()
                        app.openRoomWhenKnown(target.id)
                    } catch (e: Exception) { error = e.sentence() }
                    busy = false
                }
            }, Modifier.fillMaxWidth().testTag("link-join"), enabled = !busy) { Text(t("Join")) }
        }
    }
}
