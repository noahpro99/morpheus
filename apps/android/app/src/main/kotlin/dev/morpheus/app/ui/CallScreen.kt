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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items as listItems
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
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.offset
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.input.nestedscroll.nestedScroll
import kotlinx.coroutines.launch
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
import androidx.compose.ui.semantics.stateDescription
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
import uniffi.morpheus_ffi.callCardMenu
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

internal val speakingGreen = Color(0xFF3BA55D)

/** The call, full screen: a card for each person (and each shared screen), and the controls. */
@Composable
fun CallScreen(calls: CallModel, snapshot: CallSnapshot, onChat: () -> Unit = {}) {
    BackHandler { if (snapshot.phase == CallPhase.FAILED) calls.dismiss() else calls.show(false) }
    val speaker by calls.speaker.collectAsState()
    var panel by remember { mutableStateOf<String?>(null) }
    var focused by remember { mutableStateOf<String?>(null) }
    var showPing by remember { mutableStateOf(false) }
    var signal by remember { mutableStateOf(uniffi.morpheus_ffi.callConnectionSignal()) }
    LaunchedEffect(snapshot.phase) { while (snapshot.phase == CallPhase.CONNECTED) { signal = uniffi.morpheus_ffi.callConnectionSignal(); delay(1000) } }
    val hidden by calls.hiddenVideo.collectAsState()
    // A swipe down minimises the call to its strip (#474): the screen follows the finger, a short slow drag springs back. A drag the call's own
    // lists or sliders take (they consume it first) is theirs.
    val drop = remember { androidx.compose.animation.core.Animatable(0f) }
    var height by remember { mutableFloatStateOf(1f) }
    var speed by remember { mutableFloatStateOf(0f) }
    // How far the finger has pulled, kept here at once (the animatable follows through a coroutine, so it lags at the moment the finger lifts).
    val pulled = remember { floatArrayOf(0f) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    // The cards' grid takes vertical drags first: what it cannot scroll (a pull down at its top) comes back here as leftover and pulls the screen.
    val release: (Float) -> Unit = { fling ->
        val goes = pulled[0] > height * 0.2f || fling > 1200f
        scope.launch {
            if (goes && snapshot.phase != CallPhase.FAILED) {
                drop.animateTo(height, Motion.slide())
                calls.show(false)
                drop.snapTo(0f)
            } else drop.animateTo(0f, Motion.slide())
        }
    }
    val pull = remember {
        object : androidx.compose.ui.input.nestedscroll.NestedScrollConnection {
            override fun onPreScroll(available: androidx.compose.ui.geometry.Offset, source: androidx.compose.ui.input.nestedscroll.NestedScrollSource): androidx.compose.ui.geometry.Offset {
                if (pulled[0] > 0f && available.y < 0f) {
                    val back = available.y.coerceAtLeast(-pulled[0])
                    pulled[0] += back
                    val to = pulled[0]
                    scope.launch { drop.snapTo(to) }
                    return androidx.compose.ui.geometry.Offset(0f, back)
                }
                return androidx.compose.ui.geometry.Offset.Zero
            }

            override fun onPostScroll(consumed: androidx.compose.ui.geometry.Offset, available: androidx.compose.ui.geometry.Offset, source: androidx.compose.ui.input.nestedscroll.NestedScrollSource): androidx.compose.ui.geometry.Offset {
                if (available.y > 0f) {
                    pulled[0] += available.y
                    val to = pulled[0]
                    scope.launch { drop.snapTo(to) }
                    return androidx.compose.ui.geometry.Offset(0f, available.y)
                }
                return androidx.compose.ui.geometry.Offset.Zero
            }

            override suspend fun onPostFling(consumed: androidx.compose.ui.unit.Velocity, available: androidx.compose.ui.unit.Velocity): androidx.compose.ui.unit.Velocity {
                if (pulled[0] > 0f) release(available.y + consumed.y)
                return available
            }
        }
    }
    Column(
        Modifier.fillMaxSize()
            .onSizeChanged { height = it.height.toFloat().coerceAtLeast(1f) }
            .offset { androidx.compose.ui.unit.IntOffset(0, drop.value.toInt()) }
            .nestedScroll(pull)
            .pointerInput(snapshot.phase) {
                detectVerticalDragGestures(
                    onDragStart = { speed = 0f; pulled[0] = 0f },
                    onDragEnd = { release(speed) },
                    onDragCancel = { scope.launch { drop.animateTo(0f, Motion.slide()) } },
                ) { change, amount ->
                    val dt = (change.uptimeMillis - change.previousUptimeMillis).coerceAtLeast(1L)
                    speed = amount / dt * 1000f
                    pulled[0] = (pulled[0] + amount).coerceAtLeast(0f)
                    val to = pulled[0]
                    scope.launch { drop.snapTo(to) }
                }
            }
            .background(Palette.Surface).testTag("call-screen"),
    ) {
        Row(Modifier.fillMaxWidth().background(Palette.Background).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton({ if (snapshot.phase == CallPhase.FAILED) calls.dismiss() else calls.show(false) }, Modifier.testTag("call-minimize")) { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_down, size = 28.dp) }
            Column(Modifier.weight(1f)) {
                Text(snapshot.roomName, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.testTag("call-title"), style = MaterialTheme.typography.bodyLarge)
                Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    when (snapshot.phase) {
                        CallPhase.JOINING -> t("Connecting…")
                        CallPhase.CONNECTED -> t("Connected")
                        CallPhase.FAILED -> t("Not connected")
                        CallPhase.ENDED -> t("Ended")
                    },
                    Modifier.testTag("call-state"), color = Palette.Muted, style = MaterialTheme.typography.bodySmall,
                )
                if (showPing && signal.quality != "unknown") Text(signal.tooltip, Modifier.padding(start = 8.dp).testTag("call-signal-detail"), color = Palette.Muted, style = MaterialTheme.typography.bodySmall)
                }
            }
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_message_circle, t("Open chat"), "call-chat", onChat)
            if (snapshot.phase == CallPhase.CONNECTED && android.os.Build.VERSION.SDK_INT >= 26) IconAction(dev.morpheus.app.R.drawable.ic_lucide_minimize_2, t("Picture in picture"), "call-pip", { calls.enterPip() })
            if (snapshot.phase == CallPhase.CONNECTED) SignalBars(signal) { showPing = !showPing }
            if (snapshot.phase == CallPhase.CONNECTED) IconAction(dev.morpheus.app.R.drawable.ic_lucide_user_plus, t("Invite to call"), "call-invite-button", { panel = if (panel == INVITE_PANEL) null else INVITE_PANEL })
            if (snapshot.phase == CallPhase.CONNECTED) timerText(snapshot)?.let { Text(it, Modifier.padding(end = 12.dp).testTag("call-timer"), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge) }
        }
        snapshot.error?.let { Text(it, Modifier.padding(16.dp).testTag("call-error"), color = Palette.Danger, style = MaterialTheme.typography.bodyLarge) }
        // The volume panels are drawn in place of the cards (not as popups, which the e2e bridge cannot see).
        val screenPanel = panel?.startsWith(SCREEN_PREFIX) == true
        val person = panel?.removePrefix(SCREEN_PREFIX)?.let { id -> snapshot.people.firstOrNull { it.userId == id } }
        if (panel == SETTINGS_PANEL) {
            CallSettings(calls) { panel = null }
        } else if (panel == INVITE_PANEL) {
            InvitePanel(calls, snapshot) { panel = null }
        } else if (panel == OUTPUT_PANEL) {
            OutputPanel(calls) { panel = null }
        } else if (panel != null && person != null) {
            CardMenuPanel(calls, snapshot, person, screenPanel) { panel = null }
        } else if (snapshot.cards.firstOrNull { it.key == focused } != null) {
            FocusedCard(calls, snapshot.cards.first { it.key == focused }) { focused = null }
        } else {
            LazyVerticalGrid(
                GridCells.Fixed(if (snapshot.cards.size <= 1) 1 else 2),
                Modifier.weight(1f).padding(8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(snapshot.cards, key = { it.key }) { card -> Card(calls, snapshot, card, card.userId in hidden, { focused = card.key }) { panel = it } }
            }
        }
        Controls(calls, snapshot, speaker, openOutput = { panel = if (panel == OUTPUT_PANEL) null else OUTPUT_PANEL }) { panel = if (panel == SETTINGS_PANEL) null else SETTINGS_PANEL }
    }
}

