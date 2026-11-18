package dev.morpheus.app

import android.app.Application

/** Debug builds add the e2e bridge (docs/ANDROID.md). */
object DebugHooks {
    fun install(app: Application) {
        TestBridge(app).start()
    }
}
