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

    private val _snapshot = MutableStateFlow<CallSnapshot?>(null)

    /** What the call screen shows; null when there is no call. */
    val snapshot: StateFlow<CallSnapshot?> = _snapshot.asStateFlow()

    /** The full-screen call is open (otherwise a bar shows where the call is going). */
    private val _open = MutableStateFlow(false)
    val open: StateFlow<Boolean> = _open.asStateFlow()

    private val _speaker = MutableStateFlow(true)

    /** The call plays from the loudspeaker (Discord-style) rather than the earpiece. */
    val speaker: StateFlow<Boolean> = _speaker.asStateFlow()

    private var handle: CallHandle? = null
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

    private fun ended() {
        routed = false
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
    fun frame(feed: String) = handle?.frame(feed)

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
            if (_snapshot.value?.phase != CallPhase.ENDED) ended()
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
    fun setInputVolume(volume: Float) = morpheus.setCallInputVolume(volume)

    fun setOutputVolume(volume: Float) {
        morpheus.setCallOutputVolume(volume)
        handle?.setOutputVolume(volume)
    }

    // ---- where the sound goes

    /** A place the call's sound can go: the phone's loudspeaker or earpiece, or a headset. */
    data class Route(val key: String, val label: String, val on: Boolean)


    /** The places to pick from now (a headset appears when it is plugged in or connected). */
    fun routes(): List<Route> {
        val audio = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        // The device the system is using says what is chosen (a headset picked from here or by the system unticks both).
        val now = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) audio.communicationDevice?.type else null
        val chosen = if (now == null) { if (_speaker.value) "speaker" else "earpiece" }
        else if (now == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER) "speaker"
        else if (now == AudioDeviceInfo.TYPE_BUILTIN_EARPIECE) "earpiece"
        else ""
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
                out += Route("device-${d.id}", label, audio.communicationDevice?.id == d.id)
            }
        }
        return out
    }

    /** Send the call's sound to [key] (one of [routes]); a headset carries the microphone too. */
    fun chooseRoute(key: String) {
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
    }
}
