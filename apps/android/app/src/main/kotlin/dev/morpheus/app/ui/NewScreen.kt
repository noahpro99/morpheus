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
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.DirectoryItem
import uniffi.morpheus_ffi.FriendItem

private enum class Tab(val label: String) {
    Message("Message"), Join("Join"), Create("Create"), Browse("Browse"), Friends("Friends"),
}

/** Ways into a room: message someone, join by address, make a room, browse the directory, friends. */
@Composable
fun NewScreen(app: AppModel) {
    BackHandler { app.closeNew() }
    var tab by remember { mutableStateOf(Tab.Message) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    /** Runs `block`; a room id it returns is opened; a failure is said in words. */
    fun go(block: suspend () -> String?) {
        if (busy) return
        busy = true
        problem = null
        scope.launch {
            try {
                block()?.let { app.openNewRoom(it) }
            } catch (e: Exception) {
                problem = e.sentence()
            } finally {
                busy = false
            }
        }
    }

    Column(Modifier.fillMaxSize().background(Palette.Surface).testTag("new-screen")) {
        TopBar(t("New"), backTag = "new-back", onBack = { app.closeNew() })
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
            for (t in Tab.entries) {
                TextButton({ tab = t; problem = null }, Modifier.testTag("tab-${t.label}")) {
                    Text(t.label, fontSize = 13.sp, fontWeight = if (t == tab) FontWeight.Bold else FontWeight.Normal, color = if (t == tab) Palette.Text else Palette.Muted)
                }
            }
        }
        Column(Modifier.fillMaxSize().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            when (tab) {
                Tab.Message -> OneField(
                    "Message someone by their username, or their full Matrix ID for someone on another server.",
                    "Username or @name:server", "Message", busy,
                ) { typed -> go { app.morpheus.startDm(typed) } }
                Tab.Join -> OneField(
                    "A room's alias, its room ID, or a matrix.to or matrix: link.",
                    "#room:server", "Join", busy,
                ) { typed -> go { app.morpheus.joinAddress(typed) } }
                Tab.Create -> OneField("Make a new room.", "Room name", "Create", busy) { typed ->
                    go { app.morpheus.createRoom(typed, "") }
                }
                Tab.Browse -> Browse(app, busy) { id, via -> go { app.morpheus.joinPublic(id, via); id } }
                Tab.Friends -> Friends(app) { user -> go { app.morpheus.startDm(user) } }
            }
            problem?.let { Text(it, Modifier.testTag("new-error"), color = Palette.Danger, fontSize = 14.sp) }
        }
    }
}

@Composable
private fun OneField(hint: String, placeholder: String, button: String, busy: Boolean, onSubmit: (String) -> Unit) {
    var text by remember { mutableStateOf("") }
    Text(hint, color = Palette.Muted, fontSize = 14.sp)
    OutlinedTextField(
        text, { text = it },
        Modifier.fillMaxWidth().testTag("new-input"),
        placeholder = { Text(placeholder) },
        singleLine = true,
    )
    Button({ onSubmit(text.trim()) }, Modifier.testTag("new-submit"), enabled = text.isNotBlank() && !busy) {
        Text(if (busy) "…" else button)
    }
}

@Composable
private fun Browse(app: AppModel, busy: Boolean, onJoin: (String, String?) -> Unit) {
    var term by remember { mutableStateOf("") }
    var rooms by remember { mutableStateOf<List<DirectoryItem>?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(term) {
        kotlinx.coroutines.delay(300)
        try {
            rooms = app.morpheus.publicRooms(null, term.trim(), null).rooms.filter { !it.isSpace }
            failure = null
        } catch (e: Exception) {
            failure = e.sentence()
        }
    }
    OutlinedTextField(
        term, { term = it },
        Modifier.fillMaxWidth().testTag("directory-search"),
        placeholder = { Text(t("Search public rooms")) },
        singleLine = true,
    )
    failure?.let { Text(it, color = Palette.Danger, fontSize = 14.sp) }
    val listed = rooms
    if (listed != null && listed.isEmpty() && failure == null) Text(t("No public rooms found."), color = Palette.Muted)
    LazyColumn(Modifier.testTag("directory")) {
        items(listed.orEmpty(), key = { it.id }) { room ->
            Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(room.name, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.testTag("directory-name"))
                    Text(listOfNotNull(room.members, room.topic).joinToString(" · "), color = Palette.Muted, fontSize = 12.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
                TextButton({ onJoin(room.id, null) }, Modifier.testTag("join-${room.name}"), enabled = !busy) { Text(t("Join")) }
            }
        }
    }
}

@Composable
private fun Friends(app: AppModel, onMessage: (String) -> Unit) {
    var friends by remember { mutableStateOf<List<FriendItem>?>(null) }
    var typed by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }
    var round by remember { mutableStateOf(0) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(round) {
        try {
            friends = app.morpheus.friends()
        } catch (e: Exception) {
            failure = e.sentence()
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
            typed, { typed = it },
            Modifier.weight(1f).testTag("friend-input"),
            placeholder = { Text(t("Add a friend: username or @name:server")) },
            singleLine = true,
        )
        Button({
            scope.launch {
                try {
                    app.morpheus.setFriend(typed.trim(), true)
                    typed = ""
                    failure = null
                    round++
                } catch (e: Exception) {
                    failure = e.sentence()
                }
            }
        }, Modifier.testTag("friend-add"), enabled = typed.isNotBlank()) { Text(t("Add")) }
    }
    failure?.let { Text(it, Modifier.testTag("friend-error"), color = Palette.Danger, fontSize = 14.sp) }
    val list = friends
    if (list != null && list.isEmpty()) Text(t("No friends yet"), Modifier.testTag("no-friends"), color = Palette.Muted)
    if (list != null && list.isNotEmpty()) Text(t("All friends — %{count}", "count" to list.size.toString()), color = Palette.Muted, fontSize = 12.sp)
    LazyColumn(Modifier.testTag("friends")) {
        items(list.orEmpty(), key = { it.userId }) { friend ->
            Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Avatar(friend.avatar, friend.name, 36.dp)
                Column(
                    Modifier.weight(1f).clickable { onMessage(friend.userId) }.padding(start = 10.dp).testTag("friend-${friend.name}"),
                ) {
                    Text(friend.name, fontWeight = FontWeight.Bold, maxLines = 1)
                    Text(friend.subtitle, color = Palette.Muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                TextButton({
                    scope.launch {
                        try {
                            app.morpheus.setFriend(friend.userId, false)
                            round++
                        } catch (e: Exception) {
                            failure = e.sentence()
                        }
                    }
                }, Modifier.testTag("unfriend-${friend.name}")) { Text(t("Remove")) }
            }
        }
    }
}
