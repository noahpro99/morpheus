package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.items
import androidx.compose.ui.unit.dp
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import dev.morpheus.app.AppModel
import dev.morpheus.app.PageRequest
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.ModRow

/** The mod view (#456): a person across the rooms you share, where they stand in each, and the moderation you may do there (a role, kick, ban, unban, with a reason). `page.roomId` is the person. */
@Composable
fun ModViewPage(app: AppModel, page: PageRequest) {
    val userId = page.roomId
    var rows by remember { mutableStateOf<List<ModRow>?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var reason by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var ok by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(reload) { try { rows = app.morpheus.modViewRows(userId) } catch (e: Exception) { error = e.sentence() } }
    fun act(done: String, call: suspend () -> Unit) {
        if (busy) return
        busy = true
        scope.launch {
            try { call(); ok = done; error = null; reload++ } catch (e: Exception) { error = e.sentence() }
            busy = false
        }
    }
    ScreenScaffold("page-mod-view", { TopBar(t("Mod view")) }) { inner ->
    val modList = androidx.compose.foundation.lazy.rememberLazyListState()
    androidx.compose.foundation.lazy.LazyColumn(Modifier.stretch(modList).fillMaxSize().padding(inner).padding(Space.lg).testTag("mod-list"), state = modList, verticalArrangement = Arrangement.spacedBy(Space.md)) {
        item {
        Text(userId, Modifier.testTag("mod-user"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(reason, { reason = it }, Modifier.fillMaxWidth().testTag("mod-reason"), singleLine = true, label = { Text(t("Reason (optional)")) })
        Note(ok, error)
        }
        val list = rows
        if (list == null) item { Skeleton(Modifier.fillMaxWidth().height(48.dp)) }
        else if (list.isEmpty()) item { Text(t("You share no rooms with them."), Modifier.testTag("mod-none"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium) }
        else items(list, key = { it.roomId }) { row -> Column(Modifier.fillMaxWidth().testTag("mod-row-${row.name}"), verticalArrangement = Arrangement.spacedBy(Space.xs)) {
            Text(row.name, style = MaterialTheme.typography.titleMedium)
            Text(row.state, Modifier.testTag("mod-state-${row.name}"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
            if (row.roles.isNotEmpty()) Row(horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
                for (role in row.roles) {
                    if (role.current) Button({}, Modifier.testTag("mod-role-${row.name}-${role.key}")) { Text(role.label) }
                    else OutlinedButton({ act(t("Role changed.")) { app.morpheus.modSetRole(row.roomId, userId, role.key) } }, Modifier.testTag("mod-role-${row.name}-${role.key}"), enabled = !busy) { Text(role.label) }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(Space.xs)) {
                if (row.kick) TextButton({ act(t("Removed.")) { app.morpheus.modKick(row.roomId, userId, reason) } }, Modifier.testTag("mod-kick-${row.name}"), enabled = !busy) { Text(t("Kick")) }
                if (row.ban) TextButton({ act(t("Banned.")) { app.morpheus.modBan(row.roomId, userId, reason) } }, Modifier.testTag("mod-ban-${row.name}"), enabled = !busy) { Text(t("Ban")) }
                if (row.unban) TextButton({ act(t("Unbanned.")) { app.morpheus.modUnban(row.roomId, userId) } }, Modifier.testTag("mod-unban-${row.name}"), enabled = !busy) { Text(t("Unban")) }
            }
        } }
    }
    }
}
