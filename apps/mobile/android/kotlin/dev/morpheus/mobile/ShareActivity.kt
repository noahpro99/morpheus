package dev.morpheus.mobile

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * Morpheus in the share sheet: what another app shares (text, links,
 * pictures, files) is copied into the app's storage and left in `share.json`
 * for the app (`apps/mobile/src/push.rs`), which asks where to send it. The
 * app comes forward and this closes, as OpenRoomActivity does.
 */
class ShareActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val share = intent
        if (share != null) {
            val text = share.getStringExtra(Intent.EXTRA_TEXT)
            val files = JSONArray()
            for (uri in streams(share)) {
                copy(uri)?.let { files.put(it.absolutePath) }
            }
            if (text != null || files.length() > 0) {
                val note = JSONObject().put("files", files)
                if (text != null) note.put("text", text)
                File(dataDir(), "share.json").writeText(note.toString())
            }
        }
        packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            startActivity(launch)
        }
        finish()
    }

    private fun dataDir() = File(filesDir, "morpheus").apply { mkdirs() }

    @Suppress("DEPRECATION")
    private fun streams(share: Intent): List<Uri> = when (share.action) {
        Intent.ACTION_SEND -> listOfNotNull(
            if (Build.VERSION.SDK_INT >= 33) share.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            else share.getParcelableExtra(Intent.EXTRA_STREAM)
        )
        Intent.ACTION_SEND_MULTIPLE -> (
            if (Build.VERSION.SDK_INT >= 33) share.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            else share.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
        ) ?: emptyList()
        else -> emptyList()
    }

    /** The shared file, copied under its own name into shared-files/. */
    private fun copy(uri: Uri): File? = try {
        val name = contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
        } ?: uri.lastPathSegment ?: "shared"
        val dir = File(dataDir(), "shared-files").apply { mkdirs() }
        val out = File(dir, name.replace('/', '_'))
        contentResolver.openInputStream(uri)?.use { input ->
            out.outputStream().use { input.copyTo(it) }
        }
        out
    } catch (e: Exception) {
        null
    }
}
