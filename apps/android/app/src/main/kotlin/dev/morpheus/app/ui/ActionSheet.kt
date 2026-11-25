package dev.morpheus.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetValue
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.AppState
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
 * The sheet every long press opens: the same list as the desktop's right-click menu for that thing, a Material 3 ModalBottomSheet.
 * It opens at half the height, a drag on its top (or on its list, from the top) takes it up to nearly the whole screen, and a drag
 * down (or back, or a tap outside) closes it.
 */
@Composable
fun ActionSheetHost(model: AppModel) {
    val request by model.sheet.collectAsState()
    // The sheet that was open stays up while it hides (the sheet's own animation), as a picture only: it takes no touch and has
    // no place in the semantics tree, so the next touch goes to what is under it at once (#327).
    var last by remember { mutableStateOf<dev.morpheus.app.SheetRequest?>(null) }
    val current = request
    if (current != null && last?.id != current.id) last = current
    val haptic = LocalHapticFeedback.current
    LaunchedEffect(current?.id) {
        if (current != null && current.longPress) {
            haptic.performHapticFeedback(HapticFeedbackType.LongPress)
            AppState.haptics++
        }
    }
    val shown = last ?: return
    androidx.compose.runtime.key(shown.id) {
        ActionSheet(model, shown, live = current?.id == shown.id) { if (last?.id == shown.id) last = null }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
private fun ActionSheet(model: AppModel, request: dev.morpheus.app.SheetRequest, live: Boolean, onGone: () -> Unit) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    // Half the screen, or nearly all of it for a sheet that asks for that (the emoji picker).
    LaunchedEffect(Unit) { if (request.startFull) state.expand() }
    // Closed by the model (a row was chosen): it hides, then it is forgotten.
    LaunchedEffect(live) {
        if (!live) {
            state.hide()
            onGone()
        }
    }
    // Once it is closing (closed by the model, or let go at the bottom) its window takes no touch: the next touch goes to what is under it
    // at once, while it finishes hiding (#327).
    // The sheet is a window of its own; its view is found from a view placed in it (LocalView is the activity's).
    val inSheet = remember { arrayOfNulls<android.view.View>(1) }
    fun untouchable() {
        val view = inSheet[0] ?: return
        val root = view.rootView
        val params = root.layoutParams as? android.view.WindowManager.LayoutParams ?: return
        if (params.flags and android.view.WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE != 0) return
        params.flags = params.flags or android.view.WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
        try { view.context.getSystemService(android.view.WindowManager::class.java).updateViewLayout(root, params) } catch (_: Exception) {}
    }
    LaunchedEffect(live) { if (!live) untouchable() }
    // (Not while a finger is still on it: the drag down passes through "hidden" on its way, and taking the touch away would cancel it.)
    var fingerDown by remember { mutableStateOf(false) }
    var seen by remember { mutableStateOf(false) }
    val target = state.targetValue
    LaunchedEffect(target) { if (target != SheetValue.Hidden) seen = true }
    LaunchedEffect(target, fingerDown, seen) { if (target == SheetValue.Hidden && seen && !fingerDown) untouchable() }
    // Gone from the semantics tree the moment it is not live.
    val inert = if (live) Modifier else Modifier.clearAndSetSemantics { }
    ModalBottomSheet(
        onDismissRequest = { model.closeSheet() },
        sheetState = state,
        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor = MaterialTheme.colorScheme.onSurface,
        dragHandle = null,
    ) {
        // Back closes the sheet from wherever it is (Material's own goes from full to half first).
        androidx.activity.compose.BackHandler(enabled = live) { model.closeSheet() }
        androidx.compose.ui.viewinterop.AndroidView(factory = { c -> android.view.View(c).also { inSheet[0] = it } }, modifier = Modifier.size(0.dp))
        // What a tap on a message has always shown is tagged "actions" (the old tests look for it).
        if (request.quickReactions.isNotEmpty()) Box(Modifier.size(1.dp).testTag("actions"))
        var shown by remember { mutableStateOf<List<MenuRow>>(request.rows) }
        var trail by remember { mutableStateOf<List<MenuRow>>(emptyList()) }
        // Tall enough that half the screen is a stop and nearly all of it is the other.
        Column(Modifier.fillMaxWidth().fillMaxHeight(0.94f).pointerInput(Unit) {
            awaitPointerEventScope {
                while (true) fingerDown = awaitPointerEvent(androidx.compose.ui.input.pointer.PointerEventPass.Initial).changes.any { it.pressed }
            }
        }.then(inert).semantics { contentDescription = request.title }.testTag("sheet")) {
            Column(Modifier.fillMaxWidth().testTag("sheet-handle"), horizontalAlignment = Alignment.CenterHorizontally) {
                BottomSheetDefaults.DragHandle()
                Row(Modifier.fillMaxWidth().padding(horizontal = Space.lg).heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (trail.isNotEmpty()) {
                        IconAction(dev.morpheus.app.R.drawable.ic_lucide_chevron_left, t("Back"), "sheet-back", {
                            shown = if (trail.size == 1) request.rows else trail[trail.size - 2].children
                            trail = trail.dropLast(1)
                        })
                    }
                    Text(trail.lastOrNull()?.label ?: request.title, Modifier.weight(1f).testTag("sheet-title"), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (!live) {
                // Leaving: the rows as a picture, nothing that could take a touch.
                Column(Modifier.fillMaxWidth().weight(1f)) { for (row in shown.take(10)) SheetRow(row, interactive = false) { } }
            } else LazyColumn(Modifier.fillMaxWidth().weight(1f).testTag("sheet-list")) {
                if (trail.isEmpty() && request.quickReactions.isNotEmpty()) {
                    item(key = "quick") {
                        Row(Modifier.padding(horizontal = Space.lg, vertical = Space.sm), horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
                            for (emoji in request.quickReactions) {
                                Surface(
                                    shape = CircleShape, color = MaterialTheme.colorScheme.surface,
                                    modifier = Modifier.size(48.dp).pointerInput(emoji) { detectTapGestures { model.chooseReaction(emoji) } }.testTag("react-$emoji"),
                                ) { Box(contentAlignment = Alignment.Center) { Text(emoji, style = MaterialTheme.typography.titleMedium) } }
                            }
                        }
                    }
                }
                val parent = trail.lastOrNull()
                if (parent != null && parent.grid) {
                    item(key = "grid") {
                        FlowRow(Modifier.padding(horizontal = Space.lg, vertical = Space.sm), horizontalArrangement = Arrangement.spacedBy(Space.sm), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
                            for (cell in shown) {
                                Surface(
                                    shape = CircleShape, color = MaterialTheme.colorScheme.surface,
                                    modifier = Modifier.size(48.dp).pointerInput(cell.path) { detectTapGestures { model.chooseSheetItem(cell.path) } }.testTag("sheet-item-${cell.id}"),
                                ) { Box(contentAlignment = Alignment.Center) { Text(cell.label, style = MaterialTheme.typography.titleMedium) } }
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

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SheetRow(row: MenuRow, interactive: Boolean = true, onClick: () -> Unit) {
    var sure by remember { mutableStateOf(false) }
    val danger = row.danger
    val icon = rowIcon(row.icon)
    val tag = legacyTag(row.id) ?: "sheet-item-${row.id}"
    // A delete takes a second tap, as it always has on the phone.
    val needsSure = danger && row.id.startsWith("Delete")
    ListRow(
        if (sure) t("Tap again to delete") else row.label,
        tag,
        danger = danger,
        leading = {
            if (row.avatar != null) Avatar(null, row.avatar!!, Size.avatarSm)
            else if (icon != 0) GlyphIcon(icon) else Box(Modifier.size(24.dp))
        },
        trailing = if (row.checked || row.children.isNotEmpty()) {
            {
                Row(horizontalArrangement = Arrangement.spacedBy(Space.sm), verticalAlignment = Alignment.CenterVertically) {
                    if (row.checked) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_check, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.testTag("sheet-check-${row.id}"))
                    if (row.children.isNotEmpty()) GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_chevron_right)
                }
            }
        } else null,
        enabled = interactive,
        onClick = if (interactive) ({ if (needsSure && !sure) sure = true else onClick() }) else null,
    )
}
