package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import dev.morpheus.app.AppModel
import dev.morpheus.app.AppState
import dev.morpheus.app.openLink
import dev.morpheus.app.sentence
import kotlinx.coroutines.launch

/** Settings > My Account: who is signed in, the display name and picture to change, and the link to the account's own page when the homeserver has one (#451, #453). */
@Composable
fun AccountPage(app: AppModel, t: (String) -> String, say: (String, String) -> Unit) {
    val own by app.ownProfile.collectAsState()
    val ownKey by app.ownAvatarKey.collectAsState()
    val account = app.morpheus.account()
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var name by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var manage by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        app.refreshOwnProfile()
        manage = AppState.manageUrl ?: try { app.morpheus.ownManageUrl() } catch (_: Exception) { null }
    }
    // The box starts with the name the account has; what is typed is kept when the profile is read again.
    LaunchedEffect(own?.name) { if (name.isEmpty()) name = own?.name.orEmpty() }

    fun run(done: String, call: suspend () -> Unit) {
        if (busy) return
        busy = true
        scope.launch {
            try { call(); app.refreshOwnProfile(); say(done, "settings-note") } catch (e: Exception) { say(e.sentence(), "settings-error") }
            busy = false
        }
    }
    val picker = rememberPicturePicker { square -> run(t("Picture updated.")) { app.morpheus.setOwnPicture("picture.png", square) } }

    SmallHeading(t("Account"), Modifier.padding(horizontal = Space.lg))
    Text(account?.userId.orEmpty(), Modifier.padding(horizontal = Space.lg).testTag("settings-account"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
    account?.deviceId?.takeIf { it.isNotBlank() }?.let {
        Text(t("This device: %{id}").replace("%{id}", it), Modifier.padding(horizontal = Space.lg).testTag("settings-device"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
    }

    SmallHeading(t("Edit profile"), Modifier.padding(horizontal = Space.lg))
    Row(Modifier.padding(horizontal = Space.lg), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.md)) {
        Avatar(own?.avatar, own?.name.orEmpty(), Size.avatarLg, key = ownKey)
        Column(verticalArrangement = Arrangement.spacedBy(Space.xs)) {
            OutlinedButton({ picker.pick() }, Modifier.testTag("own-picture")) { Text(t("Choose a picture…")) }
            if (own?.avatar != null || ownKey != null) TextButton({ run(t("Picture removed.")) { app.morpheus.removeOwnPicture() } }, Modifier.testTag("own-picture-remove"), enabled = !busy) { Text(t("Remove picture")) }
        }
    }
    Column(Modifier.padding(horizontal = Space.lg)) { picker.Show() }
    OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth().padding(horizontal = Space.lg).testTag("own-name"), singleLine = true, label = { Text(t("Display name")) })
    Button({ run(t("Saved.")) { app.morpheus.setOwnName(name) } }, Modifier.padding(horizontal = Space.lg).testTag("own-name-save"), enabled = !busy) { Text(t("Save")) }

    manage?.let { url ->
        OutlinedButton({ context.openLink(url)?.let { say(it, "settings-error") } }, Modifier.padding(horizontal = Space.lg).testTag("manage-account")) { Text(t("Manage account")) }
    }
    Button({ app.signOut() }, Modifier.padding(horizontal = Space.lg).testTag("settings-sign-out")) { Text(t("Sign out")) }
}
