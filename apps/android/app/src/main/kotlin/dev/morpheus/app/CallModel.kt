package dev.morpheus.app

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
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
        _snapshot.value = snapshot
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
    fun setCamera(on: Boolean) = handle?.setCamera(on)
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
