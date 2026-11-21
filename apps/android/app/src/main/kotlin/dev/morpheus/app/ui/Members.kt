package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.RoomModel
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.MemberRow
import uniffi.morpheus_ffi.ProfileCard

/** The people in the open room: the pane a swipe to the left in the chat uncovers. A tap opens a person's card. */
@Composable
fun MembersPane(app: AppModel, model: RoomModel) {
    var members by remember { mutableStateOf<List<MemberRow>?>(null) }
    var card by remember { mutableStateOf<ProfileCard?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(model) { members = try { model.members() } catch (_: Exception) { emptyList() } }
    Box(Modifier.fillMaxSize().background(Palette.Surface).testTag("members-pane")) {
        Column(Modifier.fillMaxSize()) {
            TopBar(t("Members"))
            val rows = members
            when {
                rows == null -> Column(Modifier.padding(top = Space.xl)) { SkeletonRows(7) }
                rows.isEmpty() -> EmptyState(Icons.Default.AccountCircle, t("No members to show"), t("Nobody has joined this room yet."))
                else -> LazyColumn(Modifier.fillMaxSize()) {
                    items(rows) { row ->
                        when (row) {
                            is MemberRow.Heading -> SectionHeader(row.title, Modifier.testTag("member-heading"))
                            is MemberRow.Member -> Row(
                                Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable {
                                    scope.launch { card = try { model.profile(row.member.userId) } catch (_: Exception) { null } }
                                }.padding(horizontal = Space.lg).testTag("member-${row.member.name}"),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(Space.md),
                            ) {
                                Avatar(row.member.avatar, row.member.name, Size.avatar)
                                Text(row.member.name, style = Type.body, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    }
                }
            }
        }
        card?.let { ProfileSheet(app, it) { card = null } }
    }
}
