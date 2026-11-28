package dev.morpheus.app

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import java.util.concurrent.ConcurrentLinkedQueue
import uniffi.morpheus_ffi.CallSnapshot
import uniffi.morpheus_ffi.callRingbackDue
import uniffi.morpheus_ffi.callSoundRate
import uniffi.morpheus_ffi.callSoundSamples
import uniffi.morpheus_ffi.callSounds

/**
 * The sounds of a call (#439): which ones a snapshot change makes, and the
 * samples, come from the shared core (`view_model::calls`), the same as the
 * desktop's. They play on the call's own audio stream (voice communication),
 * so they follow the speaker or earpiece route the call uses.
 */
object CallSounds {
    /** The keys played, newest last, for the e2e bridge. */
    val played = ConcurrentLinkedQueue<String>()

    private val tracks = HashMap<String, AudioTrack>()

    /** The audio service, set when the call model starts: the effects read the stream volume they belong to from it. */
    @Volatile
    var audio: AudioManager? = null

    /** What the last effect was asked to play on: "call" (the call stream, in a call), "media" (the media stream, out of one), or "silent" (that stream is at 0). */
    @Volatile
    var lastStream = ""

    /** In a call the effects follow the call stream (the voice-call volume); out of one, they follow media. */
    private fun inCall() = audio?.mode == AudioManager.MODE_IN_COMMUNICATION

    /** Whether the stream the effect belongs to is turned up: at 0 the person asked for silence (#571). */
    private fun audible() = audio?.let { it.getStreamVolume(if (inCall()) AudioManager.STREAM_VOICE_CALL else AudioManager.STREAM_MUSIC) > 0 } ?: true

    /** Plays what the step from [before] to [after] makes. */
    fun changed(before: CallSnapshot?, after: CallSnapshot) {
        for (key in callSounds(before, after)) play(key)
    }

    /** The ringback, while this account's call rings the other person: the caller says whether it is due. */
    fun ringbackDue(ringingOut: Boolean, sinceMs: Long, lastMs: Long?) =
        callRingbackDue(ringingOut, sinceMs.toULong(), lastMs?.toULong())

    fun play(key: String) {
        played.add(key)
        while (played.size > 200) played.poll()
        val call = inCall()
        if (!audible()) {
            lastStream = "silent"
            return
        }
        lastStream = if (call) "call" else "media"
        try {
            val track = synchronized(tracks) { tracks.getOrPut("$key/$lastStream") { make(key, call) ?: return } }
            track.stop()
            track.reloadStaticData()
            track.play()
        } catch (_: Exception) {
            // A device with no audio output stays quiet.
        }
    }

    private fun make(key: String, call: Boolean): AudioTrack? {
        val samples = callSoundSamples(key)
        if (samples.isEmpty()) return null
        val data = ShortArray(samples.size) { samples[it] }
        val track = AudioTrack.Builder()
            .setAudioAttributes(AudioAttributes.Builder().setUsage(if (call) AudioAttributes.USAGE_VOICE_COMMUNICATION else AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
            .setAudioFormat(AudioFormat.Builder().setSampleRate(callSoundRate().toInt()).setEncoding(AudioFormat.ENCODING_PCM_16BIT).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
            .setBufferSizeInBytes(data.size * 2)
            .setTransferMode(AudioTrack.MODE_STATIC)
            .build()
        track.write(data, 0, data.size)
        return track
    }
}
