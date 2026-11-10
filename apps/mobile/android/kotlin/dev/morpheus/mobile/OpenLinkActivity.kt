package dev.morpheus.mobile

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import java.io.File

/**
 * Where a Matrix link opened elsewhere lands (`matrix:` links, and matrix.to
 * links when Morpheus is picked for them): leaves the link in a file the app
 * picks up (`apps/mobile/src/push.rs`), brings the app forward and closes,
 * as OpenRoomActivity does for notifications.
 */
class OpenLinkActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        intent?.dataString?.takeIf { it.isNotEmpty() }?.let { link ->
            File(File(filesDir, "morpheus").apply { mkdirs() }, "open_link").writeText(link)
        }
        packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            startActivity(launch)
        }
        finish()
    }
}
