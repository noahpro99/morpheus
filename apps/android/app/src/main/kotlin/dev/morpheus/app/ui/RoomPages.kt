package dev.morpheus.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import dev.morpheus.app.RoomHooks
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import dev.morpheus.app.AppModel
import dev.morpheus.app.PageRequest
import dev.morpheus.app.sentence
import dev.morpheus.app.People
import dev.morpheus.app.read
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.Overview
import uniffi.morpheus_ffi.PrivacyInfo
import uniffi.morpheus_ffi.RoomDetails

/** The page a sheet item opened, over whatever is on screen. */
@Composable
@Suppress("ModifierMissing") // a host: it draws whatever page is asked for over the screen, with no layout of its own
fun RoomPageHost(app: AppModel) {
    val request by app.page.collectAsState()
    val page = request ?: return
    BackHandler { app.closePage() }
    androidx.compose.runtime.key(page) {
        SwipeBack(Modifier.fillMaxSize()) {
        when (page.kind) {
            "invite" -> InvitePage(app, page)
            "create" -> CreatePage(app, page)
            "edit" -> EditPage(app, page)
            "privacy" -> PrivacyPage(app, page)
            "overview" -> OverviewPage(app, page)
            "reactions" -> ReactionsPage(app, page)
            "pinned" -> PinnedPage(app, page)
            "full-message" -> FullMessagePage(app, page)
            "select-text" -> SelectTextPage(app, page)
            "report" -> ReportPage(app, page)
            "rename-folder" -> RenameFolderPage(app, page)
            "unlock" -> UnlockPage(app, page)
            "lock-set" -> LockSetPage(app, page)
            "lock-remove" -> LockRemovePage(app, page)
            "leave" -> LeavePage(app, page)
            "category" -> CategoryPage(app, page)
            "create-server" -> CreateServerPage(app)
            "status" -> StatusPage(app)
            "nickname" -> NicknamePage(app, page)
            "profile" -> ProfilePage(app, page)
            "full-profile" -> FullProfilePage(app, page)
            "report-user" -> ReportUserPage(app, page)
        }
        }
    }
}

@Composable
private fun PageFrame(app: AppModel, title: String, tag: String, content: @Composable () -> Unit) {
    ScreenScaffold(tag, { TopBar(t(title)) }) { inner ->
        Column(Modifier.fillMaxSize().padding(inner).verticalScroll(rememberScrollState()).padding(Space.lg), verticalArrangement = Arrangement.spacedBy(Space.md)) { content() }
    }
}

/** What a form says after it ran: a note that it worked, or why it did not. */
@Composable
private fun Note(ok: String?, error: String?) {
    // Said in a toast: that it worked, or why it did not.
    LaunchedEffect(ok) { ok?.let { Toasts.say(it, "page-note") } }
    LaunchedEffect(error) { error?.let { Toasts.say(it, "page-error", long = true) } }
}

@Composable
private fun InvitePage(app: AppModel, page: PageRequest) {
    var typed by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Invite people", "page-invite") {
        Text(t("Enter a full Matrix ID like @name:server"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(typed, { typed = it }, Modifier.fillMaxWidth().testTag("invite-input"), singleLine = true, placeholder = { Text("@name:server") })
        Button({
            scope.launch {
                try { ok = app.morpheus.invitePeople(page.roomId, typed); error = null; typed = "" } catch (e: Exception) { error = e.message; ok = null }
            }
        }, Modifier.testTag("invite-send")) { Text(t("Invite")) }
        Note(ok, error)
    }
}

@Composable
private fun CreatePage(app: AppModel, page: PageRequest) {
    var name by remember { mutableStateOf("") }
    var topic by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Create room", "page-create") {
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("create-name"), singleLine = true, label = { Text(t("Name")) })
        OutlinedTextField(topic, { topic = it }, Modifier.fillMaxWidth().testTag("create-topic"), label = { Text(t("Topic")) })
        Button({
            scope.launch {
                try {
                    val id = app.morpheus.createChannelIn(name, topic, page.roomId.ifEmpty { null })
                    app.closePage()
                    app.openRoomWhenKnown(id)
                } catch (e: Exception) { error = e.message }
            }
        }, Modifier.testTag("create-submit")) { Text(t("Create room")) }
        Note(null, error)
    }
}

