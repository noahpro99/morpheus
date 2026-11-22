package dev.morpheus.app

import android.app.Application
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.Morpheus

class MorpheusApp : Application() {
    /** The Rust core, one per process; everything it keeps is in the app's private folder. */
    val morpheus: Morpheus by lazy {
        AppState.mark("core: new")
        Morpheus(filesDir.resolve("morpheus").apply { mkdirs() }.absolutePath).also { AppState.mark("core: ready") }
    }

    /** The phone's camera, for calls. */
    val camera: CameraFeed by lazy {
        CameraFeed(this).also { uniffi.morpheus_ffi.callSetCameraDriver(it) }
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
        AppState.mark("application: onCreate")
        super.onCreate()
        // WebRTC (calls) finds its Java classes from JNI_OnLoad, which runs when the library is
        // loaded this way (UniFFI's JNA load does not run it), and wants the app's context.
        try {
            System.loadLibrary("morpheus_ffi")
            AppState.mark("library loaded")
            livekit.org.webrtc.ContextUtils.initialize(this)
        } catch (e: Throwable) {
            android.util.Log.w("MorpheusCalls", "calls are not available: $e")
        }
        // The Rust core (the first UniFFI call loads JNA and checks every binding, then the store opens) and the camera
        // driver start on a thread of their own while the activity is made; whoever needs the core first waits for it
        // (the lazies lock). Saves what the main thread would spend before the first frame (#317).
        Thread({
            try {
                morpheus
                camera
            } catch (e: Throwable) {
                android.util.Log.w("MorpheusStart", "the core did not start: $e")
            }
            AppState.mark("core and camera driver up (thread)")
        }, "morpheus-start").start()
        DebugHooks.install(this)
        AppState.mark("debug hooks")
        dev.morpheus.app.push.Push.track(this)
    }
}
