package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import dev.morpheus.app.People
import androidx.compose.runtime.collectAsState
import dev.morpheus.app.RoomModel
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.MemberRow
import uniffi.morpheus_ffi.ProfileCard
import uniffi.morpheus_ffi.RoomItem

/** A room's details: what it is, how it notifies, who is in it (a tap opens their card), and leaving it. */
@Composable
fun RoomInfo(app: AppModel, model: RoomModel, room: RoomItem, title: String, onClose: () -> Unit) {
    var card by remember { mutableStateOf<ProfileCard?>(null) }
    BackHandler { if (card != null) card = null else onClose() }
    var members by remember { mutableStateOf<List<MemberRow>?>(null) }
    var mode by remember { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var sure by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(Unit) {
        try {
            members = model.members()
        } catch (e: Exception) {
            problem = e.sentence()
        }
    }
    LaunchedEffect(Unit) { mode = model.notifyMode() }

    Box(Modifier.fillMaxSize().testTag("room-info")) {
        LaunchedEffect(problem) { problem?.let { Toasts.say(it, "info-error", long = true) } }
        ScreenScaffold("room-info-screen", { TopBar(t("Room details"), backTag = "info-back", onBack = { onClose() }) }) { inner ->
            LazyColumn(Modifier.fillMaxSize().padding(inner)) {
                item {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.xs), modifier = Modifier.padding(top = Space.md)) {
                        Text(title, Modifier.padding(horizontal = Space.lg).testTag("info-name"), style = MaterialTheme.typography.titleLarge)
                        room.topic?.let { Text(it, Modifier.padding(horizontal = Space.lg).testTag("info-topic"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge) }
                        // A real lock icon (not an emoji) before the line; the icon and text take the on-surface variant colour.
                        Row(Modifier.padding(horizontal = Space.lg), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                            GlyphIcon(if (room.isEncrypted) dev.morpheus.app.R.drawable.ic_lucide_lock else dev.morpheus.app.R.drawable.ic_lucide_lock_open, Modifier.testTag("info-lock"), size = 16.dp, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(
                                if (room.isEncrypted) t("End-to-end encrypted") else t("Not encrypted"),
                                Modifier.testTag("info-encryption"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                        SmallHeading(t("Notifications"), Modifier.padding(start = Space.lg, end = Space.lg, top = Space.lg))
                        for ((key, words) in listOf("all" to "All messages", "mentions" to "Only mentions", "mute" to "Muted")) {
                            ChoiceRow(t(words), mode == key, "room-notify-$key") {
                                mode = key
                                model.setNotifyMode(key)
                            }
                        }
                        SmallHeading(t("Members"), Modifier.padding(start = Space.lg, end = Space.lg, top = Space.lg))
                        if (members == null && problem == null) Text(t("Loading…"), Modifier.padding(horizontal = Space.lg), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
                    }
                }
                items(members.orEmpty()) { row ->
                    when (row) {
                        is MemberRow.Heading -> Text(row.title, Modifier.padding(start = Space.lg, end = Space.lg, top = Space.sm).testTag("member-heading"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelLarge)
                        is MemberRow.Member -> ListRow(
                            row.member.name, "member-${row.member.name}",
                            leading = { Avatar(row.member.avatar, row.member.name, 36.dp) },
                        ) {
                            scope.launch {
                                try {
                                    card = model.profile(row.member.userId)
                                } catch (e: Exception) {
                                    problem = e.sentence()
                                }
                            }
                        }
                    }
                }
                item {
                    Column(Modifier.padding(horizontal = Space.lg, vertical = Space.xl)) {
                        androidx.compose.material3.FilledTonalButton(
                            {
                                if (!sure) {
                                    sure = true
                                } else {
                                    scope.launch {
                                        try {
                                            app.morpheus.leaveRoom(room.id)
                                            app.closeRoom()
                                        } catch (e: Exception) {
                                            problem = e.sentence()
                                        }
                                    }
                                }
                            },
                            Modifier.testTag("leave-room"),
                            // A destructive action reads as one: the error container (the scheme's own pair), not the accent.
                            colors = androidx.compose.material3.ButtonDefaults.filledTonalButtonColors(containerColor = MaterialTheme.colorScheme.errorContainer, contentColor = MaterialTheme.colorScheme.onErrorContainer),
                        ) { Text(if (sure) t("Tap again to leave") else t("Leave room")) }
                    }
                }
            }
        }
        card?.let { ProfileSheet(app, it, { card = null }) }
    }
}

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
internal fun ProfileSheet(app: AppModel, card: ProfileCard, onClose: () -> Unit) {
    var friend by remember(card.userId) { mutableStateOf(card.isFriend) }
    var problem by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    // A note only I can see: loaded, then kept a moment after the last key (and when the card goes away).
    var note by remember(card.userId) { mutableStateOf("") }
    var kept by remember(card.userId) { mutableStateOf<String?>(null) }
    val latestNote by androidx.compose.runtime.rememberUpdatedState(note)
    val latestKept by androidx.compose.runtime.rememberUpdatedState(kept)
    LaunchedEffect(card.userId) {
        if (!card.isMe) try { app.morpheus.noteOf(card.userId).let { note = it; kept = it } } catch (_: Exception) {}
    }
    LaunchedEffect(note) {
        if (kept != null && note != kept) {
            kotlinx.coroutines.delay(600)
            app.saveNote(card.userId, note)
            kept = note
        }
    }
    androidx.compose.runtime.DisposableEffect(card.userId) {
        onDispose { latestKept?.let { if (latestNote != it) app.saveNote(card.userId, latestNote) } }
    }
    // A page opened from the card's menu covers it: the card goes.
    val page by app.page.collectAsState()
    LaunchedEffect(page) { if (page != null) onClose() }
    androidx.compose.material3.ModalBottomSheet(
        onDismissRequest = onClose,
        modifier = Modifier.testTag("profile-card"),
        sheetState = androidx.compose.material3.rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Column(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Avatar(card.avatar, card.name, 56.dp)
                Column {
                    Text(card.name, Modifier.testTag("card-name"), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
                    Text(card.userId, Modifier.testTag("card-id"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
                }
            }
            if (!card.isMe) IconAction(dev.morpheus.app.R.drawable.ic_lucide_ellipsis, t("More"), "card-more", { app.openSheet(card.name) { app.morpheus.profileMoreRows(card.userId) } }, Modifier.align(Alignment.End))
            card.role?.let { Text(it, Modifier.testTag("card-role"), color = Palette.Accent, style = MaterialTheme.typography.bodyMedium) }
            card.status?.let { Text(it, color = Palette.Muted, style = MaterialTheme.typography.bodyLarge) }
            Text(card.mutualHeading, Modifier.testTag("card-mutual"), color = Palette.Muted, style = MaterialTheme.typography.bodySmall)
            Text(card.mutualRooms.joinToString(", "), maxLines = 3, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium)
            if (!card.isMe) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button({
                        scope.launch {
                            try {
                                app.openNewRoom(app.morpheus.startDm(card.userId))
                            } catch (e: Exception) {
                                problem = e.sentence()
                            }
                        }
                    }, Modifier.testTag("card-message")) { Text(t("Message")) }
                    TextButton({
                        scope.launch {
                            try {
                                app.morpheus.setFriend(card.userId, !friend)
                                friend = !friend
                            } catch (e: Exception) {
                                problem = e.sentence()
                            }
                        }
                    }, Modifier.testTag("card-friend")) { Text(if (friend) "Remove friend" else "Add friend") }
                    val ignored by People.ignored.collectAsState()
                    TextButton({ app.setIgnored(card.userId, card.userId !in ignored) }, Modifier.testTag("card-ignore")) {
                        Text(if (card.userId in ignored) t("Unignore") else t("Ignore"))
                    }
                }
            }
            if (!card.isMe && kept != null) {
                androidx.compose.material3.OutlinedTextField(
                    note, { note = it }, Modifier.fillMaxWidth().testTag("card-note"),
                    label = { Text(t("Note")) }, placeholder = { Text(t("Only you can see this")) },
                )
            }
            problem?.let { Text(it, color = Palette.Danger, style = MaterialTheme.typography.bodyMedium) }
        }
    }
}
