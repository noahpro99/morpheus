package dev.morpheus.app.ui

import android.graphics.BitmapFactory
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.CallModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.CallCard
import uniffi.morpheus_ffi.CallPerson
import uniffi.morpheus_ffi.CallPhase
import uniffi.morpheus_ffi.CallSnapshot
import uniffi.morpheus_ffi.CardKind
import uniffi.morpheus_ffi.callTimer

/** The text of the call's timer, ticking each second. */
@Composable
private fun timerText(snapshot: CallSnapshot): String? {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(1000)
            now = System.currentTimeMillis()
        }
    }
    return callTimer(now.toULong(), snapshot.startedMs)
}

private val speakingGreen = Color(0xFF3BA55D)

/** The call, full screen: a card for each person (and each shared screen), and the controls. */
@Composable
fun CallScreen(calls: CallModel, snapshot: CallSnapshot) {
    BackHandler { if (snapshot.phase == CallPhase.FAILED) calls.dismiss() else calls.show(false) }
    val speaker by calls.speaker.collectAsState()
    var panel by remember { mutableStateOf<String?>(null) }
    Column(Modifier.fillMaxSize().background(Palette.Surface).testTag("call-screen")) {
        Row(Modifier.fillMaxWidth().background(Palette.Background).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton({ if (snapshot.phase == CallPhase.FAILED) calls.dismiss() else calls.show(false) }, Modifier.testTag("call-minimize")) { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_down, size = 28.dp) }
            Column(Modifier.weight(1f)) {
                Text(snapshot.roomName, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.testTag("call-title"))
                Text(
                    when (snapshot.phase) {
                        CallPhase.JOINING -> t("Connecting…")
                        CallPhase.CONNECTED -> t("Connected")
                        CallPhase.FAILED -> t("Not connected")
                        CallPhase.ENDED -> t("Ended")
                    },
                    Modifier.testTag("call-state"), color = Palette.Muted, style = MaterialTheme.typography.bodySmall,
                )
            }
            if (snapshot.phase == CallPhase.CONNECTED) timerText(snapshot)?.let { Text(it, Modifier.padding(end = 12.dp).testTag("call-timer"), color = Palette.Muted) }
        }
        snapshot.error?.let { Text(it, Modifier.padding(16.dp).testTag("call-error"), color = Palette.Danger) }
        // The volume panels are drawn in place of the cards (not as popups, which the e2e bridge cannot see).
        val person = panel?.let { id -> snapshot.people.firstOrNull { it.userId == id } }
        if (panel == SETTINGS_PANEL) {
            CallSettings(calls) { panel = null }
        } else if (panel != null && person != null) {
            PersonVolumePanel(calls, person) { panel = null }
        } else {
            LazyVerticalGrid(
                GridCells.Fixed(if (snapshot.cards.size <= 1) 1 else 2),
                Modifier.weight(1f).padding(8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(snapshot.cards, key = { it.key }) { card -> Card(calls, snapshot, card) { panel = it } }
            }
        }
        Controls(calls, snapshot, speaker) { panel = if (panel == SETTINGS_PANEL) null else SETTINGS_PANEL }
    }
}

@Composable
private fun Card(calls: CallModel, snapshot: CallSnapshot, card: CallCard, openVolume: (String) -> Unit) {
    val person = snapshot.people.firstOrNull { it.userId == card.userId }
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(if (card.kind == CardKind.PERSON) 1f else 16f / 9f)
            .clip(RoundedCornerShape(12.dp))
            .background(Palette.Raised)
            .border(if (card.speaking) 3.dp else 0.dp, if (card.speaking) speakingGreen else Color.Transparent, RoundedCornerShape(12.dp))
            .testTag(if (card.kind == CardKind.SCREEN || card.kind == CardKind.UNWATCHED_SCREEN) "card-screen-${card.name}" else "card-${card.name}"),
        contentAlignment = Alignment.Center,
    ) {
        when (card.kind) {
            CardKind.PERSON -> Avatar(card.avatar, card.name, 64.dp)
            CardKind.CAMERA, CardKind.SCREEN -> VideoPicture(calls, card)
            CardKind.UNWATCHED_SCREEN -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(t("%{name}'s screen", "name" to card.name), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
                Button({ calls.watch(card.userId, true) }, Modifier.testTag("watch-${card.name}")) { Text(t("Watch")) }
            }
        }
        if (card.kind == CardKind.SCREEN) {
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_x, t("Stop watching"), "stop-watching-${card.name}", { calls.watch(card.userId, false) }, Modifier.align(Alignment.TopEnd), tint = Color.White)
        }
        if ((card.kind == CardKind.PERSON || card.kind == CardKind.CAMERA) && person != null && !person.me) {
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_volume_2, t("Volume"), "person-volume-${card.name}", { openVolume(card.userId) }, Modifier.align(Alignment.TopEnd), tint = Color.White)
        }
        Row(
            Modifier.align(Alignment.BottomStart).padding(8.dp).background(Color(0x99000000), RoundedCornerShape(6.dp)).padding(horizontal = 6.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (card.speaking) Box(Modifier.size(8.dp).clip(CircleShape).background(speakingGreen).testTag("speaking-${card.name}"))
            Text(
                if (card.kind == CardKind.SCREEN || card.kind == CardKind.UNWATCHED_SCREEN) t("%{name}'s screen", "name" to card.name) else card.name,
                color = Color.White, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis,
            )
            if (card.kind != CardKind.SCREEN && card.kind != CardKind.UNWATCHED_SCREEN) {
                if (person?.deafened == true) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_headphone_off, Modifier.testTag("deafened-${card.name}"), size = 16.dp, tint = Color.White)
                else if (person?.muted == true) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_mic_off, Modifier.testTag("muted-${card.name}"), size = 16.dp, tint = Color.White)
            }
        }
    }
}

