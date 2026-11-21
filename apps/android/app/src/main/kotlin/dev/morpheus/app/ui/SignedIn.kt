package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.ExitToApp
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.List
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.People
import dev.morpheus.app.Screen
import dev.morpheus.app.Tab
import dev.morpheus.app.syncStates
import uniffi.morpheus_ffi.InviteItem
import uniffi.morpheus_ffi.RoomItem
import uniffi.morpheus_ffi.ScopeItem
import uniffi.morpheus_ffi.ScopeKind

/**
 * Signed in: the place the bottom bar names. Servers is the rail with the open server's channels
 * beside it, Direct messages the list of people, Profile the account.
 */
@Composable
fun SignedInScreen(model: AppModel, state: Screen.SignedIn, onOpenRoom: (RoomItem) -> Unit) {
    val tab by model.tab.collectAsState()
    // Back from Direct messages or Profile goes to Servers, the home; only from there does it leave the app.
    androidx.activity.compose.BackHandler(enabled = tab != Tab.Servers) { model.selectTab(Tab.Servers) }
    Column(Modifier.fillMaxSize().testTag("signed-in")) {
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when (tab) {
                Tab.Servers -> ServersPane(model, onOpenRoom)
                Tab.Dms -> DmsPane(model, onOpenRoom)
                Tab.Profile -> ProfilePane(model, state)
            }
        }
        if (tab != Tab.Profile) SecurityBanner(model)
        BottomBar(model, tab)
    }
}

/** The rail of servers and the channels of the one it points at (also the pane a swipe in a chat uncovers). */
@Composable
fun ServersPane(model: AppModel, onOpenRoom: (RoomItem) -> Unit, modifier: Modifier = Modifier, selectedRoom: String? = null) {
    val sidebar by model.sidebar.collectAsState()
    val key by model.scopeKey.collectAsState()
    val bar = sidebar
    if (bar == null) {
        Row(modifier.fillMaxSize()) {
            Box(Modifier.width(Size.rail).fillMaxHeight().background(Palette.Background))
            Column(Modifier.weight(1f).fillMaxHeight().background(Palette.Surface).padding(top = Space.xl)) { SkeletonRows(8, avatars = false) }
        }
        return
    }
    val scopes = bar.scopes.filter { it.kind != ScopeKind.DMS }
    val scope = scopes.firstOrNull { it.key == key } ?: scopes.firstOrNull()
    Row(modifier.fillMaxSize()) {
        Rail(scopes, scope?.key, onReorder = { model.openReorder() }) { model.selectScope(it.key) }
        if (scope == null) {
            EmptyState(Icons.Default.Home, t("Nothing here yet"), t("Join a server or start a conversation with the + button."), Modifier.weight(1f).background(Palette.Surface))
        } else {
            ChannelList(scope, bar.invites, model, onOpenRoom, Modifier.weight(1f), selectedRoom)
        }
    }
}

/** Direct messages: one list of people, the same row as a channel. */
@Composable
fun DmsPane(model: AppModel, onOpenRoom: (RoomItem) -> Unit) {
    val bar by model.sidebar.collectAsState()
    val scope = bar?.scopes?.firstOrNull { it.kind == ScopeKind.DMS }
    if (scope == null) {
        Column(Modifier.fillMaxSize().background(Palette.Surface).padding(top = Space.xl)) { SkeletonRows(8) }
    } else {
        ChannelList(scope, emptyList(), model, onOpenRoom, Modifier.fillMaxSize(), null)
    }
}

