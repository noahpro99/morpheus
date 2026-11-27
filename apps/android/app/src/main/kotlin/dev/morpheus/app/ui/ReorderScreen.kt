package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.morpheus.app.AppModel
import uniffi.morpheus_ffi.RailRowItem
import uniffi.morpheus_ffi.ScopeItem
import uniffi.morpheus_ffi.ScopeKind

/**
 * The servers as the account keeps them (every device's rail): in order, each with Up and Down, and
 * in folders, which are made here, renamed, and servers put in them or taken out.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
fun ReorderScreen(app: AppModel) {
    val sidebar by app.sidebar.collectAsState()
    val kept by app.railRows.collectAsState()
    val servers = sidebar?.scopes.orEmpty().filter { it.kind == ScopeKind.SERVER }
    fun server(id: String): ScopeItem? = servers.firstOrNull { it.key == "space:$id" || it.key == id }
    // Top-level entries as the account lists them (those of servers that are gone left out), then the servers it does not list yet.
    val listed = kept.filter { row -> row.folderId != null || server(row.spaces.first()) != null }
    val inRail = kept.flatMap { it.spaces }.toSet()
    val unlisted = servers.map { it.key.removePrefix("space:") }.filter { it !in inRail }.map { RailRowItem(it, null, "", listOf(it)) }
    val rows = listed + unlisted
    val folders = rows.filter { it.folderId != null }
    var renaming by remember { mutableStateOf<RailRowItem?>(null) }
    BackHandler { app.closeReorder() }
    fun move(at: Int, by: Int) {
        if (at + by !in rows.indices) return
        val next = rows.map { it.key }.toMutableList()
        next.add(at + by, next.removeAt(at))
        app.setRailOrder(next)
    }
    ScreenScaffold("reorder-screen", { TopBar(t("Servers")) }) { inner ->
        LazyColumn(Modifier.fillMaxSize().padding(inner)) {
            items(rows, key = { it.key }) { row ->
                val at = rows.indexOf(row)
                if (row.folderId == null) {
                    val item = server(row.spaces.first()) ?: return@items
                    ServerLine(app, item, row.spaces.first(), folders, inFolder = false, 12.dp, canUp = at > 0, canDown = at < rows.size - 1) { move(at, it) }
                    return@items
                }
                Column {
                    Row(
                        Modifier.fillMaxWidth().background(Palette.Raised).padding(horizontal = 12.dp, vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Row(
                            Modifier.weight(1f).testTag("folder-${row.name}").combinedClickable(onClick = {}, onLongClick = { app.openSheet(row.name) { app.morpheus.folderMenuRows(row.folderId.orEmpty(), row.name) } }),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(Space.sm),
                        ) {
                            GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_folder, size = 20.dp)
                            Text(row.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        TextButton({ renaming = row }, Modifier.testTag("folder-rename-${row.name}")) { Text(t("Rename")) }
                        TextButton({ move(at, -1) }, Modifier.testTag("folder-up-${row.name}"), enabled = at > 0) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_up, size = 20.dp) }
                        TextButton({ move(at, 1) }, Modifier.testTag("folder-down-${row.name}"), enabled = at < rows.size - 1) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_down, size = 20.dp) }
                    }
                    if (renaming?.key == row.key) {
                        // The name asked for in a dialog.
                        var name by remember(row.key) { mutableStateOf(row.name) }
                        ConfirmDialog(
                            title = t("Rename"), confirm = t("Save"), confirmTag = "folder-name-save", confirmEnabled = name.isNotBlank(),
                            onConfirm = { app.renameFolder(row.folderId.orEmpty(), name); renaming = null },
                            onDismiss = { renaming = null },
                            content = { OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("folder-name-input"), singleLine = true) },
                        )
                    }
                    for (id in row.spaces) {
                        val item = server(id) ?: continue
                        ServerLine(app, item, id, folders, inFolder = true, 32.dp, canUp = false, canDown = false) { }
                    }
                }
            }
        }
    }
}

@Composable
private fun ServerLine(app: AppModel, server: ScopeItem, id: String, folders: List<RailRowItem>, inFolder: Boolean, indent: androidx.compose.ui.unit.Dp, canUp: Boolean, canDown: Boolean, move: (Int) -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Column {
    Row(
        Modifier.fillMaxWidth().padding(start = indent, end = 12.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Avatar(server.avatar, server.title, 40.dp)
        Text(server.title, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
        if (!inFolder) {
            TextButton({ move(-1) }, Modifier.testTag("server-up-${server.title}"), enabled = canUp) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_up, size = 20.dp) }
            TextButton({ move(1) }, Modifier.testTag("server-down-${server.title}"), enabled = canDown) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_keyboard_arrow_down, size = 20.dp) }
        } else {
            TextButton({ app.moveOutOfFolder(id) }, Modifier.testTag("server-out-${server.title}")) { Text(t("Take out")) }
        }
        Box {
            TextButton({ menu = true }, Modifier.testTag("server-folder-${server.title}")) { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_create_new_folder, size = 20.dp) }
            // Where the server goes: a dropdown menu.
            androidx.compose.material3.DropdownMenu(menu, { menu = false }) {
                androidx.compose.material3.DropdownMenuItem({ Text(t("New folder")) }, { menu = false; app.moveToFolder(id, null) }, Modifier.testTag("folder-new"))
                for (folder in folders) {
                    androidx.compose.material3.DropdownMenuItem({ Text(folder.name) }, { menu = false; app.moveToFolder(id, folder.folderId) }, Modifier.testTag("folder-into-${folder.name}"))
                }
            }
        }
    }
    }
}
