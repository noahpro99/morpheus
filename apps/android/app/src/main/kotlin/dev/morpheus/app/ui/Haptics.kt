package dev.morpheus.app.ui

import android.os.Build
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import dev.morpheus.app.AppState

/**
 * The one place a haptic is fired from (#485): light ones for a tap, for a swipe that goes through and for the ticks of a scrub, each through the
 * view's own haptics, so the system's touch-feedback setting is respected. Each one is counted for the debug bridge.
 */
object Haptics {
    /** A tap on something that handled it. */
    fun tap(view: View) = fire(view, "tap", HapticFeedbackConstants.VIRTUAL_KEY)

    /** A swipe that goes through. */
    fun commit(view: View) = fire(view, "commit", if (Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.GESTURE_END else HapticFeedbackConstants.VIRTUAL_KEY)

    /** A step of a scrub (the date under the fast-scroll handle changing). */
    fun tick(view: View) = fire(view, "tick", if (Build.VERSION.SDK_INT >= 34) HapticFeedbackConstants.SEGMENT_FREQUENT_TICK else HapticFeedbackConstants.CLOCK_TICK)

    /** Taking hold of a handle. */
    fun grab(view: View) = fire(view, "grab", HapticFeedbackConstants.LONG_PRESS)

    /** The long press that opens a sheet (its own haptic, through Compose's feedback). */
    fun longPress() = record("long_press")

    private fun fire(view: View, name: String, kind: Int) {
        view.performHapticFeedback(kind)
        record(name)
    }

    private fun record(name: String) {
        AppState.haptics++
        AppState.hapticLog.add(name)
    }
}

/**
 * A light haptic for every tap that something handled (a button, a row, a chip, a switch): the pointer's release, seen after the
 * children had their say, was consumed and the finger had not moved or been held long (that is a long press, which has its own
 * haptic). Whatever new tap target a screen adds is covered with no code of its own (#485).
 */
@Composable
fun Modifier.tapHaptics(): Modifier {
    val view = rememberUpdatedState(LocalView.current)
    return pointerInput(Unit) {
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            var moved = false
            while (true) {
                val event = awaitPointerEvent(PointerEventPass.Final)
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                if (event.changes.size > 1 || (change.position - down.position).getDistance() > viewConfiguration.touchSlop) moved = true
                if (change.previousPressed && !change.pressed) {
                    if (!moved && change.isConsumed && change.uptimeMillis - down.uptimeMillis < viewConfiguration.longPressTimeoutMillis) Haptics.tap(view.value)
                    break
                }
                if (!change.pressed) break
            }
        }
    }
}
