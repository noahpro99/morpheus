package dev.morpheus.app

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import uniffi.morpheus_ffi.callMicWanted
import uniffi.morpheus_ffi.pushCallMic

/**
 * The phone's microphone for a call that cleans it with the strong noise model (#531): the call asks for the frames (`callMicWanted`),
 * this captures them with the voice-communication source (so the phone's echo canceller stays on) and hands them to Rust, which cleans
 * them with DeepFilterNet and sends them into the call. It runs while a call is on screen and the call wants it.
 */
class MicPump {
    @Volatile
    private var running = false
    private var thread: Thread? = null

    /** Frames handed over (the debug bridge reports it, so a test sees the pump work). */
    @Volatile
    var frames = 0L
        private set

    @SuppressLint("MissingPermission")
    @Synchronized
    fun start() {
        if (running) return
        running = true
        thread = Thread({
            var record: AudioRecord? = null
            try {
                // The call asks once it has made its track; wait for that.
                var waited = 0
                while (running && !callMicWanted() && waited < 100) { Thread.sleep(100); waited++ }
                if (!running || !callMicWanted()) return@Thread
                val rate = 48_000
                val chunk = rate / 100
                val size = maxOf(AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT), chunk * 8)
                record = AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, size)
                if (record.state != AudioRecord.STATE_INITIALIZED) return@Thread
                record.startRecording()
                val buffer = ShortArray(chunk)
                while (running) {
                    val n = record.read(buffer, 0, chunk)
                    if (n <= 0) continue
                    if (!pushCallMic(buffer.take(n))) break
                    frames++
                }
            } catch (_: Exception) {
            } finally {
                try { record?.stop() } catch (_: Exception) {}
                record?.release()
                running = false
            }
        }, "morpheus-call-mic").also { it.start() }
    }

    @Synchronized
    fun stop() {
        running = false
        thread?.join(1500)
        thread = null
    }
}
