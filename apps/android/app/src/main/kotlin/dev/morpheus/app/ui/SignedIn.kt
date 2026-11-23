package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.zIndex
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
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
fun SignedInScreen(model: AppModel, state: Screen.SignedIn, restoring: Boolean = false, onOpenRoom: (RoomItem) -> Unit) {
    val saved by model.tab.collectAsState()
    // While the session is restored there is nothing live for You to show: the bar opens on Servers then.
    val tab = if (restoring && saved == Tab.Profile) Tab.Servers else saved
    // Back from Direct messages or Profile goes to Servers, the home; only from there does it leave the app.
    androidx.activity.compose.BackHandler(enabled = tab != Tab.Servers) { model.selectTab(Tab.Servers) }
    Column(Modifier.fillMaxSize().testTag("signed-in")) {
        Box(Modifier.weight(1f).fillMaxWidth().lastRoomSwipe(model)) {
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
    Row(modifier.fillMaxSize().background(Palette.Background)) {
        Rail(scopes, scope?.key, onReorder = { model.setRailOrder(it) }, onManage = { model.openSheet(it.title) {
            // The desktop's menu for the server, and the phone's way to the servers screen (order, folders) at its end.
            model.morpheus.serverMenuRows(it.key.removePrefix("space:")) + uniffi.morpheus_ffi.MenuRow("reorder", "Reorder servers", t("Reorder servers"), "move", false, false, false, null, emptyList())
        } }) { model.selectScope(it.key) }
        if (scope == null) {
            EmptyState(dev.morpheus.app.R.drawable.ic_lucide_hash, t("Nothing here yet"), t("Join a server or start a conversation with the + button."), Modifier.weight(1f).background(Palette.Surface))
        } else {
            ChannelList(scope, bar.invites, model, onOpenRoom, Modifier.weight(1f), selectedRoom, rounded = true)
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
fun BottomBar(model: AppModel, tab: Tab) {
    val bar by model.sidebar.collectAsState()
    val scopes = bar?.scopes.orEmpty()
    val serverNews = scopes.filter { it.kind != ScopeKind.DMS }.sumOf { it.highlights }
    val serverUnread = scopes.any { it.kind != ScopeKind.DMS && it.hasUnread }
    val dmRooms = scopes.firstOrNull { it.kind == ScopeKind.DMS }?.sections?.flatMap { it.rooms }.orEmpty().filter { !it.muted }
    val dmUnread = dmRooms.sumOf { it.unread + it.highlights }
    Column(Modifier.fillMaxWidth().background(Palette.Background)) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(Palette.Divider))
        Row(Modifier.fillMaxWidth().height(Size.tabBar).testTag("tab-bar")) {
            TabItem(Tab.Servers, tab, t("Servers"), "tab-servers", mentions = serverNews, dot = serverUnread, onClick = { model.selectTab(Tab.Servers) }) { on ->
                GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_hash, tint = if (on) Palette.Accent else Palette.Muted)
            }
            TabItem(Tab.Dms, tab, t("DMs"), "tab-dms", mentions = dmUnread, onClick = { model.selectTab(Tab.Dms) }) { on ->
                GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_messages_square, Modifier.testTag("tab-dms-bubble"), tint = if (on) Palette.Accent else Palette.Muted)
            }
            TabItem(Tab.Profile, tab, t("You"), "tab-profile", onClick = { model.selectTab(Tab.Profile) },
                onLong = { model.openSheet(t("You")) { model.morpheus.accountMenuRows() } },
                onSecondTap = { model.nextAccount() },
            ) { on ->
                // Your own picture, ringed when this is the tab in use.
                val me = when (val s = model.screen.collectAsState().value) {
                    is dev.morpheus.app.Screen.SignedIn -> s.account.userId
                    is dev.morpheus.app.Screen.Restoring -> s.account.userId
                    else -> ""
                }
                val names by People.names.collectAsState()
                val own by model.ownProfile.collectAsState()
                val ownKey by model.ownAvatarKey.collectAsState()
                val name = own?.name ?: People.nameOf(me, names)
                Box(
                    Modifier.size(30.dp).then(if (on) Modifier.border(2.dp, Palette.Accent, CircleShape).testTag("tab-profile-ring") else Modifier),
                    contentAlignment = Alignment.Center,
                ) { Avatar(own?.avatar, name, 24.dp, Modifier.testTag("tab-profile-avatar"), key = ownKey) }
            }
        }
    }
}

@Composable
private fun RowScope.TabItem(
    me: Tab, current: Tab, label: String, tag: String,
    mentions: ULong = 0UL, dot: Boolean = false, onClick: () -> Unit, onLong: (() -> Unit)? = null, onSecondTap: (() -> Unit)? = null,
    icon: @Composable (Boolean) -> Unit,
) {
    var lastTap by remember { androidx.compose.runtime.mutableLongStateOf(0L) }
    val on = me == current
    Column(
        Modifier
            .weight(1f).fillMaxHeight()
            // Taps by pointerInput, not clickable: that would merge the icon and avatar into one node (the tests look inside).
            .pointerInput(onClick, onLong, onSecondTap) {
                // A tap acts at once (no wait to see whether a second comes); a second one close behind it is the double tap.
                detectTapGestures(
                    onLongPress = onLong?.let { long -> { long() } },
                    onTap = {
                        val now = android.os.SystemClock.uptimeMillis()
                        if (onSecondTap != null && now - lastTap < 350) { lastTap = 0; onSecondTap() } else { lastTap = now; onClick() }
                    },
                )
            }
            .semantics { role = Role.Tab; onClick { onClick(); true } }
            .semantics { selected = on }
            .testTag(tag),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(contentAlignment = Alignment.TopEnd) {
            Box(
                Modifier.height(32.dp).width(60.dp).clip(CircleShape).background(if (on) Palette.Accent.copy(alpha = 0.22f) else Color.Transparent),
                contentAlignment = Alignment.Center,
            ) { icon(on) }
            if (mentions > 0UL) MentionPill(mentions, "$tag-badge", Modifier.padding(end = Space.xs))
            else if (dot) Box(Modifier.padding(top = 4.dp, end = 14.dp).size(10.dp).clip(CircleShape).background(Palette.Text).testTag("$tag-dot"))
        }
        Text(label, Modifier.padding(top = 2.dp), style = Type.caption, color = if (on) Palette.Text else Palette.Muted, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal)
    }
}

@Composable
private fun Rail(scopes: List<ScopeItem>, selected: String?, onReorder: (List<String>) -> Unit, onManage: (ScopeItem) -> Unit, onSelect: (ScopeItem) -> Unit) {
    // A long press on a server picks it up: drag it to where it goes. Let go without moving and the servers screen
    // (folders, order) opens. The order is the account's, so every device has it.
    val step = with(androidx.compose.ui.platform.LocalDensity.current) { (Size.row + Space.sm).toPx() }
    var dragging by remember { mutableStateOf<String?>(null) }
    var shift by remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    PullDown(Modifier.width(Size.rail).fillMaxHeight().background(Palette.Background).testTag("rail")) {
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(bottom = Space.md),
        verticalArrangement = Arrangement.spacedBy(Space.sm),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        items(scopes, key = { it.key }) { scope ->
            val on = scope.key == selected
            val dragged = dragging == scope.key
            val index = scopes.indexOf(scope)
            Box(
                Modifier.size(Size.rail, Size.row)
                    .zIndex(if (dragged) 1f else 0f)
                    .graphicsLayer { translationY = if (dragged) shift else 0f; alpha = if (dragged) 0.85f else 1f }
                    .then(
                        if (scope.kind != ScopeKind.SERVER) Modifier else Modifier.pointerInput(scopes.map { it.key }) {
                            detectDragGesturesAfterLongPress(
                                onDragStart = { dragging = scope.key; shift = 0f },
                                onDrag = { change, amount -> change.consume(); shift += amount.y },
                                onDragEnd = {
                                    val by = Math.round(shift / step)
                                    dragging = null; shift = 0f
                                    if (by == 0) onManage(scope) else {
                                        val order = scopes.toMutableList()
                                        order.add((index + by).coerceIn(0, order.lastIndex), order.removeAt(index))
                                        onReorder(order.filter { it.kind == ScopeKind.SERVER }.map { it.key.removePrefix("space:") })
                                    }
                                },
                                // A press let go without a move ends as a cancel.
                                onDragCancel = { val still = dragging == scope.key && shift == 0f; dragging = null; shift = 0f; if (still) onManage(scope) },
                            )
                        },
                    ),
                contentAlignment = Alignment.Center,
            ) {
                // As on the desktop's rail: every icon a rounded square (48 dp, a corner of a third of it), the selected one
                // in the accent with a tall white mark at the edge, 8 dp for news and none otherwise.
                val shape = RoundedCornerShape(16.dp)
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
                        ScopeKind.SERVER -> Avatar(scope.avatar, scope.title, 48.dp, square = true, key = scope.avatarKey)
                        else -> GlyphIcon(
                            when (scope.kind) {
                                ScopeKind.FAVORITES -> dev.morpheus.app.R.drawable.ic_lucide_star
                                ScopeKind.NETWORK -> dev.morpheus.app.R.drawable.ic_lucide_link
                                else -> dev.morpheus.app.R.drawable.ic_lucide_hash
                            },
                            tint = if (on) onColour(Palette.Accent) else Palette.Text,
                        )
                    }
                }
                Box(
                    Modifier.align(Alignment.CenterStart).width(4.dp)
                        .height(if (on) 40.dp else if (scope.hasUnread) 8.dp else 0.dp)
                        .clip(RoundedCornerShape(topEnd = 4.dp, bottomEnd = 4.dp)).background(Color.White) // fixed, as the desktop's mark: white on the rail
                        .then(if (on) Modifier.testTag("rail-pill-selected") else if (scope.hasUnread && scope.highlights == 0UL) Modifier.testTag("dot-${scope.title}") else Modifier),
                )
                if (scope.highlights > 0UL) {
                    MentionPill(scope.highlights, "mentions-${scope.title}", Modifier.align(Alignment.BottomEnd).padding(end = Space.sm))
                }
            }
        }
    }
    }
}