@Composable
private fun Card(calls: CallModel, snapshot: CallSnapshot, card: CallCard, videoHidden: Boolean, focus: () -> Unit, openVolume: (String) -> Unit) {
    val person = snapshot.people.firstOrNull { it.userId == card.userId }
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(if (card.kind == CardKind.PERSON) 1f else 16f / 9f)
            .clip(RoundedCornerShape(12.dp))
            .background(Palette.Raised)
            .pointerInput(card.kind) { if (card.kind != CardKind.UNWATCHED_SCREEN) detectTapGestures(onTap = { focus() }) }
            .border(if (card.speaking) 4.dp else 0.dp, if (card.speaking) speakingGreen else Color.Transparent, RoundedCornerShape(12.dp))
            .testTag(if (card.kind == CardKind.SCREEN || card.kind == CardKind.UNWATCHED_SCREEN) "card-screen-${card.name}" else "card-${card.name}"),
        contentAlignment = Alignment.Center,
    ) {
        when (card.kind) {
            CardKind.PERSON -> Avatar(card.avatar, card.name, 64.dp)
            CardKind.CAMERA -> if (videoHidden) Avatar(card.avatar, card.name, 64.dp) else VideoPicture(calls, card)
            CardKind.SCREEN -> VideoPicture(calls, card)
            CardKind.UNWATCHED_SCREEN -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(t("%{name}'s screen", "name" to card.name), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
                Button({ calls.watch(card.userId, true) }, Modifier.testTag("watch-${card.name}")) { Text(t("Watch")) }
            }
        }
        if (card.kind == CardKind.SCREEN) {
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_x, t("Stop watching"), "stop-watching-${card.name}", { calls.watch(card.userId, false) }, Modifier.align(Alignment.TopEnd), tint = Color.White)
            // The screen's own menu: its sound's volume and mute for me (#436).
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_volume_2, t("Stream volume"), "stream-menu-${card.name}", { openVolume(SCREEN_PREFIX + card.userId) }, Modifier.align(Alignment.TopStart), tint = Color.White)
        }
        if ((card.kind == CardKind.PERSON || card.kind == CardKind.CAMERA) && person != null && !person.me) {
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_volume_2, t("Volume"), "person-volume-${card.name}", { openVolume(card.userId) }, Modifier.align(Alignment.TopEnd), tint = Color.White)
        }
        Row(
            Modifier.align(Alignment.BottomStart).padding(8.dp).background(Color(0x99000000), RoundedCornerShape(8.dp)).padding(horizontal = 8.dp, vertical = 2.dp),
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
internal fun VideoPicture(calls: CallModel, card: CallCard, zoom: Modifier = Modifier) {
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
        Image(shown, card.name, Modifier.fillMaxSize().then(zoom).testTag(if (card.kind == CardKind.SCREEN) "video-screen-${card.name}" else "video-${card.name}"), contentScale = ContentScale.Fit)
    } else {
        Text("…", color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
private fun Controls(calls: CallModel, snapshot: CallSnapshot, speaker: Boolean, openOutput: () -> Unit, openSettings: () -> Unit) {
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
        if (snapshot.canShareScreen) Control(if (snapshot.people.any { it.me && it.screen }) dev.morpheus.app.R.drawable.ic_lucide_screen_share_off else dev.morpheus.app.R.drawable.ic_lucide_screen_share, t("Share screen"), "call-share", snapshot.people.any { it.me && it.screen }) { calls.toggleScreenShare() }
        if (snapshot.canSendVideo) Control(dev.morpheus.app.R.drawable.ic_lucide_video, t("Camera"), "call-camera", snapshot.camera) { calls.toggleCamera() }
        if (snapshot.camera && calls.canFlipCamera) Control(dev.morpheus.app.R.drawable.ic_lucide_refresh_cw, t("Flip camera"), "call-camera-flip", false) { calls.flipCamera() }
        Control(if (speaker) dev.morpheus.app.R.drawable.ic_lucide_volume_2 else dev.morpheus.app.R.drawable.ic_lucide_phone, t("Speaker"), "call-speaker", speaker) { openOutput() }
        Control(dev.morpheus.app.R.drawable.ic_lucide_settings, t("Call settings"), "call-settings", false) { openSettings() }
        androidx.compose.material3.FilledIconButton(
            { calls.hangUp() },
            Modifier.size(48.dp).semantics { contentDescription = t("Hang up") }.testTag("call-hangup"),
            shape = RoundedCornerShape(Radius.md),
            colors = androidx.compose.material3.IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.error, contentColor = MaterialTheme.colorScheme.onError),
        ) { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_phone_off) }
    }
}

