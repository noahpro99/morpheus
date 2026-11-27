package dev.morpheus.app.ui

import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.offset
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import kotlinx.coroutines.launch

/** The speed of a drag, so a quick flick commits a swipe whatever its length (the rule is view-model list_swipes, #486). */
class FlickTracker(private val view: android.view.View? = null) {
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
        val goes = uniffi.morpheus_ffi.swipeCommitsWithVelocity(distance, velocity(), width)
        // A swipe that goes through gives a light haptic (#485).
        if (goes) view?.let { Haptics.commit(it) }
        return goes
    }
}

/**
 * A pushed screen (settings and its pages, room details, security, bridges, new, reorder, the inbox, the message search, a room's pages): a
 * horizontal swipe either way goes back (#507), by the same rule as the chat swipes: the screen follows the finger, and a short drag
 * or a quick flick (view-model `swipe_commits_with_velocity`, #486) sends it off the way the finger went, a short slow one springs back.
 * Going back is the system back (the activity's dispatcher), so a swipe reaches the same place as the gesture, a button or a `BackHandler`.
 * A slider, a horizontal list or a text selection that takes the drag first keeps it: this only sees a drag nothing inside consumed.
 */
@Composable
fun SwipeBack(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val offset = remember { androidx.compose.animation.core.Animatable(0f) }
    var width by remember { androidx.compose.runtime.mutableFloatStateOf(1f) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val haptic = androidx.compose.ui.platform.LocalView.current
    val flick = remember { FlickTracker(haptic) }
    var lock by remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    val dispatcher = androidx.activity.compose.LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher
    Box(
        modifier
            .onSizeChanged { width = it.width.toFloat().coerceAtLeast(1f) }
            .offset { androidx.compose.ui.unit.IntOffset(offset.value.toInt(), 0) }
            .pointerInput(Unit) {
                detectHorizontalDragGestures(
                    onDragStart = { flick.reset(); lock = 0f },
                    onDragEnd = {
                        val goes = flick.commits(kotlin.math.abs(offset.value), width)
                        scope.launch {
                            if (goes) {
                                offset.animateTo(if (offset.value < 0f) -width else width, Motion.slide())
                                dispatcher?.onBackPressed()
                                offset.snapTo(0f)
                            } else offset.animateTo(0f, Motion.slide())
                        }
                    },
                    onDragCancel = { scope.launch { offset.animateTo(0f, Motion.slide()) } },
                ) { change, amount ->
                    // Reversing only undoes the drag; it never crosses into the other way (#525).
                    lock = uniffi.morpheus_ffi.swipeDragLock(lock, amount)
                    val next = uniffi.morpheus_ffi.swipeDragTotal(offset.value, amount, lock)
                    flick.add(change, next - offset.value)
                    scope.launch { offset.snapTo(next) }
                }
            },
    ) { content() }
}