@Composable
private fun ChannelList(
    scope: ScopeItem, invites: List<InviteItem>, model: AppModel, onOpenRoom: (RoomItem) -> Unit,
    modifier: Modifier, selectedRoom: String?, rounded: Boolean = false,
) {
    androidx.compose.runtime.SideEffect { dev.morpheus.app.AppState.listDrawn() }
    Column(
        modifier.then(if (rounded) Modifier.clip(RoundedCornerShape(topStart = Radius.lg)) else Modifier).background(Palette.Surface).testTag("channel-panel"),
    ) {
        Row(Modifier.fillMaxWidth().padding(start = Space.lg, end = Space.xs).height(Size.topBar), verticalAlignment = Alignment.CenterVertically) {
            Text(
                if (scope.kind == ScopeKind.DMS) t("DMs") else scope.title,
                Modifier.weight(1f).testTag("scope-title"),
                style = Type.title,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val unreadIds = scope.sections.flatMap { it.rooms }.filter { it.unread > 0UL || it.highlights > 0UL }.map { it.id }
            if (unreadIds.isNotEmpty()) IconAction(dev.morpheus.app.R.drawable.ic_lucide_check_check, t("Mark read"), "mark-read", { model.markRoomsRead(unreadIds) }, tint = Palette.Muted)
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_plus, t("New"), "new-button", { model.openNew() })
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(Palette.Divider))
        if (scope.sections.all { it.rooms.isEmpty() } && invites.isEmpty()) {
            EmptyState(
                if (scope.kind == ScopeKind.DMS) dev.morpheus.app.R.drawable.ic_lucide_messages_square else dev.morpheus.app.R.drawable.ic_lucide_hash,
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
            PullDown(Modifier.fillMaxSize().testTag("rooms-list")) {
            LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(bottom = Space.lg)) {
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
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun RoomRow(room: RoomItem, onOpen: (RoomItem) -> Unit, model: AppModel, selected: Boolean) {
    val unread = (room.unread > 0UL || room.highlights > 0UL) && !room.muted
    Box(Modifier.fillMaxWidth().padding(horizontal = Space.sm, vertical = 1.dp).clip(RoundedCornerShape(Radius.md)).background(if (selected) Palette.Raised else Color.Transparent)) {
        Column {
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = if (room.isDm) 60.dp else Size.row - 8.dp)
                    .combinedClickable(onClick = { if (room.isVoice) model.startCall(room.id, room.name) else onOpen(room) }, onLongClick = { model.openSheet(room.name) { model.morpheus.roomMenuRows(room.id) } })
                    .padding(start = Space.sm, end = 96.dp)
                    .testTag("room-${room.name}"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.md),
            ) {
                if (room.isDm) {
                    Avatar(room.avatar, room.name, Size.avatar, key = room.avatarKey)
                } else {
                    GlyphIcon(if (room.isVoice) dev.morpheus.app.R.drawable.ic_lucide_volume_2 else dev.morpheus.app.R.drawable.ic_lucide_hash, Modifier.testTag(if (room.isVoice) "voice-icon-${room.name}" else "text-icon-${room.name}"), size = 20.dp, tint = if (unread) Palette.Text else Palette.Muted)
                }
                if (room.pinned) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_pin, Modifier.semantics { contentDescription = t("Pinned") }.testTag("pin-${room.name}"), size = 16.dp, tint = Palette.Muted)
                if (room.muted) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_bell_off, Modifier.semantics { contentDescription = t("Muted") }.testTag("muted-room-${room.name}"), size = 16.dp, tint = Palette.Muted)
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
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
                            Text(name, Modifier.testTag("in-call-${room.name}-$name"), color = Palette.Muted, style = Type.secondary)
                            if (member.muted || member.deafened) GlyphIcon(
                                if (member.deafened) dev.morpheus.app.R.drawable.ic_lucide_headphone_off else dev.morpheus.app.R.drawable.ic_lucide_mic_off,
                                Modifier.testTag("in-call-muted-${room.name}-$name"), size = 14.dp, tint = Palette.Muted,
                            )
                        }
                    }
                }
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
    val own by model.ownProfile.collectAsState()
    val ownKey by model.ownAvatarKey.collectAsState()
    val name = own?.name ?: People.nameOf(state.account.userId, names)
    val sync by remember { model.morpheus.syncStates() }.collectAsState(initial = "")
    val online = sync == "running" || sync.isEmpty()
    Column(Modifier.fillMaxSize().background(Palette.Surface).verticalScroll(rememberScrollState()).testTag("profile-pane")) {
        Column(Modifier.fillMaxWidth().background(Palette.Background).padding(Space.xl), horizontalAlignment = Alignment.CenterHorizontally) {
            Avatar(own?.avatar, name, Size.avatarXl, key = ownKey)
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
        ProfileRow(dev.morpheus.app.R.drawable.ic_lucide_settings, t("Settings"), "settings-button") { model.openSettings() }
        ProfileRow(dev.morpheus.app.R.drawable.ic_lucide_lock, t("Security"), "security-button") { model.openSecurity() }
        Box(Modifier.padding(top = Space.lg)) { ProfileRow(dev.morpheus.app.R.drawable.ic_lucide_log_out, t("Sign out"), "sign-out", danger = true) { model.signOut() } }
    }
}

@Composable
private fun ProfileRow(@androidx.annotation.DrawableRes icon: Int, label: String, tag: String, danger: Boolean = false, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Size.row).clickable(onClick = onClick).padding(horizontal = Space.lg).testTag(tag),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.lg),
    ) {
        GlyphIcon(icon, tint = if (danger) Palette.Danger else Palette.Muted)
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
        GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_shield, tint = Palette.Accent)
        Text(t("Verify this device to read your encrypted messages"), Modifier.weight(1f), style = Type.secondary)
        GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_right, tint = Palette.Muted)
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


