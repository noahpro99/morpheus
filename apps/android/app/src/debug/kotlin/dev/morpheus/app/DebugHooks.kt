package dev.morpheus.app

import android.app.Application

/** Debug builds add the e2e bridge (docs/ANDROID.md), and calls send a tone instead of the microphone. */
object DebugHooks {
    fun install(app: Application) {
        // Unless a test marked the app for the real microphone (a file in its folder).
        if (!app.filesDir.resolve("morpheus/real_audio").exists()) {
            try {
                uniffi.morpheus_ffi.callUseTestTone()
            } catch (e: Throwable) {
                android.util.Log.w("MorpheusCalls", "no test tone: $e")
            }
        }
        TestBridge(app).start()
    }
}
