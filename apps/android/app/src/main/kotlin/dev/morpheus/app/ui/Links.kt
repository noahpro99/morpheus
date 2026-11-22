package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.gestures.detectTapGestures
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

private const val LINK = "URL"

/** A message's text with its links drawn as links: a tap on one opens it, a tap elsewhere offers the message's actions. */
@Composable
fun LinkedText(body: MessageBody.Text, edited: Boolean, onActions: () -> Unit) {
    val context = LocalContext.current
    val text = remember(body, edited) {
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
    if (body.segments.none { it.url != null }) {
        Text(text, Modifier.testTag("msg-body"), style = Type.body)
        return
    }
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    Text(
        text,
        Modifier
            .pointerInput(text) {
                detectTapGestures { at ->
                    val offset = layout?.getOffsetForPosition(at)
                    val link = offset?.let { text.getStringAnnotations(LINK, it, it).firstOrNull() }
                    if (link != null) context.openLink(link.item) else onActions()
                }
            }
            .testTag("msg-body"),
        onTextLayout = { layout = it },
        style = Type.body,
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
            .padding(top = 6.dp)
            .widthIn(max = 320.dp)
            .background(Palette.Raised, RoundedCornerShape(8.dp))
            .pointerInput(url) { detectTapGestures { context.openLink(url) } }
            .padding(10.dp)
            .testTag("preview-card"),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Row {
            Text(shown.site ?: "", Modifier.weight(1f), color = Palette.Muted, fontSize = 12.sp, maxLines = 1)
            GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_x, Modifier.pointerInput(eventId) { detectTapGestures { model.dismissPreview(eventId) } }.semantics { contentDescription = t("Dismiss preview") }.testTag("preview-dismiss"), size = 18.dp, tint = Palette.Muted)
        }
        shown.title?.let { Text(it, Modifier.testTag("preview-title"), color = Palette.Accent, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis) }
        shown.description?.let { Text(it, Modifier.testTag("preview-description"), fontSize = 13.sp, maxLines = 3, overflow = TextOverflow.Ellipsis) }
        bitmap?.let {
            androidx.compose.foundation.Image(it, null, Modifier.fillMaxWidth().padding(top = 4.dp).testTag("preview-image"), contentScale = androidx.compose.ui.layout.ContentScale.FillWidth)
        }
    }
}
