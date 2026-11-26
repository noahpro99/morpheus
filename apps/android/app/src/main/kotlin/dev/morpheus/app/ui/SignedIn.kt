package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.indication
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.offset
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import kotlinx.coroutines.launch
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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
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
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.stateDescription
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
    Box(Modifier.fillMaxSize().testTag("signed-in")) {
        Column(Modifier.fillMaxSize()) {
            ListsPane(model, tab, state, onOpenRoom, Modifier.weight(1f).fillMaxWidth())
            if (tab != Tab.Profile) SecurityBanner(model)
        }
        if (tab != Tab.Profile) SwipeHint(model, Modifier.align(Alignment.BottomCenter).padding(bottom = 88.dp, start = Space.lg, end = Space.lg))
    }
}

/**
 * The place the bar names. DMs and Servers are two pages side by side (DMs on the left, as the bar has them): a drag moves them with
 * the finger, a swipe past a tenth of the width (or a quick flick) commits (to the other list, or on to the last chat; which direction does what is in
 * view-model list_swipes), a shorter one springs back, and a tap on the bar slides them the same way (#459). While they are at rest only
 * the one shown is composed. The lists scroll vertically, so this takes only sideways drags. You is not one of the pages.
 */
@Composable
private fun ListsPane(model: AppModel, tab: Tab, state: Screen.SignedIn, onOpenRoom: (RoomItem) -> Unit, modifier: Modifier) {
    val slide = model.slide
    // 0 is the DMs, 1 is Servers, and between is a slide.
    val place = remember { androidx.compose.animation.core.Animatable(if (tab == Tab.Dms) 0f else 1f) }
    var settle by remember { mutableIntStateOf(0) }
    var dragging by remember { mutableStateOf(false) }
    var before by remember { mutableStateOf(tab) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    LaunchedEffect(tab, settle) {
        if (tab == Tab.Profile) { before = tab; return@LaunchedEffect }
        val goal = if (tab == Tab.Dms) 0f else 1f
        // Coming from You, or from under a chat that is moving off the screen, there is nothing to slide from.
        if (before == Tab.Profile || slide.f.value > 0f) place.snapTo(goal)
        else if (!dragging) place.animateTo(goal, Motion.slide())
        before = tab
    }
    if (tab == Tab.Profile) {
        Box(modifier) { ProfilePane(model, state) }
        return
    }
    BoxWithConstraints(modifier.clipToBounds().pointerInput(model, tab) {
        var total = 0f
        var peeking = false
        val flick = FlickTracker()
        val list = if (tab == Tab.Dms) uniffi.morpheus_ffi.SwipeList.DMS else uniffi.morpheus_ffi.SwipeList.SERVERS
        fun endPeek() {
            if (!peeking) return
            peeking = false
            model.closeRoom()
            scope.launch { slide.f.snapTo(0f) }
        }
        detectHorizontalDragGestures(
            onDragStart = { total = 0f; flick.reset(); dragging = true; peeking = false },
            onDragEnd = {
                dragging = false
                val commits = flick.commits(total, size.width.toFloat())
                if (peeking) {
                    // The chat came in under the finger: past a tenth it stays and comes the rest of the way, else it goes back out.
                    peeking = false
                    if (!commits) model.closeRoom()
                } else if (commits) {
                    (model.swipeStep(list, fingerLeft = total < 0f) as? uniffi.morpheus_ffi.SwipeStep.Show)?.let {
                        model.selectTab(if (it.list == uniffi.morpheus_ffi.SwipeList.DMS) Tab.Dms else Tab.Servers)
                    }
                }
                slide.dragging = false
                settle++
            },
            onDragCancel = { dragging = false; endPeek(); slide.dragging = false; total = 0f; settle++ },
        ) { change, amount ->
            total += amount
            flick.add(change, amount)
            // What the finger is bringing in, a chat (it opens at once and follows the finger) or the other list.
            val step = model.swipeStep(list, fingerLeft = total < 0f)
            if (step is uniffi.morpheus_ffi.SwipeStep.Open) {
                if (!peeking && model.peekChat(step.room)) { peeking = true; slide.dragging = true }
                if (peeking) scope.launch { slide.f.snapTo((kotlin.math.abs(total) / size.width).coerceIn(0f, 1f)) }
            } else endPeek()
            // The finger moves the lists (past the far edge of either there is nothing to show, so they stay).
            scope.launch { place.snapTo((place.value - amount / size.width).coerceIn(0f, 1f)) }
        }
    }) {
        val width = constraints.maxWidth
        val at = place.value
        if (at < 1f) Box(Modifier.fillMaxSize().offset { androidx.compose.ui.unit.IntOffset((-at * width).toInt(), 0) }.testTag("dms-pane").onGloballyPositioned { dev.morpheus.app.AppState.slide(2, it.positionInWindow().x) }) { DmsPane(model, onOpenRoom) }
        if (at > 0f) Box(Modifier.fillMaxSize().offset { androidx.compose.ui.unit.IntOffset(((1f - at) * width).toInt(), 0) }.testTag("servers-pane").onGloballyPositioned { dev.morpheus.app.AppState.slide(3, it.positionInWindow().x) }) { ServersPane(model, onOpenRoom) }
    }
}

/** The rail of servers and the channels of the one it points at, with the search bar under them (the quick switcher, #500). */
@Composable
fun ServersPane(model: AppModel, onOpenRoom: (RoomItem) -> Unit, modifier: Modifier = Modifier, selectedRoom: String? = null) {
    val sidebar by model.sidebar.collectAsState()
    val key by model.scopeKey.collectAsState()
    val bar = sidebar
    if (bar == null) {
        Column(modifier.fillMaxSize().background(Palette.Background)) {
            Row(Modifier.weight(1f).fillMaxWidth()) {
                Column(Modifier.weight(1f).fillMaxHeight().background(Palette.Surface).padding(top = Space.xl)) { SkeletonRows(8, avatars = false) }
                Box(Modifier.width(Size.rail).fillMaxHeight().background(Palette.Background))
            }
            ListSearchBar(model, t("Search"))
        }
        return
    }
    val scopes = bar.scopes.filter { it.kind != ScopeKind.DMS }
    val scope = scopes.firstOrNull { it.key == key } ?: scopes.firstOrNull()
    // The desktop's menu for the server, and the phone's way to the servers screen (order, folders) at its end.
    val manage: (ScopeItem) -> Unit = { shown ->
        model.openSheet(shown.title) {
            (if (shown.kind == ScopeKind.SERVER) model.morpheus.serverMenuRows(shown.key.removePrefix("space:")) else emptyList()) + uniffi.morpheus_ffi.MenuRow("reorder", "Reorder servers", t("Reorder servers"), "move", false, false, false, null, emptyList())
        }
    }
    Column(modifier.fillMaxSize().background(Palette.Background)) {
        Row(Modifier.weight(1f).fillMaxWidth()) {
            if (scope == null) {
                EmptyState(dev.morpheus.app.R.drawable.ic_lucide_hash, t("Nothing here yet"), t("Join a server or start a conversation from the search bar."), Modifier.weight(1f).background(Palette.Surface))
            } else {
                val record by model.lastChats.collectAsState()
                ChannelList(scope, bar.invites, model, onOpenRoom, Modifier.weight(1f), selectedRoom ?: model.lastChannelIn(scope.key, record), rounded = true)
            }
            Rail(scopes, scope?.key, onReorder = { model.setRailOrder(it) }, onManage = manage) { model.selectScope(it.key) }
        }
        if (scope == null) ListSearchBar(model, t("Search"))
        else ListSearchBar(model, t("Search %{name}", "name" to scope.title)) { ServerButton(scope, { manage(scope) }) }
    }
}

/** Direct messages: one list of people, the same row as a channel, newest at the bottom, with the search bar under it. */
@Composable
fun DmsPane(model: AppModel, onOpenRoom: (RoomItem) -> Unit) {
    val bar by model.sidebar.collectAsState()
    val scope = bar?.scopes?.firstOrNull { it.kind == ScopeKind.DMS }
    // The DM a swipe right would open is marked like a server's last channel (#509); it follows the record, so it changes as soon as another is viewed.
    val record by model.lastChats.collectAsState()
    val lastDm = remember(record) { (model.swipeStep(uniffi.morpheus_ffi.SwipeList.DMS, fingerLeft = false) as? uniffi.morpheus_ffi.SwipeStep.Open)?.room }
    Column(Modifier.fillMaxSize().background(Palette.Surface)) {
        if (scope == null) {
            Column(Modifier.weight(1f).fillMaxWidth().padding(top = Space.xl)) { SkeletonRows(8) }
        } else {
            ChannelList(scope, emptyList(), model, onOpenRoom, Modifier.weight(1f).fillMaxWidth(), lastDm)
        }
        ListSearchBar(model, t("Search DMs")) { MeButton(model) }
    }
}

@Composable
private fun Rail(scopes: List<ScopeItem>, selected: String?, onReorder: (List<String>) -> Unit, onManage: (ScopeItem) -> Unit, onSelect: (ScopeItem) -> Unit) {
    // A long press on a server picks it up: drag it to where it goes. Let go without moving and the servers screen
    // (folders, order) opens. The order is the account's, so every device has it.
    val step = with(androidx.compose.ui.platform.LocalDensity.current) { (Size.railItem + Space.lg).toPx() }
    var dragging by remember { mutableStateOf<String?>(null) }
    var shift by remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    PullDown(Modifier.width(Size.rail).fillMaxHeight().background(Palette.Background).testTag("rail")) {
    LazyColumn(
        Modifier.fillMaxSize(), reverseLayout = true,
        // The first server is at the bottom, near the thumb (#503); the same 16 dp under it as under the channel list, so its bottom item is level with the list's.
        contentPadding = PaddingValues(vertical = Space.lg),
        verticalArrangement = Arrangement.spacedBy(Space.lg, Alignment.Bottom),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        items(scopes, key = { it.key }) { scope ->
            val on = scope.key == selected
            val dragged = dragging == scope.key
            // A press that became a long press is not also a tap when the finger lifts.
            val held = remember { booleanArrayOf(false) }
            // The gesture outlives list updates (a sync mid-press must not restart it and lose the press),
            // so it reads the current list and server when it ends.
            val currentScopes by androidx.compose.runtime.rememberUpdatedState(scopes)
            val currentScope by androidx.compose.runtime.rememberUpdatedState(scope)
            val manage by androidx.compose.runtime.rememberUpdatedState(onManage)
            val reorder by androidx.compose.runtime.rememberUpdatedState(onReorder)
            Box(
                Modifier.size(Size.rail, Size.railItem)
                    .zIndex(if (dragged) 1f else 0f)
                    .graphicsLayer { translationY = if (dragged) shift else 0f; alpha = if (dragged) 0.85f else 1f }
                    .pointerInput(Unit) {
                        awaitEachGesture {
                            awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                            held[0] = false
                        }
                    }
                    .then(
                        if (scope.kind != ScopeKind.SERVER) Modifier else Modifier.pointerInput(scope.key) {
                            detectDragGesturesAfterLongPress(
                                onDragStart = { held[0] = true; dragging = currentScope.key; shift = 0f },
                                onDrag = { change, amount -> change.consume(); shift += amount.y },
                                onDragEnd = {
                                    // The list is drawn from the bottom: a drag up moves the server later in the order.
                                    val by = -Math.round(shift / step)
                                    dragging = null; shift = 0f
                                    if (by == 0) manage(currentScope) else {
                                        val order = currentScopes.toMutableList()
                                        val index = order.indexOfFirst { it.key == currentScope.key }
                                        if (index >= 0) {
                                            order.add((index + by).coerceIn(0, order.lastIndex), order.removeAt(index))
                                            reorder(order.filter { it.kind == ScopeKind.SERVER }.map { it.key.removePrefix("space:") })
                                        }
                                    }
                                },
                                // A press let go without a move ends as a cancel.
                                onDragCancel = { val still = dragging == currentScope.key && shift == 0f; dragging = null; shift = 0f; if (still) manage(currentScope) },
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
                        .clickable { if (!held[0]) onSelect(scope) }
                        .semantics { contentDescription = scope.title }
                        .testTag("scope-${scope.title}"),
                    contentAlignment = Alignment.Center,
                ) {
                    when (scope.kind) {
                        ScopeKind.SERVER -> ServerIcon(scope.avatar, scope.title, 48.dp, key = scope.avatarKey)
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
                    Modifier.align(Alignment.CenterEnd).width(4.dp)
                        .height(if (on) 40.dp else if (scope.hasUnread) 8.dp else 0.dp)
                        .clip(RoundedCornerShape(topStart = 4.dp, bottomStart = 4.dp)).background(Color.White) // fixed, as the desktop's mark: white on the rail
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
        modifier.then(if (rounded) Modifier.clip(RoundedCornerShape(topEnd = Radius.lg)) else Modifier).background(Palette.Surface).testTag("channel-panel"),
    ) {
        if (scope.sections.all { it.rooms.isEmpty() } && invites.isEmpty()) {
            EmptyState(
                if (scope.kind == ScopeKind.DMS) dev.morpheus.app.R.drawable.ic_lucide_messages_square else dev.morpheus.app.R.drawable.ic_lucide_hash,
                if (scope.kind == ScopeKind.DMS) t("No conversations yet") else t("No channels yet"),
                t("Start one from the search bar."),
            )
            return@Column
        }
        // Each rail entry has its own scroll position.
        key(scope.key) {
            val listState = rememberLazyListState()
            // A new invitation goes above the rooms (the far end of the list): the list stays on the room it was showing, so go and look.
            LaunchedEffect(invites.size) { if (invites.isNotEmpty()) listState.animateScrollToItem(listState.layoutInfo.totalItemsCount - 1) }
            // Every list is anchored at the bottom, near the thumb: the first room is the lowest, later ones go up, and a short list rests at the
            // bottom (#471, #503). The list is laid out from the bottom, so a section's rooms come before its heading (drawn above them), and the
            // invitations come last (they are at the top).
            PullDown(Modifier.fillMaxSize().testTag("rooms-list")) {
            LazyColumn(Modifier.fillMaxSize(), state = listState, reverseLayout = true, contentPadding = PaddingValues(vertical = Space.lg)) {
                for ((index, section) in scope.sections.withIndex()) {
                    items(section.rooms, key = { it.id }) { room ->
                        RoomRow(
                            room, room.id == selectedRoom,
                            onOpen = { if (room.isVoice) model.startCall(room.id, room.name) else onOpenRoom(room) },
                            onMenu = { model.openSheet(room.name) { model.morpheus.roomMenuRows(room.id) } },
                            callName = { id ->
                                val names by model.callNames.collectAsState()
                                LaunchedEffect(id) { model.wantName(id) }
                                model.nameOf(id, names)
                            },
                        )
                    }
                    section.title?.let { item(key = "section-$index-$it") { SectionHeader(it) } }
                }
                if (invites.isNotEmpty()) {
                    items(invites, key = { "invite-${it.id}" }) { invite -> InviteCard(invite, model) }
                    item(key = "invites-heading") { SectionHeader(t("Invites")) }
                }
            }
            }
        }
    }
}

/**
 * A channel or a DM on a list: as wide as the list (M3 ListItem: its press, its selection and its tap target span the row; a count sits in the trailing
 * slot, so a name never runs under it), at least 48 dp tall (a DM 60). `callName` names someone in the room's call.
 */
@Composable
fun RoomRow(room: RoomItem, selected: Boolean, onOpen: () -> Unit, onMenu: () -> Unit, callName: @Composable (String) -> String) {
    val unread = (room.unread > 0UL || room.highlights > 0UL) && !room.muted
    Box(Modifier.fillMaxWidth().background(if (selected) Palette.Raised else Color.Transparent)) {
        Column {
            ListRow(
                room.name, "room-${room.name}",
                // 60 dp for a person, 48 (the touch target) for a channel.
                modifier = Modifier.height(if (room.isDm) 60.dp else Size.target).semantics { if (selected) stateDescription = t("Last opened") },
                strong = unread, quiet = !unread,
                leading = {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
                        if (room.isDm) {
                            Avatar(room.avatar, room.name, Size.avatar, key = room.avatarKey)
                        } else {
                            GlyphIcon(if (room.isVoice) dev.morpheus.app.R.drawable.ic_lucide_volume_2 else dev.morpheus.app.R.drawable.ic_lucide_hash, Modifier.testTag(if (room.isVoice) "voice-icon-${room.name}" else "text-icon-${room.name}"), size = 20.dp, tint = if (unread) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        if (room.pinned) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_pin, Modifier.semantics { contentDescription = t("Pinned") }.testTag("pin-${room.name}"), size = 16.dp)
                        if (room.muted) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_bell_off, Modifier.semantics { contentDescription = t("Muted") }.testTag("muted-room-${room.name}"), size = 16.dp)
                    }
                },
                trailing = if (room.highlights > 0UL || room.unread > 0UL) ({
                    Row(horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
                        if (room.highlights > 0UL) MentionPill(room.highlights, "mention-${room.name}")
                        if (room.unread > 0UL) UnreadBadge(room.unread, "badge-${room.name}", muted = room.muted)
                    }
                }) else null,
                onLongClick = onMenu,
                onClick = onOpen,
            )
            if (room.call.isNotEmpty()) {
                Column(Modifier.padding(start = 44.dp, bottom = Space.sm)) {
                    for (member in room.call) {
                        val name = callName(member.userId)
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
                            Text(name, Modifier.testTag("in-call-${room.name}-$name"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
                            if (member.muted || member.deafened) GlyphIcon(
                                if (member.deafened) dev.morpheus.app.R.drawable.ic_lucide_headphone_off else dev.morpheus.app.R.drawable.ic_lucide_mic_off,
                                Modifier.testTag("in-call-muted-${room.name}-$name"), size = 16.dp, tint = Palette.Muted,
                            )
                        }
                    }
                }
            }
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
    val status by model.ownStatus.collectAsState()
    LaunchedEffect(state.account.userId) { model.refreshStatus() }
    val sync by remember { model.morpheus.syncStates() }.collectAsState(initial = "")
    val online = sync == "running" || sync.isEmpty()
    Column(Modifier.fillMaxSize().background(Palette.Surface).verticalScroll(rememberScrollState()).testTag("profile-pane")) {
        Column(Modifier.fillMaxWidth().background(Palette.Background).padding(Space.xl), horizontalAlignment = Alignment.CenterHorizontally) {
            Avatar(own?.avatar, name, Size.avatarXl, key = ownKey)
            Text(name, Modifier.padding(top = Space.md).testTag("profile-name"), style = MaterialTheme.typography.titleLarge)
            Text(
                state.account.userId,
                Modifier.padding(top = Space.xs).testTag("account"),
                maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium, color = Palette.Muted,
            )
            Row(
                Modifier.padding(top = Space.md).clip(CircleShape).background(Palette.Raised).padding(horizontal = Space.md, vertical = Space.xs),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm),
            ) {
                Box(Modifier.size(12.dp).clip(CircleShape).background(if (online) Color(0xFF3BA55D) else Palette.Muted))
                Text(if (online) t("Online") else t("Offline"), Modifier.testTag("profile-status"), style = MaterialTheme.typography.bodyMedium)
            }
            if (status.isNotBlank()) Text(status, Modifier.padding(top = Space.sm).testTag("profile-status-message"), style = MaterialTheme.typography.bodyMedium, color = Palette.Muted)
        }
        SectionHeader(t("Account"))
        ProfileRow(dev.morpheus.app.R.drawable.ic_lucide_smile, t("Set status"), "status-button") { model.openPage(dev.morpheus.app.PageRequest("status", "")) }
        ProfileRow(dev.morpheus.app.R.drawable.ic_lucide_settings, t("Settings"), "settings-button") { model.openSettings() }
        ProfileRow(dev.morpheus.app.R.drawable.ic_lucide_lock, t("Security"), "security-button") { model.openSecurity() }
        Box(Modifier.padding(top = Space.lg)) { ProfileRow(dev.morpheus.app.R.drawable.ic_lucide_log_out, t("Sign out"), "sign-out", danger = true) { model.signOut() } }
    }
}

@Composable
private fun ProfileRow(@androidx.annotation.DrawableRes icon: Int, label: String, tag: String, danger: Boolean = false, onClick: () -> Unit) {
    ListRow(label, tag, danger = danger, leading = { GlyphIcon(icon) }, onClick = onClick)
}

/** Shown while this device is not verified or cannot read the key backup yet. */
@Composable
private fun SecurityBanner(model: AppModel) {
    val security by model.security.collectAsState()
    val status = security?.status?.collectAsState()?.value ?: return
    if (status.deviceVerified && status.recovery != uniffi.morpheus_ffi.RecoveryKind.NEEDS_KEY) return
    ListRow(
        t("Verify this device to read your encrypted messages"), "verify-banner",
        selected = true,
        leading = { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_shield, tint = MaterialTheme.colorScheme.primary) },
        trailing = { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_right) },
    ) { model.openSecurity() }
}

/** An invitation: who asked, to what, and the two answers. */
@Composable
fun InviteCard(invite: InviteItem, model: AppModel, modifier: Modifier = Modifier) {
    androidx.compose.material3.Card(
        modifier.fillMaxWidth().padding(horizontal = Space.md, vertical = Space.xs).testTag("invite-${invite.name}"),
        colors = androidx.compose.material3.CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh, contentColor = MaterialTheme.colorScheme.onSurface),
    ) {
        Column(Modifier.padding(Space.md), verticalArrangement = Arrangement.spacedBy(Space.xs)) {
            Text(invite.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(invite.line, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.testTag("invite-line"))
            Row(horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                androidx.compose.material3.Button({ model.answerInvite(invite.id, true) }, Modifier.testTag("invite-accept-${invite.name}")) { Text(t("Accept")) }
                TextButton({ model.answerInvite(invite.id, false) }, Modifier.testTag("invite-decline-${invite.name}")) { Text(t("Decline")) }
            }
        }
    }
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
