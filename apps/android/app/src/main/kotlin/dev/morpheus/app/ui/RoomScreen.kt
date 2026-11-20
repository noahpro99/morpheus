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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
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
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.RoomModel
import uniffi.morpheus_ffi.MediaHandle
import uniffi.morpheus_ffi.MessageBody
import uniffi.morpheus_ffi.MessageItem
import uniffi.morpheus_ffi.RoomItem
import uniffi.morpheus_ffi.TimelineRow

private val quickReactions = listOf("👍", "❤️", "😂", "🎉", "😮")

/** A room: its messages (newest at the bottom), a composer, and what a tap on a message offers. */
@Composable
fun RoomScreen(app: AppModel, room: RoomItem) {
    val model = remember(room.id) { RoomModel(app.morpheus, room) }
    DisposableEffect(model) { onDispose { model.close() } }
    val rows by model.rows.collectAsState()
    val replyingTo by model.replyingTo.collectAsState()
    val editing by model.editing.collectAsState()
    var actions by remember { mutableStateOf<MessageItem?>(null) }
    var viewing by remember { mutableStateOf<MediaHandle?>(null) }

    BackHandler {
        when {
            viewing != null -> viewing = null
            actions != null -> actions = null
            editing != null -> model.edit(null)
            else -> app.closeRoom()
        }
    }

    // What is read is what is in front of the person: the newest message.
    val newest = (rows.lastOrNull { it is TimelineRow.Message } as? TimelineRow.Message)?.message?.key
    LaunchedEffect(newest) { if (newest != null) model.markRead() }

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize().background(Palette.Surface)) {
            Row(
                Modifier.fillMaxWidth().background(Palette.Background).padding(horizontal = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextButton({ app.closeRoom() }, Modifier.testTag("back")) { Text("‹", fontSize = 28.sp) }
                Text(
                    room.name,
                    Modifier.testTag("room-title"),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Timeline(model, rows, Modifier.weight(1f), onActions = { actions = it }, onImage = { viewing = it })
            replyingTo?.let { reply ->
                Row(
                    Modifier.fillMaxWidth().background(Palette.Raised).padding(start = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        "Replying to ${reply.senderName}: ${summary(reply.body)}",
                        Modifier.weight(1f).testTag("reply-banner"),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        color = Palette.Muted,
                        fontSize = 13.sp,
                    )
                    TextButton({ model.replyTo(null) }, Modifier.testTag("reply-cancel")) { Text("✕") }
                }
            }
            editing?.let {
                Row(
                    Modifier.fillMaxWidth().background(Palette.Raised).padding(start = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Editing your message", Modifier.weight(1f).testTag("edit-banner"), color = Palette.Muted, fontSize = 13.sp)
                    TextButton({ model.edit(null) }, Modifier.testTag("edit-cancel")) { Text("✕") }
                }
            }
            val problem by model.error.collectAsState()
            problem?.let {
                Text(it, Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("room-error"), color = Palette.Danger, fontSize = 13.sp)
            }
            Composer(editing) { model.send(it) }
        }
        actions?.let { message ->
            Actions(
                message,
                onReact = { key ->
                    message.eventId?.let { model.react(it, key) }
                    actions = null
                },
                onReply = {
                    model.replyTo(message)
                    actions = null
                },
                onEdit = {
                    model.edit(message)
                    actions = null
                },
                onDelete = {
                    message.eventId?.let { model.delete(it) }
                    actions = null
                },
                onDismiss = { actions = null },
            )
        }
        viewing?.let { ImageViewer(it) { viewing = null } }
    }
}

private fun summary(body: MessageBody): String = when (body) {
    is MessageBody.Text -> body.text
    is MessageBody.Image -> "🖼 ${body.caption}"
    is MessageBody.Other -> body.summary
}

@Composable
private fun Timeline(
    model: RoomModel,
    rows: List<TimelineRow>,
    modifier: Modifier,
    onActions: (MessageItem) -> Unit,
    onImage: (MediaHandle) -> Unit,
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

    LazyColumn(modifier.testTag("timeline"), state = state, reverseLayout = true) {
        items(items, key = { rowKey(it) }) { row ->
            when (row) {
                is TimelineRow.Message -> MessageRow(row.message, model, onActions, onImage)
                is TimelineRow.Date -> Text(
                    row.label,
                    Modifier.fillMaxWidth().padding(vertical = 12.dp).testTag("date-divider"),
                    textAlign = TextAlign.Center,
                    color = Palette.Muted,
                    fontSize = 12.sp,
                )
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
    onActions: (MessageItem) -> Unit,
    onImage: (MediaHandle) -> Unit,
) {
    // A tap anywhere on a message offers its actions. (Not `clickable`: that
    // merges the message into one semantics node, and the tests, like a
    // screen reader, want the parts.)
    Row(
        Modifier
            .fillMaxWidth()
            .alpha(if (message.sending) 0.6f else 1f)
            .pointerInput(message.key) { detectTapGestures { onActions(message) } }
            .semantics { onClick("Message actions") { onActions(message); true } }
            .padding(horizontal = 12.dp, vertical = if (message.showHeader) 6.dp else 1.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (message.showHeader) {
            Avatar(message.senderAvatar, message.senderName, 36.dp)
        } else {
            Spacer(Modifier.size(36.dp))
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
                    Text(message.time, Modifier.testTag("msg-time"), color = Palette.Muted, fontSize = 11.sp)
                }
            }
            message.replyTo?.let { reply ->
                Text(
                    "↩ ${reply.senderName ?: ""}: ${reply.summary ?: "…"}",
                    Modifier
                        .padding(vertical = 2.dp)
                        .background(Palette.Raised, RoundedCornerShape(6.dp))
                        .padding(horizontal = 8.dp, vertical = 2.dp)
                        .testTag("msg-reply"),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    color = Palette.Muted,
                    fontSize = 13.sp,
                )
            }
            when (val body = message.body) {
                is MessageBody.Text -> Text(
                    if (message.edited) body.text + "  (edited)" else body.text,
                    Modifier.testTag("msg-body"),
                )
                is MessageBody.Other -> Text(body.summary, Modifier.testTag("msg-body"), color = Palette.Muted)
                is MessageBody.Image -> PictureBody(body, onImage)
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
                                .padding(horizontal = 8.dp, vertical = 3.dp)
                                .testTag("reaction-${reaction.key}"),
                            fontSize = 13.sp,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun PictureBody(body: MessageBody.Image, onImage: (MediaHandle) -> Unit) {
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
                .pointerInput(body.media.key()) { detectTapGestures { onImage(body.media) } }
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
private fun Composer(editing: MessageItem?, onSend: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    LaunchedEffect(editing?.key) {
        if (editing != null) text = (editing.body as? MessageBody.Text)?.text.orEmpty()
    }
    Row(
        Modifier.fillMaxWidth().background(Palette.Background).padding(8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedTextField(
            text, { text = it },
            Modifier.weight(1f).heightIn(max = 140.dp).testTag("composer"),
            placeholder = { Text("Message") },
            maxLines = 5,
        )
        Button(
            { onSend(text.trim()); text = "" },
            Modifier.testTag("send"),
            enabled = text.isNotBlank(),
        ) { Text("Send") }
    }
}

@Composable
private fun Actions(
    message: MessageItem,
    onReact: (String) -> Unit,
    onReply: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
    onDismiss: () -> Unit,
) {
    Box(
        Modifier
            .fillMaxSize()
            .background(Color(0x99000000))
            .pointerInput(Unit) { detectTapGestures { onDismiss() } }
            .testTag("actions-scrim"),
    ) {
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(Palette.Raised, RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp))
                .pointerInput(Unit) { detectTapGestures { } }
                .padding(16.dp)
                .testTag("actions"),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(message.senderName, color = Palette.Muted, fontSize = 12.sp)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (emoji in quickReactions) {
                    Box(
                        Modifier
                            .size(48.dp)
                            .background(Palette.Surface, RoundedCornerShape(24.dp))
                            .pointerInput(emoji) { detectTapGestures { onReact(emoji) } }
                            .testTag("react-$emoji"),
                        contentAlignment = Alignment.Center,
                    ) { Text(emoji, fontSize = 22.sp) }
                }
            }
            TextButton(onReply, Modifier.testTag("action-reply")) { Text("Reply") }
            val clipboard = LocalClipboardManager.current
            val text = (message.body as? MessageBody.Text)?.text
            if (text != null) {
                TextButton({ clipboard.setText(AnnotatedString(text)); onDismiss() }, Modifier.testTag("action-copy")) { Text("Copy text") }
            }
            if (message.canEdit) TextButton(onEdit, Modifier.testTag("action-edit")) { Text("Edit") }
            if (message.canDelete) {
                var sure by remember { mutableStateOf(false) }
                TextButton({ if (sure) onDelete() else sure = true }, Modifier.testTag("action-delete")) {
                    Text(if (sure) "Tap again to delete" else "Delete", color = Palette.Danger)
                }
            }
        }
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
