package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
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
            when (val current = screen) {
                Screen.Loading -> CircularProgressIndicator(
                    Modifier.align(Alignment.Center).testTag("loading"),
                )
                Screen.SignedOut -> SignInScreen(model)
                is Screen.SignedIn -> {
                    val room by model.openRoom.collectAsState()
                    val open = room
                    val securityOpen by model.securityOpen.collectAsState()
                    val security by model.security.collectAsState()
                    val shown = security
                    if (open != null) RoomScreen(model, open)
                    else if (securityOpen && shown != null) SecurityScreen(model, shown)
                    else SignedInScreen(model, current) { model.open(it) }
                }
            }
        }
    }
}
