package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.Icon
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.semantics.contentDescription
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

/** A room: its messages (newest at the bottom), a composer, and what a tap on a message offers. */
@Composable
fun RoomScreen(app: AppModel, room: RoomItem, threadRoot: String? = null, onClose: () -> Unit = { app.closeRoom() }) {
    val model = remember(room.id, threadRoot) { RoomModel(app.morpheus, room, threadRoot, ready = { app.awaitSession() }) }
    var thread by remember(room.id) { mutableStateOf<String?>(null) }
    DisposableEffect(model) { onDispose { model.close() } }
    val rows by model.rows.collectAsState()
    val replyingTo by model.replyingTo.collectAsState()
    if (rows.any { it is TimelineRow.Message }) androidx.compose.runtime.SideEffect { dev.morpheus.app.AppState.roomDrawn() }
    // A room just joined is listed before its name is known: the title follows the list.
    val sidebar by app.sidebar.collectAsState()
    val title = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == room.id }?.name ?: room.name
    val editing by model.editing.collectAsState()
    var info by remember(room.id) { mutableStateOf(false) }
    var searching by remember(room.id) { mutableStateOf(false) }
    val typingLine by model.typing.collectAsState()
    var viewing by remember { mutableStateOf<MediaHandle?>(null) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch { context.read(uri)?.let { model.sendFile(it.name, it.mime, it.bytes) } }
    }
    if (threadRoot == null) DisposableEffect(model) {
        RoomHooks.attach = { name, mime, bytes -> model.sendFile(name, mime, bytes) }
        onDispose { RoomHooks.attach = null }
    }

    val pager = rememberPagerState(initialPage = 1) { 3 }
    // A swipe onto the chat focuses the composer and shows the keyboard; a swipe off it puts the keyboard away.
    val composerFocus = remember { androidx.compose.ui.focus.FocusRequester() }
    val keyboard = androidx.compose.ui.platform.LocalSoftwareKeyboardController.current
    val focusManager = androidx.compose.ui.platform.LocalFocusManager.current
    val openedBySwipe = remember { app.takeSwipeOpened() }
    // Driven by where the pager is heading, not where it has settled: the keyboard is asked for (or put away) as soon as the
    // swipe is committed, not after the pane has stopped moving, and a swipe that springs back brings it back (#379).
    LaunchedEffect(pager, threadRoot) {
        if (threadRoot != null) return@LaunchedEffect
        var first = true
        androidx.compose.runtime.snapshotFlow { pager.targetPage }.collect { page ->
            if (page == 1) {
                if (!first || openedBySwipe) {
                    try { composerFocus.requestFocus() } catch (_: Exception) {}
                    keyboard?.show()
                    dev.morpheus.app.AppState.imeShownAt = android.os.SystemClock.uptimeMillis()
                    dev.morpheus.app.AppState.imeShown++
                }
            } else {
                focusManager.clearFocus()
                keyboard?.hide()
                dev.morpheus.app.AppState.imeHiddenAt = android.os.SystemClock.uptimeMillis()
                dev.morpheus.app.AppState.imeHidden++
            }
            first = false
        }
    }
    BackHandler {
        when {
            searching -> searching = false
            info -> info = false
            viewing != null -> viewing = null
            editing != null -> model.edit(null)
            pager.currentPage != 1 -> scope.launch { pager.animateScrollToPage(1) }
            else -> onClose()
        }
    }

    // What is read is what is in front of the person: the newest message.
    val newest = (rows.lastOrNull { it is TimelineRow.Message } as? TimelineRow.Message)?.message?.key
    // Only while this room is the one open: one that is sliding out (it stays drawn a moment) reads nothing more.
    val openNow by app.openRoom.collectAsState()
    LaunchedEffect(newest, openNow?.id == room.id) { if (newest != null && threadRoot == null && openNow?.id == room.id) model.markRead() }

    Box(Modifier.fillMaxSize()) {
        if (searching) SearchPanel(model) { id ->
            searching = false
            if (id.isNotEmpty()) model.jumpTo(id)
        }
        else if (info) RoomInfo(app, model, room, title) { info = false }
        else if (thread == null) RoomPages(app, model, room, pager, threadRoot == null) { ScreenScaffold(if (threadRoot == null) "room-screen" else "thread-screen", {
            val inThis = app.calls.snapshot.collectAsState().value?.roomId == room.id
            TopBar(
                if (threadRoot == null) title else t("Thread"),
                backTag = if (threadRoot == null) "back" else "thread-back",
                onBack = { onClose() },
                titleTag = if (threadRoot == null) "room-title" else "thread-title",
                onTitle = if (threadRoot == null) ({ info = true }) else null,
            ) {
                if (threadRoot == null) {
                    IconAction(dev.morpheus.app.R.drawable.ic_lucide_phone, t("Call"), "call-button", { if (inThis) app.calls.show(true) else app.startCall(room.id, title) })
                    IconAction(dev.morpheus.app.R.drawable.ic_lucide_search, t("Search"), "search-button", { searching = true })
                    IconAction(dev.morpheus.app.R.drawable.ic_lucide_info, t("Room details"), "room-info-button", { info = true })
                }
            }
        }) { inner -> Column(Modifier.fillMaxSize().padding(inner)) {
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
                                is uniffi.morpheus_ffi.MenuOutcome.Delete -> message.eventId?.let { model.delete(it) }
                                is uniffi.morpheus_ffi.MenuOutcome.ViewReactions -> app.openPage(dev.morpheus.app.PageRequest("reactions", room.id, message = message))
                                is uniffi.morpheus_ffi.MenuOutcome.Report -> message.eventId?.let { app.openPage(dev.morpheus.app.PageRequest("report", room.id, it)) }
                                is uniffi.morpheus_ffi.MenuOutcome.OpenRoom -> {}
                                else -> {}
                            }
                        },
                    ) { app.morpheus.messageMenuRows(room.id, message) }
                },
                onThread = { thread = it },
                onImage = { viewing = it },
                onFile = { media, name -> scope.launch { context.open(media, name)?.let { model.fail(it) } } },
            )
            typingLine?.let {
                Text(it, Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp).testTag("typing-line"), color = Palette.Muted, fontSize = 12.sp)
            }
            replyingTo?.let { reply ->
                Row(
                    Modifier.fillMaxWidth().background(Palette.Raised).padding(start = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        t("Replying to %{name}: %{text}", "name" to reply.senderName.toString(), "text" to summary(reply.body)),
                        Modifier.weight(1f).testTag("reply-banner"),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        color = Palette.Muted,
                        fontSize = 13.sp,
                    )
                    IconAction(dev.morpheus.app.R.drawable.ic_lucide_x, t("Cancel reply"), "reply-cancel", { model.replyTo(null) }, tint = Palette.Muted)
                }
            }
            editing?.let {
                Row(
                    Modifier.fillMaxWidth().background(Palette.Raised).padding(start = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(t("Editing your message"), Modifier.weight(1f).testTag("edit-banner"), color = Palette.Muted, fontSize = 13.sp)
                    IconAction(dev.morpheus.app.R.drawable.ic_lucide_x, t("Cancel editing"), "edit-cancel", { model.edit(null) }, tint = Palette.Muted)
                }
            }
            // Messages that did not go stay here, to send again or drop (the text is not lost with the composer).
            val unsent by model.failed.collectAsState()
            for (item in unsent) {
                Row(Modifier.fillMaxWidth().background(Palette.Raised).padding(start = Space.lg).testTag("send-failed"), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(t("Not sent: %{text}", "text" to (item.text ?: t("a change"))), Modifier.testTag("send-failed-text"), color = Palette.Danger, style = Type.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        Text(item.reason, color = Palette.Muted, style = Type.caption, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    TextButton({ model.retry(item.id) }, Modifier.testTag("send-retry")) { Text(t("Retry")) }
                    TextButton({ model.discard(item.id) }, Modifier.testTag("send-discard")) { Text(t("Discard")) }
                }
            }
            val problem by model.error.collectAsState()
            // Over the composer, so it stays a line above it (a toast would cover the send button).
            problem?.let {
                Text(it, Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("room-error"), color = MaterialTheme.colorScheme.error, style = Type.secondary)
            }
            Composer(model, editing, composerFocus, onAttach = { picker.launch("*/*") }) { model.send(it) }
        } } }
        viewing?.let { ImageViewer(it) { viewing = null } }
        thread?.let { root ->
            Box(Modifier.fillMaxSize().testTag("thread-panel")) { RoomScreen(app, room, root) { thread = null } }
        }
    }
}

