package dev.morpheus.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.gestures.ScrollableDefaults
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.overscroll
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.unit.Velocity

/**
 * The platform's stretch when a list is pulled past its end, on every list (#516, #573). A list that scrolls has it from its own scroll. One
 * that fits its space does not scroll and Compose draws no stretch for it, so it gets the same platform effect (the system's EdgeEffect stretch,
 * the one the chat has): the pull the list could not use is handed up as nested scroll and fed to that effect, which draws and settles it as it
 * does for any list. (A list that fits is never made non-scrolling: `userScrollEnabled = false` would kill the stretch.) Every `LazyColumn` and
 * `verticalScroll` carries this (the android_rules check). Put it first in the list's modifier.
 */
fun Modifier.stretch(state: LazyListState): Modifier = stretchWhen(!state.canScrollForward && !state.canScrollBackward)

/** The same for a grid. */
fun Modifier.stretch(state: androidx.compose.foundation.lazy.grid.LazyGridState): Modifier = stretchWhen(!state.canScrollForward && !state.canScrollBackward)

/** The same for a column scrolled by `verticalScroll`. */
fun Modifier.stretch(state: ScrollState): Modifier = stretchWhen(!state.canScrollForward && !state.canScrollBackward)

@OptIn(ExperimentalFoundationApi::class)
@Suppress("ModifierComposed")
private fun Modifier.stretchWhen(fits: Boolean): Modifier = composed {
    if (!fits) {
        Modifier
    } else {
        val effect = ScrollableDefaults.overscrollEffect()
        dev.morpheus.app.AppState.overscrollEffects.add(effect.javaClass.simpleName)
        val connection = remember(effect) {
            object : NestedScrollConnection {
                override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                    // The pull is drawn but not claimed: a parent that acts on the leftover (the call screen minimising by a pull down) still gets all of it.
                    if (source == NestedScrollSource.UserInput && available != Offset.Zero) effect.applyToScroll(available, source) { Offset.Zero }
                    return Offset.Zero
                }

                override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity {
                    effect.applyToFling(available) { Velocity.Zero }
                    return Velocity.Zero
                }
            }
        }
        Modifier.overscroll(effect).nestedScroll(connection)
    }
}
