package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.RoomModel
import uniffi.morpheus_ffi.HitItem

/** Search this room's messages; a tap on a result goes to the message. */
@Composable
fun SearchPanel(model: RoomModel, onPick: (String) -> Unit) {
    var query by remember { mutableStateOf("") }
    var hits by remember { mutableStateOf<List<HitItem>?>(null) }
    BackHandler { onPick("") }
    LaunchedEffect(query) {
        if (query.isBlank()) {
            hits = null
            return@LaunchedEffect
        }
        kotlinx.coroutines.delay(250)
        hits = model.search(query)
    }
    ScreenScaffold("search-panel", {
        Row(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background), verticalAlignment = Alignment.CenterVertically) {
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_chevron_left, t("Back"), "search-back", { onPick("") })
            SearchField(query, { query = it }, t("Search messages"), "search-input", Modifier.weight(1f).padding(end = Space.sm, top = Space.sm, bottom = Space.sm))
        }
    }) { inner ->
      Column(Modifier.fillMaxSize().padding(inner)) {
        Text(t("Searches the messages this device has loaded."), Modifier.padding(horizontal = 16.dp, vertical = 6.dp), color = Palette.Muted, fontSize = 12.sp)
        val found = hits
        if (found != null && found.isEmpty()) Text(t("No messages found."), Modifier.padding(16.dp).testTag("search-none"), color = Palette.Muted)
        LazyColumn(Modifier.fillMaxSize().testTag("search-results"), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            items(found.orEmpty(), key = { it.eventId }) { hit ->
                androidx.compose.material3.ListItem(
                    headlineContent = { Text(hit.body, maxLines = 3, overflow = TextOverflow.Ellipsis) },
                    modifier = Modifier.fillMaxWidth().clickable { if (hit.eventId.isNotEmpty()) onPick(hit.eventId) }.testTag("search-hit"),
                    overlineContent = { Text(hit.sender + "  " + hit.`when`, fontWeight = FontWeight.Bold, maxLines = 1) },
                    colors = androidx.compose.material3.ListItemDefaults.colors(containerColor = androidx.compose.ui.graphics.Color.Transparent),
                )
            }
        }
      }
    }
}
