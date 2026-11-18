package dev.morpheus.app

import android.app.Application
import uniffi.morpheus_ffi.Morpheus

class MorpheusApp : Application() {
    /** The Rust core, one per process; everything it keeps is in the app's private folder. */
    val morpheus: Morpheus by lazy {
        Morpheus(filesDir.resolve("morpheus").apply { mkdirs() }.absolutePath)
    }

    override fun onCreate() {
        super.onCreate()
        DebugHooks.install(this)
    }
}
