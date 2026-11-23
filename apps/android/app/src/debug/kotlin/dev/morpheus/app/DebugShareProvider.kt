package dev.morpheus.app

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File

/**
 * Debug builds only: a picture another app could share, so the share e2e tests can send an ACTION_SEND with a content address
 * and the permission to read it from `adb` (the shell cannot grant a read of the gallery's). Any `content://dev.morpheus.mobile.debugshare/<name>.png`
 * is a small PNG of that name.
 */
class DebugShareProvider : ContentProvider() {
    override fun onCreate() = true

    override fun getType(uri: Uri) = "image/png"

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        val file = File(context!!.cacheDir, "debug-share-" + (uri.lastPathSegment ?: "x.png").replace('/', '_'))
        val bitmap = android.graphics.Bitmap.createBitmap(64, 48, android.graphics.Bitmap.Config.ARGB_8888).apply { eraseColor(android.graphics.Color.rgb(200, 60, 90)) }
        file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun query(uri: Uri, projection: Array<String>?, selection: String?, selectionArgs: Array<String>?, sortOrder: String?): Cursor =
        MatrixCursor(arrayOf(OpenableColumns.DISPLAY_NAME)).apply { addRow(arrayOf(uri.lastPathSegment ?: "image.png")) }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?) = 0
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<String>?) = 0
}