@Composable
private fun Control(@androidx.annotation.DrawableRes icon: Int, description: String, tag: String, on: Boolean, onClick: () -> Unit) {
    // On: the primary colour with its on-colour; off: the raised surface with the text colour.
    // A rounded square like the desktop's call buttons (#495), a little rounder while on: the corner morphs on the M3 spatial spring.
    val corner by androidx.compose.animation.core.animateDpAsState(if (on) Radius.lg else Radius.md, androidx.compose.animation.core.tween(Motion.Short4, easing = Motion.Standard), label = "corner")
    androidx.compose.material3.FilledIconToggleButton(
        on, { onClick() },
        Modifier.size(48.dp).semantics { contentDescription = description }.testTag(tag),
        shape = RoundedCornerShape(corner),
        colors = androidx.compose.material3.IconButtonDefaults.filledIconToggleButtonColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh, contentColor = MaterialTheme.colorScheme.onSurface,
            checkedContainerColor = MaterialTheme.colorScheme.primary, checkedContentColor = MaterialTheme.colorScheme.onPrimary,
        ),
    ) { GlyphIcon(icon) }
}

private const val SETTINGS_PANEL = "settings"
private const val OUTPUT_PANEL = "output"
private const val INVITE_PANEL = "invite"
private const val SCREEN_PREFIX = "screen:"

