package dev.morpheus.app.ui

import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import dev.morpheus.app.AppModel
import dev.morpheus.app.openLink

/** The phone as the diagnostics name it. */
private val osName = "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})"
private val deviceName = "${Build.MANUFACTURER} ${Build.MODEL}"

/** Settings > Help & Feedback (#445): the version, what is new in it, a bug report with the diagnostics filled in, and the diagnostics to copy. */
@Composable
fun HelpPage(app: AppModel, t: (String) -> String) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    val info = remember { uniffi.morpheus_ffi.helpInfo() }
    val update by app.update.collectAsState()
    var diagnostics by remember { mutableStateOf("") }
    LaunchedEffect(Unit) { diagnostics = try { app.morpheus.diagnosticsText(osName, deviceName) } catch (_: Exception) { "" } }
    fun open(url: String) { context.openLink(url)?.let { Toasts.say(it, "settings-error", long = true) } }

    SmallHeading(t("Help & Feedback"), Modifier.padding(horizontal = Space.lg))
    Text(info.versionLine, Modifier.padding(horizontal = Space.lg).testTag("help-version"), style = MaterialTheme.typography.titleMedium)
    update?.let { offer ->
        ListRow(offer.headline, "update-notice", supporting = offer.detail, leading = { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_refresh) }) { open(offer.link) }
    }
    ListRow(t("What's new"), "help-whats-new", supporting = t("The patch notes of this version"), leading = { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_link) }) { open(info.releasePage) }
    ListRow(t("Report a bug"), "help-report-bug", supporting = t("Opens a report with the diagnostics below filled in"), leading = { GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_bug_report) }) {
        app.reportBug(osName, deviceName) { open(it) }
    }
    SmallHeading(t("Diagnostics"), Modifier.padding(horizontal = Space.lg))
    Column(Modifier.padding(horizontal = Space.lg).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
        Text(diagnostics, Modifier.testTag("help-diagnostics"), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        OutlinedButton({ clipboard.setText(AnnotatedString(diagnostics)) }, Modifier.testTag("help-diag-copy")) { Text(t("Copy all")) }
    }
}
