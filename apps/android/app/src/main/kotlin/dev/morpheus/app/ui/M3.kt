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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.unit.dp
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

    /**
     * Say `text` with an Undo button for a few seconds (#415). The caller has already made the thing look done: `onUndo` takes it back,
     * `onCommit` runs when the moment has passed (or another toast took the place), and the thing is done for good.
     */
    fun undoable(text: String, onUndo: () -> Unit, onCommit: () -> Unit, tag: String = "toast") {
        this.tag = tag
        scope.launch {
            host.currentSnackbarData?.dismiss()
            val seconds = uniffi.morpheus_ffi.undoSeconds().toLong()
            val result = kotlinx.coroutines.withTimeoutOrNull(seconds * 1000) {
                host.showSnackbar(text, actionLabel = t("Undo"), duration = SnackbarDuration.Indefinite)
            }
            if (result == androidx.compose.material3.SnackbarResult.ActionPerformed) onUndo() else onCommit()
        }
    }
}

@Composable
fun ToastHost(modifier: Modifier = Modifier) {
    SnackbarHost(Toasts.host, modifier) { data ->
        val action = data.visuals.actionLabel
        if (action == null) {
            Snackbar(data, Modifier.semantics(mergeDescendants = true) {}.testTag(Toasts.tag), shape = MaterialTheme.shapes.small)
        } else {
            // With an action the parts stay apart: the words, and a button a test (or a screen reader) can find.
            Snackbar(
                Modifier.testTag(Toasts.tag),
                shape = MaterialTheme.shapes.small,
                action = { TextButton({ data.performAction() }, Modifier.testTag("toast-action")) { Text(action) } },
            ) { Text(data.visuals.message, Modifier.testTag("toast-text")) }
        }
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
                    if (text != null) Text(text, if (textTag != null) Modifier.testTag(textTag) else Modifier, style = MaterialTheme.typography.bodyLarge)
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


/**
 * A Material 3 search bar: the docked bar (a pill 56 dp tall in the surface container colour) with a search icon, a clear button
 * while there is text, and a hint, its colours read against the bar's fill (#266). Used wherever the phone searches.
 */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
fun SearchField(query: String, onQuery: (String) -> Unit, hint: String, tag: String, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val fill = ComposerColours.fill(scheme)
    val ink = ComposerColours.text(scheme)
    val muted = ComposerColours.hint(scheme)
    val colours = androidx.compose.material3.SearchBarDefaults.colors(
        containerColor = fill,
        inputFieldColors = androidx.compose.material3.TextFieldDefaults.colors(
            focusedTextColor = ink, unfocusedTextColor = ink, cursorColor = scheme.primary,
            focusedPlaceholderColor = muted, unfocusedPlaceholderColor = muted,
            focusedLeadingIconColor = muted, unfocusedLeadingIconColor = muted, focusedTrailingIconColor = muted, unfocusedTrailingIconColor = muted,
            focusedContainerColor = Color.Transparent, unfocusedContainerColor = Color.Transparent, disabledContainerColor = Color.Transparent,
        ),
    )
    androidx.compose.material3.DockedSearchBar(
        inputField = {
            androidx.compose.material3.SearchBarDefaults.InputField(
                query = query, onQueryChange = onQuery, onSearch = {}, expanded = false, onExpandedChange = {},
                modifier = Modifier.testTag(tag),
                placeholder = { Text(hint) },
                leadingIcon = { GlyphIcon(dev.morpheus.app.R.drawable.ic_lucide_search, tint = muted) },
                trailingIcon = if (query.isEmpty()) null else ({
                    IconAction(dev.morpheus.app.R.drawable.ic_lucide_x, t("Clear"), "$tag-clear", { onQuery("") }, tint = muted)
                }),
                colors = colours.inputFieldColors,
            )
        },
        expanded = false, onExpandedChange = {},
        modifier = modifier.fillMaxWidth(),
        shape = CircleShape,
        colors = colours,
        tonalElevation = 0.dp, shadowElevation = 0.dp,
    ) {}
}

/**
 * Material 3's motion tokens (m3.material.io, Motion > Easing and duration): the easing curves and the durations every transition uses (#464, #465).
 * The emphasized curve is two segments in the spec; the usual single cubic stands in for it, as Material's own libraries do.
 */
object Motion {
    val Standard = androidx.compose.animation.core.CubicBezierEasing(0.2f, 0f, 0f, 1f)
    val StandardDecelerate = androidx.compose.animation.core.CubicBezierEasing(0f, 0f, 0f, 1f)
    val StandardAccelerate = androidx.compose.animation.core.CubicBezierEasing(0.3f, 0f, 1f, 1f)
    val Emphasized = androidx.compose.animation.core.CubicBezierEasing(0.2f, 0f, 0f, 1f)
    val EmphasizedDecelerate = androidx.compose.animation.core.CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f)
    val EmphasizedAccelerate = androidx.compose.animation.core.CubicBezierEasing(0.3f, 0f, 0.8f, 0.15f)

    const val Short1 = 50
    const val Short2 = 100
    const val Short3 = 150
    const val Short4 = 200
    const val Medium1 = 250
    const val Medium2 = 300
    const val Medium3 = 350
    const val Medium4 = 400
    const val Long1 = 450
    const val Long2 = 500

    /** The shared axis (X) transition: a pane slides in or out over medium2 on the emphasized curve (the lists, the chat). */
    fun <T> slide() = androidx.compose.animation.core.tween<T>(Medium2, easing = Emphasized)
}