/** A volume from 0 to 2 as a percentage, in steps of a tenth. */
internal fun steps(v: Float) = (Math.round(v.coerceIn(0f, 2f) * 10) / 10f)

/** A volume: less, the percentage, more, and a slider; each tagged `<tag>-less`, `-label`, `-more`, `-slider`. */
@Composable
internal fun VolumeRow(label: String, tag: String, start: Float, onChange: (Float) -> Unit) {
    var value by remember { mutableFloatStateOf(steps(start)) }
    Column(Modifier.fillMaxWidth().padding(vertical = 8.dp)) {
        Text(label, color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_minus, t("Less"), "$tag-less", { value = steps(value - 0.1f); onChange(value) })
            Text("${Math.round(value * 100)}%", Modifier.testTag("$tag-label"), style = MaterialTheme.typography.bodyLarge)
            IconAction(dev.morpheus.app.R.drawable.ic_lucide_plus, t("More"), "$tag-more", { value = steps(value + 0.1f); onChange(value) })
            Slider(value, { value = steps(it); onChange(value) }, Modifier.weight(1f).testTag("$tag-slider"), valueRange = 0f..2f)
        }
    }
}

/**
 * The menu of one call card (#436): the rows the desktop's right-click offers (`call_card_menu`, from
 * `view_model::calls::call_card_items`): the person's volume and mute for me, a camera's Hide video, a
 * shared screen's Stop watching and its sound's volume and mute. Drawn in place of the cards.
 */