/** The three tabs: a pill behind the icon of the one in use, an unread mark on the ones with news. */
@Composable
private fun BottomBar(model: AppModel, tab: Tab) {
    val bar by model.sidebar.collectAsState()
    val scopes = bar?.scopes.orEmpty()
    val serverNews = scopes.filter { it.kind != ScopeKind.DMS }.sumOf { it.highlights }
    val serverUnread = scopes.any { it.kind != ScopeKind.DMS && it.hasUnread }
    val dmRooms = scopes.firstOrNull { it.kind == ScopeKind.DMS }?.sections?.flatMap { it.rooms }.orEmpty().filter { !it.muted }
    val dmUnread = dmRooms.sumOf { it.unread + it.highlights }
    Column(Modifier.fillMaxWidth().background(Palette.Background)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(Palette.Divider))
        Row(Modifier.fillMaxWidth().height(Size.tabBar).testTag("tab-bar")) {
            TabItem(Tab.Servers, tab, t("Servers"), Icons.Default.Home, "tab-servers", mentions = serverNews, dot = serverUnread) { model.selectTab(Tab.Servers) }
            TabItem(Tab.Dms, tab, t("Messages"), Icons.Default.Email, "tab-dms", mentions = dmUnread) { model.selectTab(Tab.Dms) }
            TabItem(Tab.Profile, tab, t("You"), Icons.Default.AccountCircle, "tab-profile") { model.selectTab(Tab.Profile) }
        }
    }
}

@Composable
private fun RowScope.TabItem(
    me: Tab, current: Tab, label: String, icon: ImageVector, tag: String,
    mentions: ULong = 0UL, dot: Boolean = false, onClick: () -> Unit,
) {
    val on = me == current
    Column(
        Modifier
            .weight(1f).fillMaxHeight()
            .clickable(onClick = onClick, role = Role.Tab)
            .semantics { selected = on }
            .testTag(tag),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(contentAlignment = Alignment.TopEnd) {
            Box(
                Modifier.height(32.dp).width(60.dp).clip(CircleShape).background(if (on) Palette.Accent.copy(alpha = 0.22f) else Color.Transparent),
                contentAlignment = Alignment.Center,
            ) { Icon(icon, null, Modifier.size(24.dp), tint = if (on) Palette.Accent else Palette.Muted) }
            if (mentions > 0UL) MentionPill(mentions, "$tag-badge", Modifier.padding(end = Space.xs))
            else if (dot) Box(Modifier.padding(top = 4.dp, end = 14.dp).size(10.dp).clip(CircleShape).background(Palette.Text).testTag("$tag-dot"))
        }
        Text(label, Modifier.padding(top = 2.dp), style = Type.caption, color = if (on) Palette.Text else Palette.Muted, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal)
    }
}

@Composable
private fun Rail(scopes: List<ScopeItem>, selected: String?, onReorder: () -> Unit, onSelect: (ScopeItem) -> Unit) {
    LazyColumn(
        Modifier.width(Size.rail).fillMaxHeight().background(Palette.Background).testTag("rail"),
        contentPadding = PaddingValues(vertical = Space.md),
        verticalArrangement = Arrangement.spacedBy(Space.sm),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        items(scopes, key = { it.key }) { scope ->
            val on = scope.key == selected
            Box(Modifier.size(Size.rail, Size.row), contentAlignment = Alignment.Center) {
                // The selected one is a rounded square with a bar at the edge, as on the desktop.
                val shape = if (on) RoundedCornerShape(16.dp) else CircleShape
                Box(
                    Modifier
                        .size(48.dp)
                        .clip(shape)
                        .background(if (on) Palette.Accent else Palette.Raised)
                        .clickable { onSelect(scope) }
                        .semantics { contentDescription = scope.title }
                        .testTag("scope-${scope.title}"),
                    contentAlignment = Alignment.Center,
                ) {
                    when (scope.kind) {
                        ScopeKind.SERVER -> Avatar(scope.avatar, scope.title, 48.dp, square = true)
                        else -> Icon(
                            when (scope.kind) {
                                ScopeKind.FAVORITES -> Icons.Default.Star
                                ScopeKind.NETWORK -> Icons.Default.Share
                                else -> Icons.Default.List
                            },
                            null, Modifier.size(24.dp), tint = if (on) onColour(Palette.Accent) else Palette.Text,
                        )
                    }
                }
                Box(
                    Modifier.align(Alignment.CenterStart).width(4.dp)
                        .height(if (on) 32.dp else if (scope.hasUnread) 8.dp else 0.dp)
                        .clip(RoundedCornerShape(topEnd = 4.dp, bottomEnd = 4.dp)).background(Palette.Text)
                        .then(if (scope.hasUnread && !on && scope.highlights == 0UL) Modifier.testTag("dot-${scope.title}") else Modifier),
                )
                if (scope.highlights > 0UL) {
                    MentionPill(scope.highlights, "mentions-${scope.title}", Modifier.align(Alignment.BottomEnd).padding(end = Space.sm))
                }
            }
        }
        if (scopes.count { it.kind == ScopeKind.SERVER } > 1) {
            item(key = "reorder-servers") {
                IconAction(Icons.Default.Menu, t("Reorder servers"), "reorder-servers", onReorder, tint = Palette.Muted)
            }
        }
    }
}

