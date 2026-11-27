package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.PageRequest
import dev.morpheus.app.sentence
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.ChannelLayout

/** A server's channels and categories in the order everyone sees (#424): each moves up or down, a channel also into another category. `page.roomId` is the server. */
@Composable
fun ChannelOrderPage(app: AppModel, page: PageRequest) {
    var layout by remember { mutableStateOf<ChannelLayout?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var menuFor by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(reload) { try { layout = app.morpheus.channelLayout(page.roomId) } catch (e: Exception) { error = e.sentence() } }
    /** Do a change, then read the list until it shows it (the homeserver's echo comes with the next sync). */
    fun change(call: suspend () -> Unit) {
        scope.launch {
            try {
                val before = layout?.places?.map { it.id to it.container }
                call()
                for (round in 0 until 20) {
                    delay(500)
                    val now = app.morpheus.channelLayout(page.roomId)
                    if (now.places.map { it.id to it.container } != before) { layout = now; break }
                }
                error = null
            } catch (e: Exception) { error = e.sentence() }
        }
    }
    ScreenScaffold("page-channel-order", { TopBar(t("Reorder channels")) }) { inner ->
        val shown = layout
        LazyColumn(Modifier.fillMaxSize().padding(inner).testTag("channel-order-list")) {
            if (shown != null && !shown.canEdit) item { Text(t("Only people who may change this server's channels can reorder them."), Modifier.padding(Space.lg).testTag("channel-order-note"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium) }
            error?.let { item { Text(it, Modifier.padding(Space.lg).testTag("page-error"), color = Palette.Danger, style = MaterialTheme.typography.bodyMedium) } }
            items(shown?.places.orEmpty(), key = { it.id }) { place ->
                Row(
                    Modifier.fillMaxWidth().padding(start = if (place.isCategory || place.container == page.roomId) Space.lg else Space.xl, end = Space.sm).testTag("channel-row-${place.name}"),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.sm),
                ) {
                    Text(place.name, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = if (place.isCategory) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyLarge, fontWeight = if (place.isCategory) FontWeight.Bold else null)
                    if (shown?.canEdit == true) {
                        TextButton({ change { app.morpheus.moveChannel(page.roomId, place.id, -1) } }, Modifier.testTag("channel-up-${place.name}"), enabled = place.canUp) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_up, size = 20.dp) }
                        TextButton({ change { app.morpheus.moveChannel(page.roomId, place.id, 1) } }, Modifier.testTag("channel-down-${place.name}"), enabled = place.canDown) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_down, size = 20.dp) }
                        if (!place.isCategory) Box {
                            TextButton({ menuFor = place.id }, Modifier.testTag("channel-into-${place.name}")) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_folder, size = 20.dp) }
                            DropdownMenu(menuFor == place.id, { menuFor = null }) {
                                for (choice in shown.containers.filter { it.id != place.container }) {
                                    DropdownMenuItem({ Text(choice.name) }, { menuFor = null; change { app.morpheus.moveChannelInto(page.roomId, place.id, choice.id) } }, Modifier.testTag("channel-into-item-${choice.name}"))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
