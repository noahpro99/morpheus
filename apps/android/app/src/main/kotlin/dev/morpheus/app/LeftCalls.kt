package dev.morpheus.app

import androidx.compose.runtime.mutableStateMapOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * The voice channels this phone just left (#497): my own name leaves their list at once, without waiting for the server's echo of the leave
 * (which can take long on a slow or busy homeserver). It comes back when I join again, or when the server still lists me after a minute (then
 * it is the truth).
 */
object LeftCalls {
    private const val HOLD_MS = 60_000L
    private val left = mutableStateMapOf<String, Long>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    /** Who I am (set at sign-in): only my own row is hidden. */
    @Volatile
    var me: String = ""

    fun mark(roomId: String) {
        left[roomId] = System.currentTimeMillis()
        scope.launch {
            delay(HOLD_MS)
            left.remove(roomId)
        }
    }

    fun clear(roomId: String) {
        left.remove(roomId)
    }

    /** Whether my own name is held out of this room's list now. */
    fun hidesMe(roomId: String): Boolean = left[roomId]?.let { System.currentTimeMillis() - it < HOLD_MS } == true
}