@Composable
private fun ChannelList(
    scope: ScopeItem, invites: List<InviteItem>, model: AppModel, onOpenRoom: (RoomItem) -> Unit,
    modifier: Modifier, selectedRoom: String?,
) {
    Column(modifier.background(Palette.Surface)) {
        Row(Modifier.fillMaxWidth().padding(start = Space.lg, end = Space.xs).height(Size.topBar), verticalAlignment = Alignment.CenterVertically) {
            Text(
                scope.title,
                Modifier.weight(1f).testTag("scope-title"),
                style = Type.title,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val unreadIds = scope.sections.flatMap { it.rooms }.filter { it.unread > 0UL || it.highlights > 0UL }.map { it.id }
            if (unreadIds.isNotEmpty()) IconAction(Icons.Default.Done, t("Mark read"), "mark-read", { model.markRoomsRead(unreadIds) }, tint = Palette.Muted)
            IconAction(Icons.Default.Add, t("New"), "new-button", { model.openNew() })
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(Palette.Divider))
        if (scope.sections.all { it.rooms.isEmpty() } && invites.isEmpty()) {
            EmptyState(
                if (scope.kind == ScopeKind.DMS) Icons.Default.Email else Icons.Default.Home,
                if (scope.kind == ScopeKind.DMS) t("No conversations yet") else t("No channels yet"),
                t("Start one with the + button."),
            )
            return@Column
        }
        // Each rail entry has its own scroll position.
        key(scope.key) {
            val listState = rememberLazyListState()
            // A new invitation goes above the rooms: the list stays on the room it was showing, so go and look.
            LaunchedEffect(invites.size) { if (invites.isNotEmpty()) listState.animateScrollToItem(0) }
            LazyColumn(Modifier.fillMaxSize().testTag("rooms-list"), state = listState, contentPadding = PaddingValues(bottom = Space.lg)) {
                if (invites.isNotEmpty()) {
                    item(key = "invites-heading") { SectionHeader(t("Invites")) }
                    items(invites, key = { "invite-${it.id}" }) { invite -> InviteCard(invite, model) }
                }
                for ((index, section) in scope.sections.withIndex()) {
                    section.title?.let { item(key = "section-$index-$it") { SectionHeader(it) } }
                    items(section.rooms, key = { it.id }) { room -> RoomRow(room, onOpenRoom, model, room.id == selectedRoom) }
                }
            }
        }
    }
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun RoomRow(room: RoomItem, onOpen: (RoomItem) -> Unit, model: AppModel, selected: Boolean) {
    val unread = (room.unread > 0UL || room.highlights > 0UL) && !room.muted
    var menu by remember { mutableStateOf(false) }
    var choosing by remember { mutableStateOf(false) }
    Box(Modifier.fillMaxWidth().padding(horizontal = Space.sm, vertical = 1.dp).clip(RoundedCornerShape(Radius.md)).background(if (selected) Palette.Raised else Color.Transparent)) {
        Column {
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = if (room.isDm) 60.dp else Size.row - 8.dp)
                    .combinedClickable(onClick = { if (room.isVoice) model.startCall(room.id, room.name) else onOpen(room) }, onLongClick = { menu = true })
                    .padding(start = Space.sm, end = 96.dp)
                    .testTag("room-${room.name}"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.md),
            ) {
                if (room.isDm) {
                    Avatar(room.avatar, room.name, Size.avatar)
                } else {
                    Text(
                        if (room.isVoice) "🔊" else "#",
                        Modifier.width(Size.avatarSm), textAlign = TextAlign.Center,
                        color = if (unread) Palette.Text else Palette.Muted, style = Type.heading,
                    )
                }
                if (room.pinned) Text("📌", Modifier.testTag("pin-${room.name}"), style = Type.caption)
                if (room.muted) Text("🔕", Modifier.testTag("muted-room-${room.name}"), style = Type.caption)
                Text(
                    room.name,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = Type.body,
                    fontWeight = if (unread) FontWeight.Bold else FontWeight.Normal,
                    color = if (unread) Palette.Text else Palette.Muted,
                )
            }
            if (room.call.isNotEmpty()) {
                val names by model.callNames.collectAsState()
                Column(Modifier.padding(start = 44.dp, bottom = Space.sm)) {
                    for (member in room.call) {
                        LaunchedEffect(member.userId) { model.wantName(member.userId) }
                        val name = model.nameOf(member.userId, names)
                        Text(
                            name + if (member.muted || member.deafened) " 🔇" else "",
                            Modifier.testTag("in-call-${room.name}-$name"),
                            color = Palette.Muted, style = Type.secondary,
                        )
                    }
                }
            }
        }
        // In the tree, not a popup window: the list itself carries it (and so do the tests).
        if (menu) {
            if (choosing) {
                Column(Modifier.align(Alignment.CenterEnd).background(Palette.Raised, RoundedCornerShape(Radius.md)).testTag("mute-choices")) {
                    for ((index, choice) in remember { uniffi.morpheus_ffi.muteChoices() }.withIndex()) {
                        TextButton({ menu = false; choosing = false; model.muteRoom(room.id, choice.ms) }, Modifier.testTag("mute-choice-$index")) {
                            Text(choice.label, style = Type.secondary)
                        }
                    }
                }
            } else Row(Modifier.align(Alignment.CenterEnd).background(Palette.Raised, RoundedCornerShape(Radius.md)).testTag("dm-menu")) {
                if (room.isDm) {
                    TextButton({ menu = false; model.setDmMark(room.id, "pinned", !room.pinned) }, Modifier.testTag("dm-pin")) {
                        Text(if (room.pinned) t("Unpin") else t("Pin"), style = Type.secondary)
                    }
                    TextButton({ menu = false; model.setDmMark(room.id, "closed", true) }, Modifier.testTag("dm-close")) {
                        Text(t("Close"), style = Type.secondary)
                    }
                }
                TextButton({ if (room.muted) { menu = false; model.muteRoom(room.id, null) } else choosing = true }, Modifier.testTag("room-mute")) {
                    Text(if (room.muted) t("Unmute") else t("Mute"), style = Type.secondary)
                }
                TextButton({ menu = false; choosing = false }, Modifier.testTag("dm-cancel")) { Text("✕", style = Type.secondary) }
            }
        }
        Row(
            Modifier.align(Alignment.CenterEnd).padding(end = Space.md),
            horizontalArrangement = Arrangement.spacedBy(Space.xs),
        ) {
            if (room.highlights > 0UL) MentionPill(room.highlights, "mention-${room.name}")
            if (room.unread > 0UL) UnreadBadge(room.unread, "badge-${room.name}", muted = room.muted)
        }
    }
}

