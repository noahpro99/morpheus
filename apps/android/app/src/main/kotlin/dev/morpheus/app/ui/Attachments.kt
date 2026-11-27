package dev.morpheus.app.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import dev.morpheus.app.Picked
import dev.morpheus.app.readNow
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** A file waiting in the message box for the next message (the desktop's `Attached`): its tile's id, name, type and bytes. */
class Staged(val id: Long, val name: String, val mime: String, val bytes: ByteArray)

/** The files waiting in the box, as tiles above it: a picture or a file icon, the name, a cross that takes it away (#418). */
@Composable
fun AttachmentTiles(files: List<Staged>, onRemove: (Staged) -> Unit) {
    if (files.isEmpty()) return
    val scheme = MaterialTheme.colorScheme
    val fill = ComposerColours.fill(scheme)
    val ink = ComposerColours.text(scheme)
    LazyRow(
        Modifier.fillMaxWidth().padding(horizontal = Space.sm).testTag("attachments"),
        horizontalArrangement = Arrangement.spacedBy(Space.sm),
    ) {
        items(files, key = { it.id }) { file ->
            // The tile is as round as the pill it sits over; its picture is concentric with it (radius less the padding).
            Row(
                Modifier.widthIn(max = 220.dp).clip(RoundedCornerShape(16.dp)).background(fill).padding(start = Space.xs).testTag("attach-tile-${file.name}"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.xs),
            ) {
                val thumb = remember(file.id) {
                    if (file.mime.startsWith("image/")) BitmapFactory.Options().let { o ->
                        o.inSampleSize = 4
                        BitmapFactory.decodeByteArray(file.bytes, 0, file.bytes.size, o)?.asImageBitmap()
                    } else null
                }
                Box(Modifier.size(40.dp).clip(RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
                    if (thumb != null) Image(thumb, null, Modifier.size(40.dp), contentScale = ContentScale.Crop)
                    else GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_paperclip, size = 24.dp, tint = ink)
                }
                Text(file.name, Modifier.weight(1f, fill = false), color = ink, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium)
                IconAction(dev.morpheus.app.R.drawable.ic_lucide_x, t("Remove %{name}", "name" to file.name), "attach-remove-${file.name}", { onRemove(file) }, tint = ink)
            }
        }
    }
}

/**
 * Around a message box: a picture, GIF or sticker the keyboard hands over (its image button or a clipboard suggestion, #419) becomes a tile waiting
 * in the box. The field says it takes images (the editor info's content types) and the keyboard's `commitContent` lands in [onFile].
 */
@OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)
@Composable
fun ReceivesFiles(context: android.content.Context, onFile: (Picked) -> Unit, content: @Composable () -> Unit) {
    val take = androidx.compose.runtime.rememberUpdatedState(onFile)
    val interceptor = remember(context) {
        androidx.compose.ui.platform.PlatformTextInputInterceptor { request, nextHandler ->
            nextHandler.startInputMethod(object : androidx.compose.ui.platform.PlatformTextInputMethodRequest {
                override fun createInputConnection(outAttributes: android.view.inputmethod.EditorInfo): android.view.inputmethod.InputConnection {
                    val base = request.createInputConnection(outAttributes)
                    androidx.core.view.inputmethod.EditorInfoCompat.setContentMimeTypes(outAttributes, arrayOf("image/*"))
                    return androidx.core.view.inputmethod.InputConnectionCompat.createWrapper(base, outAttributes) { info, flags, _ ->
                        // The keyboard grants a read of the picture for as long as this call lasts.
                        val granted = flags and androidx.core.view.inputmethod.InputConnectionCompat.INPUT_CONTENT_GRANT_READ_URI_PERMISSION != 0
                        if (granted) try { info.requestPermission() } catch (_: Exception) { return@createWrapper false }
                        val picked = context.readNow(info.contentUri)
                        if (granted) info.releasePermission()
                        picked?.let { take.value(it); true } ?: false
                    }
                }
            })
        }
    }
    androidx.compose.ui.platform.InterceptPlatformTextInput(interceptor, content)
}
