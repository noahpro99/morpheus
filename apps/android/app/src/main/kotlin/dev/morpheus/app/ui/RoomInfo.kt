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
        Column(Modifier.fillMaxSize().background(Palette.Surface)) {
            TopBar(t("Room details"), backTag = "info-back", onBack = { onClose() })
            LazyColumn(Modifier.fillMaxSize().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                item {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(top = 12.dp)) {
                        Text(title, Modifier.testTag("info-name"), style = Type.title)
                        room.topic?.let { Text(it, Modifier.testTag("info-topic"), color = Palette.Muted) }
                        Text(
                            if (room.isEncrypted) "🔒 End-to-end encrypted" else "Not encrypted",
                            Modifier.testTag("info-encryption"), color = Palette.Muted, fontSize = 13.sp,
                        )
                        Text(t("Notifications").uppercase(), Modifier.padding(top = Space.lg), color = Palette.Muted, style = Type.label)
                        for ((key, words) in listOf("all" to "All messages", "mentions" to "Only mentions", "mute" to "Muted")) {
                            Row(
                                Modifier.fillMaxWidth().clickable {
                                    mode = key
                                    model.setNotifyMode(key)
                                }.padding(vertical = 10.dp).testTag("room-notify-$key"),
                                horizontalArrangement = Arrangement.spacedBy(10.dp),
                            ) {
                                Text(if (mode == key) "●" else "○", color = if (mode == key) Palette.Accent else Palette.Muted)
                                Text(words)
                            }
                        }
                        Text(t("Members").uppercase(), Modifier.padding(top = Space.lg), color = Palette.Muted, style = Type.label)
                        if (members == null && problem == null) Text(t("Loading…"), color = Palette.Muted)
                    }
                }
                items(members.orEmpty()) { row ->
                    when (row) {
                        is MemberRow.Heading -> Text(row.title, Modifier.padding(top = 8.dp).testTag("member-heading"), color = Palette.Muted, fontSize = 13.sp)
                        is MemberRow.Member -> Row(
                            Modifier.fillMaxWidth().clickable {
                                scope.launch {
                                    try {
                                        card = model.profile(row.member.userId)
                                    } catch (e: Exception) {
                                        problem = e.sentence()
                                    }
                                }
                            }.padding(vertical = 6.dp).testTag("member-${row.member.name}"),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            Avatar(row.member.avatar, row.member.name, 36.dp)
                            Text(row.member.name, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
                item {
                    Column(Modifier.padding(vertical = 20.dp)) {
                        problem?.let { Text(it, Modifier.testTag("info-error"), color = Palette.Danger, fontSize = 14.sp) }
                        Button(
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
                        ) { Text(if (sure) "Tap again to leave" else "Leave room") }
                    }
                }
            }
        }
        card?.let { ProfileSheet(app, it, { card = null }) }
    }
}

@Composable
internal fun ProfileSheet(app: AppModel, card: ProfileCard, onClose: () -> Unit) {
    var friend by remember(card.userId) { mutableStateOf(card.isFriend) }
    var problem by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    Box(
        Modifier.fillMaxSize().background(androidx.compose.ui.graphics.Color(0x99000000)).pointerInput(Unit) { detectTapGestures { onClose() } }.testTag("profile-scrim"),
    ) {
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(Palette.Raised, RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp))
                .pointerInput(Unit) { detectTapGestures { } }
                .padding(16.dp)
                .testTag("profile-card"),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Avatar(card.avatar, card.name, 56.dp)
                Column {
                    Text(card.name, Modifier.testTag("card-name"), fontSize = 20.sp, fontWeight = FontWeight.Bold)
                    Text(card.userId, Modifier.testTag("card-id"), color = Palette.Muted, fontSize = 13.sp)
                }
            }
            card.role?.let { Text(it, Modifier.testTag("card-role"), color = Palette.Accent, fontSize = 13.sp) }
            card.status?.let { Text(it, color = Palette.Muted) }
            Text(card.mutualHeading, Modifier.testTag("card-mutual"), color = Palette.Muted, fontSize = 12.sp)
            Text(card.mutualRooms.joinToString(", "), maxLines = 3, overflow = TextOverflow.Ellipsis, fontSize = 13.sp)
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
            problem?.let { Text(it, color = Palette.Danger, fontSize = 13.sp) }
        }
    }
}
