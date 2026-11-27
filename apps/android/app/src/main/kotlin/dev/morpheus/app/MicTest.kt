package dev.morpheus.app

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import kotlin.math.PI
import kotlin.math.sin
import uniffi.morpheus_ffi.micTestLevel

/**
 * The microphone test (#442): listens to the microphone and keeps how loud it is (0 to 1, the bar's level from the
 * shared core's `micTestLevel`), at the input volume chosen. It plays nothing back: a phone's speaker beside its
 * microphone would howl; the call's own noise handling is the system's. Debug tests set [tone] to hear a tone instead
 * of the emulator's silent microphone.
 */
class MicTest(private val inputVolume: () -> Float) {
    @Volatile var level = 0f
        private set

    @Volatile var running = false
        private set

    /** Why it could not start (no permission, the microphone taken), or null. */
    @Volatile var problem: String? = null
        private set

    private var thread: Thread? = null

    @SuppressLint("MissingPermission")
    @Synchronized
    fun start() {
        if (running) return
        problem = null
        running = true
        level = 0f
        thread = Thread({
            val rate = 16_000
            val chunk = rate / 20
            val buffer = ShortArray(chunk)
            var record: AudioRecord? = null
            try {
                if (!tone) {
                    val size = maxOf(AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT), chunk * 4)
                    record = AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, size)
                    if (record.state != AudioRecord.STATE_INITIALIZED) {
                        problem = "The microphone could not be opened."
                        return@Thread
                    }
                    record.startRecording()
                }
                var phase = 0.0
                while (running) {
                    if (record != null) {
                        val n = record.read(buffer, 0, chunk)
                        if (n <= 0) continue
                    } else {
                        for (i in buffer.indices) { buffer[i] = (sin(phase) * 8000).toInt().toShort(); phase += 2 * PI * 440 / rate }
                        Thread.sleep(50)
                    }
                    val gain = inputVolume()
                    val scaled = ShortArray(buffer.size) { (buffer[it] * gain).toInt().coerceIn(-32768, 32767).toShort() }
                    level = micTestLevel(level, scaled.toList())
                }
            } catch (e: Exception) {
                problem = e.message ?: "The microphone could not be opened."
            } finally {
                try { record?.stop() } catch (_: Exception) {}
                record?.release()
                running = false
                level = 0f
            }
        }, "morpheus-mic-test").also { it.start() }
    }

    @Synchronized
    fun stop() {
        running = false
        thread?.join(500)
        thread = null
    }

    companion object {
        /** Debug tests: a tone in place of the microphone. */
        @Volatile var tone = false
    }
}
