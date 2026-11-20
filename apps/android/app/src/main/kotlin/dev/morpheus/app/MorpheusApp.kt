package dev.morpheus.app

import android.app.Application
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.Morpheus

class MorpheusApp : Application() {
    /** The Rust core, one per process; everything it keeps is in the app's private folder. */
    val morpheus: Morpheus by lazy {
        Morpheus(filesDir.resolve("morpheus").apply { mkdirs() }.absolutePath)
    }

    private val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.IO)

    /** Tell the homeserver where to push, when there is both a push endpoint and a signed-in account. */
    fun registerPushNow() {
        val endpoint = dev.morpheus.app.push.Push.endpoint(this) ?: return
        if (morpheus.account() == null) return
        scope.launch {
            try {
                morpheus.registerPush(endpoint, android.os.Build.MODEL)
            } catch (e: Exception) {
                android.util.Log.w("MorpheusPush", "registering the pusher failed: $e")
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        // WebRTC (calls) finds its Java classes from JNI_OnLoad, which runs when the library is
        // loaded this way (UniFFI's JNA load does not run it), and wants the app's context.
        try {
            System.loadLibrary("morpheus_ffi")
            livekit.org.webrtc.ContextUtils.initialize(this)
        } catch (e: Throwable) {
            android.util.Log.w("MorpheusCalls", "calls are not available: $e")
        }
        DebugHooks.install(this)
        dev.morpheus.app.push.Push.track(this)
    }
}