private fun summary(body: MessageBody): String = when (body) {
    is MessageBody.Text -> body.text
    is MessageBody.Image -> "🖼 ${body.caption}"
    is MessageBody.File -> "📎 ${body.name}"
    is MessageBody.Other -> body.summary
}

@Composable
private fun Timeline(
    model: RoomModel,
    rows: List<TimelineRow>,
    modifier: Modifier,
    onActions: (MessageItem, Boolean) -> Unit,
    onThread: (String) -> Unit,
    onImage: (MediaHandle) -> Unit,
    onFile: (MediaHandle, String) -> Unit,
) {
    val state = rememberLazyListState()
    // Newest first: the list is laid out from the bottom.
    val items = remember(rows) { rows.asReversed() }
    val atStart = rows.any { it is TimelineRow.Start }
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
    // A new message at the bottom stays in view when the person is looking at the bottom.
    val newestKey = (items.firstOrNull() as? TimelineRow.Message)?.message?.key
    LaunchedEffect(newestKey) { if (state.firstVisibleItemIndex <= 1) state.animateScrollToItem(0) }
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
    LazyColumn(Modifier.fillMaxSize().testTag("timeline"), state = state, reverseLayout = true) {
        items(items, key = { rowKey(it) }) { row ->
            when (row) {
                is TimelineRow.Message -> if (row.message.senderId in ignored && row.message.key !in revealed) {
                    Text(
                        t("Ignored message · Show"),
                        Modifier.fillMaxWidth().clickable { revealed = revealed + row.message.key }.padding(horizontal = 16.dp, vertical = 8.dp).testTag("ignored-fold"),
                        color = Palette.Muted, fontSize = 13.sp,
                    )
                } else MessageRow(row.message, model, onActions, onThread, onImage, onFile, lit != null && lit == row.message.eventId)
                is TimelineRow.Date -> Row(
                    Modifier.fillMaxWidth().padding(horizontal = Space.lg, vertical = Space.md),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.md),
                ) {
                    Box(Modifier.weight(1f).height(1.dp).background(Palette.Divider))
                    Text(dayLabel(row.timestampMs.toLong(), rememberNow()), Modifier.testTag("date-divider"), color = Palette.Muted, style = Type.caption.copy(fontWeight = FontWeight.SemiBold))
                    Box(Modifier.weight(1f).height(1.dp).background(Palette.Divider))
                }
                TimelineRow.Start -> Text(
                    "This is the beginning of the room.",
                    Modifier.fillMaxWidth().padding(24.dp).testTag("timeline-start"),
                    textAlign = TextAlign.Center,
                    color = Palette.Muted,
                )
                TimelineRow.New -> Text(
                    "NEW",
                    Modifier.fillMaxWidth().padding(vertical = 4.dp).testTag("new-divider"),
                    textAlign = TextAlign.Center,
                    color = Palette.Danger,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Bold,
                )
            }
        }
    }
    if (dividerAt >= 0 && !dividerShowing) {
        val scope = androidx.compose.runtime.rememberCoroutineScope()
        Button(
            { scope.launch { state.animateScrollToItem(dividerAt) } },
            Modifier.align(Alignment.TopCenter).padding(top = 8.dp).testTag("jump-unread"),
        ) { Text(t("Jump to unread")) }
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
    highlighted: Boolean = false,
) {
    // The gesture lives as long as the message's key: it must see the message as it is now (a reaction that came since), not as it was.
    val latest by androidx.compose.runtime.rememberUpdatedState(message)
    // A tap anywhere on a message offers its actions. (Not `clickable`: that
    // merges the message into one semantics node, and the tests, like a
    // screen reader, want the parts.)
    Row(
        Modifier
            .fillMaxWidth()
            .alpha(if (message.sending) 0.6f else 1f)
            .background(if (highlighted) Palette.Accent.copy(alpha = 0.25f) else Color.Transparent)
            .pointerInput(message.key) {
                // A tap does nothing; a double tap starts a reply and a long press opens the sheet (a tap with no handler
                // waits for nothing: neither a long press nor a scroll is delayed).
                detectTapGestures(
                    onDoubleTap = { if (latest.eventId != null) model.replyTo(latest) },
                    onLongPress = { onActions(latest, true) },
                )
            }
            .semantics { onClick("Message actions") { onActions(latest, true); true } }
            .padding(horizontal = Space.md, vertical = if (message.showHeader) Space.sm else 1.dp),
        horizontalArrangement = Arrangement.spacedBy(Space.md),
    ) {
        if (message.showHeader) {
            Avatar(message.senderAvatar, message.senderName, Size.avatar, key = message.senderAvatarKey)
        } else {
            Spacer(Modifier.size(Size.avatar))
        }
        Column(Modifier.weight(1f)) {
            if (message.showHeader) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Bottom) {
                    Text(
                        message.senderName,
                        Modifier.testTag("msg-sender"),
                        color = colourOf(message.senderId),
                        fontWeight = FontWeight.Bold,
                    )
                    Text(whenSent(message.timestampMs.toLong(), rememberNow()), Modifier.testTag("msg-time"), color = Palette.Muted, style = Type.caption)
                }
            }
            message.replyTo?.let { reply ->
                Text(
                    "↩ ${reply.senderName ?: ""}: ${reply.summary ?: "…"}",
                    Modifier
                        .padding(vertical = 2.dp)
                        .clip(RoundedCornerShape(Radius.sm))
                        .background(Palette.Raised)
                        .drawBehind { drawRect(Palette.Accent, size = androidx.compose.ui.geometry.Size(3.dp.toPx(), size.height)) }
                        .padding(start = 12.dp, end = 8.dp, top = 4.dp, bottom = 4.dp)
                        .testTag("msg-reply"),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    color = Palette.Muted,
                    style = Type.secondary,
                )
            }
            when (val body = message.body) {
                is MessageBody.Text -> LinkedText(body, message.edited, message.sending)
                is MessageBody.Other -> Text(body.summary, Modifier.testTag("msg-body"), color = Palette.Muted)
                is MessageBody.Image -> PictureBody(body, onImage) { onActions(latest, true) }
                is MessageBody.File -> Text(
                    (when (body.kind) { FileKind.VIDEO -> "🎞 "; FileKind.AUDIO -> "🎵 "; else -> "📎 " }) +
                        body.name + (size(body.size).let { if (it.isEmpty()) "" else " ($it)" }),
                    Modifier
                        .padding(vertical = 2.dp)
                        .background(Palette.Raised, RoundedCornerShape(8.dp))
                        .pointerInput(body) { detectTapGestures { onFile(body.media, body.name) } }
                        .padding(horizontal = 12.dp, vertical = 8.dp)
                        .testTag("msg-file"),
                )
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
                    fontSize = 13.sp,
                )
            }
            if (message.readBy.isNotEmpty()) {
                val names by People.names.collectAsState()
                message.readBy.forEach { People.want(it) }
                Text(
                    t("Seen by %{names}", "names" to message.readBy.joinToString(", ") { People.nameOf(it, names) }),
                    Modifier.padding(top = 2.dp).testTag("read-by"),
                    color = Palette.Muted, fontSize = 11.sp,
                )
            }
            val dismissed by model.dismissed.collectAsState()
            val url = message.previewUrl
            val eventId = message.eventId
            if (url != null && eventId != null && model.previewsAllowed && eventId !in dismissed) {
                PreviewCardView(model, url, eventId)
            }
            if (message.reactions.isNotEmpty()) {
                Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (reaction in message.reactions) {
                        Text(
                            "${reaction.key} ${reaction.count}",
                            Modifier
                                .border(1.dp, if (reaction.mine) Palette.Accent else Palette.Raised, RoundedCornerShape(10.dp))
                                .background(if (reaction.mine) Palette.Accent.copy(alpha = 0.2f) else Color.Transparent, RoundedCornerShape(10.dp))
                                .pointerInput(reaction.key, message.key) {
                                    detectTapGestures { message.eventId?.let { model.react(it, reaction.key) } }
                                }
                                .padding(horizontal = 10.dp, vertical = 4.dp)
                                .testTag("reaction-${reaction.key}"),
                            style = Type.secondary,
                        )
                    }
                }
            }
        }
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
        Image(
            loaded, body.caption,
            Modifier
                .padding(top = 4.dp)
                .widthIn(max = 280.dp)
                .aspectRatio(ratio.coerceIn(0.4f, 3f))
                .pointerInput(body.media.key()) { detectTapGestures(onLongPress = { onLongPress() }) { onImage(body.media) } }
                .semantics { onClick("Open picture") { onImage(body.media); true } }
                .testTag("msg-image"),
            contentScale = ContentScale.Fit,
        )
    } else {
        Box(
            Modifier.padding(top = 4.dp).widthIn(max = 280.dp).aspectRatio(ratio.coerceIn(0.4f, 3f)).background(Palette.Raised, shape).testTag("msg-image-loading"),
        )
    }
}

