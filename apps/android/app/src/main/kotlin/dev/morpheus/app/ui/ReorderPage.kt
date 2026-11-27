package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.sentence
import kotlin.math.roundToInt
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.ReorderSection

/** One line of the flattened list: a section's header (the server's own channels have one too) or a channel. */
private sealed interface Line {
    val key: String
    class Header(val section: ReorderSection, val category: Boolean) : Line { override val key = "h-" + section.id }
    class Channel(val section: ReorderSection, val id: String, val name: String, val voice: Boolean) : Line { override val key = "c-$id" }
}

private val RowHeight = 56.dp

/** Drag channels between and within a server's sections, and categories among themselves (#424); each drop is written for everyone. */
@Composable
fun ReorderPage(app: AppModel, spaceId: String) {
    var sections by remember { mutableStateOf<List<ReorderSection>>(emptyList()) }
    var reload by remember { mutableIntStateOf(0) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(reload) {
        // A drop reaches the list with the next sync: look again a moment later too.
        repeat(if (reload == 0) 1 else 6) {
            sections = app.morpheus.reorderInfo(spaceId).orEmpty()
            kotlinx.coroutines.delay(600)
        }
    }
    val lines = remember(sections) {
        sections.flatMap { s -> listOf<Line>(Line.Header(s, s.id != spaceId)) + s.rooms.map { Line.Channel(s, it.id, it.name, it.isVoice) } }
    }
    val rowPx = with(LocalDensity.current) { RowHeight.toPx() }
    var dragging by remember { mutableStateOf<String?>(null) }
    var shift by remember { mutableFloatStateOf(0f) }
    fun drop(from: Int, to: Int) {
        if (from == to) return
        val dragged = lines[from]
        val target = lines[to]
        scope.launch {
            try {
                when (dragged) {
                    is Line.Channel -> when (target) {
                        is Line.Channel -> app.morpheus.moveChannel(spaceId, dragged.id, target.section.id, target.id, to > from)
                        is Line.Header -> app.morpheus.moveChannel(spaceId, dragged.id, target.section.id, null, false)
                    }
                    is Line.Header -> {
                        val into = (target as? Line.Header)?.section ?: (target as Line.Channel).section
                        if (into.id != spaceId && into.id != dragged.section.id) app.morpheus.moveCategory(spaceId, dragged.section.id, into.id, to > from)
                    }
                }
                error = null
                reload++
            } catch (e: Exception) { error = e.sentence() }
        }
    }
    PageFrame(app, "Reorder channels", "page-reorder") {
        Text(t("Hold a handle and drag a channel, or a category, to where it should go."), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        Column(Modifier.fillMaxWidth().testTag("reorder-list")) {
            lines.forEachIndexed { index, line ->
                val lifted = dragging == line.key
                val title = when (line) { is Line.Header -> line.section.title ?: t("Channels"); is Line.Channel -> line.name }
                val draggable = line is Line.Channel || (line as Line.Header).category
                Row(
                    Modifier.fillMaxWidth().height(RowHeight)
                        .offset { IntOffset(0, if (lifted) shift.roundToInt() else 0) }
                        .then(if (lifted) Modifier.shadow(4.dp).background(MaterialTheme.colorScheme.surfaceContainerHigh) else Modifier)
                        .testTag(if (line is Line.Header) "reorder-header-$title" else "reorder-row-$title"),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (draggable) Box(
                        Modifier.size(Size.target).testTag("reorder-handle-$title").pointerInput(line.key, lines) {
                            detectDragGestures(
                                onDragStart = { dragging = line.key; shift = 0f },
                                onDragEnd = {
                                    val to = (index + (shift / rowPx).roundToInt()).coerceIn(0, lines.size - 1)
                                    dragging = null; shift = 0f
                                    drop(index, to)
                                },
                                onDragCancel = { dragging = null; shift = 0f },
                            ) { change, amount -> change.consume(); shift += amount.y }
                        },
                        contentAlignment = Alignment.Center,
                    ) { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_move) } else Box(Modifier.size(Size.target))
                    Text(
                        title, Modifier.weight(1f).padding(end = Space.lg), maxLines = 1,
                        style = if (line is Line.Header) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyLarge,
                        color = if (line is Line.Header) Palette.Muted else MaterialTheme.colorScheme.onSurface,
                    )
                }
            }
        }
        Note(null, error)
    }
}