@Composable
private fun ColumnScope.CardMenuPanel(calls: CallModel, snapshot: CallSnapshot, person: CallPerson, screen: Boolean, close: () -> Unit) {
    val kept = remember(person.userId) { calls.personVolume(person.userId) }
    var volume by remember(person.userId, screen) { mutableFloatStateOf(if (screen) kept.stream else kept.volume) }
    var muted by remember(person.userId, screen) { mutableStateOf(if (screen) kept.streamMuted else kept.muted) }
    val hidden by calls.hiddenVideo.collectAsState()
    val watching = snapshot.cards.any { it.userId == person.userId && it.kind == CardKind.SCREEN }
    val kind = if (screen) CardKind.SCREEN else if (person.camera || person.userId in hidden || snapshot.cards.any { it.userId == person.userId && it.kind == CardKind.CAMERA }) CardKind.CAMERA else CardKind.PERSON
    val rows = callCardMenu(person.userId, kind, false, volume, muted, person.userId in hidden, watching)
    Column(Modifier.weight(1f).fillMaxWidth().padding(16.dp).testTag("person-volume-panel")) {
        Text(person.name, fontWeight = FontWeight.Bold, style = MaterialTheme.typography.bodyLarge)
        for (row in rows) {
            when (row.action) {
                "volume" -> VolumeRow(row.label, "person-volume", kept.volume) { volume = it; calls.setPersonVolume(person.userId, volume, muted) }
                "stream-volume" -> VolumeRow(row.label, "stream-volume", kept.stream) { volume = it; calls.setStreamVolume(person.userId, volume, muted) }
                "mute" -> Button({ muted = !muted; calls.setPersonVolume(person.userId, volume, muted) }, Modifier.testTag("person-volume-mute")) { Text(row.label) }
                "mute-stream" -> Button({ muted = !muted; calls.setStreamVolume(person.userId, volume, muted) }, Modifier.testTag("stream-volume-mute")) { Text(row.label) }
                "hide-video" -> TextButton({ calls.toggleHideVideo(person.userId) }, Modifier.testTag("card-hide-video").semantics { if (row.checked) stateDescription = "On" }) {
                    Text(row.label)
                    if (row.checked) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_check, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 8.dp))
                }
                "stop-watching" -> TextButton({ calls.watch(person.userId, false); close() }, Modifier.testTag("card-stop-watching")) { Text(row.label) }
            }
        }
        TextButton(close, Modifier.testTag("person-volume-close")) { Text(t("Close")) }
    }
}

/** The call's own settings: the microphone volume. The sound's volume is the system's (the volume keys, #496). */
@Composable
private fun ColumnScope.CallSettings(calls: CallModel, close: () -> Unit) {
    val kept = remember { calls.volumes() }
    Column(Modifier.weight(1f).fillMaxWidth().padding(16.dp).testTag("call-settings-panel")) {
        VolumeRow(t("Microphone volume"), "input-volume", kept.input) { calls.setInputVolume(it) }
        TextButton(close, Modifier.testTag("call-settings-close")) { Text(t("Close")) }
    }
}

