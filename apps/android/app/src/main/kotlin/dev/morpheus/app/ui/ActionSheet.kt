package dev.morpheus.app.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.AppState
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.MenuRow

/** The legacy test tags of a message's actions (what a tap on a message has offered since the first version). */
private fun legacyTag(label: String): String? = when (label) {
    "Reply" -> "action-reply"
    "Reply in thread" -> "action-thread"
    "Copy text" -> "action-copy"
    "Edit message" -> "action-edit"
    "Delete message" -> "action-delete"
    else -> null
}

/** The glyph a row names (`pin-off`) as the drawable generated from the desktop's icons, if there is one. */
@Composable
private fun rowIcon(name: String?): Int {
    val context = LocalContext.current
    if (name == null) return 0
    return remember(name) { context.resources.getIdentifier("ic_lucide_" + name.replace('-', '_'), "drawable", context.packageName) }
}

/**
 * The sheet every long press opens: the same list as the desktop's right-click menu for that thing. It opens at
 * half the height, a drag on its top (or on its list, from the top) takes it up to nearly the whole screen,
 * and a drag down (or back, or a tap outside) closes it.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ActionSheetHost(model: AppModel) {
    val request by model.sheet.collectAsState()
    val current = request ?: return
    val haptic = LocalHapticFeedback.current
    LaunchedEffect(current.id) {
        if (current.longPress) {
            haptic.performHapticFeedback(HapticFeedbackType.LongPress)
            AppState.haptics++
        }
    }
    androidx.activity.compose.BackHandler { model.closeSheet() }
    androidx.compose.runtime.key(current.id) { ActionSheet(model, current) }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ActionSheet(model: AppModel, request: dev.morpheus.app.SheetRequest) {
    val scope = rememberCoroutineScope()
    BoxWithConstraints(Modifier.fillMaxSize().testTag("sheet-layer")) {
        val full = with(LocalDensity.current) { (maxHeight * 0.06f).toPx() }
        val half = with(LocalDensity.current) { (maxHeight * 0.5f).toPx() }
        val hidden = with(LocalDensity.current) { maxHeight.toPx() }
        val top = remember { Animatable(hidden) }
        val dismiss: () -> Unit = {
            scope.launch {
                top.animateTo(hidden, tween(160))
                model.closeSheet()
            }
        }
        LaunchedEffect(Unit) { top.animateTo(if (request.startFull) full else half, tween(200)) }
        // Where a let-go sheet comes to rest: the nearest of full, half and gone, leaning the way it was thrown.
        fun settle(velocity: Float) {
            val aimed = top.value + velocity * 0.15f
            val goal = listOf(full, half, hidden).minByOrNull { kotlin.math.abs(it - aimed) } ?: half
            if (goal == hidden) dismiss() else scope.launch { top.animateTo(goal, tween(180)) }
        }
        val drag = rememberDraggableState { delta -> scope.launch { top.snapTo((top.value + delta).coerceIn(full, hidden)) } }
        // The list gives its scroll to the sheet while the sheet is not all the way up, and when it is at its top.
        val connection = remember {
            object : NestedScrollConnection {
                override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                    if (available.y < 0f && top.value > full) {
                        val used = kotlin.math.max(available.y, full - top.value)
                        scope.launch { top.snapTo(top.value + used) }
                        return Offset(0f, used)
                    }
                    return Offset.Zero
                }
                override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
                    if (available.y > 0f) {
                        scope.launch { top.snapTo((top.value + available.y).coerceIn(full, hidden)) }
                        return Offset(0f, available.y)
                    }
                    return Offset.Zero
                }
                override suspend fun onPostFling(consumed: Velocity, available: Velocity): Velocity {
                    settle(available.y)
                    return available
                }
            }
        }
        val shade = ((hidden - top.value) / (hidden - half)).coerceIn(0f, 1f)
        Box(
            Modifier.fillMaxSize().background(Color(0x99000000).copy(alpha = 0.6f * shade)).pointerInput(Unit) { detectTapGestures { dismiss() } }.testTag("sheet-scrim"),
        )
        Column(
            Modifier
                .offset { androidx.compose.ui.unit.IntOffset(0, top.value.toInt()) }
                .fillMaxWidth()
                .fillMaxHeight()
                .clip(RoundedCornerShape(topStart = Radius.sheet, topEnd = Radius.sheet))
                .background(Palette.Raised)
                .pointerInput(Unit) { detectTapGestures { } }
                .semantics { contentDescription = request.title }
                .testTag("sheet"),
        ) {
            // What a tap on a message has always shown is tagged "actions" (the old tests look for it).
            if (request.quickReactions.isNotEmpty()) Box(Modifier.size(1.dp).testTag("actions"))
            var shown by remember { mutableStateOf<List<MenuRow>>(request.rows) }
            var trail by remember { mutableStateOf<List<MenuRow>>(emptyList()) }
            Column(
                Modifier
                    .fillMaxWidth()
                    .draggable(drag, Orientation.Vertical, onDragStopped = { settle(it) })
                    .testTag("sheet-handle"),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(Modifier.padding(top = Space.sm, bottom = Space.xs).size(width = 36.dp, height = 4.dp).clip(CircleShape).background(Palette.Muted.copy(alpha = 0.5f)))
                Row(Modifier.fillMaxWidth().padding(horizontal = Space.lg).heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (trail.isNotEmpty()) {
                        IconAction(dev.morpheus.app.R.drawable.ic_lucide_chevron_left, t("Back"), "sheet-back", {
                            shown = if (trail.size == 1) request.rows else trail[trail.size - 2].children
                            trail = trail.dropLast(1)
                        })
                    }
                    Text(trail.lastOrNull()?.label ?: request.title, Modifier.weight(1f).testTag("sheet-title"), style = Type.heading, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            LazyColumn(Modifier.fillMaxWidth().weight(1f).nestedScroll(connection).testTag("sheet-list")) {
                if (trail.isEmpty() && request.quickReactions.isNotEmpty()) {
                    item(key = "quick") {
                        Row(Modifier.padding(horizontal = Space.lg, vertical = Space.sm), horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                            for (emoji in request.quickReactions) {
                                Box(
                                    Modifier.size(48.dp).background(Palette.Surface, CircleShape)
                                        .pointerInput(emoji) { detectTapGestures { model.chooseReaction(emoji) } }
                                        .testTag("react-$emoji"),
                                    contentAlignment = Alignment.Center,
                                ) { Text(emoji, style = Type.heading) }
                            }
                        }
                    }
                }
                val parent = trail.lastOrNull()
                if (parent != null && parent.grid) {
                    item(key = "grid") {
                        FlowRow(Modifier.padding(horizontal = Space.lg, vertical = Space.sm), horizontalArrangement = Arrangement.spacedBy(Space.sm), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
                            for (cell in shown) {
                                Box(
                                    Modifier.size(48.dp).background(Palette.Surface, CircleShape)
                                        .pointerInput(cell.path) { detectTapGestures { model.chooseSheetItem(cell.path) } }
                                        .testTag("sheet-item-${cell.label}"),
                                    contentAlignment = Alignment.Center,
                                ) { Text(cell.label, style = Type.heading) }
                            }
                        }
                    }
                } else {
                    items(shown, key = { it.path }) { row ->
                        SheetRow(row) {
                            if (row.children.isNotEmpty()) {
                                trail = trail + row
                                shown = row.children
                            } else model.chooseSheetItem(row.path)
                        }
                    }
                }
                item(key = "end") { Box(Modifier.size(Space.xxl)) }
            }
        }
    }
}

@Composable
private fun SheetRow(row: MenuRow, onClick: () -> Unit) {
    var sure by remember { mutableStateOf(false) }
    val danger = row.danger
    val icon = rowIcon(row.icon)
    val tag = legacyTag(row.label) ?: "sheet-item-${row.label}"
    // A delete takes a second tap, as it always has on the phone.
    val needsSure = danger && row.label.startsWith("Delete")
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = Size.row)
            .clickable { if (needsSure && !sure) sure = true else onClick() }
            .semantics { role = Role.Button }
            .padding(horizontal = Space.lg)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.lg),
    ) {
        val tint = if (danger) Palette.Danger else Palette.Muted
        if (row.avatar != null) Avatar(null, row.avatar!!, Size.avatarSm)
        else if (icon != 0) GlyphIcon(icon, tint = tint) else Box(Modifier.size(24.dp))
        Text(
            if (sure) t("Tap again to delete") else row.label,
            Modifier.weight(1f), style = Type.body, color = if (danger) Palette.Danger else Palette.Text, maxLines = 1, overflow = TextOverflow.Ellipsis,
        )
        if (row.checked) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_check, tint = Palette.Accent, modifier = Modifier.testTag("sheet-check-${row.label}"))
        if (row.children.isNotEmpty()) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_right, tint = Palette.Muted)
    }
}
