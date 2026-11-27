package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * A draggable handle on the right edge of a long history, with the date of what is under it to its left (#493, like a photo gallery). The list
 * is bottom-up (item 0 is the newest), so the handle is at the bottom at the newest and at the top at the oldest. It shows while the list
 * scrolls (and while it is held) and fades out after a moment; a haptic tick comes with each change of the date shown and on grab and release.
 */
@Composable
fun BoxScope.FastScroll(state: LazyListState, total: Int, labelAt: @Composable (Int) -> String?, minimum: Int = 40) {
    val scope = rememberCoroutineScope()
    val haptic = androidx.compose.ui.platform.LocalView.current
    var dragging by remember { mutableStateOf(false) }
    // The date bubble stays with the handle for as long as it shows after it was held (so it can be read), not for a plain scroll.
    var used by remember { mutableStateOf(false) }
    var shown by remember { mutableStateOf(false) }
    var height by remember { mutableStateOf(0) }
    val scrolling = state.isScrollInProgress
    // (The gesture reads these as they are now: a page of older messages arriving mid-drag must not restart it.)
    val latestTotal by androidx.compose.runtime.rememberUpdatedState(total)
    val latestHeight by androidx.compose.runtime.rememberUpdatedState(height)
    LaunchedEffect(scrolling, dragging) {
        if (scrolling || dragging) shown = true else { delay(1500); shown = false }
        if (!shown) used = false
    }
    if (total < minimum) return
    val handle = with(LocalDensity.current) { 56.dp.roundToPx() }
    val index = state.firstVisibleItemIndex
    val label = labelAt(index)
    // A tick as the date under the handle changes while it is held.
    LaunchedEffect(label) { if (dragging && label != null) Haptics.tick(haptic) }
    androidx.compose.animation.AnimatedVisibility(
        shown || dragging,
        Modifier.align(Alignment.TopEnd).fillMaxHeight().onSizeChanged { height = it.height },
        enter = androidx.compose.animation.fadeIn(androidx.compose.animation.core.tween(Motion.Short4)),
        exit = androidx.compose.animation.fadeOut(androidx.compose.animation.core.tween(Motion.Medium2)),
    ) {
        val room = (height - handle).coerceAtLeast(1)
        val fraction = if (total > 1) index.toFloat() / (total - 1) else 0f
        val y = ((1f - fraction) * room).toInt()
        val latestY by androidx.compose.runtime.rememberUpdatedState(y)
        Box(Modifier.fillMaxHeight()) {
            if (label != null && (dragging || used)) {
                Text(
                    label,
                    Modifier.align(Alignment.TopEnd).offset { IntOffset(0, y + handle / 4) }.padding(end = 56.dp).clip(RoundedCornerShape(16.dp))
                        .background(MaterialTheme.colorScheme.inverseSurface).padding(horizontal = 12.dp, vertical = 6.dp).testTag("fast-scroll-label"),
                    color = MaterialTheme.colorScheme.inverseOnSurface, style = MaterialTheme.typography.labelLarge,
                )
            }
            Box(
                Modifier.align(Alignment.TopEnd).offset { IntOffset(0, y) }.width(40.dp).height(56.dp)
                    .clip(RoundedCornerShape(topStart = 28.dp, bottomStart = 28.dp))
                    .background(MaterialTheme.colorScheme.secondaryContainer)
                    .semantics { contentDescription = t("Scroll through the history") }
                    .testTag("fast-scroll-handle")
                    .pointerInput(Unit) {
                        detectVerticalDragGestures(
                            onDragStart = { dragging = true; used = true; Haptics.grab(haptic) },
                            onDragEnd = { dragging = false; Haptics.tick(haptic) },
                            onDragCancel = { dragging = false },
                        ) { change, _ ->
                            change.consume()
                            // The finger's place along the track: the top is the oldest, the bottom the newest.
                            val track = (latestHeight - handle).coerceAtLeast(1)
                            val top = (change.position.y + latestY - handle / 2f).coerceIn(0f, track.toFloat())
                            val to = ((1f - top / track) * (latestTotal - 1)).toInt().coerceIn(0, latestTotal - 1)
                            scope.launch { state.scrollToItem(to) }
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevrons_up_down, size = 20.dp, tint = MaterialTheme.colorScheme.onSecondaryContainer)
            }
        }
    }
}
