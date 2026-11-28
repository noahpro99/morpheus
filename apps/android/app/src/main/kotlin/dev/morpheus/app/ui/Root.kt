package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.LocalContentColor
import androidx.compose.ui.layout.layout
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.platform.testTag
import dev.morpheus.app.AppModel
import dev.morpheus.app.Screen
import dev.morpheus.app.ShareInbox

/** The screen the app is on. */
@OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)
@Composable
fun Root(model: AppModel) {
    // The app's zoom (#542): every size and the text scale with it, as the desktop's Ctrl +/-.
    val zoom by model.uiZoom.collectAsState()
    val base = androidx.compose.ui.platform.LocalDensity.current
    CompositionLocalProvider(androidx.compose.ui.platform.LocalDensity provides androidx.compose.ui.unit.Density(base.density * zoom.factor, base.fontScale)) { RootContent(model) }
}

@OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)
@Composable
private fun RootContent(model: AppModel) {
    dev.morpheus.app.AppState.mark("root: composing")
    val screen by model.screen.collectAsState()
    androidx.compose.runtime.SideEffect { dev.morpheus.app.AppState.mark("root composed: ${screen.name()}") }
    Box(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .safeDrawingPadding()
            // Test tags are resource ids to uiautomator as well.
            .semantics { testTagsAsResourceId = true },
    ) {
        // The screen's Scaffold: it hosts the toasts (Snackbar) above everything.
        androidx.compose.material3.Scaffold(
            containerColor = androidx.compose.ui.graphics.Color.Transparent,
            contentWindowInsets = androidx.compose.foundation.layout.WindowInsets(0),
            snackbarHost = { ToastHost() },
        ) { _ -> Box(Modifier.fillMaxSize().tapHaptics()) {
        CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onBackground, LocalOpenProfile provides { id, name, media, room -> model.openProfile(id, name, media, room) }, LocalOpenMemberMenu provides { id, name, room -> model.openSheet(name, onOutcome = { o -> if (o is uniffi.morpheus_ffi.MenuOutcome.OpenRoom) model.openRoomWhenKnown(o.roomId) }) { model.morpheus.memberMenuRows(room, id) } }) {
        val rings by model.ringModel.rings.collectAsState()
            val sharing by ShareInbox.pending.collectAsState()
            if (sharing != null && screen is Screen.SignedIn) ShareScreen(model, sharing!!)
            // A share waits for the session (restoring) and for signing in; it is not lost.
            else if (sharing != null && screen !is Screen.SignedOut) CircularProgressIndicator(Modifier.align(Alignment.Center).testTag("share-waiting"))
            else when (val current = screen) {
                Screen.Loading -> {
                    androidx.compose.runtime.SideEffect { dev.morpheus.app.AppState.loadingSeen = true }
                    CircularProgressIndicator(Modifier.align(Alignment.Center).testTag("loading"))
                }
                Screen.SignedOut -> SignInScreen(model)
                // One branch for both: the last state is drawn from the cache while the session is restored behind it,
                // and the screens (a room's composer with what is typed in it) stay as they are when the session is back.
                is Screen.Restoring, is Screen.SignedIn -> {
                    val restoring = current is Screen.Restoring
                    val account = (current as? Screen.Restoring)?.account ?: (current as Screen.SignedIn).account
                    val signedIn = Screen.SignedIn(account)
                    val room by model.openRoom.collectAsState()
                    val open = room
                    val warm by model.warmRoom.collectAsState()
                    // A press that was not a tap lets the warm chat go after a moment.
                    androidx.compose.runtime.LaunchedEffect(warm) { if (warm != null) { kotlinx.coroutines.delay(1500); if (model.openRoom.value == null) model.cool() } }
                    val securityOpen by model.securityOpen.collectAsState()
                    val security by model.security.collectAsState()
                    val shown = security
                    val newOpen by model.newOpen.collectAsState()
                    val inboxOpen by model.inboxOpen.collectAsState()
                    val switcherOpen by model.switcherOpen.collectAsState()
                    val settingsOpen by model.settingsOpen.collectAsState()
                    val bridgesOpen by model.bridgesOpen.collectAsState()
                    val reorderOpen by model.reorderOpen.collectAsState()
                    val call by model.calls.snapshot.collectAsState()
                    val callOpen by model.calls.open.collectAsState()
                    val going = call
                    val inPip by model.calls.pip.collectAsState()
                    if (going != null && inPip) PipCall(model.calls, going)
                    else Box(Modifier.fillMaxSize()) {
                    // The call screen is drawn over the lists, which stay composed under it: minimising or hanging up shows them at once, not after composing them again.
                    val covered = going != null && callOpen
                    androidx.compose.foundation.layout.Column(Modifier.fillMaxSize().then(if (covered) Modifier.clearAndSetSemantics { } else Modifier)) {
                        Box(Modifier.weight(1f)) {
                            // The room covers the lists and the lists under it stay exactly where they are (they are only drawn while the room is
                            // moving or not open; #394). It comes from its own edge (a DM from the left, a channel from the right: the way the finger
                            // moves that opens it) and goes back out to it; a finger drag sets its place while it is down (#464).
                            var lastRoom by remember { mutableStateOf(open) }
                            if (open != null) lastRoom = open
                            else if (warm != null && model.slide.f.value < 0.001f) lastRoom = warm
                            val slide = model.slide
                            androidx.compose.runtime.LaunchedEffect(open != null, slide.dragging) {
                                if (!slide.dragging) {
                                    // The chat's first composition and layout is a long frame; the slide starts after it, or the frame's time is spent
                                    // from the slide's 300 ms and the pane seems to jump (the curve is fast at its start).
                                    if (open != null && slide.f.value < 0.001f && warm?.id != open.id) { androidx.compose.runtime.withFrameNanos { }; androidx.compose.runtime.withFrameNanos { } }
                                    slide.f.animateTo(if (open != null) 1f else 0f, Motion.slide())
                                }
                            }
                            val at = slide.f.value
                            val listsDrawn = open == null || at < 0.999f
                            if (listsDrawn) {
                                Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).onGloballyPositioned { dev.morpheus.app.AppState.slide(0, it.positionInWindow().x) }) {
                                    // A pushed screen goes back with a swipe either way (#507).
                                    if (settingsOpen) SwipeBack(Modifier.fillMaxSize()) { if (bridgesOpen) BridgesScreen(model) else SettingsScreen(model) }
                                    else if (reorderOpen) SwipeBack(Modifier.fillMaxSize()) { ReorderScreen(model) }
                                    else if (newOpen) SwipeBack(Modifier.fillMaxSize()) { NewScreen(model) }
                                    else if (inboxOpen) SwipeBack(Modifier.fillMaxSize()) { InboxScreen(model) }
                                    else if (securityOpen && shown != null) SwipeBack(Modifier.fillMaxSize()) { SecurityScreen(model, shown) }
                                    else SignedInScreen(model, signedIn, restoring = restoring) { model.open(it) }
                                }
                            }
                            val shownRoom = lastRoom
                            if (shownRoom != null && (open != null || at > 0.001f || warm != null)) {
                                val sign = if (slide.leaveSign != 0) slide.leaveSign else if (uniffi.morpheus_ffi.chatEdgeIsLeft(shownRoom.isDm)) -1 else 1
                                Box(
                                    Modifier.fillMaxSize()
                                        .layout { m, c -> val p = m.measure(c); layout(p.width, p.height) { p.place((sign * (1f - at) * p.width).toInt(), 0) } }
                                        .testTag("chat-page")
                                        .background(MaterialTheme.colorScheme.background)
                                        .onGloballyPositioned { dev.morpheus.app.AppState.slide(1, it.positionInWindow().x) },
                                ) { RoomScreen(model, shownRoom) }
                            }
                            RoomPageHost(model)
                            if (going != null && !callOpen) FloatingCall(model.calls, going)
                            if (switcherOpen) SwipeBack(Modifier.fillMaxSize()) { QuickFind(model) }
                        }
                    }
                    if (covered && going != null) Box(Modifier.fillMaxSize()) {
                        CallScreen(model.calls, going) { model.openRoomWhenKnown(going.roomId); model.calls.show(false) }
                    }
                    }
                }
            }
            // Adding an account: the sign-in screen over the signed-in one; back goes back.
            val adding by model.addingAccount.collectAsState()
            if (adding && screen is Screen.SignedIn) {
                androidx.activity.compose.BackHandler { model.cancelAddAccount() }
                Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) { SignInScreen(model) }
            }
            if (screen is Screen.SignedIn) ActionSheetHost(model)
            if (screen is Screen.SignedIn) {
                val profile by model.profile.collectAsState()
                profile?.let { ProfileSheet(model, it) { model.closeProfile() } }
            }
            rings.lastOrNull()?.takeIf { screen is Screen.SignedIn }?.let { ring ->
                Box(Modifier.align(Alignment.TopCenter)) { RingCard(ring, { model.answerRing(ring) }, { model.declineRing(ring) }) }
            }
        }
        } }
    }
}
