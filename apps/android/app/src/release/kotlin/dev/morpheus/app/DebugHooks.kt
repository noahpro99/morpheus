package dev.morpheus.app

import android.app.Application

/** Release builds add nothing to the app (debug builds start the e2e bridge: src/debug). */
object DebugHooks {
    fun install(app: Application) = Unit
}