/** A swipe from the lists over to the chat (a finger drag to the left, as the chat is the pane to their right) opens the chat opened most recently, wherever it was. */
private fun Modifier.lastRoomSwipe(model: AppModel): Modifier = pointerInput(model) {
    var total = 0f
    detectHorizontalDragGestures(
        onDragStart = { total = 0f },
        onDragEnd = { if (total < -(size.width / 6f)) model.reopenLast() },
        onDragCancel = { total = 0f },
    ) { _, amount -> total += amount }
}


/**
 * A list that can be pulled down past its top and stays there, up to half its height: the top item is then
 * about half way up the screen, where a thumb reaches. A drag up gives the pull back before the list scrolls.
 * A launch is not pulled: it starts at the top.
 */
@Composable
private fun PullDown(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    var pull by remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    var most by remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    val connection = remember {
        object : androidx.compose.ui.input.nestedscroll.NestedScrollConnection {
            override fun onPreScroll(available: androidx.compose.ui.geometry.Offset, source: androidx.compose.ui.input.nestedscroll.NestedScrollSource): androidx.compose.ui.geometry.Offset {
                if (pull <= 0f || available.y >= 0f) return androidx.compose.ui.geometry.Offset.Zero
                val used = minOf(pull, -available.y)
                pull -= used
                return androidx.compose.ui.geometry.Offset(0f, -used)
            }
            override fun onPostScroll(consumed: androidx.compose.ui.geometry.Offset, available: androidx.compose.ui.geometry.Offset, source: androidx.compose.ui.input.nestedscroll.NestedScrollSource): androidx.compose.ui.geometry.Offset {
                if (available.y <= 0f || most <= 0f) return androidx.compose.ui.geometry.Offset.Zero
                val used = minOf(available.y, most - pull)
                pull += used
                return androidx.compose.ui.geometry.Offset(0f, used)
            }
        }
    }
    Box(modifier.clipToBounds().onSizeChanged { most = it.height / 2f }.nestedScroll(connection)) {
        Box(Modifier.fillMaxSize().graphicsLayer { translationY = pull }) { content() }
    }
}
