package dev.morpheus.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
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
    Column(Modifier.fillMaxSize().background(Palette.Surface).testTag("reorder-screen")) {
        TopBar(t("Servers"), backTag = "reorder-back", onBack = { app.closeReorder() })
        LazyColumn(Modifier.fillMaxSize()) {
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
                        Text("📁 ${row.name}", Modifier.weight(1f).testTag("folder-${row.name}"), fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        TextButton({ renaming = row }, Modifier.testTag("folder-rename-${row.name}")) { Text(t("Rename")) }
                        TextButton({ move(at, -1) }, Modifier.testTag("folder-up-${row.name}"), enabled = at > 0) { Text("▲") }
                        TextButton({ move(at, 1) }, Modifier.testTag("folder-down-${row.name}"), enabled = at < rows.size - 1) { Text("▼") }
                    }
                    if (renaming?.key == row.key) {
                        // In the tree, not a dialog window (the tests reach only the tree).
                        var name by remember(row.key) { mutableStateOf(row.name) }
                        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                            OutlinedTextField(name, { name = it }, Modifier.weight(1f).testTag("folder-name-input"), singleLine = true)
                            TextButton({ app.renameFolder(row.folderId.orEmpty(), name); renaming = null }, Modifier.testTag("folder-name-save"), enabled = name.isNotBlank()) { Text(t("Save")) }
                            TextButton({ renaming = null }) { Text(t("Cancel")) }
                        }
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
        Text(server.title, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (!inFolder) {
            TextButton({ move(-1) }, Modifier.testTag("server-up-${server.title}"), enabled = canUp) { Text("▲") }
            TextButton({ move(1) }, Modifier.testTag("server-down-${server.title}"), enabled = canDown) { Text("▼") }
        } else {
            TextButton({ app.moveOutOfFolder(id) }, Modifier.testTag("server-out-${server.title}")) { Text(t("Take out")) }
        }
        TextButton({ menu = true }, Modifier.testTag("server-folder-${server.title}")) { Text("📁") }
    }
    // In the tree, not a popup window (the tests reach only the tree).
    if (menu) {
        Column(Modifier.padding(start = indent + 48.dp).background(Palette.Raised)) {
            TextButton({ menu = false; app.moveToFolder(id, null) }, Modifier.testTag("folder-new")) { Text(t("New folder")) }
            for (folder in folders) {
                TextButton({ menu = false; app.moveToFolder(id, folder.folderId) }, Modifier.testTag("folder-into-${folder.name}")) { Text(folder.name) }
            }
        }
    }
    }
}