/** You: who is signed in, how the connection is, and the things that are about the account. */
@Composable
private fun ProfilePane(model: AppModel, state: Screen.SignedIn) {
    val names by People.names.collectAsState()
    LaunchedEffect(state.account.userId) { People.want(state.account.userId) }
    val name = People.nameOf(state.account.userId, names)
    val sync by remember { model.morpheus.syncStates() }.collectAsState(initial = "")
    val online = sync == "running" || sync.isEmpty()
    Column(Modifier.fillMaxSize().background(Palette.Surface).verticalScroll(rememberScrollState()).testTag("profile-pane")) {
        Column(Modifier.fillMaxWidth().background(Palette.Background).padding(Space.xl), horizontalAlignment = Alignment.CenterHorizontally) {
            Avatar(null, name, Size.avatarXl)
            Text(name, Modifier.padding(top = Space.md).testTag("profile-name"), style = Type.title)
            Text(
                state.account.userId,
                Modifier.padding(top = Space.xs).testTag("account"),
                maxLines = 1, overflow = TextOverflow.Ellipsis, style = Type.secondary, color = Palette.Muted,
            )
            Row(
                Modifier.padding(top = Space.md).clip(CircleShape).background(Palette.Raised).padding(horizontal = Space.md, vertical = Space.xs),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm),
            ) {
                Box(Modifier.size(10.dp).clip(CircleShape).background(if (online) Color(0xFF3BA55D) else Palette.Muted))
                Text(if (online) t("Online") else t("Offline"), Modifier.testTag("profile-status"), style = Type.secondary)
            }
        }
        SectionHeader(t("Account"))
        ProfileRow(Icons.Default.Settings, t("Settings"), "settings-button") { model.openSettings() }
        ProfileRow(Icons.Default.Lock, t("Security"), "security-button") { model.openSecurity() }
        Box(Modifier.padding(top = Space.lg)) { ProfileRow(Icons.Default.ExitToApp, t("Sign out"), "sign-out", danger = true) { model.signOut() } }
    }
}

