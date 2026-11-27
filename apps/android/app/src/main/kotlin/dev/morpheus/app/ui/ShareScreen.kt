package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.Share
import dev.morpheus.app.ShareInbox
import dev.morpheus.app.SharedFile
import dev.morpheus.app.size
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.ShareTarget

/**
 * Where to send what another app shared (#340). The search bar is at the bottom with the keyboard up; the results sit above it,
 * the best match nearest the bar (the recent conversations, the newest nearest, when nothing is typed). A tap shows what is
 * shared with a box for a message; Send sends it and opens the room; Back cancels.
 */
@Composable
fun ShareScreen(model: AppModel, share: Share) {
    var query by remember { mutableStateOf("") }
    var picked by remember { mutableStateOf<ShareTarget?>(null) }
    // The list is the core's (the palette's fuzzy match); it is asked again when the channel list changes.
    val sidebar by model.sidebar.collectAsState()
    val targets by produceState(emptyList<ShareTarget>(), query, sidebar) {
        value = withContext(Dispatchers.Default) { model.morpheus.shareTargets(query) }
    }
    val chosen = picked
    BackHandler {
        if (chosen != null) picked = null else {
            ShareInbox.cancel()
        }
    }
    ScreenScaffold("share-screen", {
        TopBar(
            if (chosen == null) t("Share to") else t("Send to %{name}", "name" to chosen.name),
        )
    }) { inner ->
      Column(Modifier.fillMaxSize().padding(inner)) {
        if (chosen == null) {
            Pick(share, targets, query, { query = it }) { picked = it }
        } else {
            Confirm(model, share, chosen)
        }
      }
    }
}

@Composable
private fun ColumnScope.Pick(share: Share, targets: List<ShareTarget>, query: String, onQuery: (String) -> Unit, onPick: (ShareTarget) -> Unit) {
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(Unit) {
        delay(150)
        runCatching { focus.requestFocus() }
        keyboard?.show()
    }
    Text(summary(share), Modifier.padding(horizontal = 16.dp, vertical = 4.dp).testTag("share-summary"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    // Reversed: the first item (the best match, or the newest conversation) is at the bottom, nearest the search bar.
    // The best match stays at the bottom, in view, as the query or the list changes (a list that grew kept its place and left it below the bar).
    val list = androidx.compose.foundation.lazy.rememberLazyListState()
    LaunchedEffect(query, targets.map { it.id }) { list.scrollToItem(0) }
    LazyColumn(Modifier.stretch(list).weight(1f).fillMaxWidth().testTag("share-results"), state = list, reverseLayout = true) {
        items(targets, key = { it.id }) { target ->
            ListRow(
                target.name, "share-target-${target.name}",
                supporting = target.server ?: if (target.isDm) t("Direct message") else t("Room"),
                leading = { if (!target.isDm && target.server == null) ServerIcon(target.avatar, target.name, 36.dp) else Avatar(target.avatar, target.name, 36.dp) },
            ) { onPick(target) }
        }
    }
    OutlinedTextField(
        query, onQuery,
        Modifier.fillMaxWidth().padding(8.dp).focusRequester(focus).testTag("share-search"),
        placeholder = { Text(t("Search rooms and people")) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
    )
}

@Composable
private fun Confirm(model: AppModel, share: Share, target: ShareTarget) {
    // A text alone is what is sent (the box adds to it); with files the box is their caption, and a text that came with them starts in it.
    var message by remember { mutableStateOf(if (share.files.isEmpty()) "" else share.text.orEmpty()) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { delay(150); runCatching { focus.requestFocus() } }
    val send = { ShareInbox.send(model, target.id, message) }
    Column(Modifier.fillMaxSize().padding(16.dp).testTag("share-confirm"), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f).fillMaxWidth().testTag("share-preview"), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (share.files.isEmpty()) {
                Text(share.text.orEmpty(), Modifier.clip(RoundedCornerShape(Radius.md)).background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(12.dp), maxLines = 8, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
            } else {
                share.files.take(4).forEach { Preview(it) }
                if (share.files.size > 4) Text(t("and %{count} more", "count" to (share.files.size - 4).toString()), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
            }
        }
        OutlinedTextField(
            message, { message = it },
            Modifier.fillMaxWidth().focusRequester(focus).testTag("share-message"),
            placeholder = { Text(t("Add a message")) },
            maxLines = 4,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Default),
        )
        Button({ send() }, Modifier.fillMaxWidth().testTag("share-send")) { Text(t("Send")) }
    }
}

@Composable
private fun Preview(shared: SharedFile) {
    val picture by produceState<ImageBitmap?>(null, shared) {
        value = if (shared.mime.startsWith("image/")) withContext(Dispatchers.IO) { thumbnail(shared.file) } else null
    }
    val shown = picture
    if (shown != null) {
        Image(shown, shared.name, Modifier.fillMaxWidth().heightIn(max = 240.dp).clip(RoundedCornerShape(Radius.md)).testTag("share-image"), contentScale = ContentScale.Fit)
    } else {
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(Radius.md)).background(Palette.Surface).padding(12.dp).testTag("share-file"), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(shared.name, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
            Text(size(shared.file.length().toULong()), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
        }
    }
}

private fun thumbnail(file: java.io.File): ImageBitmap? {
    val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
    android.graphics.BitmapFactory.decodeFile(file.path, bounds)
    var sample = 1
    while (bounds.outWidth / sample > 1200) sample *= 2
    return android.graphics.BitmapFactory.decodeFile(file.path, android.graphics.BitmapFactory.Options().apply { inSampleSize = sample })?.asImageBitmap()
}

private fun summary(share: Share): String = when {
    share.files.isEmpty() -> share.text.orEmpty().replace('\n', ' ')
    share.files.size == 1 -> share.files[0].name
    else -> t("%{count} files", "count" to share.files.size.toString())
}
