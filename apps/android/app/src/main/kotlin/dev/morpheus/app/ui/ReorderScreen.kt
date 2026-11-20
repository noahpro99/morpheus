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
import uniffi.morpheus_ffi.ScopeKind

/** The servers in the order the account keeps (every device's rail), each with Up and Down. */
@Composable
fun ReorderScreen(app: AppModel) {
    val sidebar by app.sidebar.collectAsState()
    val servers = sidebar?.scopes.orEmpty().filter { it.kind == ScopeKind.SERVER }
    // What the person just did shows at once; the list catches up when the account has it.
    var order by remember { mutableStateOf<List<String>?>(null) }
    val keys = order?.filter { key -> servers.any { it.key == key } }?.plus(servers.map { it.key }.filter { it !in order.orEmpty() }) ?: servers.map { it.key }
    val shown = keys.mapNotNull { key -> servers.firstOrNull { it.key == key } }
    BackHandler { app.closeReorder() }
    Column(Modifier.fillMaxSize().background(Palette.Surface).testTag("reorder-screen")) {
        Row(Modifier.fillMaxWidth().background(Palette.Background).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton({ app.closeReorder() }, Modifier.testTag("reorder-back")) { Text("‹", fontSize = 28.sp) }
            Text(t("Servers"), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        }
        LazyColumn(Modifier.fillMaxSize()) {
            items(shown, key = { it.key }) { server ->
                val at = shown.indexOf(server)
                fun move(by: Int) {
                    val next = shown.map { it.key }.toMutableList()
                    next.add(at + by, next.removeAt(at))
                    order = next
                    app.setRailOrder(next.map { it.removePrefix("space:") })
                }
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Avatar(server.avatar, server.title, 40.dp)
                    Text(server.title, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    TextButton({ move(-1) }, Modifier.testTag("server-up-${server.title}"), enabled = at > 0) { Text("▲") }
                    TextButton({ move(1) }, Modifier.testTag("server-down-${server.title}"), enabled = at < shown.size - 1) { Text("▼") }
                }
            }
        }
    }
}