@Composable
private fun ProfileRow(icon: ImageVector, label: String, tag: String, danger: Boolean = false, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Size.row).clickable(onClick = onClick).padding(horizontal = Space.lg).testTag(tag),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.lg),
    ) {
        Icon(icon, null, Modifier.size(24.dp), tint = if (danger) Palette.Danger else Palette.Muted)
        Text(label, Modifier.weight(1f), style = Type.body, color = if (danger) Palette.Danger else Palette.Text)
    }
}

/** Shown while this device is not verified or cannot read the key backup yet. */
@Composable
private fun SecurityBanner(model: AppModel) {
    val security by model.security.collectAsState()
    val status = security?.status?.collectAsState()?.value ?: return
    if (status.deviceVerified && status.recovery != uniffi.morpheus_ffi.RecoveryKind.NEEDS_KEY) return
    Row(
        Modifier.fillMaxWidth().background(Palette.Raised).clickable { model.openSecurity() }.padding(Space.lg).testTag("verify-banner"),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md),
    ) {
        Icon(Icons.Default.Warning, null, Modifier.size(20.dp), tint = Palette.Accent)
        Text(t("Verify this device to read your encrypted messages"), Modifier.weight(1f), style = Type.secondary)
        Icon(Icons.Default.KeyboardArrowRight, null, tint = Palette.Muted)
    }
}

/** An invitation: who asked, to what, and the two answers. */
@Composable
private fun InviteCard(invite: InviteItem, model: AppModel) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = Space.md, vertical = Space.xs)
            .background(Palette.Raised, RoundedCornerShape(Radius.md)).padding(Space.md).testTag("invite-${invite.name}"),
        verticalArrangement = Arrangement.spacedBy(Space.xs),
    ) {
        Text(invite.name, style = Type.bodyStrong, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(invite.line, color = Palette.Muted, style = Type.secondary, modifier = Modifier.testTag("invite-line"))
        Row(horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
            androidx.compose.material3.Button({ model.answerInvite(invite.id, true) }, Modifier.testTag("invite-accept-${invite.name}")) { Text(t("Accept")) }
            TextButton({ model.answerInvite(invite.id, false) }, Modifier.testTag("invite-decline-${invite.name}")) { Text(t("Decline")) }
        }
    }
}
