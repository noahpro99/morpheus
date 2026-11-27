package dev.morpheus.app.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import kotlinx.coroutines.launch
import kotlin.math.abs

/**
 * The stretch when a list is pulled past its end, on every list (#516). A list that scrolls gets the platform's own from its scroll. One that
 * fits its space does not scroll, and Compose draws no stretch for it, so this one watches the drag (it takes nothing from the list or from what
 * is inside it) and stretches what is drawn from the edge being pulled, easing it back with a spring when the finger lets go. (A list that fits is
 * never made non-scrolling: `userScrollEnabled = false` would kill the stretch.) Every `LazyColumn` and `verticalScroll` carries this (the
 * android_rules check). Put it first in the list's modifier.
 */
fun Modifier.stretch(state: LazyListState): Modifier = stretchWhen(!state.canScrollForward && !state.canScrollBackward)

/** The same for a column scrolled by `verticalScroll`. */
fun Modifier.stretch(state: ScrollState): Modifier = stretchWhen(!state.canScrollForward && !state.canScrollBackward)

/** How far a pull of this fraction of the list's height stretches it: it eases in and never passes about a fifth. */
private fun stretchOf(pull: Float): Float = 0.2f * abs(pull) / (abs(pull) + 0.5f)

@Suppress("ModifierComposed")
private fun Modifier.stretchWhen(fits: Boolean): Modifier = composed {
    if (!fits) {
        Modifier
    } else {
        val pull = remember { Animatable(0f) }
        val scope = rememberCoroutineScope()
        Modifier
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    var x = 0f
                    var y = 0f
                    do {
                        val event = awaitPointerEvent(PointerEventPass.Initial)
                        val change = event.changes.firstOrNull() ?: break
                        x += change.positionChange().x
                        y += change.positionChange().y
                        // Only a drag that is mostly vertical, past the touch slop.
                        if (abs(y) > viewConfiguration.touchSlop && abs(y) > abs(x)) {
                            val height = size.height.toFloat().coerceAtLeast(1f)
                            scope.launch { pull.snapTo((y / height).coerceIn(-1f, 1f)) }
                        }
                    } while (event.changes.any { it.pressed })
                    scope.launch { pull.animateTo(0f, spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium)) }
                }
            }
            .graphicsLayer {
                val stretch = stretchOf(pull.value)
                scaleY = 1f + stretch
                // Anchored at the edge that is not being pulled: a pull down stretches from the top.
                transformOrigin = TransformOrigin(0.5f, if (pull.value >= 0f) 0f else 1f)
            }
    }
}
