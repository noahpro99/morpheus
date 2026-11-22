package dev.morpheus.app.ui

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
import dev.morpheus.app.People
import dev.morpheus.app.read
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.Overview
import uniffi.morpheus_ffi.PrivacyInfo
import uniffi.morpheus_ffi.RoomDetails

/** The page a sheet item opened, over whatever is on screen. */
@Composable
fun RoomPageHost(app: AppModel) {
    val request by app.page.collectAsState()
    val page = request ?: return
    BackHandler { app.closePage() }
    androidx.compose.runtime.key(page) {
        when (page.kind) {
            "invite" -> InvitePage(app, page)
            "create" -> CreatePage(app, page)
            "edit" -> EditPage(app, page)
            "privacy" -> PrivacyPage(app, page)
            "overview" -> OverviewPage(app, page)
            "reactions" -> ReactionsPage(app, page)
            "report" -> ReportPage(app, page)
            "rename-folder" -> RenameFolderPage(app, page)
        }
    }
}

@Composable
private fun PageFrame(app: AppModel, title: String, tag: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().background(Palette.Surface).testTag(tag)) {
        TopBar(t(title), backTag = "page-back", onBack = { app.closePage() })
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(Space.lg), verticalArrangement = Arrangement.spacedBy(Space.md)) { content() }
    }
}

/** What a form says after it ran: a note that it worked, or why it did not. */
@Composable
private fun Note(ok: String?, error: String?) {
    if (ok != null) Text(ok, Modifier.testTag("page-note"), color = Palette.Accent, style = Type.secondary)
    if (error != null) Text(error, Modifier.testTag("page-error"), color = Palette.Danger, style = Type.secondary)
}

@Composable
private fun InvitePage(app: AppModel, page: PageRequest) {
    var typed by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Invite people", "page-invite") {
        Text(t("Enter a full Matrix ID like @name:server"), color = Palette.Muted, style = Type.secondary)
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
    val context = LocalContext.current
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch {
            try {
                val picked = context.read(uri) ?: return@launch
                app.morpheus.setRoomPicture(page.roomId, picked.name, picked.bytes)
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
                TextButton({ picker.launch("image/*") }, Modifier.testTag("edit-picture")) { Text(t("Choose a picture…")) }
            }
        }
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
private fun ChoiceRow(label: String, selected: Boolean, tag: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Size.target).clickable(onClick = onClick).testTag(tag),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md),
    ) {
        GlyphIcon(if (selected) dev.morpheus.app.R.drawable.ic_lucide_check else dev.morpheus.app.R.drawable.ic_lucide_minus, tint = if (selected) Palette.Accent else Palette.Muted)
        Text(label, Modifier.weight(1f), style = Type.body)
    }
}

@Composable
private fun PrivacyPage(app: AppModel, page: PageRequest) {
    var info by remember { mutableStateOf<PrivacyInfo?>(null) }
    var join by remember { mutableStateOf("public") }
    var history by remember { mutableStateOf("shared") }
    var confirming by remember { mutableStateOf(false) }
    var reload by remember { mutableStateOf(0) }
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
                    null -> Text("…", color = Palette.Muted)
                    true -> Text(i.encryptionNote, Modifier.testTag("privacy-encryption"), style = Type.body)
                    false -> {
                        Text(i.encryptionNote, Modifier.testTag("privacy-encryption"), style = Type.body)
                        if (!confirming) TextButton({ confirming = true }, Modifier.testTag("privacy-encrypt")) { Text(t("Turn on encryption")) }
                        else {
                            Text(i.encryptionWarning, Modifier.testTag("privacy-warning"), color = Palette.Danger, style = Type.secondary)
                            Button({
                                scope.launch {
                                    try { app.morpheus.turnOnEncryption(page.roomId); confirming = false; reload++ } catch (e: Exception) { error = e.message }
                                }
                            }, Modifier.testTag("privacy-encrypt-confirm")) { Text(t("Turn on encryption")) }
                        }
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
    var reload by remember { mutableStateOf(0) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val sidebar by app.sidebar.collectAsState()
    LaunchedEffect(reload) { try { overview = app.morpheus.serverOverview(page.roomId) } catch (e: Exception) { error = e.message } }
    PageFrame(app, "Server overview", "page-overview") {
        overview?.let { o ->
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
                Avatar(o.avatar, o.name, Size.avatarXl, square = true)
                Column {
                    Text(o.name, Modifier.testTag("overview-name"), style = Type.title)
                    Text(t("%{count} members", "count" to o.members.toString()), Modifier.testTag("overview-members"), color = Palette.Muted, style = Type.secondary)
                }
            }
            o.topic?.takeIf { it.isNotBlank() }?.let { Text(it, Modifier.testTag("overview-topic"), style = Type.body) }
            val rooms = sidebar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }.orEmpty()
            val first = o.channels.firstOrNull { it.joined && !it.voice }?.let { c -> rooms.firstOrNull { it.id == c.id } }
            if (first != null) Button({ app.closePage(); app.open(first) }, Modifier.testTag("overview-start")) { Text(t("Start chatting")) }
            SectionHeader(t("Channels"))
            for (c in o.channels) {
                Row(Modifier.fillMaxWidth().heightIn(min = Size.target).testTag("overview-channel-${c.name}"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
                    GlyphIcon(if (c.voice) dev.morpheus.app.R.drawable.ic_lucide_volume_2 else dev.morpheus.app.R.drawable.ic_lucide_hash, tint = Palette.Muted)
                    Column(Modifier.weight(1f)) {
                        Text(c.name, style = Type.body, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        c.topic?.takeIf { it.isNotBlank() }?.let { Text(it, color = Palette.Muted, style = Type.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                    }
                    if (!c.joined) TextButton({
                        scope.launch { try { app.morpheus.joinChannel(c.id, c.via); reload++ } catch (e: Exception) { error = e.message } }
                    }, Modifier.testTag("overview-join-${c.name}")) { Text(t("Join")) }
                }
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
                Text("${reaction.key}  ${reaction.count}", style = Type.heading)
                Text(reaction.senders.joinToString(", ") { People.nameOf(it, names) }, Modifier.testTag("reaction-people-${reaction.key}"), color = Palette.Muted, style = Type.secondary)
            }
        }
    }
}

@Composable
private fun ReportPage(app: AppModel, page: PageRequest) {
    var reason by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    PageFrame(app, "Report message", "page-report") {
        Text(t("Tell the admins of the server what is wrong with this message."), color = Palette.Muted, style = Type.secondary)
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
