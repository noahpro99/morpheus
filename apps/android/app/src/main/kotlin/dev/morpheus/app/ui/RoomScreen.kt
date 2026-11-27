package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitHorizontalTouchSlopOrCancellation
import androidx.compose.foundation.gestures.horizontalDrag
import kotlinx.coroutines.flow.first
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Icon
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import dev.morpheus.app.RoomHooks
import dev.morpheus.app.open
import dev.morpheus.app.read
import dev.morpheus.app.size
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.FileKind
import uniffi.morpheus_ffi.SuggestionItem
import uniffi.morpheus_ffi.withEmoji
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.People
import dev.morpheus.app.RoomModel
import uniffi.morpheus_ffi.MediaHandle
import uniffi.morpheus_ffi.MessageBody
import uniffi.morpheus_ffi.MessageItem
import uniffi.morpheus_ffi.RoomItem
import uniffi.morpheus_ffi.TimelineRow

private val quickReactions = listOf("👍", "❤️", "😂", "🎉", "😮")

/** The tint and bar of a message that mentions you (the desktop's gold). */
private val MentionGold = Color(0xFFFAA81A)

/** A room: its messages (newest at the bottom), a composer, and what a tap on a message offers. */
@Composable
fun RoomScreen(app: AppModel, room: RoomItem, threadRoot: String? = null, onClose: () -> Unit = { app.closeRoom() }) {
    val model = remember(room.id, threadRoot) { RoomModel(app.morpheus, room, threadRoot, ready = { app.awaitSession() }) }
    var thread by remember(room.id) { mutableStateOf<String?>(null) }
    DisposableEffect(model) { onDispose { model.close() } }
    val allRows by model.rows.collectAsState()
    val hidden by model.hidden.collectAsState()
    val rows = remember(allRows, hidden) { if (hidden.isEmpty()) allRows else allRows.filterNot { it is TimelineRow.Message && it.message.eventId in hidden } }
    val replyingTo by model.replyingTo.collectAsState()
    if (rows.any { it is TimelineRow.Message }) androidx.compose.runtime.SideEffect { dev.morpheus.app.AppState.roomDrawn() }
    // A room just joined is listed before its name is known: the title follows the list.
    val sidebar by app.sidebar.collectAsState()
    val title = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == room.id }?.name ?: room.name
    // (A direct message just made is listed before its account data says it is one: read the list as it is now.)
    val isDm = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == room.id }?.isDm ?: room.isDm
    val encrypted = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == room.id }?.isEncrypted ?: room.isEncrypted
    // What the composer's label shows before the name (#502): the person's picture for a DM, the server's icon for a channel.
    val listed = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == room.id } ?: room
    val serverOfRoom = if (isDm) null else sidebar?.scopes?.firstOrNull { it.kind == uniffi.morpheus_ffi.ScopeKind.SERVER && it.sections.any { s -> s.rooms.any { r -> r.id == room.id } } }
    // The button row above the composer goes while the box is focused or has text and returns when it is empty and not focused, or on a pull down (#502).
    var composing by remember(room.id, threadRoot) { mutableStateOf(false) }
    var pulled by remember(room.id, threadRoot) { mutableStateOf(false) }
    var boxEmpty by remember(room.id, threadRoot) { mutableStateOf(true) }
    val scrollFocus = androidx.compose.ui.platform.LocalFocusManager.current
    val scrollKeyboard = androidx.compose.ui.platform.LocalSoftwareKeyboardController.current
    val editing by model.editing.collectAsState()
    var info by remember(room.id) { mutableStateOf(false) }
    var searching by remember(room.id) { mutableStateOf(false) }
    val typingLine by model.typing.collectAsState()
    var viewing by remember { mutableStateOf<MediaHandle?>(null) }
    val selection = remember(room.id, threadRoot) { MessageSelection() }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch { context.read(uri)?.let { model.sendFile(it.name, it.mime, it.bytes) } }
    }
    if (threadRoot == null) DisposableEffect(model) {
        RoomHooks.attach = { name, mime, bytes -> model.sendFile(name, mime, bytes) }
        onDispose { RoomHooks.attach = null }
    }

    // Opening a chat (a tap or a swipe) never focuses the message box or raises the keyboard (#480); a tap on the box does.
    val composerFocus = remember { androidx.compose.ui.focus.FocusRequester() }
    BackHandler {
        when {
            searching -> searching = false
            info -> info = false
            selection.active -> selection.clear()
            viewing != null -> viewing = null
            editing != null -> model.edit(null)
            else -> onClose()
        }
    }

    // What is read is what is in front of the person: the newest message.
    val newest = (rows.lastOrNull { it is TimelineRow.Message } as? TimelineRow.Message)?.message?.key
    // Only while this room is the one open: one that is sliding out (it stays drawn a moment) reads nothing more.
    val openNow by app.openRoom.collectAsState()
    LaunchedEffect(newest, openNow?.id == room.id) { if (newest != null && threadRoot == null && openNow?.id == room.id) model.markRead() }

    Box(Modifier.fillMaxSize()) {
        if (searching) SwipeBack(Modifier.fillMaxSize()) { SearchPanel(model) { id ->
            searching = false
            if (id.isNotEmpty()) model.jumpTo(id)
        } }
        else if (info) SwipeBack(Modifier.fillMaxSize()) { RoomInfo(app, model, room, title) { info = false } }
        else if (thread == null) RoomPages(app, room, threadRoot == null, isDm) { ScreenScaffold(if (threadRoot == null) "room-screen" else "thread-screen", {
            // A chat has no top bar (#502): its name is the composer's label and its buttons are the row above the composer. A thread keeps a title.
            if (threadRoot != null) TopBar(t("Thread"), titleTag = "thread-title")
        }) { inner -> Column(Modifier.fillMaxSize().padding(inner)) {
            androidx.compose.runtime.CompositionLocalProvider(LocalProfileRoom provides room.id, LocalSelection provides selection) {
            Timeline(
                model, rows, Modifier.weight(1f),
                onActions = { message, held ->
                    app.openSheet(
                        message.senderName, longPress = held, startFull = false, quickReactions = quickReactions,
                        onOutcome = { outcome ->
                            when (outcome) {
                                is uniffi.morpheus_ffi.MenuOutcome.Reply -> model.replyTo(message)
                                is uniffi.morpheus_ffi.MenuOutcome.Edit -> model.edit(message)
                                is uniffi.morpheus_ffi.MenuOutcome.Thread -> thread = message.eventId
                                is uniffi.morpheus_ffi.MenuOutcome.React -> message.eventId?.let { model.react(it, outcome.key) }
                                is uniffi.morpheus_ffi.MenuOutcome.Delete -> message.eventId?.let { model.deleteUndoable(it) }
                                is uniffi.morpheus_ffi.MenuOutcome.ViewReactions -> app.openPage(dev.morpheus.app.PageRequest("reactions", room.id, message = message))
                                is uniffi.morpheus_ffi.MenuOutcome.Report -> message.eventId?.let { app.openPage(dev.morpheus.app.PageRequest("report", room.id, it)) }
                                is uniffi.morpheus_ffi.MenuOutcome.OpenRoom -> {}
                                is uniffi.morpheus_ffi.MenuOutcome.Page -> if (outcome.kind == "select-messages") selection.start(message.key)
                                else -> {}
                            }
                        },
                    ) { app.morpheus.messageMenuRows(room.id, message) + uniffi.morpheus_ffi.MenuRow("select-messages", "Select messages", t("Select messages"), "check", false, false, false, null, emptyList()) }
                },
                onThread = { thread = it },
                onImage = { viewing = it },
                onFile = { media, name -> scope.launch { context.open(media, name)?.let { model.fail(it) } } },
                onFull = { app.openPage(dev.morpheus.app.PageRequest("full-message", room.id, message = it)) },
                isDm = isDm,
                onPull = { pulled = true },
                // Starting to scroll towards older messages with an empty box puts the keyboard away, for more of the conversation (#526).
                onScrollOlder = { if (boxEmpty) { scrollFocus.clearFocus(); scrollKeyboard?.hide() } },
                listTag = if (threadRoot == null) "timeline" else "thread-timeline",
            )
            }
            typingLine?.let {
                Text(it, Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp).testTag("typing-line"), color = Palette.Muted, style = MaterialTheme.typography.bodySmall)
            }
            replyingTo?.let { reply ->
                ComposerBanner(dev.morpheus.app.R.drawable.ic_ms_reply, t("Replying to %{name}: %{text}", "name" to reply.senderName.toString(), "text" to summary(reply.body)), "reply-bar", "reply-banner", t("Cancel reply"), "reply-cancel") { model.replyTo(null) }
            }
            editing?.let {
                ComposerBanner(dev.morpheus.app.R.drawable.ic_ms_edit, t("Editing your message"), "edit-bar", "edit-banner", t("Cancel editing"), "edit-cancel") { model.edit(null) }
            }
            // Messages that did not go stay here, to send again or drop (the text is not lost with the composer).
            val unsent by model.failed.collectAsState()
            for (item in unsent) {
                Row(Modifier.fillMaxWidth().background(Palette.Raised).padding(start = Space.lg).testTag("send-failed"), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(t("Not sent: %{text}", "text" to (item.text ?: t("a change"))), Modifier.testTag("send-failed-text"), color = Palette.Danger, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        Text(item.reason, color = Palette.Muted, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    TextButton({ model.retry(item.id) }, Modifier.testTag("send-retry")) { Text(t("Retry")) }
                    TextButton({ model.discard(item.id) }, Modifier.testTag("send-discard")) { Text(t("Discard")) }
                }
            }
            // The floating call window (#528) keeps above this, the composer.
            KeepClear("composer")
            val problem by model.error.collectAsState()
            // Over the composer, so it stays a line above it (a toast would cover the send button).
            problem?.let {
                Text(it, Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("room-error"), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
            }
            if (selection.active) SelectionBar(selection, rows.filterIsInstance<TimelineRow.Message>().map { it.message })
            ComposerDock {
            Composer(
                model, room.id + "|" + (threadRoot ?: ""), editing, composerFocus, title = if (threadRoot == null) title else null, dm = isDm, encrypted = encrypted, room = listed, server = serverOfRoom,
                onComposing = { composing = it; pulled = false }, onEmpty = { boxEmpty = it }, onAttach = { picker.launch("*/*") },
            ) { model.send(it) }
            if (threadRoot == null) {
                val inThis = app.calls.snapshot.collectAsState().value?.roomId == room.id
                ChatActions(
                    visible = !composing || pulled,
                    onCall = { if (inThis) app.calls.show(true) else app.startCall(room.id, title) },
                    onSearch = { searching = true },
                    onPins = { app.openPage(dev.morpheus.app.PageRequest("pinned", room.id)) },
                    onInfo = { info = true },
                )
            }
            }
        } } }
        viewing?.let { shown ->
            val pictures = remember(rows) { rows.filterIsInstance<TimelineRow.Message>().mapNotNull { (it.message.body as? MessageBody.Image)?.media } }
            PictureViewer(pictures.ifEmpty { listOf(shown) }, pictures.indexOfFirst { it.key() == shown.key() }.coerceAtLeast(0)) { viewing = null }
        }
        thread?.let { root ->
            Box(Modifier.fillMaxSize().testTag("thread-panel")) { RoomScreen(app, room, root) { thread = null } }
        }
    }
}

private fun summary(body: MessageBody): String = when (body) {
    is MessageBody.Text -> body.text
    is MessageBody.Image -> body.caption
    is MessageBody.File -> body.name
    is MessageBody.Other -> body.summary
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun Timeline(
    model: RoomModel,
    rows: List<TimelineRow>,
    modifier: Modifier,
    onActions: (MessageItem, Boolean) -> Unit,
    onThread: (String) -> Unit,
    onImage: (MediaHandle) -> Unit,
    onFile: (MediaHandle, String) -> Unit,
    onFull: (MessageItem) -> Unit,
    onPull: () -> Unit,
    isDm: Boolean,
    onScrollOlder: () -> Unit = {},
    listTag: String = "timeline",
) {
    val state = rememberLazyListState()
    // The start of a scroll towards older messages (the first item of this bottom-up list moves away from the newest), once per gesture.
    val older by androidx.compose.runtime.rememberUpdatedState(onScrollOlder)
    LaunchedEffect(state) {
        var from = -1L
        var fired = false
        androidx.compose.runtime.snapshotFlow { Triple(state.isScrollInProgress, state.firstVisibleItemIndex, state.firstVisibleItemScrollOffset) }.collect { (going, i, o) ->
            val pos = i.toLong() * 100_000 + o
            if (!going) { from = -1L; fired = false }
            else if (from < 0) from = pos
            else if (!fired && pos - from > 20) { fired = true; older() }
        }
    }
    // A pull down at the newest message (the finger moving down while the list rests at the bottom) brings the button row back (#502).
    val pull by androidx.compose.runtime.rememberUpdatedState(onPull)
    val pullDown = remember(state) {
        object : androidx.compose.ui.input.nestedscroll.NestedScrollConnection {
            override fun onPreScroll(available: androidx.compose.ui.geometry.Offset, source: androidx.compose.ui.input.nestedscroll.NestedScrollSource): androidx.compose.ui.geometry.Offset {
                if (available.y > 0f && state.firstVisibleItemIndex == 0 && state.firstVisibleItemScrollOffset == 0) pull()
                return androidx.compose.ui.geometry.Offset.Zero
            }
        }
    }
    // Newest first: the list is laid out from the bottom.
    val items = remember(rows) { rows.asReversed() }
    val atStart = rows.any { it is TimelineRow.Start }
    val sayHi = remember(rows, isDm) { uniffi.morpheus_ffi.sayHiOffered(isDm, atStart, rows.filterIsInstance<TimelineRow.Message>().map { it.message.isEvent }) }
    val nearTop by remember {
        derivedStateOf {
            val last = state.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            last >= state.layoutInfo.totalItemsCount - 6
        }
    }
    // Asked again when a request ends: the page that arrived may not have been
    // the last, and a request made while one was out was ignored.
    val loading by model.loadingOlder.collectAsState()
    LaunchedEffect(nearTop, items.size, atStart, loading) { if (nearTop && !atStart && !loading) model.loadOlder() }
    // The skeleton stays until the older rows are in the list, not only until the request ends: the rows arrive a moment after it, and a list at
    // its far end that lost the skeleton first would shift under the reader (#501).
    var askedAt by remember { androidx.compose.runtime.mutableIntStateOf(-1) }
    LaunchedEffect(loading) { if (loading) askedAt = rows.size else { kotlinx.coroutines.delay(400); askedAt = -1 } }
    val waitingForOlder = (loading || (askedAt >= 0 && rows.size == askedAt)) && !atStart && rows.isNotEmpty()
    // The list stays pinned to the newest message while the person is at the bottom (#537): a message sent or received (the list anchors on what was in
    // view, so a new row would land below it), the composer, the button row or the keyboard changing height, a row growing. A drag or fling away from the
    // bottom unpins it; coming back to the bottom pins it again.
    var userScrolling by remember { mutableStateOf(false) }
    var pinned by remember { mutableStateOf(true) }
    LaunchedEffect(state) {
        var sawGoing = false
        launch { state.interactionSource.interactions.collect { if (it is androidx.compose.foundation.interaction.DragInteraction.Start) { userScrolling = true; sawGoing = false } } }
        androidx.compose.runtime.snapshotFlow { state.isScrollInProgress }.collect { going -> if (going) sawGoing = true else if (sawGoing) userScrolling = false }
    }
    LaunchedEffect(state) {
        androidx.compose.runtime.snapshotFlow { listOf(state.firstVisibleItemIndex, state.firstVisibleItemScrollOffset, state.layoutInfo.viewportSize.height, state.layoutInfo.totalItemsCount, if (userScrolling) 1 else 0) }.collect {
            val atBottom = state.firstVisibleItemIndex == 0 && state.firstVisibleItemScrollOffset == 0
            if (atBottom) pinned = true else if (userScrolling) pinned = false else if (pinned) state.scrollToItem(0)
        }
    }
    // What the last run kept (#313) is drawn first and the live rows replace it: the list anchors on the message that
    // was in view, so the first live rows would leave a person who was at the newest looking at old messages. Go back to
    // the newest once, unless they have scrolled since.
    val live by model.live.collectAsState()
    var touched by remember { mutableStateOf(false) }
    LaunchedEffect(state) { state.interactionSource.interactions.collect { if (it is androidx.compose.foundation.interaction.DragInteraction.Start) touched = true } }
    LaunchedEffect(live) { if (live && !touched) state.scrollToItem(0) }

    // A search result to go to: scroll to it once it is in the list, and light it a moment.
    val jump by model.jump.collectAsState()
    val lit by model.highlight.collectAsState()
    LaunchedEffect(jump, items) {
        val target = jump ?: return@LaunchedEffect
        val at = items.indexOfFirst { it is TimelineRow.Message && it.message.eventId == target }
        if (at >= 0) {
            pinned = false
            state.animateScrollToItem(at)
            model.highlight(target)
            model.clearJump()
            kotlinx.coroutines.delay(2000)
            model.highlight(null)
        }
    }
    // People you ignore are folded into a line that shows the message when tapped.
    val ignored by People.ignored.collectAsState()
    var revealed by remember { mutableStateOf(setOf<String>()) }
    val dividerAt = items.indexOfFirst { it is TimelineRow.New }
    val dividerShowing by remember(dividerAt) { derivedStateOf { state.layoutInfo.visibleItemsInfo.any { it.index == dividerAt } } }
    Box(modifier) {
    // Nothing yet: placeholders where the messages will be.
    if (rows.isEmpty()) Column(Modifier.align(Alignment.BottomStart).padding(bottom = Space.lg)) { SkeletonRows(7) }
    LazyColumn(Modifier.stretch(state).fillMaxSize().nestedScroll(pullDown).testTag(listTag), state = state, reverseLayout = true) {
        items(items, key = { rowKey(it) }) { row ->
            when (row) {
                is TimelineRow.Message -> if (row.message.senderId in ignored && row.message.key !in revealed) {
                    Text(
                        t("Ignored message · Show"),
                        Modifier.fillMaxWidth().clickable { revealed = revealed + row.message.key }.padding(horizontal = 16.dp, vertical = 8.dp).testTag("ignored-fold"),
                        color = Palette.Muted, style = MaterialTheme.typography.bodyMedium,
                    )
                } else MessageRow(row.message, model, onActions, onThread, onImage, onFile, onFull, lit != null && lit == row.message.eventId)
                is TimelineRow.Date -> Row(
                    Modifier.fillMaxWidth().padding(horizontal = Space.lg, vertical = Space.md),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.md),
                ) {
                    Box(Modifier.weight(1f).height(1.dp).background(Palette.Divider))
                    Text(dayLabel(row.timestampMs.toLong(), rememberNow()), Modifier.testTag("date-divider"), color = Palette.Muted, style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.SemiBold))
                    Box(Modifier.weight(1f).height(1.dp).background(Palette.Divider))
                }
                TimelineRow.Start -> Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Space.md)) {
                    // No words here: the list's native stretch says it is the start (#524). A 1 dp marker is what the tests look for.
                    Spacer(Modifier.height(1.dp).testTag("timeline-start"))
                    // An empty direct message offers a greeting (#421).
                    if (sayHi) {
                        androidx.compose.material3.FilledTonalButton({ model.send(uniffi.morpheus_ffi.sayHiGreeting()) }, Modifier.testTag("say-hi")) { Text(t("Say hi! 👋")) }
                    }
                }
                TimelineRow.New -> Text(
                    "NEW",
                    Modifier.fillMaxWidth().padding(vertical = 4.dp).testTag("new-divider"),
                    textAlign = TextAlign.Center,
                    color = Palette.Danger,
                    style = MaterialTheme.typography.bodySmall,
                    fontWeight = FontWeight.Bold,
                )
            }
        }
        // Older messages on their way (#501): skeleton rows above the oldest, so scrolling up never meets a blank edge. They are the last item of
        // this bottom-up list, past the one the list is anchored on (#470), so their coming and going moves nothing the person is reading.
        if (waitingForOlder) item(key = "older-skeleton") { Box(Modifier.padding(vertical = Space.md).testTag("older-skeleton")) { SkeletonRows(3) } }
    }
    // A long history has a handle on its right edge with the date under it (#493).
    val dateAt = remember(items) { items.indices.filter { items[it] is TimelineRow.Date } }
    val clock = rememberNow()
    FastScroll(state, items.size, { i -> dateAt.firstOrNull { it >= i }?.let { dayLabel((items[it] as TimelineRow.Date).timestampMs.toLong(), clock) } })
    if (dividerAt >= 0 && !dividerShowing) {
        val scope = androidx.compose.runtime.rememberCoroutineScope()
        Button(
            { scope.launch { state.animateScrollToItem(dividerAt) } },
            Modifier.align(Alignment.TopCenter).padding(top = 8.dp).testTag("jump-unread"),
        ) { Text(t("Jump to unread")) }
    }
    // Scrolled away from the newest messages: a way back down (#414).
    val density = androidx.compose.ui.platform.LocalDensity.current.density
    val away by remember { derivedStateOf { uniffi.morpheus_ffi.awayFromPresent(state.firstVisibleItemIndex.toUInt(), state.firstVisibleItemScrollOffset / density) } }
    // (The scope lives outside the button: the button goes while the scroll is still on its way.)
    val jumpScope = androidx.compose.runtime.rememberCoroutineScope()
    // How many messages came in while the reader was up in the history: the badge on the button.
    val messageCount = items.count { it is TimelineRow.Message }
    var countWhenAway by remember { mutableIntStateOf(messageCount) }
    LaunchedEffect(away) { if (away) countWhenAway = messageCount }
    if (away) {
        val scope = jumpScope
        val fresh = (messageCount - countWhenAway).coerceAtLeast(0)
        // A small round button with a down arrow at the bottom right, above the composer (#492); a count of what is new below on its corner.
        androidx.compose.material3.BadgedBox(
            { if (fresh > 0) androidx.compose.material3.Badge { Text(if (fresh > 99) "99+" else fresh.toString(), Modifier.testTag("jump-present-count")) } },
            Modifier.align(Alignment.BottomEnd).padding(end = Space.lg, bottom = Space.lg),
        ) {
            androidx.compose.material3.SmallFloatingActionButton(
                { scope.launch { state.animateScrollToItem(0) } },
                Modifier.semantics { contentDescription = t("Jump to present") }.testTag("jump-present"),
            ) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_down, size = 24.dp) }
        }
    }
    }
}

