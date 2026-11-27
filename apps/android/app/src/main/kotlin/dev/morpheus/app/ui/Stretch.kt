package dev.morpheus.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.overscroll
import androidx.compose.foundation.rememberOverscrollEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed

/**
 * The platform's stretch when a list is pulled past its end, on every list (#516). A list that scrolls has it from its own scroll; one that
 * fits its space does not scroll, and Compose gives it no stretch then, so it gets a scroll of its own that moves nothing and passes the pull to
 * the same effect. (A list that fits is never made non-scrolling: `userScrollEnabled = false` would kill the stretch.) Every `LazyColumn`
 * and `verticalScroll` carries this (the android_rules check). Put it first in the list's modifier, before anything that takes a drag.
 */
fun Modifier.stretch(state: LazyListState): Modifier = stretchWhen(!state.canScrollForward && !state.canScrollBackward)

/** The same for a column scrolled by `verticalScroll`. */
fun Modifier.stretch(state: ScrollState): Modifier = stretchWhen(!state.canScrollForward && !state.canScrollBackward)

@OptIn(ExperimentalFoundationApi::class)
@Suppress("ModifierComposed")
private fun Modifier.stretchWhen(fits: Boolean): Modifier = composed {
    if (!fits) {
        Modifier
    } else {
        val effect = rememberOverscrollEffect()
        val still = rememberScrollableState { 0f }
        Modifier.overscroll(effect).scrollable(still, Orientation.Vertical, overscrollEffect = effect)
    }
}
