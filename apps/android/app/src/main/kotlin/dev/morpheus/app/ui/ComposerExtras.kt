package dev.morpheus.app.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.items as gridItems
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.RoomModel
import uniffi.morpheus_ffi.CustomEmojiSection
import uniffi.morpheus_ffi.EmojiSection
import uniffi.morpheus_ffi.MediaHandle
import uniffi.morpheus_ffi.SuggestionItem
import uniffi.morpheus_ffi.SuggestionKind

/** What the word being typed could become: people after @, emoji after a colon. */
@Composable
fun SuggestionStrip(items: List<SuggestionItem>, onPick: (SuggestionItem) -> Unit) {
    if (items.isEmpty()) return
    LazyRow(
        Modifier.fillMaxWidth().background(Palette.Raised).testTag("suggestions"),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        items(items, key = { it.kind.name + it.label + it.detail }) { item ->
            Row(
                Modifier
                    .clickable { onPick(item) }
                    .padding(horizontal = 12.dp, vertical = 10.dp)
                    .testTag("suggestion-${item.label}"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                when (item.kind) {
                    SuggestionKind.EMOJI -> {
                        Text(item.label, fontSize = 22.sp)
                        Text(item.detail.orEmpty(), color = Palette.Muted, fontSize = 12.sp)
                    }
                    SuggestionKind.CUSTOM -> {
                        item.media?.let { CustomEmojiImage(it, 28) }
                        Text(":${item.label}:", color = Palette.Muted, fontSize = 13.sp)
                    }
                    SuggestionKind.MEMBER -> Text("@${item.label}")
                }
            }
        }
    }
}

@Composable
fun CustomEmojiImage(media: MediaHandle, size: Int) {
    val bitmap by produceState<ImageBitmap?>(null, media.key()) { value = Images.load(media, size * 3, size * 3) }
    Box(Modifier.size(size.dp)) {
        bitmap?.let { Image(it, null, Modifier.size(size.dp)) }
    }
}

/** The emoji panel: a search box, every category, and the custom emoji in reach. */
@Composable
fun EmojiPanel(model: RoomModel, onPick: (String) -> Unit) {
    var query by remember { mutableStateOf("") }
    var sections by remember { mutableStateOf<List<EmojiSection>>(emptyList()) }
    var custom by remember { mutableStateOf<List<CustomEmojiSection>>(emptyList()) }
    LaunchedEffect(query) {
        sections = model.emojiSections(query)
        custom = model.customEmoji(query)
    }
    androidx.compose.foundation.layout.Column(Modifier.fillMaxWidth().height(300.dp).background(Palette.Surface).testTag("emoji-panel")) {
        SearchField(query, { query = it }, t("Search emoji"), "emoji-search", Modifier.padding(8.dp))
        LazyVerticalGrid(GridCells.Adaptive(44.dp), Modifier.fillMaxWidth()) {
            for (pack in custom) {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    Text(pack.name, Modifier.padding(8.dp), color = Palette.Muted, fontSize = 12.sp)
                }
                gridItems(pack.emotes, key = { "c-" + pack.name + it.shortcode }) { emote ->
                    Box(
                        Modifier.size(44.dp).clickable { onPick(":${emote.shortcode}:") }.testTag("custom-${emote.shortcode}"),
                        contentAlignment = Alignment.Center,
                    ) { CustomEmojiImage(emote.media, 32) }
                }
            }
            for (section in sections) {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    Text(section.heading, Modifier.padding(8.dp), color = Palette.Muted, fontSize = 12.sp)
                }
                gridItems(section.cells, key = { section.heading + it }) { glyph ->
                    Box(
                        Modifier.size(44.dp).clickable { onPick(glyph) }.testTag("emoji-$glyph"),
                        contentAlignment = Alignment.Center,
                    ) { Text(glyph, fontSize = 24.sp) }
                }
            }
        }
    }
}
