package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.platform.testTag
import dev.morpheus.app.AppModel
import dev.morpheus.app.Screen

/** The screen the app is on. */
@OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)
@Composable
fun Root(model: AppModel) {
    val screen by model.screen.collectAsState()
    Box(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .safeDrawingPadding()
            // Test tags are resource ids to uiautomator as well.
            .semantics { testTagsAsResourceId = true },
    ) {
        CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onBackground) {
        val rings by model.ringModel.rings.collectAsState()
            when (val current = screen) {
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
                            if (open != null) RoomScreen(model, open)
                            else if (settingsOpen) { if (bridgesOpen) BridgesScreen(model) else SettingsScreen(model) }
                            else if (reorderOpen) ReorderScreen(model)
                            else if (newOpen) NewScreen(model)
                            else if (securityOpen && shown != null) SecurityScreen(model, shown)
                            else SignedInScreen(model, signedIn, restoring = restoring) { model.open(it) }
                        }
                    }
                }
            }
            rings.lastOrNull()?.takeIf { screen is Screen.SignedIn }?.let { ring ->
                Box(Modifier.align(Alignment.TopCenter)) { RingCard(ring, { model.answerRing(ring) }, { model.declineRing(ring) }) }
            }
        }
    }
}
