package dev.morpheus.app

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import dev.morpheus.app.ui.t
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.CallHandle
import uniffi.morpheus_ffi.CallListener
import uniffi.morpheus_ffi.CallPhase
import uniffi.morpheus_ffi.CallSnapshot
import uniffi.morpheus_ffi.Morpheus

/**
 * The call this account is in (one at a time). The call itself runs in Rust
 * (crates/ffi `calls`: signalling, media, who speaks, who is watched); this
 * keeps its latest [CallSnapshot], the speaker route, and the foreground
 * service that keeps the microphone working with the app in the background.
 */
class CallModel(private val morpheus: Morpheus, private val context: Context) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)

    /** The microphone, captured here while a call cleans it with the strong noise model (#531). */
    val micPump = MicPump()

    private val _snapshot = MutableStateFlow<CallSnapshot?>(null)

    /** What the call screen shows; null when there is no call. */
    val snapshot: StateFlow<CallSnapshot?> = _snapshot.asStateFlow()

    /** The full-screen call is open (otherwise a bar shows where the call is going). */
    private val _open = MutableStateFlow(false)
    val open: StateFlow<Boolean> = _open.asStateFlow()

    private val _pip = MutableStateFlow(false)

    /** The app is in a small picture-in-picture window (#433): the call shows only its main picture. */
    val pip: StateFlow<Boolean> = _pip.asStateFlow()

    private val _pipRequests = kotlinx.coroutines.flow.MutableSharedFlow<Unit>(extraBufferCapacity = 1)

    /** The call screen asks the activity to enter picture-in-picture. */
    val pipRequests: kotlinx.coroutines.flow.SharedFlow<Unit> = _pipRequests
    fun enterPip() { _pipRequests.tryEmit(Unit) }

    private val _spot = MutableStateFlow(dev.morpheus.app.ui.FloatSpot(right = true, yFraction = 0.6f))

    /** Where the floating call window (#528, #532) rests: a side and a height, kept while the app runs and across its screens. */
    val floatSpot: StateFlow<dev.morpheus.app.ui.FloatSpot> = _spot.asStateFlow()
    fun setFloatSpot(spot: dev.morpheus.app.ui.FloatSpot) { _spot.value = spot }

    fun setPip(on: Boolean) { _pip.value = on; AppState.pip = on }

    private val _speaker = MutableStateFlow(true)

    /** The call plays from the loudspeaker (Discord-style) rather than the earpiece. */
    val speaker: StateFlow<Boolean> = _speaker.asStateFlow()

    private var handle: CallHandle? = null

    /** Whether a room is a direct message (set by the app model): only a DM rings, so only a DM has a ringback (#439). */
    var isDm: (String) -> Boolean = { false }
    private var ringingSince: Long? = null
    private var lastRingback: Long? = null
    private var routed = false

    /** The room of the call going, or null. */
    val roomId: String? get() = _snapshot.value?.roomId

    fun join(roomId: String, roomName: String) {
        if (_snapshot.value?.roomId == roomId && _snapshot.value?.phase != CallPhase.ENDED && _snapshot.value?.phase != CallPhase.FAILED) {
            _open.value = true
            return
        }
        scope.launch {
            handle?.let { it.leave() }
            current = this@CallModel
            CallService.update(context, roomId, roomName)
            _open.value = true
            AppState.screen = "call"
            val listener = object : CallListener {
                override fun onCall(snapshot: CallSnapshot) {
                    scope.launch { changed(snapshot) }
                }
            }
            try {
                handle = morpheus.joinCall(roomId, listener)
            } catch (e: Exception) {
                CallService.update(context, "", "")
                _open.value = false
                _snapshot.value = failure(roomId, roomName, e.sentence())
            }
        }
    }

    private fun failure(roomId: String, roomName: String, why: String) = CallSnapshot(
        roomId, roomName, CallPhase.FAILED, why, false, false, false, false, false, emptyList(), emptyList(), 0u, null,
    )

    private fun changed(snapshot: CallSnapshot) {
        val before = _snapshot.value
        _snapshot.value = snapshot
        CallSounds.changed(before, snapshot)
        speakers.value = snapshot.roomId to snapshot.people.filter { it.speaking }.map { it.userId }.toSet()
        ringbackStep(snapshot)
        val sharingNow = snapshot.people.any { it.me && it.screen }
        if (Streamer.sharing != sharingNow) { Streamer.sharing = sharingNow; Streamer.update(morpheus) }
        if (snapshot.phase == CallPhase.CONNECTED) micPump.start() else if (snapshot.phase == CallPhase.ENDED || snapshot.phase == CallPhase.FAILED) micPump.stop()
        // The notification's Mute and Deafen say what the call is now.
        if (before != null && (before.muted != snapshot.muted || before.deafened != snapshot.deafened) && snapshot.phase != CallPhase.ENDED) {
            CallService.update(context, snapshot.roomId, snapshot.roomName, snapshot.muted, snapshot.deafened)
        }
        if (snapshot.phase == CallPhase.CONNECTED && !routed) {
            routed = true
            route(_speaker.value)
        }
        if (snapshot.phase == CallPhase.ENDED) {
            ended()
        }
    }

    /** The ringback repeats while this call rings the other person: connected, in a DM, nobody else there. */
    private fun ringbackStep(snapshot: CallSnapshot) {
        val out = snapshot.phase == CallPhase.CONNECTED && isDm(snapshot.roomId) && snapshot.people.none { !it.me }
        if (!out) { ringingSince = null; lastRingback = null; return }
        if (ringingSince != null) return
        val since = System.currentTimeMillis()
        ringingSince = since
        scope.launch {
            while (_snapshot.value?.let { it.roomId == snapshot.roomId && it.phase == CallPhase.CONNECTED && it.people.none { p -> !p.me } } == true && ringingSince == since) {
                val elapsed = System.currentTimeMillis() - since
                if (CallSounds.ringbackDue(true, elapsed, lastRingback?.let { it - since })) {
                    lastRingback = since + elapsed
                    CallSounds.play("ringback")
                }
                kotlinx.coroutines.delay(500)
                if (elapsed > 31_000) break
            }
        }
    }

    private fun ended() {
        routed = false
        speakers.value = "" to emptySet()
        _hidden.value = emptySet()
        handle = null
        CallService.update(context, "", "")
        _open.value = false
        if (AppState.screen == "call") AppState.screen = "signed-in"
        current = null
    }

    fun show(open: Boolean) {
        _open.value = open
        AppState.screen = if (open) "call" else "signed-in"
    }

    fun toggleMute() = handle?.toggleMute()
    fun toggleDeafen() = handle?.toggleDeafen()
    /** Asked of the activity: the camera permission, before the camera is turned on. */
    val needCamera = MutableStateFlow(false)

    private var cameraPaused = false

    /** The camera button: on (asking for the permission first when it is not allowed) or off. */
    fun toggleCamera() {
        val on = _snapshot.value?.camera == true
        cameraPaused = false
        if (on) {
            handle?.setCamera(false)
        } else if (cameraAllowed()) {
            handle?.setCamera(true)
        } else {
            needCamera.value = true
        }
    }

    fun cameraAnswered(granted: Boolean) {
        needCamera.value = false
        if (granted) handle?.setCamera(true)
    }

    // ---- sharing the screen (#432)

    /** Asked of the activity: the system's permission dialog for capturing the screen. */
    val needProjection = MutableStateFlow(false)
    private var projectionResult: Pair<Int, android.content.Intent>? = null

    private val screenShare get() = (context.applicationContext as MorpheusApp).screen

    /** The share button: ends the share, or asks the system for the permission first. */
    fun toggleScreenShare() {
        if (_snapshot.value?.people?.any { it.me && it.screen } == true) stopScreenShare() else needProjection.value = true
    }

    /** The system answered: with its permission the foreground service starts, and the capture follows once it is up. */
    fun projectionAnswered(resultCode: Int, data: android.content.Intent?) {
        needProjection.value = false
        if (resultCode != android.app.Activity.RESULT_OK || data == null) return
        projectionResult = resultCode to data
        ScreenShareService.start(context)
    }

    /** The service is in the foreground (the system requires it before the projection is taken): capture and send. */
    fun projectionServiceUp() {
        val (code, data) = projectionResult ?: return
        projectionResult = null
        try {
            screenShare.take(code, data)
            handle?.setScreenShare(true)
        } catch (e: Exception) {
            android.util.Log.w("MorpheusCalls", "screen share did not start: $e")
            ScreenShareService.stop(context)
        }
    }

    fun stopScreenShare() {
        handle?.setScreenShare(false)
    }

    /** The system ended the capture (its chip, another app taking the screen): the call stops sending it. */
    fun screenEnded() {
        handle?.setScreenShare(false)
    }

    private fun cameraAllowed() = androidx.core.content.ContextCompat.checkSelfPermission(context, android.Manifest.permission.CAMERA) == android.content.pm.PackageManager.PERMISSION_GRANTED

    /** The app left the screen (not into picture-in-picture): the camera stops, and comes back with the app. */
    fun appLeft() {
        if (_snapshot.value?.camera == true) {
            cameraPaused = true
            handle?.setCamera(false)
        }
    }

    fun appBack() {
        if (cameraPaused) {
            cameraPaused = false
            if (cameraAllowed()) handle?.setCamera(true)
        }
    }

    /** The other camera (front or back), when the phone has both. */
    val canFlipCamera: Boolean get() = (context.applicationContext as MorpheusApp).camera.canSwitch()

    fun flipCamera() = (context.applicationContext as MorpheusApp).camera.flip()
    fun watch(userId: String, on: Boolean) = handle?.watch(userId, on)

    /** My friends, to ask into the call (#438). */
    suspend fun friends() = morpheus.friends()

    /** Ask a friend into the call of [roomId]: a DM's own invite, or a message with the channel's link. */
    suspend fun invite(roomId: String, userId: String) = morpheus.inviteToCall(roomId, userId)
    fun frame(feed: String) = handle?.frame(feed)

    private val _hidden = MutableStateFlow<Set<String>>(emptySet())

    /** People whose camera is hidden on their card for now (#436): nothing of it is downloaded. */
    val hiddenVideo: StateFlow<Set<String>> = _hidden.asStateFlow()

    fun toggleHideVideo(userId: String) {
        val hide = userId !in _hidden.value
        _hidden.value = if (hide) _hidden.value + userId else _hidden.value - userId
        handle?.hideVideo(userId, hide)
    }

    /** How loud `userId`'s shared screen plays for me (0 to 2), or silent for me alone when [muted]; kept and applied now. */
    fun setStreamVolume(userId: String, volume: Float, muted: Boolean) {
        morpheus.keepStreamVolume(userId, volume, muted)
        handle?.setStreamVolume(userId, if (muted) 0f else volume)
    }

    fun hangUp() {
        val call = handle
        if (call == null) {
            // A failed call is dismissed.
            _snapshot.value = null
            _open.value = false
            return
        }
        scope.launch {
            call.leave()
            if (_snapshot.value?.phase != CallPhase.ENDED) {
                if (_snapshot.value?.phase == CallPhase.CONNECTED) CallSounds.play("leave")
                ended()
            }
            _snapshot.value = null
        }
    }

    /** Dismiss a call screen that says the call failed. */
    fun dismiss() {
        if (_snapshot.value?.phase == CallPhase.FAILED) {
            _snapshot.value = null
            _open.value = false
            CallService.update(context, "", "")
            if (AppState.screen == "call") AppState.screen = "signed-in"
        }
    }

    // ---- volumes (kept by the Rust core, as the desktop keeps them)

    fun personVolume(userId: String) = morpheus.personVolume(userId)

    /** How loud `userId`'s voice plays for me (0 to 2), or silent for me alone when [muted]; kept and applied now. */
    fun setPersonVolume(userId: String, volume: Float, muted: Boolean) {
        morpheus.keepPersonVolume(userId, volume, muted)
        handle?.setPersonVolume(userId, if (muted) 0f else volume)
    }

    fun volumes() = morpheus.callVolumes()

    /** Noise suppression (#434): kept, and used by the microphone of the next call; a call going keeps what it began with. */
    fun noiseChoices() = morpheus.noiseChoices()
    fun setNoiseChoice(key: String) = morpheus.setNoiseChoice(key)

    /** What the call going made its microphone with: `off`, `standard` or `strong`; null outside a call. */
    fun noiseApplied(): String? = morpheus.appliedNoiseChoice()
    fun noiseSuppressionApplied(): Boolean? = noiseApplied()?.let { it != "off" }
    fun setInputVolume(volume: Float) = morpheus.setCallInputVolume(volume)

    fun setOutputVolume(volume: Float) {
        morpheus.setCallOutputVolume(volume)
        handle?.setOutputVolume(volume)
    }

    // ---- where the sound goes

    /** A place the call's sound can go: the phone's loudspeaker or earpiece, or a headset. */
    data class Route(val key: String, val label: String, val on: Boolean)


    private var picked: String? = null

    /** The places to pick from now (a headset appears when it is plugged in or connected). */
    fun routes(): List<Route> {
        val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        // What was picked from here is what is chosen at once (the system switches later); with nothing picked the system's device says.
        val now = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) audio.communicationDevice else null
        val ids = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) audio.availableCommunicationDevices.map { "device-${it.id}" }.toSet() else emptySet()
        if (picked?.startsWith("device-") == true && picked !in ids) picked = null
        val chosen = picked ?: when {
            now == null -> if (_speaker.value) "speaker" else "earpiece"
            now.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "speaker"
            now.type == AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "earpiece"
            else -> "device-${now.id}"
        }
        val out = mutableListOf(
            Route("speaker", t("Loudspeaker"), chosen == "speaker"),
            Route("earpiece", t("Earpiece"), chosen == "earpiece"),
        )
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            for (d in audio.availableCommunicationDevices) {
                val label = when (d.type) {
                    AudioDeviceInfo.TYPE_BLUETOOTH_SCO, AudioDeviceInfo.TYPE_BLE_HEADSET -> d.productName.toString().ifBlank { t("Bluetooth") }
                    AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> t("Headphones")
                    AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE -> d.productName.toString().ifBlank { t("USB headset") }
                    else -> continue
                }
                out += Route("device-${d.id}", label, chosen == "device-${d.id}")
            }
        }
        return out
    }

    /** Send the call's sound to [key] (one of [routes]); a headset carries the microphone too. */
    fun chooseRoute(key: String) {
        picked = key
        when (key) {
            "speaker" -> { _speaker.value = true; route(true) }
            "earpiece" -> { _speaker.value = false; route(false) }
            else -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
                audio.availableCommunicationDevices.firstOrNull { "device-${it.id}" == key }?.let { audio.setCommunicationDevice(it) }
            }
        }
    }

    fun toggleSpeaker() {
        picked = null
        _speaker.value = !_speaker.value
        route(_speaker.value)
    }

    /** Sound to the loudspeaker or the earpiece. */
    private fun route(loud: Boolean) {
        val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val wanted = if (loud) AudioDeviceInfo.TYPE_BUILTIN_SPEAKER else AudioDeviceInfo.TYPE_BUILTIN_EARPIECE
            audio.availableCommunicationDevices.firstOrNull { it.type == wanted }?.let { audio.setCommunicationDevice(it) }
        } else {
            @Suppress("DEPRECATION")
            audio.isSpeakerphoneOn = loud
        }
    }

    companion object {
        /** The call going, for the notification's Hang up. */
        @Volatile
        var current: CallModel? = null

        /** Who speaks in the call going, with its room (the channel list marks them, #441). */
        val speakers = MutableStateFlow("" to emptySet<String>())
    }
}