@Composable
private fun Composer(model: RoomModel, editing: MessageItem?, focus: androidx.compose.ui.focus.FocusRequester, onAttach: () -> Unit, onSend: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    var panel by remember { mutableStateOf(false) }
    var suggestions by remember { mutableStateOf<List<SuggestionItem>>(emptyList()) }
    LaunchedEffect(text) { suggestions = model.suggest(text) }
    LaunchedEffect(text) { if (text.isNotBlank()) model.typing() }
    LaunchedEffect(editing?.key) {
        if (editing != null) text = (editing.body as? MessageBody.Text)?.text.orEmpty()
    }
    Column(Modifier.background(Palette.Background)) {
    SuggestionStrip(suggestions) { text = it.draftAfter; suggestions = emptyList() }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = Space.sm, vertical = Space.sm),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(Space.xs),
    ) {
        IconAction(dev.morpheus.app.R.drawable.ic_lucide_paperclip, t("Attach a file"), "attach", onAttach, tint = Palette.Muted)
        OutlinedTextField(
            text, { text = it },
            Modifier.weight(1f).heightIn(min = 48.dp, max = 140.dp).focusRequester(focus).testTag("composer"),
            placeholder = { Text(t("Message"), color = Palette.Muted) },
            maxLines = 5,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            shape = RoundedCornerShape(24.dp),
            textStyle = Type.body,
            colors = androidx.compose.material3.OutlinedTextFieldDefaults.colors(
                focusedContainerColor = Palette.Raised, unfocusedContainerColor = Palette.Raised,
                focusedBorderColor = Color.Transparent, unfocusedBorderColor = Color.Transparent,
                focusedTextColor = Palette.Text, unfocusedTextColor = Palette.Text, cursorColor = Palette.Accent,
            ),
            trailingIcon = {
                androidx.compose.material3.IconButton({ panel = !panel }, Modifier.size(40.dp).semantics { contentDescription = t("Emoji") }.testTag("emoji-button")) { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_smile, tint = MaterialTheme.colorScheme.onSurfaceVariant) }
            },
        )
        val canSend = text.isNotBlank()
        androidx.compose.material3.FilledIconButton(
            { onSend(text.trim()); text = ""; model.stopTyping() },
            Modifier.size(Size.target).semantics { contentDescription = t("Send") }.testTag("send"),
            enabled = canSend,
            colors = androidx.compose.material3.IconButtonDefaults.filledIconButtonColors(
                containerColor = MaterialTheme.colorScheme.primary, contentColor = MaterialTheme.colorScheme.onPrimary,
                disabledContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh, disabledContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
            ),
        ) { GlyphIcon(dev.morpheus.app.R.drawable.ic_send, size = 24.dp) }
    }
    if (panel) EmojiPanel(model) { text = if (it.startsWith(":")) withEmoji(text, it) else withEmoji(text, it) }
    }
}

