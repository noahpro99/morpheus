package dev.morpheus.app.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateMap
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.draw.clip
import kotlinx.coroutines.launch
import dev.morpheus.app.CallModel
import uniffi.morpheus_ffi.CallCard
import uniffi.morpheus_ffi.CallPhase
import uniffi.morpheus_ffi.CallSnapshot
import uniffi.morpheus_ffi.CardKind

/**
 * The video of the call that is being looked at (#529): a shared screen that is watched, else someone else's camera that is not hidden.
 * Null for a voice call, a screen that is shared but not watched, and a camera hidden from the card's menu. The call screen, the floating
 * window (#528) and the picture-in-picture window all show this one, and the app goes into picture-in-picture outside itself only when it exists.
 */
fun watchedVideo(snapshot: CallSnapshot, hidden: Set<String>): CallCard? {
    if (snapshot.phase != CallPhase.CONNECTED) return null
    val me = snapshot.people.firstOrNull { it.me }?.userId
    return snapshot.cards.firstOrNull { it.kind == CardKind.SCREEN && it.userId != me }
        ?: snapshot.cards.firstOrNull { it.kind == CardKind.CAMERA && it.userId != me && it.userId !in hidden }
}

/** What the call square shows when there is no video: whoever speaks, else the first other person, else me. */
fun speakerCard(snapshot: CallSnapshot): CallCard? {
    val me = snapshot.people.firstOrNull { it.me }?.userId
    val people = snapshot.cards.filter { it.kind == CardKind.PERSON || it.kind == CardKind.CAMERA }
    return people.firstOrNull { it.speaking && it.userId != me } ?: people.firstOrNull { it.userId != me } ?: people.firstOrNull()
}

/** The corners of the floating call window, and where it sits: pure, so it can be tested without a screen. */
data class FloatCorner(val right: Boolean, val bottom: Boolean)

/** The corner nearest to a window's centre point `(cx, cy)` in an area of `width` by `height`. */
fun nearestCorner(cx: Float, cy: Float, width: Float, height: Float) = FloatCorner(cx > width / 2f, cy > height / 2f)

/** Where the square's top left goes in a corner: inside `[0, width]` by `[top, bottom]`, `size` wide and `margin` from the edges. */
fun cornerPosition(corner: FloatCorner, width: Float, top: Float, bottom: Float, size: Float, margin: Float): Pair<Float, Float> {
    val x = if (corner.right) width - size - margin else margin
    val y = if (corner.bottom) bottom - size - margin else top + margin
    return x to maxOf(y, top)
}

/**
 * What the floating window has to stay clear of: the tops (window y, pixels) of the search bar, the composer, anything at the foot of a screen
 * that the square would hide. A screen reports its own, with [KeepClear], and it is forgotten when the screen goes.
 */
object FloatingAvoid {
    internal val tops: SnapshotStateMap<String, Float> = mutableStateMapOf()
}

/** A zero-height marker: what is below it in this column is kept clear of the floating call window. */
@Composable
fun KeepClear(key: String, modifier: Modifier = Modifier) {
    DisposableEffect(key) { onDispose { FloatingAvoid.tops.remove(key) } }
    Box(modifier.fillMaxWidth().height(0.dp).onGloballyPositioned { FloatingAvoid.tops[key] = it.positionInWindow().y })
}

private val SquareSize = 96.dp
private val EdgeMargin = 12.dp
private val TopBarClearance = 64.dp

/**
 * The call in the app (#528), when its screen is not open: a small draggable square over everything but the call screen, showing the video being
 * watched or whoever speaks, and whether I am muted. A tap opens the call; a drag lets go into the nearest corner, which is kept across screens.
 * It stays above the search bar and the composer (see [KeepClear]).
 */
@Composable
fun BoxScope.FloatingCall(calls: CallModel, snapshot: CallSnapshot, modifier: Modifier = Modifier) {
    val density = LocalDensity.current
    val hidden by calls.hiddenVideo.collectAsState()
    val corner by calls.floatCorner.collectAsState()
    val scope = rememberCoroutineScope()
    var area by remember { mutableStateOf(androidx.compose.ui.unit.IntSize.Zero) }
    var origin by remember { mutableStateOf(Offset.Zero) }
    val size = with(density) { SquareSize.toPx() }
    val margin = with(density) { EdgeMargin.toPx() }
    val statusTop = WindowInsets.statusBars.getTop(density).toFloat()
    val navBottom = WindowInsets.navigationBars.getBottom(density).toFloat()
    val avoid = FloatingAvoid.tops.values.minOrNull()
    val width = area.width.toFloat()
    val top = maxOf(0f, statusTop - origin.y) + with(density) { TopBarClearance.toPx() }
    val bottom = if (avoid != null) avoid - origin.y else area.height - navBottom
    val target = cornerPosition(corner, width, top, bottom, size, margin).let { Offset(it.first, it.second) }
    val position = remember { Animatable(target, Offset.VectorConverter) }
    var dragging by remember { mutableStateOf(false) }
    LaunchedEffect(target, dragging, area) {
        if (dragging || area.width == 0) return@LaunchedEffect
        if (position.value == Offset.Zero) position.snapTo(target) else position.animateTo(target, spring(stiffness = 600f))
    }
    Box(modifier.fillMaxSize().onGloballyPositioned { area = it.size; origin = it.positionInWindow() }) {
        if (area.width == 0) return@Box
        val video = watchedVideo(snapshot, hidden)
        val card = video ?: speakerCard(snapshot)
        val me = snapshot.people.firstOrNull { it.me }
        Box(
            Modifier
                .offset { IntOffset(position.value.x.toInt(), position.value.y.toInt()) }
                .size(SquareSize)
                .background(Color.Black, RoundedCornerShape(Radius.lg))
                .border(if (card?.speaking == true) 4.dp else 0.dp, if (card?.speaking == true) speakingGreen else Color.Transparent, RoundedCornerShape(Radius.lg))
                .clip(RoundedCornerShape(Radius.lg))
                .pointerInput(Unit) { detectTapGestures(onTap = { calls.show(true) }) }
                .pointerInput(area, origin, avoid) {
                    detectDragGestures(
                        onDragStart = { dragging = true },
                        onDrag = { change, delta -> change.consume(); scope.launch { position.snapTo(position.value + delta) } },
                        onDragEnd = {
                            dragging = false
                            calls.setFloatCorner(nearestCorner(position.value.x + size / 2f, position.value.y + size / 2f, width, area.height.toFloat()))
                        },
                        onDragCancel = { dragging = false },
                    )
                }
                .semantics { role = Role.Button; contentDescription = t("Call: %{name}", "name" to snapshot.roomName); onClick { calls.show(true); true } }
                .testTag("call-float"),
            contentAlignment = Alignment.Center,
        ) {
            if (card != null) when (card.kind) {
                CardKind.SCREEN, CardKind.CAMERA -> if (video != null) VideoPicture(calls, card) else Avatar(card.avatar, card.name, 56.dp)
                else -> Avatar(card.avatar, card.name, 56.dp)
            }
            if (me?.muted == true) Box(Modifier.align(Alignment.BottomEnd).padding(8.dp).background(Color(0x99000000), RoundedCornerShape(8.dp)).padding(4.dp).testTag("float-muted")) {
                GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_mic_off, size = 16.dp, tint = Color.White)
            }
        }
    }
}
