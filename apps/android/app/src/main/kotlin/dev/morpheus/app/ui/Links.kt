package dev.morpheus.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.RoomModel
import dev.morpheus.app.openLink
import uniffi.morpheus_ffi.MessageBody

private const val LINK = LINK_TAG

/** A message's text with its links drawn as links: a tap on one opens it, a tap elsewhere offers the message's actions. */
@Composable
fun LinkedText(body: MessageBody.Text, edited: Boolean, sending: Boolean = false, maxLines: Int = Int.MAX_VALUE, lines: List<uniffi.morpheus_ffi.RichLine> = emptyList()) {
    val context = LocalContext.current
    val scheme = Palette.scheme
    val rich = remember(lines, scheme) { if (lines.isEmpty()) null else richOf(lines, Palette.Muted, Palette.Raised, Palette.Accent, Palette.Muted) }
    val text = rich?.text ?: remember(body, edited) {
        buildAnnotatedString {
            for (segment in body.segments) {
                if (segment.url != null) {
                    pushStringAnnotation(LINK, segment.url!!)
                    withStyle(SpanStyle(color = Palette.Accent, textDecoration = TextDecoration.Underline)) { append(segment.text) }
                    pop()
                } else {
                    append(segment.text)
                }
            }
            if (edited) append("  (edited)")
        }
    }
    val linked = if (rich != null) text.getStringAnnotations(LINK, 0, text.length).isNotEmpty() else body.segments.any { it.url != null }
    if (!linked) {
        Text(text, Modifier.semantics { if (sending) stateDescription = "sending" }.testTag("msg-body"), inlineContent = rich?.inline ?: emptyMap(), style = MaterialTheme.typography.bodyLarge, maxLines = maxLines, overflow = TextOverflow.Ellipsis)
        return
    }
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    Text(
        text,
        Modifier
            .pointerInput(text) {
                // Only a tap that lands on a link is this text's: anything else is left for the message (a double tap, a long press).
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    val offset = layout?.getOffsetForPosition(down.position)
                    val link = offset?.let { text.getStringAnnotations(LINK, it, it).firstOrNull() } ?: return@awaitEachGesture
                    val up = waitForUpOrCancellation()
                    // Held longer than a tap: that is the message's long press, not a link followed.
                    if (up != null && up.uptimeMillis - down.uptimeMillis < viewConfiguration.longPressTimeoutMillis) {
                        up.consume()
                        context.openLink(link.item)
                    }
                }
            }
            .semantics { if (sending) stateDescription = "sending" }
            .testTag("msg-body"),
        inlineContent = rich?.inline ?: emptyMap(),
        onTextLayout = { layout = it },
        style = MaterialTheme.typography.bodyLarge,
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
    )
}

/** The card under a message with a link: site, title, description and picture; a tap opens the page. */
@Composable
fun PreviewCardView(model: RoomModel, url: String, eventId: String) {
    val context = LocalContext.current
    val card by produceState<uniffi.morpheus_ffi.PreviewCard?>(null, url) { value = model.preview(url) }
    val shown = card ?: return
    val bitmap by produceState<ImageBitmap?>(null, shown.url) { value = shown.image?.let { Images.load(it, 600, 400) } }
    Column(
        Modifier
            .padding(top = 8.dp)
            .widthIn(max = 320.dp)
            .background(Palette.Raised, RoundedCornerShape(8.dp))
            .pointerInput(url) { detectTapGestures { context.openLink(url) } }
            .padding(12.dp)
            .testTag("preview-card"),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row {
            Text(shown.site ?: "", Modifier.weight(1f), color = Palette.Muted, style = MaterialTheme.typography.bodySmall, maxLines = 1)
            GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_close, Modifier.pointerInput(eventId) { detectTapGestures { model.dismissPreview(eventId) } }.semantics { contentDescription = t("Dismiss preview") }.testTag("preview-dismiss"), size = 20.dp, tint = Palette.Muted)
        }
        shown.title?.let { Text(it, Modifier.testTag("preview-title"), color = Palette.Accent, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge) }
        shown.description?.let { Text(it, Modifier.testTag("preview-description"), style = MaterialTheme.typography.bodyMedium, maxLines = 3, overflow = TextOverflow.Ellipsis) }
        bitmap?.let {
            androidx.compose.foundation.Image(it, null, Modifier.fillMaxWidth().padding(top = 4.dp).testTag("preview-image"), contentScale = androidx.compose.ui.layout.ContentScale.FillWidth)
        }
    }
}
