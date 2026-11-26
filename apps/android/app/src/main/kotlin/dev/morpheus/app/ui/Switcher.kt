package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.SwitcherKind

/**
 * The quick switcher (#500, #455): the desktop's Ctrl+K over every room, DM, server and command, as the bottom search bar of a list opens it. The
 * input stays at the bottom, right above the keyboard, and the results stand above it with the best one at the bottom, next to the thumb. The
 * matching and the order are the core's (`switcher_search`, view-model `switcher`).
 */
@Composable
fun QuickFind(model: AppModel, modifier: Modifier = Modifier) {
    var query by remember { mutableStateOf("") }
    val sidebar by model.sidebar.collectAsState()
    var hits by remember { mutableStateOf(emptyList<uniffi.morpheus_ffi.SwitcherHit>()) }
    LaunchedEffect(query, sidebar) { hits = withContext(Dispatchers.Default) { model.switcherSearch(query) } }
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(Unit) {
        delay(50)
        runCatching { focus.requestFocus() }
        keyboard?.show()
    }
    BackHandler { model.closeSwitcher() }
    // Whatever it opened, the keyboard goes with the switcher (opening a chat never raises it, #480).
    androidx.compose.runtime.DisposableEffect(Unit) { onDispose { keyboard?.hide() } }
    // The best result stays at the bottom, in view, as the query changes.
    val list = rememberLazyListState()
    LaunchedEffect(query, hits.map { it.id + it.label }) { list.scrollToItem(0) }
    Column(modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).imePadding().testTag("switcher")) {
        LazyColumn(Modifier.weight(1f).fillMaxWidth().testTag("switcher-results"), state = list, reverseLayout = true) {
            items(hits, key = { it.kind.name + it.id + it.label }) { hit ->
                ListRow(
                    hit.label, "switcher-hit-${hit.label}",
                    supporting = hit.detail.takeIf { it.isNotEmpty() },
                    leading = { Leading(hit) },
                ) { model.pickSwitcherHit(hit) }
            }
        }
        SearchField(query, { query = it }, t("Search everything"), "switcher-input", Modifier.padding(horizontal = Space.md, vertical = Space.sm), focus = focus)
    }
}

@Composable
private fun Leading(hit: uniffi.morpheus_ffi.SwitcherHit) {
    when (hit.kind) {
        SwitcherKind.SERVER -> Avatar(hit.avatar, hit.label, 40.dp, square = true, key = hit.avatarKey)
        SwitcherKind.DM -> Avatar(hit.avatar, hit.label, 40.dp, key = hit.avatarKey)
        SwitcherKind.CHANNEL -> GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_hash)
        SwitcherKind.ACTION, SwitcherKind.SETTING -> GlyphIcon(
            when (hit.icon) {
                "plus" -> dev.morpheus.app.R.drawable.ic_lucide_plus
                "compass" -> dev.morpheus.app.R.drawable.ic_lucide_compass
                "message-circle" -> dev.morpheus.app.R.drawable.ic_lucide_message_circle
                "log-in" -> dev.morpheus.app.R.drawable.ic_lucide_log_in
                "inbox" -> dev.morpheus.app.R.drawable.ic_lucide_inbox
                "settings" -> dev.morpheus.app.R.drawable.ic_lucide_settings
                else -> if (hit.kind == SwitcherKind.SETTING) dev.morpheus.app.R.drawable.ic_lucide_settings else dev.morpheus.app.R.drawable.ic_lucide_search
            },
        )
    }
}

/**
 * The inbox the bell opens: invitations waiting, then the rooms with news (mentions first), a tap goes to the room. "Mark read" clears them all.
 * (Recent mentions beyond what the lists know are #448.)
 */
@Composable
fun InboxScreen(model: AppModel, modifier: Modifier = Modifier) {
    BackHandler { model.closeInbox() }
    val bar by model.sidebar.collectAsState()
    val rooms = bar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.distinctBy { it.id }.orEmpty()
        .filter { !it.muted && (it.unread > 0UL || it.highlights > 0UL) }
        .sortedByDescending { it.highlights }
    ScreenScaffold("inbox-screen", modifier = modifier, topBar = {
        TopBar(t("Inbox")) {
            if (rooms.isNotEmpty()) IconAction(dev.morpheus.app.R.drawable.ic_lucide_check_check, t("Mark read"), "mark-read", { model.markRoomsRead(rooms.map { it.id }) })
        }
    }) { inner ->
        Column(Modifier.fillMaxSize().padding(inner)) {
            val invites = bar?.invites.orEmpty()
            if (rooms.isEmpty() && invites.isEmpty()) {
                EmptyState(dev.morpheus.app.R.drawable.ic_lucide_inbox, t("All caught up"), t("Mentions and unread rooms show here."))
            } else {
                LazyColumn(Modifier.fillMaxSize().testTag("inbox-list")) {
                    items(invites, key = { "invite-${it.id}" }) { invite ->
                        InviteCard(invite, model)
                    }
                    items(rooms, key = { it.id }) { room ->
                        ListRow(
                            room.name, "inbox-room-${room.name}",
                            supporting = if (room.highlights > 0UL) t("%{count} mentions", "count" to room.highlights.toString()) else t("%{count} unread", "count" to room.unread.toString()),
                            leading = { if (room.isDm) Avatar(room.avatar, room.name, 40.dp, key = room.avatarKey) else GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_hash) },
                        ) { model.closeInbox(); model.open(room) }
                    }
                }
            }
        }
    }
}