/** A camera or a shared screen: the pictures of its feed as they arrive. */
@Composable
private fun VideoPicture(calls: CallModel, card: CallCard) {
    val feed = card.feed ?: return
    var picture by remember(feed) { mutableStateOf<ImageBitmap?>(null) }
    LaunchedEffect(feed) {
        var seen = -1L
        while (true) {
            val frame = withContext(Dispatchers.Default) { calls.frame(feed) }
            if (frame != null && frame.seq.toLong() != seen) {
                seen = frame.seq.toLong()
                picture = withContext(Dispatchers.Default) { BitmapFactory.decodeByteArray(frame.jpeg, 0, frame.jpeg.size)?.asImageBitmap() }
            }
            delay(100)
        }
    }
    val shown = picture
    if (shown != null) {
        Image(shown, card.name, Modifier.fillMaxSize().testTag(if (card.kind == CardKind.SCREEN) "video-screen-${card.name}" else "video-${card.name}"), contentScale = ContentScale.Fit)
    } else {
        Text("…", color = Palette.Muted)
    }
}

@Composable
private fun Controls(calls: CallModel, snapshot: CallSnapshot, speaker: Boolean, openSettings: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(Palette.Background).padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.SpaceEvenly,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (snapshot.phase == CallPhase.FAILED) {
            Button({ calls.dismiss() }, Modifier.testTag("call-dismiss")) { Text(t("Close")) }
            return@Row
        }
        Control(if (snapshot.muted) dev.morpheus.app.R.drawable.ic_lucide_mic_off else dev.morpheus.app.R.drawable.ic_lucide_mic, t("Mute"), "call-mute", snapshot.muted) { calls.toggleMute() }
        Control(if (snapshot.deafened) dev.morpheus.app.R.drawable.ic_lucide_headphone_off else dev.morpheus.app.R.drawable.ic_lucide_headphones, t("Deafen"), "call-deafen", snapshot.deafened) { calls.toggleDeafen() }
        if (snapshot.canSendVideo) Control(dev.morpheus.app.R.drawable.ic_lucide_video, t("Camera"), "call-camera", snapshot.camera) { calls.toggleCamera() }
        if (snapshot.camera && calls.canFlipCamera) Control(dev.morpheus.app.R.drawable.ic_lucide_refresh_cw, t("Flip camera"), "call-camera-flip", false) { calls.flipCamera() }
        Control(if (speaker) dev.morpheus.app.R.drawable.ic_lucide_volume_2 else dev.morpheus.app.R.drawable.ic_lucide_phone, t("Speaker"), "call-speaker", speaker) { calls.toggleSpeaker() }
        Control(dev.morpheus.app.R.drawable.ic_lucide_settings, t("Call settings"), "call-settings", false) { openSettings() }
        androidx.compose.material3.FilledIconButton(
            { calls.hangUp() },
            Modifier.size(48.dp).semantics { contentDescription = t("Hang up") }.testTag("call-hangup"),
            colors = androidx.compose.material3.IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.error, contentColor = MaterialTheme.colorScheme.onError),
        ) { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_phone_off) }
    }
}

