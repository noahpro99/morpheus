package dev.morpheus.mobile

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import java.io.File

/**
 * The system's file picker for the app (an avatar, an attachment): an
 * invisible activity that opens the picker, copies what was picked into the
 * app's own files (the Rust side reads plain paths) and leaves the path in
 * `picked` (empty when cancelled) for `apps/mobile/src/main.rs` to read.
 */
class PickFileActivity : Activity() {
    private fun dataDir() = File(filesDir, "morpheus").apply { mkdirs() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return
        val pick = Intent(Intent.ACTION_GET_CONTENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = if (intent?.getBooleanExtra("images", false) == true) "image/*" else "*/*"
        }
        try {
            startActivityForResult(pick, PICK)
        } catch (_: Exception) {
            done("")
        }
    }

    @Deprecated("The platform's own result API is enough for one picker.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val uri = data?.data
        if (requestCode != PICK || resultCode != RESULT_OK || uri == null) {
            done("")
            return
        }
        done(copy(uri)?.absolutePath ?: "")
    }

    /** The picked document, copied under `picked-files/` by its own name. */
    private fun copy(uri: Uri): File? {
        return try {
            val name = contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            }?.replace('/', '_') ?: "picked"
            val dir = File(dataDir(), "picked-files").apply { mkdirs() }
            val out = File(dir, name)
            val copied = contentResolver.openInputStream(uri)?.use { input ->
                out.outputStream().use { input.copyTo(it) }
            }
            if (copied == null) null else out
        } catch (_: Exception) {
            null
        }
    }

    private fun done(path: String) {
        File(dataDir(), "picked").writeText(path)
        finish()
    }

    companion object {
        const val PICK = 1

        /** Called from Rust (`main.rs`): show the picker. */
        @JvmStatic
        fun start(context: Context, images: Boolean) {
            File(File(context.filesDir, "morpheus").apply { mkdirs() }, "picked").delete()
            context.startActivity(
                Intent(context, PickFileActivity::class.java)
                    .putExtra("images", images)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
        }
    }
}