/** Where the call's sound goes (#496): every output there is as a radio row, the current one chosen; a headset coming or going updates the list. */
@Composable
private fun ColumnScope.OutputPanel(calls: CallModel, close: () -> Unit) {
    var tick by remember { mutableIntStateOf(0) }
    val context = androidx.compose.ui.platform.LocalContext.current
    androidx.compose.runtime.DisposableEffect(Unit) {
        val audio = context.getSystemService(android.content.Context.AUDIO_SERVICE) as android.media.AudioManager
        val watch = object : android.media.AudioDeviceCallback() {
            override fun onAudioDevicesAdded(added: Array<out android.media.AudioDeviceInfo>?) { tick++ }
            override fun onAudioDevicesRemoved(removed: Array<out android.media.AudioDeviceInfo>?) { tick++ }
        }
        audio.registerAudioDeviceCallback(watch, null)
        onDispose { audio.unregisterAudioDeviceCallback(watch) }
    }
    Column(Modifier.weight(1f).fillMaxWidth().padding(16.dp).testTag("output-panel")) {
        Text(t("Sound goes to"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        tick
        for (route in calls.routes()) {
            ChoiceRow(route.label, route.on, "route-${route.key}") { calls.chooseRoute(route.key); tick++ }
        }
        TextButton(close, Modifier.testTag("output-close")) { Text(t("Close")) }
    }
}

/** Ask a friend into the call (#438): your friends, each with a button that invites them (a DM's call: the room's invite; a voice channel: a message with its link). */
@Composable
private fun ColumnScope.InvitePanel(calls: CallModel, snapshot: CallSnapshot, close: () -> Unit) {
    var friends by remember { mutableStateOf<List<uniffi.morpheus_ffi.FriendItem>?>(null) }
    var states by remember { mutableStateOf(mapOf<String, String>()) }
    val direct = calls.isDm(snapshot.roomId)
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    LaunchedEffect(Unit) { friends = try { calls.friends() } catch (_: Exception) { emptyList() } }
    Column(Modifier.weight(1f).fillMaxWidth().padding(16.dp).testTag("call-invite-panel")) {
        Text(if (direct) t("Invite to call") else t("Invite to voice channel"), fontWeight = FontWeight.Bold, style = MaterialTheme.typography.bodyLarge)
        Text(
            if (direct) t("They are invited to this conversation and can join the call.") else t("They get a message with a link that opens the channel."),
            color = Palette.Muted, style = MaterialTheme.typography.bodyMedium,
        )
        val list = friends
        if (list == null) Text("…", color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        else if (list.isEmpty()) Text(t("No friends yet"), Modifier.testTag("call-invite-none"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        androidx.compose.foundation.lazy.LazyColumn(Modifier.weight(1f)) {
            listItems(list.orEmpty(), key = { it.userId }) { friend ->
                val state = states[friend.userId]
                Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Avatar(friend.avatar, friend.name, 40.dp)
                    Text(friend.name, Modifier.weight(1f).padding(start = 12.dp), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
                    Button(
                        {
                            states = states + (friend.userId to "working")
                            scope.launch {
                                states = states + (friend.userId to try { calls.invite(snapshot.roomId, friend.userId); "done" } catch (e: Exception) { "failed" })
                            }
                        },
                        Modifier.testTag("call-invite-${friend.name}"),
                        enabled = state != "working" && state != "done",
                    ) {
                        Text(
                            when (state) {
                                "done" -> if (direct) t("Invited") else t("Sent")
                                "failed" -> t("Try again")
                                else -> if (direct) t("Invite") else t("Send invite")
                            },
                        )
                    }
                }
            }
        }
        TextButton(close, Modifier.testTag("call-invite-close")) { Text(t("Close")) }
    }
}

/** Three bars lit by how good the call's connection is (green good, yellow fair, red poor; #440); a tap shows the round-trip delay beside "Connected". */
@Composable
private fun SignalBars(signal: uniffi.morpheus_ffi.ConnectionSignal, onClick: () -> Unit) {
    val lit = when (signal.quality) {
        "good" -> Palette.Green
        "fair" -> Palette.Warn
        "poor" -> Palette.Danger
        else -> Palette.Muted
    }
    Box(
        Modifier.size(Size.target).clip(CircleShape).clickable(onClick = onClick)
            .semantics { contentDescription = t("Connection"); stateDescription = signal.quality }.testTag("call-signal"),
        contentAlignment = Alignment.Center,
    ) {
        Row(Modifier.height(16.dp), horizontalArrangement = Arrangement.spacedBy(2.dp), verticalAlignment = Alignment.Bottom) {
            for (i in 0 until 3) {
                Box(Modifier.width(4.dp).height((8 + 4 * i).dp).clip(RoundedCornerShape(1.dp)).background(if (i < signal.bars.toInt()) lit else Palette.Divider))
            }
        }
    }
}

/** The most a shared screen zooms in. */
private const val MAX_ZOOM = 5f

/**
 * One card filling the call's room (#435): a person's avatar, a camera, or a shared screen, which pinches to zoom (and pans while zoomed);
 * a double tap zooms to 2x and back. `focus-zoom` says the zoom ("1.0", "2.0"...). Back, or the close button, returns to the cards.
 */
@Composable
private fun ColumnScope.FocusedCard(calls: CallModel, card: CallCard, close: () -> Unit) {
    BackHandler { close() }
    var zoom by remember { mutableFloatStateOf(1f) }
    var pan by remember { mutableStateOf(androidx.compose.ui.geometry.Offset.Zero) }
    val zoomable = card.kind == CardKind.SCREEN
    Box(
        Modifier.weight(1f).fillMaxWidth().background(Color.Black).clip(RectangleShape).testTag("focus-view")
            .then(
                if (!zoomable) Modifier else Modifier
                    .pointerInput(Unit) {
                        detectTransformGestures { _, drag, scale, _ ->
                            zoom = (zoom * scale).coerceIn(1f, MAX_ZOOM)
                            pan = if (zoom == 1f) androidx.compose.ui.geometry.Offset.Zero else pan + drag
                        }
                    }
                    .pointerInput(Unit) {
                        detectTapGestures(onDoubleTap = {
                            zoom = if (zoom > 1f) 1f else 2f
                            pan = androidx.compose.ui.geometry.Offset.Zero
                        })
                    },
            ),
        contentAlignment = Alignment.Center,
    ) {
        when (card.kind) {
            CardKind.SCREEN, CardKind.CAMERA -> VideoPicture(calls, card, Modifier.graphicsLayer { scaleX = zoom; scaleY = zoom; translationX = pan.x; translationY = pan.y })
            else -> Avatar(card.avatar, card.name, 160.dp)
        }
        IconAction(dev.morpheus.app.R.drawable.ic_lucide_chevron_down, t("Back to the call"), "focus-close", close, Modifier.align(Alignment.TopStart), tint = Color.White)
        Text(
            if (card.kind == CardKind.SCREEN) t("%{name}'s screen", "name" to card.name) else card.name,
            Modifier.align(Alignment.BottomStart).padding(12.dp).testTag("focus-name"), color = Color.White, style = MaterialTheme.typography.bodyMedium,
        )
        if (zoomable) Box(Modifier.size(1.dp).semantics { stateDescription = "%.1f".format(java.util.Locale.ROOT, zoom) }.testTag("focus-zoom"))
    }
}

/** The call in the small picture-in-picture window (#433): its main picture alone (a shared screen, else a camera, else the first person), no controls. */
@Composable
fun PipCall(calls: CallModel, snapshot: CallSnapshot) {
    val card = snapshot.cards.firstOrNull { it.kind == CardKind.SCREEN }
        ?: snapshot.cards.firstOrNull { it.kind == CardKind.CAMERA && it.userId != snapshot.people.firstOrNull { p -> p.me }?.userId }
        ?: snapshot.cards.firstOrNull { it.kind == CardKind.PERSON && it.userId != snapshot.people.firstOrNull { p -> p.me }?.userId }
        ?: snapshot.cards.firstOrNull()
    Box(Modifier.fillMaxSize().background(Color.Black).testTag("pip-view"), contentAlignment = Alignment.Center) {
        if (card != null) when (card.kind) {
            CardKind.SCREEN, CardKind.CAMERA -> VideoPicture(calls, card)
            else -> Avatar(card.avatar, card.name, 64.dp)
        }
    }
}