@Composable
private fun Control(@androidx.annotation.DrawableRes icon: Int, description: String, tag: String, on: Boolean, onClick: () -> Unit) {
    // On: the primary colour with its on-colour; off: the raised surface with the text colour.
    androidx.compose.material3.FilledIconToggleButton(
        on, { onClick() },
        Modifier.size(48.dp).semantics { contentDescription = description }.testTag(tag),
        colors = androidx.compose.material3.IconButtonDefaults.filledIconToggleButtonColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh, contentColor = MaterialTheme.colorScheme.onSurface,
            checkedContainerColor = MaterialTheme.colorScheme.primary, checkedContentColor = MaterialTheme.colorScheme.onPrimary,
        ),
    ) { GlyphIcon(icon) }
}

/** The call going, as a bar over the rest of the app: back to it, mute, hang up. */
@Composable
fun CallBar(calls: CallModel, snapshot: CallSnapshot) {
    Row(
        Modifier.fillMaxWidth().background(Palette.Accent).clickable { calls.show(true) }.padding(horizontal = 12.dp, vertical = 6.dp).testTag("call-bar"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(snapshot.roomName, color = Palette.OnAccent, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (snapshot.phase == CallPhase.CONNECTED) t("In a call") else t("Connecting…"),
                color = Palette.OnAccent, style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("call-bar-state"),
            )
        }
        IconAction(if (snapshot.muted) dev.morpheus.app.R.drawable.ic_lucide_mic_off else dev.morpheus.app.R.drawable.ic_lucide_mic, t("Mute"), "bar-mute", { calls.toggleMute() }, tint = onColour(Palette.Accent))
        IconAction(dev.morpheus.app.R.drawable.ic_lucide_phone_off, t("Hang up"), "bar-hangup", { calls.hangUp() }, tint = onColour(Palette.Accent))
    }
}

private const val SETTINGS_PANEL = "settings"

/** A volume from 0 to 2 as a percentage, in steps of a tenth. */
private fun steps(v: Float) = (Math.round(v.coerceIn(0f, 2f) * 10) / 10f)

/** A volume: less, the percentage, more, and a slider; each tagged `<tag>-less`, `-label`, `-more`, `-slider`. */
@Composable
private fun VolumeRow(label: String, tag: String, start: Float, onChange: (Float) -> Unit) {
    var value by remember { mutableFloatStateOf(steps(start)) }
    Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(label, color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_minus, t("Less"), "$tag-less", { value = steps(value - 0.1f); onChange(value) })
            Text("${Math.round(value * 100)}%", Modifier.testTag("$tag-label"))
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_plus, t("More"), "$tag-more", { value = steps(value + 0.1f); onChange(value) })
            Slider(value, { value = steps(it); onChange(value) }, Modifier.weight(1f).testTag("$tag-slider"), valueRange = 0f..2f)
        }
    }
}

/** How loud one person plays for me, and silencing them for me alone. */
@Composable
private fun ColumnScope.PersonVolumePanel(calls: CallModel, person: CallPerson, close: () -> Unit) {
    val kept = remember(person.userId) { calls.personVolume(person.userId) }
    var volume by remember(person.userId) { mutableFloatStateOf(kept.volume) }
    var muted by remember(person.userId) { mutableStateOf(kept.muted) }
    Column(Modifier.weight(1f).fillMaxWidth().padding(16.dp).testTag("person-volume-panel")) {
        Text(person.name, fontWeight = FontWeight.Bold)
        VolumeRow(t("Volume"), "person-volume", kept.volume) { volume = it; calls.setPersonVolume(person.userId, volume, muted) }
        Button({ muted = !muted; calls.setPersonVolume(person.userId, volume, muted) }, Modifier.testTag("person-volume-mute")) {
            Text(if (muted) t("Unmute for me") else t("Mute for me"))
        }
        TextButton(close, Modifier.testTag("person-volume-close")) { Text(t("Close")) }
    }
}

/** The call's own volumes and where its sound goes. */
@Composable
private fun ColumnScope.CallSettings(calls: CallModel, close: () -> Unit) {
    val kept = remember { calls.volumes() }
    var tick by remember { mutableIntStateOf(0) }
    Column(Modifier.weight(1f).fillMaxWidth().padding(16.dp).testTag("call-settings-panel")) {
        VolumeRow(t("Output volume"), "output-volume", kept.output) { calls.setOutputVolume(it) }
        VolumeRow(t("Microphone volume"), "input-volume", kept.input) { calls.setInputVolume(it) }
        Text(t("Sound goes to"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = 8.dp))
        tick
        for (route in calls.routes()) {
            ChoiceRow(route.label, route.on, "route-${route.key}") { calls.chooseRoute(route.key); tick++ }
        }
        TextButton(close, Modifier.testTag("call-settings-close")) { Text(t("Close")) }
    }
}
