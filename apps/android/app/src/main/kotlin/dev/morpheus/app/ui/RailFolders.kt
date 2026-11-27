package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import uniffi.morpheus_ffi.RailRowItem
import uniffi.morpheus_ffi.ScopeItem
import uniffi.morpheus_ffi.ScopeKind

/** What the rail draws, top to bottom in the account's order (#428): a server or other scope, or a folder of servers (its servers follow it while it is open). */
sealed interface RailEntry {
    val key: String

    /** A scope; `inFolder` when it is shown under its open folder. */
    data class Scope(val item: ScopeItem, val inFolder: Boolean) : RailEntry {
        override val key get() = item.key
    }

    /** A folder; `selected` when the scope shown is inside it while it is closed, `unread` when a closed folder has something new. */
    data class Folder(val id: String, val name: String, val open: Boolean, val selected: Boolean, val unread: Boolean, val servers: List<ScopeItem>) : RailEntry {
        override val key get() = "folder:$id"
    }
}

/** The scopes in order with the account's folders (`rows`) put in: a folder stands at its first server, and its servers show only while it is `open`. Servers the rows do not name stay plain. */
fun railEntries(scopes: List<ScopeItem>, rows: List<RailRowItem>, open: Set<String>, selected: String?): List<RailEntry> {
    val folderOf = HashMap<String, RailRowItem>()
    for (row in rows) if (row.folderId != null) for (space in row.spaces) folderOf["space:$space"] = row
    val out = ArrayList<RailEntry>()
    val done = HashSet<String>()
    for (scope in scopes) {
        val folder = if (scope.kind == ScopeKind.SERVER) folderOf[scope.key] else null
        if (folder == null) { out += RailEntry.Scope(scope, false); continue }
        val id = folder.folderId ?: continue
        if (!done.add(id)) continue
        val members = scopes.filter { folderOf[it.key]?.folderId == id }
        val isOpen = id in open
        out += RailEntry.Folder(id, folder.name, isOpen, !isOpen && members.any { it.key == selected }, !isOpen && members.any { it.hasUnread }, members)
        if (isOpen) members.forEach { out += RailEntry.Scope(it, true) }
    }
    return out
}

/** A folder on the rail (a long press opens its menu: rename, ungroup): a rounded square with a folder glyph, the accent when the scope shown is in it, a dot when it holds news; a tap opens or closes it. */
@androidx.compose.runtime.Composable
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
internal fun RailFolderBadge(folder: RailEntry.Folder, manage: () -> Unit, toggle: () -> Unit) {
    val shape = androidx.compose.foundation.shape.RoundedCornerShape(16.dp)
    androidx.compose.foundation.layout.Box(Modifier.size(Size.rail, Size.railItem), contentAlignment = Alignment.Center) {
        androidx.compose.foundation.layout.Box(
            Modifier.size(48.dp).clip(shape).background(if (folder.selected) Palette.Accent else Palette.Raised)
                .combinedClickable(onClick = toggle, onLongClick = manage)
                .semantics { contentDescription = folder.name }
                .testTag("rail-folder-${folder.name}"),
            contentAlignment = Alignment.Center,
        ) {
            GlyphIcon(
                if (folder.open) dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_up else dev.morpheus.app.R.drawable.ic_ms_folder,
                tint = if (folder.selected) onColour(Palette.Accent) else Palette.Text,
            )
        }
        if (folder.unread) {
            androidx.compose.foundation.layout.Box(
                Modifier.align(Alignment.CenterEnd).width(4.dp).height(8.dp)
                    .clip(androidx.compose.foundation.shape.RoundedCornerShape(topStart = 4.dp, bottomStart = 4.dp)).background(androidx.compose.ui.graphics.Color.White)
                    .testTag("dot-folder-${folder.name}"),
            )
        }
    }
}
