package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.morpheus.app.CallModel
import kotlinx.coroutines.delay
import uniffi.morpheus_ffi.CallListing
import uniffi.morpheus_ffi.callTimer

/**
 * What a voice channel's row says about each person in its call (#441), as the desktop's list does: muted or
 * deafened, a camera, LIVE for a shared screen, and, for someone speaking in the call this phone is in, a ring on
 * their name. Tags: `in-call-muted-`, `in-call-camera-`, `in-call-live-`, `in-call-speaking-` + `<room>-<name>`.
 */
@Composable
fun CallMemberMarks(roomId: String, room: String, name: String, userId: String, member: CallListing) {
    val speakers by CallModel.speakers.collectAsState()
    Row(horizontalArrangement = Arrangement.spacedBy(Space.xs), verticalAlignment = Alignment.CenterVertically) {
        if (speakers.first == roomId && userId in speakers.second) {
            androidx.compose.foundation.layout.Box(Modifier.padding(end = 2.dp).size(8.dp).clip(CircleShape).background(Palette.Green).testTag("in-call-speaking-$room-$name"))
        }
        if (member.camera) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_video, Modifier.testTag("in-call-camera-$room-$name"), size = 16.dp, tint = Palette.Muted)
        if (member.screen) Text(t("LIVE"), Modifier.testTag("in-call-live-$room-$name"), color = Palette.Danger, style = MaterialTheme.typography.labelSmall)
    }
}

/** How long the room's call has been going, ticking each second (the desktop row's timer). */
@Composable
fun CallRowTimer(room: String, startedMs: ULong?) {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(1000)
            now = System.currentTimeMillis()
        }
    }
    callTimer(now.toULong(), startedMs)?.let { Text(it, Modifier.testTag("call-row-timer-$room"), color = Palette.Muted, style = MaterialTheme.typography.labelMedium) }
}
