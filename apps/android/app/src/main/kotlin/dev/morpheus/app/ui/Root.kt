package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.LocalContentColor
import androidx.compose.animation.togetherWith
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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
        ) { _ -> Box(Modifier.fillMaxSize()) {
        CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onBackground) {
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
                    val securityOpen by model.securityOpen.collectAsState()
                    val security by model.security.collectAsState()
                    val shown = security
                    val newOpen by model.newOpen.collectAsState()
                    val settingsOpen by model.settingsOpen.collectAsState()
                    val bridgesOpen by model.bridgesOpen.collectAsState()
                    val reorderOpen by model.reorderOpen.collectAsState()
                    val call by model.calls.snapshot.collectAsState()
                    val callOpen by model.calls.open.collectAsState()
                    val going = call
                    if (going != null && callOpen) CallScreen(model.calls, going)
                    else androidx.compose.foundation.layout.Column(Modifier.fillMaxSize()) {
                        if (going != null) CallBar(model.calls, going)
                        Box(Modifier.weight(1f)) {
                            // The room covers the lists: it slides in over them from the right and out again, and the lists under it stay
                            // exactly where they are (they are only drawn while the room is moving or not open; #394).
                            var lastRoom by remember { mutableStateOf(open) }
                            if (open != null) lastRoom = open
                            val roomShown = remember { androidx.compose.animation.core.MutableTransitionState(open != null) }
                            roomShown.targetState = open != null
                            val listsDrawn = !roomShown.isIdle || !roomShown.currentState
                            val spec = androidx.compose.animation.core.spring<androidx.compose.ui.unit.IntOffset>(dampingRatio = 0.85f, stiffness = androidx.compose.animation.core.Spring.StiffnessMediumLow)
                            if (listsDrawn) {
                                Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).onGloballyPositioned { dev.morpheus.app.AppState.slide(0, it.positionInWindow().x) }) {
                                    if (settingsOpen) { if (bridgesOpen) BridgesScreen(model) else SettingsScreen(model) }
                                    else if (reorderOpen) ReorderScreen(model)
                                    else if (newOpen) NewScreen(model)
                                    else if (securityOpen && shown != null) SecurityScreen(model, shown)
                                    else SignedInScreen(model, signedIn, restoring = restoring) { model.open(it) }
                                }
                            }
                            androidx.compose.animation.AnimatedVisibility(
                                visibleState = roomShown,
                                enter = androidx.compose.animation.slideInHorizontally(spec) { it },
                                exit = androidx.compose.animation.slideOutHorizontally(spec) { it },
                            ) {
                                val shownRoom = lastRoom
                                Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).onGloballyPositioned { dev.morpheus.app.AppState.slide(1, it.positionInWindow().x) }) {
                                    if (shownRoom != null) RoomScreen(model, shownRoom)
                                }
                            }
                            RoomPageHost(model)
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
            rings.lastOrNull()?.takeIf { screen is Screen.SignedIn }?.let { ring ->
                Box(Modifier.align(Alignment.TopCenter)) { RingCard(ring, { model.answerRing(ring) }, { model.declineRing(ring) }) }
            }
        }
        } }
    }
}