@Composable
private fun EditPage(app: AppModel, page: PageRequest) {
    var details by remember { mutableStateOf<RoomDetails?>(null) }
    var name by remember { mutableStateOf("") }
    var topic by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val picker = rememberPicturePicker { square ->
        scope.launch {
            try {
                app.morpheus.setRoomPicture(page.roomId, "picture.png", square)
                ok = t("Picture changed."); error = null
            } catch (e: Exception) { error = e.message }
        }
    }
    LaunchedEffect(Unit) {
        try { details = app.morpheus.roomDetails(page.roomId).also { name = it.name; topic = it.topic } } catch (e: Exception) { error = e.message }
    }
    PageFrame(app, if (details?.isSpace == true) "Server settings" else "Edit room", "page-edit") {
        details?.let { d ->
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
                Avatar(d.avatar, d.name, Size.avatarLg)
                TextButton({ picker.pick() }, Modifier.testTag("edit-picture")) { Text(t("Choose a picture…")) }
            }
        }
        picker.Show()
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("edit-name"), singleLine = true, label = { Text(t("Name")) })
        OutlinedTextField(topic, { topic = it }, Modifier.fillMaxWidth().testTag("edit-topic"), label = { Text(t("Topic")) })
        Button({
            scope.launch {
                try { app.morpheus.editRoom(page.roomId, name, topic); ok = t("Saved."); error = null } catch (e: Exception) { error = e.message; ok = null }
            }
        }, Modifier.testTag("edit-save")) { Text(t("Save")) }
        Note(ok, error)
    }
}

