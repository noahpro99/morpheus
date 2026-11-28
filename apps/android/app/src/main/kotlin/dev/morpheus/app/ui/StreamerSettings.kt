package dev.morpheus.app.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import dev.morpheus.app.AppModel
import dev.morpheus.app.Streamer

/** Settings > Streamer mode (#540): hide names, IDs and notification words while the screen is shared, or by hand. */
@Composable
@Suppress("ModifierMissing") // a settings page: the host lays it out
fun StreamerPage(app: AppModel, t: (String) -> String) {
    var choices by remember { mutableStateOf(app.morpheus.streamerChoices()) }
    fun flip(key: String, now: Boolean) {
        app.morpheus.setStreamerChoice(key, !now)
        choices = app.morpheus.streamerChoices()
        Streamer.update(app.morpheus)
    }
    SmallHeading(t("Streamer mode"), Modifier.padding(horizontal = Space.lg))
    ToggleRow(t("Turn on now"), choices.now, "streamer-now", supporting = t("Hides what could give people away.")) { flip("now", choices.now) }
    ToggleRow(t("Turn on while I share my screen"), choices.auto, "streamer-auto") { flip("auto", choices.auto) }
    ToggleRow(t("Abbreviate names"), choices.names, "streamer-names", supporting = t("Alice shows as A•••.")) { flip("names", choices.names) }
    ToggleRow(t("Hide IDs and links"), choices.ids, "streamer-ids", supporting = t("User, room and device IDs, e-mail addresses, phone numbers and links show as dots.")) { flip("ids", choices.ids) }
    ToggleRow(t("Quiet notifications"), choices.notifications, "streamer-notifications", supporting = t("Notifications say nothing of who or what.")) { flip("notifications", choices.notifications) }
}
