package dev.morpheus.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

// The Material 3 pieces every screen is built from (#404): list rows, the three selection controls, the toast, the confirm dialog.
// Their colours are the MaterialTheme's, which is the app's own theme (or Material You) with every on-colour picked by contrast.

/** The colours of a row on the screen's surface: nothing behind it (or the raised surface while it is the one open), text and icons that read. */
@Composable
private fun rowColours(danger: Boolean, selected: Boolean, quiet: Boolean = false) = ListItemDefaults.colors(
    containerColor = if (selected) MaterialTheme.colorScheme.surfaceContainerHigh else Color.Transparent,
    headlineColor = if (danger) MaterialTheme.colorScheme.error else if (quiet) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
    leadingIconColor = if (danger) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
    trailingIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
    supportingColor = MaterialTheme.colorScheme.onSurfaceVariant,
    overlineColor = MaterialTheme.colorScheme.onSurfaceVariant,
)

/**
 * A list row: a Material 3 ListItem (headline, supporting line, leading and trailing slots) that takes a tap and a long press
 * with a ripple. `tag` is the test tag of the whole row.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ListRow(
    headline: String,
    tag: String,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
    danger: Boolean = false,
    selected: Boolean = false,
    enabled: Boolean = true,
    /** Bold: a row with news. */
    strong: Boolean = false,
    /** The secondary text colour: a row that is read already. */
    quiet: Boolean = false,
    onLongClick: (() -> Unit)? = null,
    onClick: (() -> Unit)? = null,
) {
    ListItem(
        headlineContent = { Text(headline, maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = if (strong) androidx.compose.ui.text.font.FontWeight.Bold else null) },
        modifier = modifier
            .fillMaxWidth()
            .then(
                if (onClick != null || onLongClick != null) {
                    Modifier.combinedClickable(enabled = enabled, role = Role.Button, onLongClick = onLongClick, onClick = onClick ?: {})
                } else Modifier,
            )
            .testTag(tag),
        supportingContent = supporting?.let { { Text(it, maxLines = 2, overflow = TextOverflow.Ellipsis) } },
        leadingContent = leading,
        trailingContent = trailing,
        colors = rowColours(danger, selected, quiet),
    )
}

/** One of several choices (a language, a mode): a radio button on a row that takes the tap. */
@Composable
fun ChoiceRow(label: String, chosen: Boolean, tag: String, modifier: Modifier = Modifier, supporting: String? = null, onPick: () -> Unit) {
    ListItem(
        headlineContent = { Text(label) },
        modifier = modifier.fillMaxWidth().selectable(selected = chosen, role = Role.RadioButton, onClick = onPick).testTag(tag),
        supportingContent = supporting?.let { { Text(it) } },
        leadingContent = { RadioButton(selected = chosen, onClick = null) },
        colors = rowColours(false, false),
    )
}

/** A setting that is on or off: a switch on a row that takes the tap. */
@Composable
fun ToggleRow(label: String, checked: Boolean, tag: String, modifier: Modifier = Modifier, supporting: String? = null, onToggle: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(label) },
        modifier = modifier.fillMaxWidth().toggleable(value = checked, role = Role.Switch, onValueChange = onToggle).testTag(tag),
        supportingContent = supporting?.let { { Text(it) } },
        trailingContent = { Switch(checked = checked, onCheckedChange = null, colors = SwitchDefaults.colors()) },
        colors = rowColours(false, false),
    )
}

/** An item picked among others, any number of them: a checkbox on a row that takes the tap. */
@Composable
fun CheckRow(label: String, checked: Boolean, tag: String, modifier: Modifier = Modifier, supporting: String? = null, leading: (@Composable () -> Unit)? = null, onToggle: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        modifier = modifier.fillMaxWidth().toggleable(value = checked, role = Role.Checkbox, onValueChange = onToggle).testTag(tag),
        supportingContent = supporting?.let { { Text(it) } },
        leadingContent = leading,
        trailingContent = { Checkbox(checked = checked, onCheckedChange = null) },
        colors = rowColours(false, false),
    )
}

/**
 * The app's toasts: a Material 3 Snackbar at the foot of the screen (Root's Scaffold hosts it). `tag` is the snackbar's test tag, so a
 * test (or a screen reader) finds what was said.
 */
object Toasts {
    val host = SnackbarHostState()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    @Volatile
    var tag: String = "toast"
        private set

    /** Say `text` (replacing what was on show). */
    fun say(text: String, tag: String = "toast", long: Boolean = false) {
        this.tag = tag
        scope.launch {
            host.currentSnackbarData?.dismiss()
            host.showSnackbar(text, duration = if (long) SnackbarDuration.Long else SnackbarDuration.Short)
        }
    }
}

@Composable
fun ToastHost(modifier: Modifier = Modifier) {
    SnackbarHost(Toasts.host, modifier) { data ->
        Snackbar(data, Modifier.semantics(mergeDescendants = true) {}.testTag(Toasts.tag), shape = MaterialTheme.shapes.small)
    }
}

/** A question with two answers: a Material 3 AlertDialog. The tags are the buttons' (and `dialog` the whole). */
@Composable
fun ConfirmDialog(
    title: String,
    text: String? = null,
    confirm: String,
    confirmTag: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
    dismiss: String = t("Cancel"),
    danger: Boolean = false,
    content: (@Composable () -> Unit)? = null,
    confirmEnabled: Boolean = true,
    textTag: String? = null,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        modifier = Modifier.testTag("dialog"),
        title = { Text(title) },
        text = if (text != null || content != null) {
            {
                androidx.compose.foundation.layout.Column {
                    if (text != null) Text(text, if (textTag != null) Modifier.testTag(textTag) else Modifier)
                    content?.invoke()
                }
            }
        } else null,
        confirmButton = {
            TextButton(
                onConfirm,
                Modifier.testTag(confirmTag),
                enabled = confirmEnabled,
                colors = androidx.compose.material3.ButtonDefaults.textButtonColors(contentColor = if (danger) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary),
            ) { Text(confirm) }
        },
        dismissButton = { TextButton(onDismiss, Modifier.testTag("dialog-cancel")) { Text(dismiss) } },
        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        titleContentColor = MaterialTheme.colorScheme.onSurface,
        textContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

/** A screen: a Material 3 Scaffold with its top bar, on the surface colour. `tag` is the screen's test tag. */
@Composable
fun ScreenScaffold(tag: String, topBar: @Composable () -> Unit, modifier: Modifier = Modifier, content: @Composable (androidx.compose.foundation.layout.PaddingValues) -> Unit) {
    androidx.compose.material3.Scaffold(
        modifier.fillMaxSize().testTag(tag),
        topBar = topBar,
        containerColor = MaterialTheme.colorScheme.surface,
        contentColor = MaterialTheme.colorScheme.onSurface,
        contentWindowInsets = androidx.compose.foundation.layout.WindowInsets(0),
        content = content,
    )
}

