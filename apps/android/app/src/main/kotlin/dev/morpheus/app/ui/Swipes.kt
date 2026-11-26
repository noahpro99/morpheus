package dev.morpheus.app.ui

import androidx.compose.ui.input.pointer.PointerInputChange

/** The speed of a drag, so a quick flick commits a swipe whatever its length (the rule is view-model list_swipes, #486). */
class FlickTracker {
    private val times = ArrayList<Long>()
    private val xs = ArrayList<Float>()
    private var along = 0f

    fun reset() {
        times.clear()
        xs.clear()
        along = 0f
    }

    /** A move of the drag: `amount` px since the last one (the pane under the finger moves, so its own positions would not do). */
    fun add(change: PointerInputChange, amount: Float) {
        along += amount
        times.add(change.uptimeMillis)
        xs.add(along)
    }

    /** Pixels a second over the last 100 ms of the drag (two moves are enough). */
    private fun velocity(): Float {
        if (times.size < 2) return 0f
        var first = times.size - 1
        while (first > 0 && times.last() - times[first - 1] <= 100) first--
        val dt = times.last() - times[first]
        return if (dt <= 0L) 0f else (xs.last() - xs[first]) / dt * 1000f
    }

    /** Whether a drag of `distance` px across a pane `width` px wide goes through, by its length or by how fast the finger left. */
    fun commits(distance: Float, width: Float): Boolean {
        return uniffi.morpheus_ffi.swipeCommitsWithVelocity(distance, velocity(), width)
    }
}
