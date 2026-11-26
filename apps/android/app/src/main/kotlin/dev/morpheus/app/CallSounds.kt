package dev.morpheus.app

import android.media.AudioAttributes
import android.media.AudioFormat
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
        try {
            val track = synchronized(tracks) { tracks.getOrPut(key) { make(key) ?: return } }
            track.stop()
            track.reloadStaticData()
            track.play()
        } catch (_: Exception) {
            // A device with no audio output stays quiet.
        }
    }

    private fun make(key: String): AudioTrack? {
        val samples = callSoundSamples(key)
        if (samples.isEmpty()) return null
        val data = ShortArray(samples.size) { samples[it] }
        val track = AudioTrack.Builder()
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
            .setAudioFormat(AudioFormat.Builder().setSampleRate(callSoundRate().toInt()).setEncoding(AudioFormat.ENCODING_PCM_16BIT).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
            .setBufferSizeInBytes(data.size * 2)
            .setTransferMode(AudioTrack.MODE_STATIC)
            .build()
        track.write(data, 0, data.size)
        return track
    }
}
