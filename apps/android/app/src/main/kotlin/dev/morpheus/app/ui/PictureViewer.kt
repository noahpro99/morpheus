package dev.morpheus.app.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import kotlinx.coroutines.withTimeoutOrNull
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import uniffi.morpheus_ffi.MediaHandle

/** The biggest a picture is zoomed to. */
private const val MAX_ZOOM = 6f

/** How far a double tap zooms. */
private const val TAP_ZOOM = 2.5f

/**
 * The room's pictures full screen (#411): a swipe goes to the next or the one before, a pinch zooms, a drag moves a zoomed
 * picture, a double tap zooms in and out, a tap closes. `state` of the viewer says which one it is ("2/3"), of the picture how far it is zoomed.
 */
@Composable
fun PictureViewer(pictures: List<MediaHandle>, start: Int, onClose: () -> Unit) {
    // A window of its own: the chat's swipe back to its list never sees the finger that pages through the pictures.
    androidx.compose.ui.window.Dialog(onClose, androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) { PictureViewerContent(pictures, start, onClose) }
}

@Composable
private fun PictureViewerContent(pictures: List<MediaHandle>, start: Int, onClose: () -> Unit) {
    val pager = rememberPagerState(initialPage = start.coerceIn(0, (pictures.size - 1).coerceAtLeast(0))) { pictures.size }
    var zoomed by remember { mutableStateOf(false) }
    val position = "${pager.currentPage + 1}/${pictures.size}"
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .semantics { stateDescription = position }
            .testTag("image-viewer"),
    ) {
        HorizontalPager(pager, Modifier.fillMaxSize(), userScrollEnabled = !zoomed) { page ->
            ZoomablePicture(pictures[page], current = page == pager.currentPage, onZoomed = { zoomed = it }, onClose = onClose)
        }
        if (pictures.size > 1) {
            Text(
                "${pager.currentPage + 1} / ${pictures.size}",
                Modifier.align(Alignment.TopCenter).padding(top = Space.xl).background(Color.Black.copy(alpha = 0.5f), CircleShape).padding(horizontal = Space.md, vertical = Space.xs).testTag("viewer-count"),
                color = onColour(Color.Black),
                style = MaterialTheme.typography.labelLarge,
            )
        }
    }
}

@Composable
private fun ZoomablePicture(media: MediaHandle, current: Boolean, onZoomed: (Boolean) -> Unit, onClose: () -> Unit) {
    val bitmap by produceState<ImageBitmap?>(null, media.key()) { value = Images.load(media, 2048, 2048) }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    val zoomText = String.format(java.util.Locale.ROOT, "zoom %.1f", scale)
    fun within(o: Offset, s: Float, width: Float, height: Float): Offset {
        val x = (s - 1f) * width / 2f
        val y = (s - 1f) * height / 2f
        return Offset(o.x.coerceIn(-x, x), o.y.coerceIn(-y, y))
    }
    Box(
        Modifier
            .fillMaxSize()
            .clipToBounds()
            .pointerInput(Unit) {
                // One finger: a tap closes, two taps zoom in and out (a drag is the pager's, or moves a zoomed picture); a pinch always zooms.
                // (Done here, not with detectTapGestures: inside the pager the down is taken before that sees it.)
                awaitEachGesture {
                    val first = awaitFirstDown(requireUnconsumed = false)
                    var moved = false
                    do {
                        val event = awaitPointerEvent()
                        val pinching = event.changes.count { it.pressed } > 1
                        if (pinching) moved = true
                        if ((event.changes.first().position - first.position).getDistance() > viewConfiguration.touchSlop) moved = true
                        if (pinching || scale > 1f) {
                            val zoom = event.calculateZoom()
                            val pan = event.calculatePan()
                            scale = (scale * zoom).coerceIn(1f, MAX_ZOOM)
                            offset = if (scale <= 1f) Offset.Zero else within(offset + pan, scale, size.width.toFloat(), size.height.toFloat())
                            onZoomed(scale > 1f)
                            event.changes.forEach { if (it.positionChanged()) it.consume() }
                        }
                    } while (event.changes.any { it.pressed })
                    if (moved) return@awaitEachGesture
                    val second = withTimeoutOrNull(viewConfiguration.doubleTapTimeoutMillis) { awaitFirstDown(requireUnconsumed = false) }
                    if (second == null) {
                        onClose()
                    } else {
                        if (scale > 1f) { scale = 1f; offset = Offset.Zero } else scale = TAP_ZOOM
                        onZoomed(scale > 1f)
                        do { val event = awaitPointerEvent() } while (event.changes.any { it.pressed })
                    }
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        bitmap?.let {
            Image(
                it, null,
                Modifier
                    .fillMaxSize()
                    .graphicsLayer { scaleX = scale; scaleY = scale; translationX = offset.x; translationY = offset.y }
                    .then(if (current) Modifier.semantics { stateDescription = zoomText }.testTag("viewer-picture") else Modifier),
                contentScale = ContentScale.Fit,
            )
        }
    }
}
