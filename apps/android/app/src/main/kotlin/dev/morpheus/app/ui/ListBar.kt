package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.border
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.People
import dev.morpheus.app.Screen
import dev.morpheus.app.Tab
import uniffi.morpheus_ffi.ScopeItem
import uniffi.morpheus_ffi.ScopeKind

/**
 * The search bar at the foot of a list (#500): a Material 3 search bar that IS the quick switcher, so a tap on it opens the switcher over every
 * room, DM, server and command, not a filter of this list. Its hint names where you are ("Search DMs", "Search <server>"). On its right the bell
 * (the inbox) and `extra`: your picture on the DMs, the server's icon on Servers.
 */
@Composable
fun ListSearchBar(model: AppModel, hint: String, modifier: Modifier = Modifier, extra: (@Composable () -> Unit)? = null) {
    val focusManager = LocalFocusManager.current
    SearchField(
        "", {}, hint, "list-search",
        modifier.padding(horizontal = Space.md, vertical = Space.sm),
        onFocused = { focusManager.clearFocus(); model.openSwitcher() },
        trailing = {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
                Bell(model)
                extra?.invoke()
            }
        },
    )
}

/** The inbox button: a bell with the number of mentions and invitations waiting. */
@Composable
private fun Bell(model: AppModel) {
    val bar by model.sidebar.collectAsState()
    val news = bar?.let { b -> b.scopes.filter { it.kind != ScopeKind.DMS }.sumOf { it.highlights } + b.scopes.filter { it.kind == ScopeKind.DMS }.sumOf { it.highlights } + b.invites.size.toULong() } ?: 0UL
    BadgedBox(badge = { if (news > 0UL) Badge(Modifier.testTag("bell-badge")) { Text(if (news > 99UL) "99+" else news.toString()) } }) {
        IconAction(dev.morpheus.app.R.drawable.ic_lucide_bell, t("Inbox"), "bell", { model.openInbox() }, tint = ComposerColours.hint(MaterialTheme.colorScheme))
    }
}

/** On Servers: the server's own picture; a tap opens the menu the desktop's right-click on the server has. */
@Composable
fun ServerButton(scope: ScopeItem, onClick: () -> Unit, modifier: Modifier = Modifier) {
    IconButton(onClick, modifier.size(Size.target).semantics { contentDescription = scope.title }.testTag("server-button")) {
        if (scope.kind == ScopeKind.SERVER) Avatar(scope.avatar, scope.title, 32.dp, square = true, key = scope.avatarKey)
        else GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_hash, tint = ComposerColours.hint(MaterialTheme.colorScheme))
    }
}

/** On the DMs: your picture. A tap opens You (account, status, Settings); a long press opens the account menu (switch account). */
@Composable
fun MeButton(model: AppModel, modifier: Modifier = Modifier) {
    val me = when (val s = model.screen.collectAsState().value) {
        is Screen.SignedIn -> s.account.userId
        is Screen.Restoring -> s.account.userId
        else -> ""
    }
    val names by People.names.collectAsState()
    val own by model.ownProfile.collectAsState()
    val ownKey by model.ownAvatarKey.collectAsState()
    val name = own?.name ?: People.nameOf(me, names)
    // Taps by pointerInput, not clickable: that would merge the picture into one node.
    Box(
        modifier.size(Size.target)
            .pointerInput(Unit) {
                detectTapGestures(
                    onLongPress = { model.openSheet(t("You")) { model.morpheus.accountMenuRows() } },
                    onTap = { model.selectTab(Tab.Profile) },
                )
            }
            .semantics(mergeDescendants = true) { role = Role.Button; contentDescription = name; onClick { model.selectTab(Tab.Profile); true } }
            .testTag("me-button"),
        contentAlignment = Alignment.Center,
    ) { Avatar(own?.avatar, name, 32.dp, Modifier.testTag("me-avatar"), key = ownKey) }
}

/** Said once, the first time the lists show: the lists and the last chats are a swipe away. It goes after a few seconds, a swipe or a tap. */
@Composable
fun SwipeHint(model: AppModel, modifier: Modifier = Modifier) {
    val show by model.swipeHint.collectAsState()
    if (!show) return
    LaunchedEffect(Unit) {
        kotlinx.coroutines.delay(8000)
        model.dismissSwipeHint()
    }
    Surface(modifier.testTag("swipe-hint"), shape = CircleShape, color = MaterialTheme.colorScheme.inverseSurface, contentColor = MaterialTheme.colorScheme.inverseOnSurface) {
        Text(t("Swipe left or right to go between your DMs and your servers"), Modifier.padding(horizontal = Space.lg, vertical = Space.md), style = MaterialTheme.typography.bodyMedium)
    }
}