private fun rowKey(row: TimelineRow): String = when (row) {
    is TimelineRow.Message -> row.message.key
    is TimelineRow.Date -> "date-${row.timestampMs}"
    TimelineRow.Start -> "start"
    TimelineRow.New -> "new"
}

@Composable
private fun MessageRow(
    message: MessageItem,
    model: RoomModel,
    onActions: (MessageItem, Boolean) -> Unit,
    onThread: (String) -> Unit,
    onImage: (MediaHandle) -> Unit,
    onFile: (MediaHandle, String) -> Unit,
    onFull: (MessageItem) -> Unit,
    highlighted: Boolean = false,
) {
    // The gesture lives as long as the message's key: it must see the message as it is now (a reaction that came since), not as it was.
    val latest by androidx.compose.runtime.rememberUpdatedState(message)
    val selection = LocalSelection.current
    val selecting = selection?.active == true
    val picked = selecting && message.key in selection!!.keys
    // A tap anywhere on a message offers its actions. (Not `clickable`: that
    // merges the message into one semantics node, and the tests, like a
    // screen reader, want the parts.)
    Row(
        Modifier
            .fillMaxWidth()
            .alpha(if (message.sending) 0.6f else 1f)
            .background(if (highlighted || picked) Palette.Accent.copy(alpha = 0.25f) else if (message.mentionsMe) MentionGold.copy(alpha = 0.10f) else Color.Transparent)
            .then(if (message.mentionsMe) Modifier.drawBehind { drawRect(MentionGold, size = androidx.compose.ui.geometry.Size(2.dp.toPx(), size.height)) }.testTag("mention-bar") else Modifier)
            .pointerInput(message.key, selecting) {
                // A tap does nothing; a double tap starts a reply and a long press opens the sheet (a tap with no handler
                // waits for nothing: neither a long press nor a scroll is delayed).
                detectTapGestures(
                    onDoubleTap = { if (latest.eventId != null && !selecting) model.replyTo(latest) },
                    onLongPress = { if (selecting) selection?.toggle(latest.key) else onActions(latest, true) },
                    onTap = if (selecting) fun(_: androidx.compose.ui.geometry.Offset) { selection?.toggle(latest.key) } else null,
                )
            }
            .semantics { onClick("Message actions") { onActions(latest, true); true } }
            .padding(horizontal = Space.md, vertical = if (message.showHeader) Space.sm else 1.dp),
        horizontalArrangement = Arrangement.spacedBy(Space.md),
    ) {
        if (message.showHeader) {
            Avatar(message.senderAvatar, message.senderName, Size.avatar, Modifier.testTag("msg-avatar"), key = message.senderAvatarKey, userId = message.senderId)
        } else {
            Spacer(Modifier.size(Size.avatar))
        }
        Column(Modifier.weight(1f)) {
            if (message.showHeader) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Bottom) {
                    Text(
                        dev.morpheus.app.Streamer.name(message.senderName),
                        Modifier.testTag("msg-sender"),
                        color = colourOf(message.senderId),
                        fontWeight = FontWeight.Bold,
                   style = MaterialTheme.typography.bodyLarge,
               )
                    Text(whenSent(message.timestampMs.toLong(), rememberNow()), Modifier.testTag("msg-time"), color = Palette.Muted, style = MaterialTheme.typography.bodySmall)
                }
            }
            message.replyTo?.let { reply ->
                Row(
                    Modifier
                        .padding(vertical = 2.dp)
                        .clip(RoundedCornerShape(Radius.sm))
                        .background(Palette.Raised)
                        .drawBehind { drawRect(Palette.Accent, size = androidx.compose.ui.geometry.Size(4.dp.toPx(), size.height)) }
                        .padding(start = 12.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.xs),
                ) {
                    GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_reply, size = 20.dp, tint = readable(MaterialTheme.colorScheme.onSurfaceVariant, Palette.Raised))
                    Text(
                        "${dev.morpheus.app.Streamer.name(reply.senderName ?: "")}: ${reply.summary ?: "…"}",
                        Modifier.testTag("msg-reply"),
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
            when (val body = message.body) {
                is MessageBody.Text -> {
                    // A very long message shows its first lines and a bar that opens the whole text on a page (#408).
                    val shown = message.collapsedLines
                    LinkedText(body, message.edited, message.sending, maxLines = shown?.toInt() ?: Int.MAX_VALUE, lines = message.lines)
                    if (shown != null) {
                        TextButton({ onFull(message) }, Modifier.testTag("show-more")) { Text(t("Show more")) }
                    }
                }
                is MessageBody.Other -> StateOrText(body.summary, message.state)
                is MessageBody.Image -> PictureBody(body, onImage) { onActions(latest, true) }
                is MessageBody.File -> if (body.kind == FileKind.VIDEO) VideoCard(body) { onActions(latest, true) } else Row(
                    Modifier
                        .padding(vertical = 2.dp)
                        .background(Palette.Raised, RoundedCornerShape(8.dp))
                        .pointerInput(body) { detectTapGestures { onFile(body.media, body.name) } }
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.sm),
                ) {
                    GlyphIcon(
                        when (body.kind) { FileKind.VIDEO -> dev.morpheus.app.R.drawable.ic_ms_play_arrow; FileKind.AUDIO -> dev.morpheus.app.R.drawable.ic_ms_equalizer; else -> dev.morpheus.app.R.drawable.ic_ms_attach_file },
                        size = 20.dp,
                    )
                    Text(body.name + (size(body.size).let { if (it.isEmpty()) "" else " ($it)" }), Modifier.testTag("msg-file"), style = MaterialTheme.typography.bodyLarge)
                }
            }
            message.thread?.let { chip ->
                Text(
                    chip.count + (chip.latest?.let { " $it" } ?: ""),
                    Modifier
                        .padding(top = 4.dp)
                        .pointerInput(message.key) { detectTapGestures { message.eventId?.let(onThread) } }
                        .semantics { onClick("Open thread") { message.eventId?.let(onThread); true } }
                        .testTag("thread-chip"),
                    color = Palette.Accent,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            if (message.readBy.isNotEmpty()) ReadReceipts(message.readBy)
            val dismissed by model.dismissed.collectAsState()
            val url = message.previewUrl
            val eventId = message.eventId
            if (url != null && eventId != null && model.previewsAllowed && eventId !in dismissed) {
                PreviewCardView(model, url, eventId)
            }
            if (message.reactions.isNotEmpty()) {
                Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (reaction in message.reactions) {
                        Row(
                            Modifier
                                .border(1.dp, if (reaction.mine) Palette.Accent else Palette.Raised, RoundedCornerShape(12.dp))
                                .background(if (reaction.mine) Palette.Accent.copy(alpha = 0.2f) else Color.Transparent, RoundedCornerShape(12.dp))
                                .pointerInput(reaction.key, message.key) {
                                    detectTapGestures { message.eventId?.let { model.react(it, reaction.key) } }
                                }
                                .padding(horizontal = 12.dp, vertical = 4.dp)
                                .testTag("reaction-${reaction.key}"),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(4.dp),
                        ) {
                            val emoji = reaction.emoji
                            if (emoji != null) EmoteImage(emoji, Modifier.size(20.dp), "reaction-emote")
                            Text(if (emoji != null) "${reaction.count}" else "${reaction.key} ${reaction.count}", style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                }
            }
        }
        if (selecting) SelectMark(picked)
        message.shield?.let { MessageShield(it) }
    }
}

@Composable
// A long press on the picture opens the message sheet like anywhere on the message (the picture takes its own taps).
private fun PictureBody(body: MessageBody.Image, onImage: (MediaHandle) -> Unit, onLongPress: () -> Unit) {
    val bitmap by produceState<ImageBitmap?>(null, body.media.key()) {
        value = Images.load(body.media, 720, 720)
    }
    val shape = RoundedCornerShape(8.dp)
    val ratio = if ((body.width ?: 0u) > 0u && (body.height ?: 0u) > 0u) body.width!!.toFloat() / body.height!!.toFloat() else 1.5f
    val loaded = bitmap
    if (loaded != null) {
        val frame = Modifier
            .padding(top = 4.dp)
            .widthIn(max = 280.dp)
            .aspectRatio(ratio.coerceIn(0.4f, 3f))
            .pointerInput(body.media.key()) { detectTapGestures(onLongPress = { onLongPress() }) { onImage(body.media) } }
            .semantics { onClick("Open picture") { onImage(body.media); true } }
            .testTag("msg-image")
        if (body.animated) AnimatedPicture(body.media, body.caption, frame) else Image(loaded, body.caption, frame, contentScale = ContentScale.Fit)
    } else {
        Box(
            Modifier.padding(top = 4.dp).widthIn(max = 280.dp).aspectRatio(ratio.coerceIn(0.4f, 3f)).background(Palette.Raised, shape).testTag("msg-image-loading"),
        )
    }
}

@Composable
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
private fun Composer(
    model: RoomModel, draftKey: String, editing: MessageItem?, focus: androidx.compose.ui.focus.FocusRequester,
    title: String?, dm: Boolean, encrypted: Boolean, room: RoomItem, server: uniffi.morpheus_ffi.ScopeItem?, onComposing: (Boolean) -> Unit, onEmpty: (Boolean) -> Unit, onAttach: () -> Unit, onSend: (String) -> Unit,
) {
    // What is typed is kept with its cursor or selected range for this chat or thread (#460), and comes back when it is opened again.
    var field by remember(draftKey) { mutableStateOf(Drafts.get(draftKey)) }
    val current by androidx.compose.runtime.rememberUpdatedState(field)
    DisposableEffect(draftKey) { onDispose { Drafts.put(draftKey, current) } }
    val text = field.text
    fun setText(value: String) { field = androidx.compose.ui.text.input.TextFieldValue(value, androidx.compose.ui.text.TextRange(value.length)) }
    var panel by remember { mutableStateOf(false) }
    // Focused or with text: the button row is out of the way (#502). The keyboard closing (back) while the box keeps its focus lets the row return
    // (the focus stays, so a selection in the draft is kept); the keyboard coming up again, or a change of focus, takes it away again.
    var focused by remember { mutableStateOf(false) }
    val keyboardUp = androidx.compose.foundation.layout.WindowInsets.isImeVisible
    // How tall the keyboard was (above the navigation bar): the emoji panel that takes its place is as tall, so nothing jumps (#484).
    val density = androidx.compose.ui.platform.LocalDensity.current
    val imeHeight: Int = androidx.compose.foundation.layout.WindowInsets.ime.getBottom(density) - androidx.compose.foundation.layout.WindowInsets.navigationBars.getBottom(density)
    LaunchedEffect(imeHeight) { if (imeHeight > KeyboardHeight.px) KeyboardHeight.px = imeHeight }
    val panelHeight = with(density) { (if (KeyboardHeight.px > 0) KeyboardHeight.px.toDp() else 300.dp) }
    var keyboardWasUp by remember { mutableStateOf(false) }
    var dismissed by remember { mutableStateOf(false) }
    LaunchedEffect(keyboardUp, focused) {
        if (keyboardUp) { keyboardWasUp = true; dismissed = false }
        else if (keyboardWasUp) { keyboardWasUp = false; dismissed = focused }
        if (!focused) dismissed = false
    }
    val composing = (focused && !dismissed) || text.isNotEmpty() || panel
    val composingChanged by androidx.compose.runtime.rememberUpdatedState(onComposing)
    LaunchedEffect(composing, text) { composingChanged(composing) }
    val emptyChanged by androidx.compose.runtime.rememberUpdatedState(onEmpty)
    LaunchedEffect(text.isEmpty()) { emptyChanged(text.isEmpty()) }
    var suggestions by remember { mutableStateOf<List<SuggestionItem>>(emptyList()) }
    LaunchedEffect(text) { suggestions = model.suggest(text) }
    LaunchedEffect(text) { if (text.isNotBlank()) model.typing() }
    LaunchedEffect(editing?.key) {
        if (editing != null) setText((editing.body as? MessageBody.Text)?.text.orEmpty())
    }
    Column(Modifier.background(Palette.Background)) {
    SuggestionStrip(suggestions) { setText(it.draftAfter); suggestions = emptyList() }
    // One rounded filled field (attach inside it on the left, emoji on the right) with the send button beside it, 8 dp from the
    // edges and from each other; every colour on the field is read against the field's fill (the contrast rule, #266).
    val scheme = MaterialTheme.colorScheme
    val fill = ComposerColours.fill(scheme)
    val ink = ComposerColours.text(scheme)
    val hint = ComposerColours.hint(scheme)
    Row(
        Modifier.fillMaxWidth().padding(horizontal = Space.sm, vertical = Space.sm),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(Space.sm),
    ) {
        // The pill: a plus at its start, the text, the emoji button at its end. The buttons are 48 dp circles centred on the pill's end caps, inset by
        // the concentric rule's R - r (#504, view_model::concentric through ffi), so the corner curves run parallel to the buttons.
        val inset = remember { uniffi.morpheus_ffi.concentricEndCapInset(28f, Size.target.value).dp }
        val keyboard = androidx.compose.ui.platform.LocalSoftwareKeyboardController.current
        val touches = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
        val touching by touches.collectIsPressedAsState()
        // A touch on the box while the emoji panel stands in the keyboard's place brings the keyboard back and closes the panel (#484).
        LaunchedEffect(touching) { if (touching && panel) { panel = false; keyboard?.show() } }
        Row(Modifier.weight(1f).heightIn(min = 56.dp, max = 160.dp).clip(RoundedCornerShape(28.dp)).background(fill).testTag("composer-field"), verticalAlignment = Alignment.Bottom) {
        IconAction(dev.morpheus.app.R.drawable.ic_ms_add, t("Attach a file"), "attach", onAttach, Modifier.padding(start = inset, bottom = inset), tint = hint)
        androidx.compose.material3.TextField(
            field, { field = it },
            Modifier.weight(1f).heightIn(min = 56.dp, max = 160.dp).focusRequester(focus).onFocusChanged { focused = it.isFocused }.testTag("composer"),
            interactionSource = touches,
            // The chat's name is the placeholder, and floats above what is typed (#502): a lock before it when encrypted, a # for a channel that is not.
            label = { ChatLabel(title, dm, encrypted, hint, room, server) },
            maxLines = 5,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            shape = RoundedCornerShape(28.dp),
            textStyle = MaterialTheme.typography.bodyLarge,
            colors = androidx.compose.material3.TextFieldDefaults.colors(
                focusedContainerColor = Color.Transparent, unfocusedContainerColor = Color.Transparent, disabledContainerColor = Color.Transparent,
                focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent, disabledIndicatorColor = Color.Transparent,
                focusedTextColor = ink, unfocusedTextColor = ink, cursorColor = scheme.primary,
                focusedPlaceholderColor = hint, unfocusedPlaceholderColor = hint, focusedLabelColor = hint, unfocusedLabelColor = hint,
                focusedLeadingIconColor = hint, unfocusedLeadingIconColor = hint, focusedTrailingIconColor = hint, unfocusedTrailingIconColor = hint,
            ),
        )
        androidx.compose.material3.IconButton(
            // The panel takes the keyboard's place (the keyboard goes, the panel is as tall as it was); the box again brings the keyboard back.
            { if (panel) { panel = false; keyboard?.show() } else { panel = true; keyboard?.hide() } },
            Modifier.padding(end = inset, bottom = inset).size(Size.target).semantics { contentDescription = t("Emoji") }.testTag("emoji-button"),
        ) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_mood, tint = hint) }
        }
        // Send is there only when there is something to send, and comes and goes on the motion tokens (#484).
        androidx.compose.animation.AnimatedVisibility(
            text.isNotBlank(),
            enter = androidx.compose.animation.scaleIn(androidx.compose.animation.core.tween(Motion.Short4, easing = Motion.EmphasizedDecelerate)) + androidx.compose.animation.fadeIn(androidx.compose.animation.core.tween(Motion.Short4)) + androidx.compose.animation.expandHorizontally(androidx.compose.animation.core.tween(Motion.Short4, easing = Motion.EmphasizedDecelerate)),
            exit = androidx.compose.animation.scaleOut(androidx.compose.animation.core.tween(Motion.Short3, easing = Motion.EmphasizedAccelerate)) + androidx.compose.animation.fadeOut(androidx.compose.animation.core.tween(Motion.Short3)) + androidx.compose.animation.shrinkHorizontally(androidx.compose.animation.core.tween(Motion.Short3, easing = Motion.EmphasizedAccelerate)),
        ) {
            androidx.compose.material3.FilledIconButton(
                { onSend(text.trim()); setText(""); model.stopTyping() },
                // Level with a one-line field (56 dp tall): 4 dp up from its bottom.
                Modifier.padding(bottom = Space.xs).size(Size.target).semantics { contentDescription = t("Send") }.testTag("send"),
                colors = androidx.compose.material3.IconButtonDefaults.filledIconButtonColors(containerColor = scheme.primary, contentColor = scheme.onPrimary),
            ) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_send, size = 24.dp) }
        }
    }
    if (panel) EmojiPanel(model, panelHeight) { setText(withEmoji(text, it)) }
    }
}

