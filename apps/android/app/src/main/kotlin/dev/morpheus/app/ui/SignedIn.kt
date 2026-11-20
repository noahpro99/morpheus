package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.key
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.Screen
import uniffi.morpheus_ffi.InviteItem
import uniffi.morpheus_ffi.RoomItem
import uniffi.morpheus_ffi.ScopeItem
import uniffi.morpheus_ffi.ScopeKind

/** Signed in: the rail of servers on the left, the open server's channels beside it (as Discord's phone app). */
@Composable
fun SignedInScreen(model: AppModel, state: Screen.SignedIn, onOpenRoom: (RoomItem) -> Unit) {
    val sidebar by model.sidebar.collectAsState()
    val key by model.scopeKey.collectAsState()
    val scopes = sidebar?.scopes.orEmpty()
    val scope = scopes.firstOrNull { it.key == key } ?: scopes.firstOrNull()
    val invites = sidebar?.invites.orEmpty()

    Row(Modifier.fillMaxSize().testTag("signed-in")) {
        Rail(scopes, scope?.key, onReorder = { model.openReorder() }) { model.selectScope(it.key) }
        Column(Modifier.weight(1f).fillMaxHeight()) {
            if (scope == null) {
                Text(t("Loading…"), Modifier.padding(16.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                ChannelList(scope, invites, model, onOpenRoom, Modifier.weight(1f))
            }
            SecurityBanner(model)
            Account(model, state)
        }
    }
}

@Composable
private fun Rail(scopes: List<ScopeItem>, selected: String?, onReorder: () -> Unit, onSelect: (ScopeItem) -> Unit) {
    LazyColumn(
        Modifier.width(72.dp).fillMaxHeight().background(Palette.Background).testTag("rail"),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        items(scopes, key = { it.key }) { scope ->
            Box(Modifier.size(56.dp), contentAlignment = Alignment.Center) {
                val shape = if (scope.key == selected) RoundedCornerShape(16.dp) else CircleShape
                Box(
                    Modifier
                        .size(48.dp)
                        .clip(shape)
                        .background(if (scope.key == selected) Palette.Accent else Palette.Raised)
                        .clickable { onSelect(scope) }
                        .semantics { contentDescription = scope.title }
                        .testTag("scope-${scope.title}"),
                    contentAlignment = Alignment.Center,
                ) {
                    when (scope.kind) {
                        ScopeKind.SERVER -> Avatar(scope.avatar, scope.title, 48.dp)
                        else -> Text(
                            when (scope.kind) {
                                ScopeKind.DMS -> "@"
                                ScopeKind.FAVORITES -> "★"
                                ScopeKind.NETWORK -> scope.title.take(1)
                                else -> "#"
                            },
                            fontSize = 22.sp,
                            color = Palette.Text,
                        )
                    }
                }
                // Something unread in it (not a count: that is on its rooms).
                if (scope.hasUnread && scope.highlights == 0UL) {
                    Box(
                        Modifier.align(Alignment.CenterStart).size(8.dp).clip(CircleShape)
                            .background(Palette.Text).testTag("dot-${scope.title}"),
                    )
                }
                if (scope.highlights > 0UL) {
                    Badge(scope.highlights.toString(), Palette.Danger, "mentions-${scope.title}", Modifier.align(Alignment.BottomEnd))
                }
            }
        }
        if (scopes.count { it.kind == ScopeKind.SERVER } > 1) {
            item(key = "reorder-servers") {
                TextButton(onReorder, Modifier.testTag("reorder-servers")) { Text("⇅", fontSize = 20.sp, color = Palette.Muted) }
            }
        }
    }
}

@Composable
private fun Badge(text: String, colour: Color, tag: String, modifier: Modifier = Modifier) {
    Box(
        modifier.heightIn(min = 18.dp).clip(CircleShape).background(colour).padding(horizontal = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, Modifier.testTag(tag), color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun ChannelList(scope: ScopeItem, invites: List<InviteItem>, model: AppModel, onOpenRoom: (RoomItem) -> Unit, modifier: Modifier) {
    Column(modifier.background(Palette.Surface)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                scope.title,
                Modifier.weight(1f).padding(16.dp).testTag("scope-title"),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
            val unreadIds = scope.sections.flatMap { it.rooms }.filter { it.unread > 0UL || it.highlights > 0UL }.map { it.id }
            if (unreadIds.isNotEmpty()) {
                TextButton({ model.markRoomsRead(unreadIds) }, Modifier.testTag("mark-read")) { Text(t("Mark read"), fontSize = 13.sp) }
            }
            TextButton({ model.openNew() }, Modifier.testTag("new-button")) { Text("＋", fontSize = 24.sp) }
        }
        // Each rail entry has its own scroll position.
        key(scope.key) {
            val listState = androidx.compose.foundation.lazy.rememberLazyListState()
            // A new invitation goes above the rooms: the list stays on the room it was showing, so go and look.
            androidx.compose.runtime.LaunchedEffect(invites.size) { if (invites.isNotEmpty()) listState.animateScrollToItem(0) }
            LazyColumn(Modifier.fillMaxSize().testTag("rooms-list"), state = listState) {
            if (invites.isNotEmpty()) {
                item(key = "invites-heading") {
                    Text(t("Invites").uppercase(), Modifier.padding(start = 16.dp, top = 8.dp, bottom = 4.dp), color = Palette.Muted, fontSize = 12.sp, fontWeight = FontWeight.Bold)
                }
                items(invites, key = { "invite-${it.id}" }) { invite -> InviteCard(invite, model) }
            }
            for ((index, section) in scope.sections.withIndex()) {
                section.title?.let {
                    item(key = "section-$index-$it") {
                        Text(
                            it.uppercase(),
                            Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
                            color = Palette.Muted,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Bold,
                        )
                    }
                }
                items(section.rooms, key = { it.id }) { room -> RoomRow(room, onOpenRoom, model) }
            }
        } }
    }
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun RoomRow(room: RoomItem, onOpen: (RoomItem) -> Unit, model: AppModel) {
    val unread = room.unread > 0UL && !room.muted
    var menu by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }
    var choosing by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }
    Box(Modifier.fillMaxWidth().heightIn(min = 56.dp)) {
        Column {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 56.dp)
                .combinedClickable(onClick = { if (room.isVoice) model.startCall(room.id, room.name) else onOpen(room) }, onLongClick = { menu = true })
                .padding(start = 12.dp, end = 72.dp)
                .testTag("room-${room.name}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (room.isDm) {
                Avatar(room.avatar, room.name, 40.dp)
            } else {
                Text(if (room.isVoice) "🔊" else "#", color = Palette.Muted, fontSize = 20.sp, modifier = Modifier.width(40.dp))
            }
            if (room.pinned) Text("📌", Modifier.testTag("pin-${room.name}"), fontSize = 12.sp)
            if (room.muted) Text("🔕", Modifier.testTag("muted-room-${room.name}"), fontSize = 12.sp)
            Text(
                room.name,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                fontWeight = if (unread) FontWeight.Bold else FontWeight.Normal,
                color = if (unread) Palette.Text else Palette.Muted,
            )
        }
        if (room.call.isNotEmpty()) {
            val names by model.callNames.collectAsState()
            Column(Modifier.padding(start = 52.dp, bottom = 8.dp)) {
                for (member in room.call) {
                    androidx.compose.runtime.LaunchedEffect(member.userId) { model.wantName(member.userId) }
                    val name = model.nameOf(member.userId, names)
                    Text(
                        name + if (member.deafened) " 🔇" else if (member.muted) " 🔇" else "",
                        Modifier.testTag("in-call-${room.name}-$name"),
                        color = Palette.Muted, fontSize = 13.sp,
                    )
                }
            }
        }
        }
        // In the tree, not a popup window: the list itself carries it (and so do the tests).
        if (menu) {
            if (choosing) {
                Column(Modifier.align(Alignment.CenterEnd).background(Palette.Raised, RoundedCornerShape(8.dp)).testTag("mute-choices")) {
                    for ((index, choice) in androidx.compose.runtime.remember { uniffi.morpheus_ffi.muteChoices() }.withIndex()) {
                        TextButton({ menu = false; choosing = false; model.muteRoom(room.id, choice.ms) }, Modifier.testTag("mute-choice-$index")) {
                            Text(choice.label, fontSize = 13.sp)
                        }
                    }
                }
            } else Row(Modifier.align(Alignment.CenterEnd).background(Palette.Raised, RoundedCornerShape(8.dp)).testTag("dm-menu")) {
                if (room.isDm) {
                    TextButton({ menu = false; model.setDmMark(room.id, "pinned", !room.pinned) }, Modifier.testTag("dm-pin")) {
                        Text(if (room.pinned) t("Unpin") else t("Pin"), fontSize = 13.sp)
                    }
                    TextButton({ menu = false; model.setDmMark(room.id, "closed", true) }, Modifier.testTag("dm-close")) {
                        Text(t("Close"), fontSize = 13.sp)
                    }
                }
                TextButton({ if (room.muted) { menu = false; model.muteRoom(room.id, null) } else choosing = true }, Modifier.testTag("room-mute")) {
                    Text(if (room.muted) t("Unmute") else t("Mute"), fontSize = 13.sp)
                }
                TextButton({ menu = false; choosing = false }, Modifier.testTag("dm-cancel")) { Text("✕", fontSize = 13.sp) }
            }
        }
        Row(
            Modifier.align(Alignment.CenterEnd).padding(end = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            if (room.highlights > 0UL) Badge(room.highlights.toString(), Palette.Danger, "mention-${room.name}")
            if (room.unread > 0UL) {
                Badge(room.unread.toString(), if (room.muted) Palette.Raised else Palette.Accent, "badge-${room.name}")
            }
        }
    }
}

@Composable
private fun Account(model: AppModel, state: Screen.SignedIn) {
    Row(
        Modifier.fillMaxWidth().background(Palette.Background).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            state.account.userId,
            Modifier.weight(1f).testTag("account"),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            color = Palette.Muted,
        )
        TextButton({ model.openSettings() }, Modifier.testTag("settings-button")) { Text(t("Settings")) }
        TextButton({ model.signOut() }, Modifier.testTag("sign-out")) { Text(t("Sign out")) }
    }
}

/** Shown while this device is not verified or cannot read the key backup yet. */
@Composable
private fun SecurityBanner(model: AppModel) {
    val security by model.security.collectAsState()
    val status = security?.status?.collectAsState()?.value ?: return
    if (status.deviceVerified && status.recovery != uniffi.morpheus_ffi.RecoveryKind.NEEDS_KEY) return
    Row(
        Modifier.fillMaxWidth().background(Palette.Raised).clickable { model.openSecurity() }.padding(16.dp).testTag("verify-banner"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(t("Verify this device to read your encrypted messages"), Modifier.weight(1f))
        Text("›", fontSize = 22.sp, color = Palette.Muted)
    }
}

/** An invitation: who asked, to what, and the two answers. */
@Composable
private fun InviteCard(invite: InviteItem, model: AppModel) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp)
            .background(Palette.Raised, RoundedCornerShape(8.dp)).padding(12.dp).testTag("invite-${invite.name}"),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(invite.name, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(invite.line, color = Palette.Muted, fontSize = 13.sp, modifier = Modifier.testTag("invite-line"))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            androidx.compose.material3.Button({ model.answerInvite(invite.id, true) }, Modifier.testTag("invite-accept-${invite.name}")) { Text(t("Accept")) }
            TextButton({ model.answerInvite(invite.id, false) }, Modifier.testTag("invite-decline-${invite.name}")) { Text(t("Decline")) }
        }
    }
}