@Composable
private fun PrivacyPage(app: AppModel, page: PageRequest) {
    var info by remember { mutableStateOf<PrivacyInfo?>(null) }
    var join by remember { mutableStateOf("public") }
    var history by remember { mutableStateOf("shared") }
    var confirming by remember { mutableStateOf(false) }
    var reload by remember { mutableIntStateOf(0) }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(reload) {
        try { info = app.morpheus.privacyInfo(page.roomId).also { join = if (it.public) "public" else "invite"; history = it.history } } catch (e: Exception) { error = e.message }
    }
    PageFrame(app, "Privacy & encryption", "page-privacy") {
        info?.let { i ->
            if (i.hasJoinAndHistory) {
                SectionHeader(t("Who can join"))
                for (c in i.joinChoices) ChoiceRow(c.label, join == c.key, "privacy-join-${c.key}") { join = c.key }
                SectionHeader(t("History for new members"))
                for (c in i.historyChoices) ChoiceRow(c.label, history == c.key, "privacy-history-${c.key}") { history = c.key }
                Button({
                    scope.launch {
                        try { app.morpheus.setPrivacy(page.roomId, join, history); ok = t("Saved."); error = null; reload++ } catch (e: Exception) { error = e.message; ok = null }
                    }
                }, Modifier.testTag("privacy-save")) { Text(t("Save")) }
            }
            if (i.hasEncryption) {
                SectionHeader(t("Encryption"))
                when (i.encrypted) {
                    null -> Text("…", color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
                    true -> Text(i.encryptionNote, Modifier.testTag("privacy-encryption"), style = MaterialTheme.typography.bodyLarge)
                    false -> {
                        Text(i.encryptionNote, Modifier.testTag("privacy-encryption"), style = MaterialTheme.typography.bodyLarge)
                        TextButton({ confirming = true }, Modifier.testTag("privacy-encrypt")) { Text(t("Turn on encryption")) }
                        // Turning it on cannot be undone: a dialog says so first.
                        if (confirming) ConfirmDialog(
                            title = t("Turn on encryption"), text = i.encryptionWarning, textTag = "privacy-warning",
                            confirm = t("Turn on encryption"), confirmTag = "privacy-encrypt-confirm", danger = true,
                            onDismiss = { confirming = false },
                            onConfirm = {
                                scope.launch {
                                    try { app.morpheus.turnOnEncryption(page.roomId); confirming = false; reload++ } catch (e: Exception) { error = e.message; confirming = false }
                                }
                            },
                        )
                    }
                }
            }
        }
        Note(ok, error)
    }
}

@Composable
private fun OverviewPage(app: AppModel, page: PageRequest) {
    var overview by remember { mutableStateOf<Overview?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val sidebar by app.sidebar.collectAsState()
    LaunchedEffect(reload) { try { overview = app.morpheus.serverOverview(page.roomId) } catch (e: Exception) { error = e.message } }
    PageFrame(app, "Server overview", "page-overview") {
        overview?.let { o ->
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
                ServerIcon(o.avatar, o.name, Size.avatarXl)
                Column {
                    Text(o.name, Modifier.testTag("overview-name"), style = MaterialTheme.typography.titleLarge)
                    Text(t("%{count} members", "count" to o.members.toString()), Modifier.testTag("overview-members"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
                }
            }
            o.topic?.takeIf { it.isNotBlank() }?.let { Text(it, Modifier.testTag("overview-topic"), style = MaterialTheme.typography.bodyLarge) }
            val rooms = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }.orEmpty()
            val first = o.channels.firstOrNull { it.joined && !it.voice }?.let { c -> rooms.firstOrNull { it.id == c.id } }
            if (first != null) Button({ app.closePage(); app.open(first) }, Modifier.testTag("overview-start")) { Text(t("Start chatting")) }
            SectionHeader(t("Channels"))
            for (c in o.channels) {
                ListRow(
                    c.name, "overview-channel-${c.name}",
                    supporting = c.topic?.takeIf { it.isNotBlank() },
                    leading = { GlyphIcon(if (c.voice) dev.morpheus.app.R.drawable.ic_ms_volume_up else dev.morpheus.app.R.drawable.ic_ms_tag) },
                    trailing = if (!c.joined) ({
                        TextButton({
                            scope.launch { try { app.morpheus.joinChannel(c.id, c.via); reload++ } catch (e: Exception) { error = e.message } }
                        }, Modifier.testTag("overview-join-${c.name}")) { Text(t("Join")) }
                    }) else null,
                )
            }
        }
        Note(null, error)
    }
}

@Composable
private fun ReactionsPage(app: AppModel, page: PageRequest) {
    val names by People.names.collectAsState()
    val message = page.message
    LaunchedEffect(message?.key) { message?.reactions?.flatMap { it.senders }?.forEach { People.want(it) } }
    PageFrame(app, "View reactions", "page-reactions") {
        for (reaction in message?.reactions.orEmpty()) {
            Column(Modifier.fillMaxWidth().testTag("reaction-${reaction.key}"), verticalArrangement = Arrangement.spacedBy(Space.xs)) {
                Text("${reaction.key}  ${reaction.count}", style = MaterialTheme.typography.titleMedium)
                Text(reaction.senders.joinToString(", ") { People.nameOf(it, names) }, Modifier.testTag("reaction-people-${reaction.key}"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

/** A room's pinned messages, newest pin first, each with a button that unpins it (#413). */
@Composable
private fun PinnedPage(app: AppModel, page: PageRequest) {
    var pins by remember { mutableStateOf<List<uniffi.morpheus_ffi.PinnedItem>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    // Read again while the page is open: a pin made from another device shows too.
    LaunchedEffect(page.roomId) {
        while (true) {
            try { pins = app.morpheus.pinnedMessages(page.roomId) } catch (e: Exception) { error = e.sentence() }
            kotlinx.coroutines.delay(2000)
        }
    }
    PageFrame(app, "Pinned messages", "page-pinned") {
        val list = pins
        if (list != null && list.isEmpty()) Text(t("Nothing pinned yet. Long-press a message to pin it."), Modifier.testTag("pinned-empty"), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
        for (pin in list.orEmpty()) {
            Row(Modifier.fillMaxWidth().testTag("pinned-item"), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    pin.sender?.let { Text(it, Modifier.testTag("pinned-sender"), fontWeight = androidx.compose.ui.text.font.FontWeight.Bold, style = MaterialTheme.typography.bodyLarge) }
                    Text(pin.body, Modifier.testTag("pinned-body"), style = MaterialTheme.typography.bodyLarge)
                }
                IconAction(dev.morpheus.app.R.drawable.ic_ms_keep_off, t("Unpin"), "unpin-${pin.eventId}", {
                    scope.launch { try { app.morpheus.unpinMessage(page.roomId, pin.eventId); pins = app.morpheus.pinnedMessages(page.roomId) } catch (e: Exception) { error = e.sentence() } }
                })
            }
        }
        Note(null, error)
    }
}

/** A long message in full (#408): the page the "Show more" bar opens. */
@Composable
private fun FullMessagePage(app: AppModel, page: PageRequest) {
    val message = page.message
    PageFrame(app, message?.senderName ?: "Message", "page-full-message") {
        (message?.body as? uniffi.morpheus_ffi.MessageBody.Text)?.let { LinkedText(it, message?.edited == true, sending = false, lines = message?.lines ?: emptyList()) }
    }
}

@Composable
private fun ReportPage(app: AppModel, page: PageRequest) {
    var reason by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Report message", "page-report") {
        Text(t("Tell the admins of the server what is wrong with this message."), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(reason, { reason = it }, Modifier.fillMaxWidth().testTag("report-reason"), label = { Text(t("Reason")) })
        Button({
            scope.launch {
                try { app.morpheus.reportMessage(page.roomId, page.extra, reason); ok = t("Reported."); error = null } catch (e: Exception) { error = e.message; ok = null }
            }
        }, Modifier.testTag("report-send")) { Text(t("Report")) }
        Note(ok, error)
    }
}

@Composable
private fun RenameFolderPage(app: AppModel, page: PageRequest) {
    var name by remember { mutableStateOf(page.extra) }
    PageFrame(app, "Rename folder", "page-rename-folder") {
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("rename-input"), singleLine = true, label = { Text(t("Name")) })
        Button({ app.renameFolder(page.roomId, name); app.closePage() }, Modifier.testTag("rename-save"), enabled = name.isNotBlank()) { Text(t("Save")) }
    }
}

/**
 * A picture in a square frame to drag and zoom (the desktop's crop: `view_model::crop` through ffi). "Use" hands back
 * the square as a PNG; "Cancel" drops the picture.
 */
@Composable
private fun CropPicture(frame: uniffi.morpheus_ffi.CropFrame, onUse: (ByteArray) -> Unit, onCancel: () -> Unit) {
    val bitmap = remember(frame) {
        val bytes = frame.picture()
        android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
    }
    var tick by remember { mutableIntStateOf(0) }
    var framePx by remember { mutableFloatStateOf(1f) }
    val scope = rememberCoroutineScope()
    Column(Modifier.testTag("crop"), verticalArrangement = Arrangement.spacedBy(Space.md)) {
        Text(t("Drag the picture and zoom to choose what shows."), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        androidx.compose.foundation.layout.Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(1f)
                .clip(androidx.compose.foundation.shape.CircleShape)
                .background(Palette.Raised)
                .onSizeChanged { framePx = it.width.toFloat() }
                .pointerInput(frame) {
                    detectDragGestures { change, drag ->
                        change.consume()
                        frame.panBy(drag.x, drag.y, framePx)
                        tick++
                    }
                }
                .testTag("crop-frame"),
        ) {
            tick
            if (bitmap != null) {
                val offset = frame.offset(framePx)
                val shown = frame.shownSize(framePx)
                androidx.compose.foundation.Image(
                    bitmap, null,
                    Modifier
                        .offset { androidx.compose.ui.unit.IntOffset(offset.x.toInt(), offset.y.toInt()) }
                        .requiredSize(with(androidx.compose.ui.platform.LocalDensity.current) { shown.x.toDp() }, with(androidx.compose.ui.platform.LocalDensity.current) { shown.y.toDp() }),
                    contentScale = androidx.compose.ui.layout.ContentScale.FillBounds,
                )
            }
        }
        androidx.compose.material3.Slider(frame.zoomFraction().also { tick }, { frame.setZoomFraction(it); tick++ }, Modifier.testTag("crop-zoom"))
        Row(horizontalArrangement = Arrangement.spacedBy(Space.md)) {
            Button({ scope.launch { onUse(frame.cut()) } }, Modifier.testTag("crop-use")) { Text(t("Use this picture")) }
            TextButton(onCancel, Modifier.testTag("crop-cancel")) { Text(t("Cancel")) }
        }
    }
}

/** What a page does with a picture the person chose: the crop first, then the square. */
@Composable
private fun rememberPicturePicker(onSquare: (ByteArray) -> Unit): PicturePicker {
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var frame by remember { mutableStateOf<uniffi.morpheus_ffi.CropFrame?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val take: (ByteArray) -> Unit = { bytes ->
        val opened = uniffi.morpheus_ffi.openCrop(bytes)
        if (opened == null) error = t("That file is not a picture Morpheus can show.") else { frame = opened; error = null }
    }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch { context.read(uri)?.let { take(it.bytes) } }
    }
    androidx.compose.runtime.DisposableEffect(Unit) {
        RoomHooks.picture = { _, bytes -> take(bytes) }
        onDispose { RoomHooks.picture = null }
    }
    return PicturePicker({ launcher.launch("image/*") }, frame, error, { frame = null }) { bytes -> frame = null; onSquare(bytes) }
}

private class PicturePicker(val pick: () -> Unit, val frame: uniffi.morpheus_ffi.CropFrame?, val error: String?, val cancel: () -> Unit, val use: (ByteArray) -> Unit) {
    @Composable
    fun Show() {
        error?.let { Text(it, Modifier.testTag("page-error"), color = Palette.Danger, style = MaterialTheme.typography.bodyMedium) }
        frame?.let { CropPicture(it, use, cancel) }
    }
}

@Composable
private fun LeavePage(app: AppModel, page: PageRequest) {
    val info = remember { app.morpheus.leaveServerInfo(page.roomId) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Leave server", "page-leave") {
        Text(info.title, Modifier.testTag("leave-title"), style = MaterialTheme.typography.titleMedium)
        Text(info.body, Modifier.testTag("leave-body"), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
        Button({
            scope.launch {
                try { app.morpheus.leaveServer(page.roomId); app.closePage(); app.selectTab(dev.morpheus.app.Tab.Servers) } catch (e: Exception) { error = e.message }
            }
        }, Modifier.testTag("leave-server"), colors = androidx.compose.material3.ButtonDefaults.buttonColors(containerColor = Palette.Danger, contentColor = Palette.OnDanger)) { Text(t("Leave server")) }
        Note(null, error)
    }
}

@Composable
private fun CategoryPage(app: AppModel, page: PageRequest) {
    var name by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Create category", "page-category") {
        Text(app.morpheus.createCategoryBlurb(), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("category-name"), singleLine = true, label = { Text(t("Name")) })
        Button({
            scope.launch { try { app.morpheus.createCategory(page.roomId, name); app.closePage() } catch (e: Exception) { error = e.message } }
        }, Modifier.testTag("category-submit")) { Text(t("Create category")) }
        Note(null, error)
    }
}

@Composable
private fun CreateServerPage(app: AppModel) {
    var name by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Create server", "page-create-server") {
        Text(app.morpheus.createServerBlurb(), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("server-name"), singleLine = true, label = { Text(t("Name")) })
        Button({
            busy = true
            scope.launch { try { app.morpheus.createServer(name); app.closePage() } catch (e: Exception) { error = e.sentence(); busy = false } }
        }, Modifier.testTag("server-submit"), enabled = name.isNotBlank() && !busy) { Text(t("Create server")) }
        Note(null, error)
    }
}

@Composable
private fun StatusPage(app: AppModel) {
    var text by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(Unit) { try { text = app.morpheus.ownStatus() } catch (_: Exception) {} }
    fun save(to: String) {
        busy = true
        scope.launch { try { app.morpheus.setStatus(to); app.refreshStatus(); app.closePage() } catch (e: Exception) { error = e.sentence(); busy = false } }
    }
    PageFrame(app, "Set status", "page-status") {
        OutlinedTextField(text, { text = it }, Modifier.fillMaxWidth().testTag("status-input"), singleLine = true, label = { Text(t("What are you up to?")) })
        Row(horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
            Button({ save(text) }, Modifier.testTag("status-save"), enabled = !busy) { Text(t("Save")) }
            TextButton({ save("") }, Modifier.testTag("status-clear"), enabled = !busy) { Text(t("Clear status")) }
        }
        Note(null, error)
    }
}

@Composable
private fun NicknamePage(app: AppModel, page: PageRequest) {
    var text by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(page.roomId) { try { text = app.morpheus.nicknameOf(page.roomId) } catch (_: Exception) {} }
    PageFrame(app, "Set nickname", "page-nickname") {
        Text(app.morpheus.nicknameBlurb(page.extra), Modifier.testTag("nickname-blurb"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(text, { text = it }, Modifier.fillMaxWidth().testTag("nickname-input"), singleLine = true, label = { Text(t("Nickname")) })
        Button({
            busy = true
            scope.launch { try { app.morpheus.setNickname(page.roomId, text); app.nicknamesChanged(); app.closePage() } catch (e: Exception) { error = e.sentence(); busy = false } }
        }, Modifier.testTag("nickname-save"), enabled = !busy) { Text(t("Save")) }
        Note(null, error)
    }
}

@Composable
private fun FullProfilePage(app: AppModel, page: PageRequest) {
    var found by remember { mutableStateOf<uniffi.morpheus_ffi.FullProfile?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(page.roomId) { try { found = app.morpheus.fullProfile(page.roomId) } catch (e: Exception) { error = e.sentence() } }
    PageFrame(app, "Profile", "page-full-profile") {
        val profile = found
        if (profile != null) {
            Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Space.sm)) {
                Avatar(profile.avatar, profile.name, Size.avatarXl)
                Text(profile.name, Modifier.testTag("full-profile-name"), style = MaterialTheme.typography.titleLarge)
                Text(profile.userId, Modifier.testTag("full-profile-id"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
                profile.status?.let { Text(it, Modifier.testTag("full-profile-status"), style = MaterialTheme.typography.bodyLarge) }
                if (profile.isMe) Text(t("This is you"), Modifier.testTag("full-profile-me"), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
            }
            if (!profile.isMe) {
                SectionHeader(profile.mutualHeading, Modifier.testTag("full-profile-mutual"))
                for (server in profile.servers) Text(server, Modifier.testTag("full-profile-server-$server"), style = MaterialTheme.typography.bodyLarge)
                Button({
                    scope.launch { try { app.closePage(); app.openNewRoom(app.morpheus.startDm(profile.userId)) } catch (e: Exception) { error = e.sentence() } }
                }, Modifier.fillMaxWidth().testTag("full-profile-message")) { Text(t("Message")) }
            }
        }
        Note(null, error)
    }
}

@Composable
private fun ReportUserPage(app: AppModel, page: PageRequest) {
    var reason by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Report user profile", "page-report-user") {
        Text(t("Tell the admins of the server what is wrong with this person's profile."), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(reason, { reason = it }, Modifier.fillMaxWidth().testTag("report-user-reason"), label = { Text(t("Reason")) })
        Button({
            scope.launch { try { ok = app.morpheus.reportUser(page.roomId, reason); error = null } catch (e: Exception) { error = e.sentence(); ok = null } }
        }, Modifier.testTag("report-user-send")) { Text(t("Report")) }
        Note(ok, error)
    }
}

@Composable
private fun ProfilePage(app: AppModel, page: PageRequest) {
    var blurb by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var square by remember { mutableStateOf<ByteArray?>(null) }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val picker = rememberPicturePicker { bytes -> square = bytes }
    LaunchedEffect(Unit) {
        try { app.morpheus.serverProfile(page.roomId).let { blurb = it.blurb; name = it.name } } catch (e: Exception) { error = e.message }
    }
    PageFrame(app, "Edit per-server profile", "page-profile") {
        Text(blurb, Modifier.testTag("profile-blurb"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("profile-name"), singleLine = true, label = { Text(t("Name")) })
        TextButton({ picker.pick() }, Modifier.testTag("profile-picture")) { Text(if (square == null) t("Choose a picture…") else t("Picture chosen")) }
        picker.Show()
        Button({
            scope.launch {
                try { app.morpheus.saveServerProfile(page.roomId, name, "picture.png", square ?: ByteArray(0)); ok = t("Saved for this server."); error = null } catch (e: Exception) { error = e.message; ok = null }
            }
        }, Modifier.testTag("profile-save")) { Text(t("Save")) }
        Note(ok, error)
    }
}

@Composable
private fun UnlockPage(app: AppModel, page: PageRequest) {
    var password by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val sidebar by app.sidebar.collectAsState()
    val words = remember { app.morpheus.lockWords() }
    PageFrame(app, page.extra.ifEmpty { "Locked" }, "page-unlock") {
        Text(words.lockedTitle, Modifier.testTag("unlock-title"), style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(
            password, { password = it }, Modifier.fillMaxWidth().testTag("unlock-input"), singleLine = true, label = { Text(t("Password")) },
            visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
        )
        Button({
            scope.launch {
                try {
                    if (app.morpheus.unlockRoom(page.roomId, password)) {
                        val room = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == page.roomId }
                        app.closePage()
                        room?.let { app.openUnlocked(it) }
                    } else error = t("That is not the password.")
                } catch (e: Exception) { error = e.message }
            }
        }, Modifier.testTag("unlock-submit")) { Text(t("Unlock")) }
        Note(null, error)
    }
}

@Composable
private fun LockSetPage(app: AppModel, page: PageRequest) {
    var password by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val words = remember { app.morpheus.lockWords() }
    PageFrame(app, "Lock with a password…", "page-lock-set") {
        Text(words.blurb, color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        val hide = androidx.compose.ui.text.input.PasswordVisualTransformation()
        OutlinedTextField(password, { password = it }, Modifier.fillMaxWidth().testTag("lock-password"), singleLine = true, label = { Text(t("Password")) }, visualTransformation = hide)
        OutlinedTextField(again, { again = it }, Modifier.fillMaxWidth().testTag("lock-again"), singleLine = true, label = { Text(t("Password again")) }, visualTransformation = hide)
        Button({
            scope.launch { try { app.morpheus.setLock(page.roomId, password, again); app.closePage() } catch (e: Exception) { error = e.message } }
        }, Modifier.testTag("lock-set-submit")) { Text(t("Lock with a password…")) }
        Note(null, error)
    }
}

@Composable
private fun LockRemovePage(app: AppModel, page: PageRequest) {
    var password by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val words = remember { app.morpheus.lockWords() }
    PageFrame(app, "Remove lock…", "page-lock-remove") {
        OutlinedTextField(
            password, { password = it }, Modifier.fillMaxWidth().testTag("lock-remove-password"), singleLine = true, label = { Text(t("Password")) },
            visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
        )
        Text(words.forgot, color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        Button({
            scope.launch { try { app.morpheus.removeLock(page.roomId, password); app.closePage() } catch (e: Exception) { error = e.message } }
        }, Modifier.testTag("lock-remove-submit")) { Text(t("Remove lock…")) }
        Note(null, error)
    }
}