@Composable
private fun ImageViewer(media: MediaHandle, onClose: () -> Unit) {
    val bitmap by produceState<ImageBitmap?>(null, media.key()) { value = Images.load(media, 2048, 2048) }
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .pointerInput(Unit) { detectTapGestures { onClose() } }
            .testTag("image-viewer"),
        contentAlignment = Alignment.Center,
    ) {
        bitmap?.let { Image(it, null, Modifier.fillMaxSize(), contentScale = ContentScale.Fit) }
    }
}

/**
 * A room as Discord's phone app has it: the chat in the middle, the channels one swipe to the right
 * (the rail and the list, to pick another room) and the members one swipe to the left. A thread has no panes.
 */
@Composable
private fun RoomPages(app: AppModel, model: RoomModel, room: RoomItem, pager: PagerState, panes: Boolean, chat: @Composable () -> Unit) {
    if (!panes) {
        chat()
        return
    }
    // A room picked from the channels pane is the chat to show now.
    androidx.compose.runtime.LaunchedEffect(room.id) { if (pager.currentPage != 1) pager.scrollToPage(1) }
    // Quick to settle (a stiff spring, about 200 ms) and quick to switch. One page per gesture, counted from the page the finger went
    // down on, as Discord does (a drag counted from the page nearest at release went two pages, over the chat: #388).
    var startPage by remember { mutableIntStateOf(1) }
    androidx.compose.runtime.LaunchedEffect(pager) {
        pager.interactionSource.interactions.collect {
            if (it is androidx.compose.foundation.interaction.DragInteraction.Start) startPage = pager.currentPage
        }
    }
    androidx.compose.runtime.LaunchedEffect(pager) {
        androidx.compose.runtime.snapshotFlow { pager.currentPage }.collect { dev.morpheus.app.AppState.roomPage = it; dev.morpheus.app.AppState.roomPages.add(it) }
    }
    // The library's own snap ignored a drag of a third of the width and a quick flick of a fifth (it sprang back to the chat), so
    // where the pager goes on release is decided here: one page from where the finger went down, if it moved a fifth of a page, or
    // went a little way at a flick's speed (#318, #388).
    val fling = object : androidx.compose.foundation.gestures.TargetedFlingBehavior {
        override suspend fun androidx.compose.foundation.gestures.ScrollScope.performFling(initialVelocity: Float, onRemainingDistanceUpdated: (Float) -> Unit): Float {
            val pageSize = pager.layoutInfo.pageSize.coerceAtLeast(1)
            val moved = (pager.currentPage + pager.currentPageOffsetFraction) - startPage
            val pagesPerSecond = initialVelocity / pageSize
            val flicked = kotlin.math.abs(pagesPerSecond) >= 0.25f && pagesPerSecond * moved > 0f && kotlin.math.abs(moved) >= 0.04f
            val target = if (kotlin.math.abs(moved) >= 0.2f || flicked) startPage + (if (moved > 0f) 1 else -1) else startPage
            val distance = (target.coerceIn(0, pager.pageCount - 1) - (pager.currentPage + pager.currentPageOffsetFraction)) * pageSize
            var last = 0f
            androidx.compose.animation.core.animate(0f, distance, 0f, androidx.compose.animation.core.spring(dampingRatio = 1f, stiffness = 2500f)) { value, _ ->
                scrollBy(value - last)
                last = value
                onRemainingDistanceUpdated(distance - value)
            }
            return 0f
        }
    }
    HorizontalPager(pager, Modifier.fillMaxSize().testTag("room-pager").pointerInput(Unit) {
        // The finger leaving the pager: what the keyboard's lags are measured from (the debug bridge reports them, #379).
        awaitPointerEventScope {
            while (true) {
                val event = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Initial)
                if (event.changes.none { it.pressed }) dev.morpheus.app.AppState.imeLiftAt = android.os.SystemClock.uptimeMillis()
            }
        }
    }, flingBehavior = fling, key = { it }) { page ->
        when (page) {
            // The Servers tab as it is outside a room: its bar is here, the chat has none.
            0 -> Column(Modifier.fillMaxSize().testTag("pane-channels")) {
                ServersPane(app, { app.open(it) }, Modifier.weight(1f), selectedRoom = room.id)
                BottomBar(app, dev.morpheus.app.Tab.Servers)
            }
            1 -> chat()
            else -> MembersPane(app, model)
        }
    }
}
