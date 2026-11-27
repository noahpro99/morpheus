package dev.morpheus.app.ui

import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import uniffi.morpheus_ffi.MediaHandle
import uniffi.morpheus_ffi.RichKind
import uniffi.morpheus_ffi.RichLine

/** The annotation a link's text carries; a tap on it opens the link. */
internal const val LINK_TAG = "URL"

/** A formatted message (#405): the core's lines (bold, italic, code, quotes, lists, custom emoji) as one annotated string, and the pictures its custom emoji stand for. */
internal class Rich(val text: AnnotatedString, val inline: Map<String, InlineTextContent>)

internal fun richOf(lines: List<RichLine>, quoteColour: androidx.compose.ui.graphics.Color, codeBackground: androidx.compose.ui.graphics.Color, linkColour: androidx.compose.ui.graphics.Color, mutedColour: androidx.compose.ui.graphics.Color): Rich {
    val inline = mutableMapOf<String, InlineTextContent>()
    val text = buildAnnotatedString {
        lines.forEachIndexed { index, line ->
            if (index > 0) append("\n")
            if (line.kind == RichKind.RULE) {
                append("———")
                return@forEachIndexed
            }
            if (line.indent > 0u) append("  ".repeat((line.indent - 1u).toInt()))
            val marker = line.marker
            if (marker != null) append("$marker ") else if (line.indent > 0u) append("   ")
            if (line.quote > 0u) pushStyle(SpanStyle(color = quoteColour, fontStyle = FontStyle.Italic))
            for (run in line.runs) {
                val emote = run.emote
                if (emote != null) {
                    val id = "emote${inline.size}"
                    inline[id] = InlineTextContent(Placeholder(1.5.em, 1.5.em, PlaceholderVerticalAlign.TextCenter)) { EmoteImage(emote, Modifier.size(24.dp), "msg-emote") }
                    appendInlineContent(id, run.text)
                    continue
                }
                var decoration = TextDecoration.None
                if (run.underline || run.link != null) decoration += TextDecoration.Underline
                if (run.strike) decoration += TextDecoration.LineThrough
                val style = SpanStyle(
                    fontWeight = if (run.bold || run.mention) FontWeight.Bold else null,
                    fontStyle = if (run.italic) FontStyle.Italic else null,
                    textDecoration = if (decoration == TextDecoration.None) null else decoration,
                    fontFamily = if (run.code) FontFamily.Monospace else null,
                    background = if (run.code || line.kind == RichKind.CODE) codeBackground else androidx.compose.ui.graphics.Color.Unspecified,
                    color = when {
                        run.link != null || run.mention -> linkColour
                        run.faint -> mutedColour
                        else -> androidx.compose.ui.graphics.Color.Unspecified
                    },
                )
                if (run.link != null && !run.mention) pushStringAnnotation(LINK_TAG, run.link!!)
                withStyle(style) { append(run.text) }
                if (run.link != null && !run.mention) pop()
            }
            if (line.quote > 0u) pop()
        }
    }
    return Rich(text, inline)
}

/** A custom emoji's picture, as big as [modifier] says; `tag` is what the e2e tests find it by (its state says whether it has loaded). */
@Composable
fun EmoteImage(media: MediaHandle, modifier: Modifier, tag: String) {
    val bitmap by produceState<ImageBitmap?>(null, media.key()) { value = Images.load(media, 96, 96) }
    val loaded = bitmap
    androidx.compose.runtime.LaunchedEffect(loaded != null) { if (loaded != null) dev.morpheus.app.AppState.emotesDrawn++ }
    if (loaded != null) {
        Image(loaded, null, modifier.testTag(tag).semantics { stateDescription = "loaded" })
    } else {
        androidx.compose.foundation.layout.Box(modifier.clip(RoundedCornerShape(4.dp)).background(Palette.Raised).testTag(tag).semantics { stateDescription = "loading" })
    }
}