/** The chat's name as the composer's label: a picture or server icon, a lock (open when not encrypted), then the name. */
@Composable
private fun ChatLabel(title: String?, dm: Boolean, encrypted: Boolean, tint: Color, room: RoomItem, server: uniffi.morpheus_ffi.ScopeItem?) {
    // (A node of its own: a text field merges what it holds into itself, except the parts that merge their own, so the tests and a screen reader find the name.)
    Row(Modifier.semantics(mergeDescendants = true) { }.testTag(if (title != null) "room-title" else "composer-label"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
        if (title != null) {
            // The picture (a DM) or the server's squircle (a channel), then a lock or an open lock, then the name; no #.
            Box(Modifier.clearAndSetSemantics { }) {
                if (dm) Avatar(room.avatar, title, 16.dp, key = room.avatarKey)
                else if (server != null) ServerIcon(server.avatar, server.title, 16.dp, key = server.avatarKey)
            }
            GlyphIcon(
                if (encrypted) dev.morpheus.app.R.drawable.ic_ms_lock else dev.morpheus.app.R.drawable.ic_ms_lock_open,
                Modifier.semantics(mergeDescendants = true) { }.testTag(if (encrypted) "room-title-lock" else "room-title-lock-open"), size = 16.dp, tint = tint,
            )
        }
        Text(title ?: t("Message"), maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/**
 * The composer and the button row under it stand on one surface (the background colour) that reaches the edges and the bottom: the row has no fill of
 * its own, so while it slides away or back there is no band of another colour behind it (#534).
 */
@Composable
internal fun ComposerDock(content: @Composable () -> Unit) {
    Column(Modifier.fillMaxWidth().background(Palette.Background)) { content() }
}

/** Call, Search, Pins and Info as a row of rounded buttons under the composer (#502). It slides away on the motion tokens (emphasized, medium 2 in and short 4 out). */
@Composable
internal fun ChatActions(visible: Boolean, onCall: () -> Unit, onSearch: () -> Unit, onPins: () -> Unit, onInfo: () -> Unit) {
    androidx.compose.animation.AnimatedVisibility(
        visible,
        enter = androidx.compose.animation.slideInVertically(androidx.compose.animation.core.tween(Motion.Medium2, easing = Motion.EmphasizedDecelerate)) { it } +
            androidx.compose.animation.expandVertically(androidx.compose.animation.core.tween(Motion.Medium2, easing = Motion.EmphasizedDecelerate)),
        exit = androidx.compose.animation.slideOutVertically(androidx.compose.animation.core.tween(Motion.Short4, easing = Motion.EmphasizedAccelerate)) { it } +
            androidx.compose.animation.shrinkVertically(androidx.compose.animation.core.tween(Motion.Short4, easing = Motion.EmphasizedAccelerate)),
    ) {
        Row(Modifier.fillMaxWidth().padding(horizontal = Space.sm).padding(bottom = Space.sm), horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
            ChatAction(dev.morpheus.app.R.drawable.ic_ms_call, t("Call"), "call-button", onCall, Modifier.weight(1f))
            ChatAction(dev.morpheus.app.R.drawable.ic_ms_search, t("Search"), "search-button", onSearch, Modifier.weight(1f))
            ChatAction(dev.morpheus.app.R.drawable.ic_ms_keep, t("Pins"), "pinned-button", onPins, Modifier.weight(1f))
            ChatAction(dev.morpheus.app.R.drawable.ic_ms_info, t("Info"), "room-info-button", onInfo, Modifier.weight(1f))
        }
    }
}

/** One of the row's buttons: a Material 3 tonal button, a pill (its radius is half its 48 dp height: the concentric rule's end cap), icon and label. */
@Composable
private fun ChatAction(@androidx.annotation.DrawableRes icon: Int, label: String, tag: String, onClick: () -> Unit, modifier: Modifier) {
    androidx.compose.material3.FilledTonalButton(
        onClick, modifier.height(Size.target).testTag(tag), shape = CircleShape,
        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = Space.sm),
    ) {
        GlyphIcon(icon, size = 20.dp)
        Spacer(Modifier.width(Space.xs))
        Text(label, maxLines = 1, overflow = TextOverflow.Clip, style = MaterialTheme.typography.labelMedium)
    }
}

/**
 * A room as the phone has it: the chat alone, and a swipe either way takes it back to its list (the DMs for a DM, Servers for a
 * channel), the chat following the finger off the screen (#469, #464). A thread has no swipe. Nothing opens the member list by swipe.
 */
@Composable
private fun RoomPages(app: AppModel, room: RoomItem, panes: Boolean, isDm: Boolean, chat: @Composable () -> Unit) {
    if (!panes) {
        chat()
        return
    }
    var leaveDrag by remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    var leaveLock by remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    val haptic = androidx.compose.ui.platform.LocalView.current
    val flick = remember { FlickTracker(haptic) }
    val dragScope = androidx.compose.runtime.rememberCoroutineScope()
    val keyboard = androidx.compose.ui.platform.LocalSoftwareKeyboardController.current
    val focusManager = androidx.compose.ui.platform.LocalFocusManager.current
    Box(Modifier.fillMaxSize().testTag("room-pager").pointerInput(Unit) {
        detectHorizontalDragGestures(
            onDragStart = { leaveDrag = 0f; leaveLock = 0f; flick.reset(); app.slide.dragging = true },
            onDragEnd = {
                if (flick.commits(leaveDrag, size.width.toFloat())) {
                    // Put the keyboard away as the chat goes.
                    focusManager.clearFocus()
                    keyboard?.hide()
                    dev.morpheus.app.AppState.imeHiddenAt = android.os.SystemClock.uptimeMillis()
                    dev.morpheus.app.AppState.imeHidden++
                    app.selectTab(if (uniffi.morpheus_ffi.chatListIsDms(isDm)) dev.morpheus.app.Tab.Dms else dev.morpheus.app.Tab.Servers)
                } else app.slide.leaveSign = 0
                app.slide.dragging = false
            },
            onDragCancel = { leaveDrag = 0f; app.slide.leaveSign = 0; app.slide.dragging = false },
        ) { change, amount ->
            // Reversing only undoes the drag; it never crosses into the other way (#525).
            leaveLock = uniffi.morpheus_ffi.swipeDragLock(leaveLock, amount)
            val was = leaveDrag
            leaveDrag = uniffi.morpheus_ffi.swipeDragTotal(leaveDrag, amount, leaveLock)
            flick.add(change, leaveDrag - was)
            // The chat goes the way the finger goes, whichever that is.
            if (leaveDrag != 0f) app.slide.leaveSign = if (leaveDrag < 0f) -1 else 1
            dragScope.launch { app.slide.f.snapTo((1f - kotlin.math.abs(leaveDrag) / size.width).coerceIn(0f, 1f)) }
        }
    }.pointerInput(Unit) {
        // The finger leaving the chat: what the keyboard's lags are measured from (the debug bridge reports them, #379).
        awaitPointerEventScope {
            while (true) {
                val event = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Initial)
                if (event.changes.none { it.pressed }) dev.morpheus.app.AppState.imeLiftAt = android.os.SystemClock.uptimeMillis()
            }
        }
    }) { chat() }
}

/** The unsent text of each chat and thread, with the cursor or selection it was left with (kept while the app runs). */
private object Drafts {
    private val kept = HashMap<String, androidx.compose.ui.text.input.TextFieldValue>()
    fun get(key: String) = kept[key] ?: androidx.compose.ui.text.input.TextFieldValue("")
    fun put(key: String, value: androidx.compose.ui.text.input.TextFieldValue) { if (value.text.isEmpty()) kept.remove(key) else kept[key] = value }
}

/** The bar over the message box that says what it is for (a reply, an edit): the screen's width, an icon, one line, a close button. */
@Composable
internal fun ComposerBanner(@androidx.annotation.DrawableRes icon: Int, text: String, barTag: String, textTag: String, closeDescription: String, closeTag: String, onClose: () -> Unit) {
    val fill = ComposerColours.fill(MaterialTheme.colorScheme)
    val ink = ComposerColours.hint(MaterialTheme.colorScheme)
    Row(
        Modifier.fillMaxWidth().testTag(barTag).background(fill).padding(start = Space.lg),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.sm),
    ) {
        GlyphIcon(icon, size = 20.dp, tint = ink)
        Text(text, Modifier.weight(1f).testTag(textTag), maxLines = 1, overflow = TextOverflow.Ellipsis, color = ink, style = MaterialTheme.typography.bodyMedium)
        IconAction(dev.morpheus.app.R.drawable.ic_ms_close, closeDescription, closeTag, onClose, tint = ink)
    }
}

/**
 * Who has read up to a message (#483): a row of small pictures under it, overlapping, at the right, with "+N" past a few. Its description says "Seen by ..."
 * (what a screen reader and the tests read). A tap lists the names, one under the other; another tap folds it.
 */
@Composable
private fun ReadReceipts(readers: List<String>) {
    val names by People.names.collectAsState()
    val avatars by People.avatars.collectAsState()
    readers.forEach { People.want(it) }
    var open by remember { mutableStateOf(false) }
    val seen = t("Seen by %{names}", "names" to readers.joinToString(", ") { People.nameOf(it, names) })
    val face = Size.avatarSm
    val shown = readers.take(MAX_FACES)
    Column(Modifier.fillMaxWidth().padding(top = Space.xs), horizontalAlignment = Alignment.End) {
        Row(
            Modifier.heightIn(min = Size.target).clip(CircleShape).pointerInput(readers) { detectTapGestures { open = !open } }.semantics { contentDescription = seen; onClick("Show who has seen it") { open = !open; true } }.testTag("read-by"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(-Space.xs),
        ) {
            // Overlapping by 4 dp; a ring in the page's colour parts them.
            shown.forEach { id ->
                Avatar(avatars[id], People.nameOf(id, names), face, Modifier.border(1.dp, MaterialTheme.colorScheme.background, CircleShape), key = null)
            }
            if (readers.size > shown.size) Text("+${readers.size - shown.size}", Modifier.padding(start = Space.xs), color = Palette.Muted, style = MaterialTheme.typography.labelSmall)
        }
        if (open) {
            Column(Modifier.testTag("read-by-list"), horizontalAlignment = Alignment.End) {
                readers.forEach { id ->
                    Row(Modifier.padding(top = Space.xs), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                        Text(People.nameOf(id, names), Modifier.testTag("read-by-$id"), color = Palette.Muted, style = MaterialTheme.typography.labelMedium)
                        Avatar(avatars[id], People.nameOf(id, names), face, key = null, userId = id)
                    }
                }
            }
        }
    }
}

private const val MAX_FACES = 5

/** The tallest the keyboard has been in this run, in px above the navigation bar: what the emoji panel is as tall as. */
private object KeyboardHeight {
    var px = 0
}
