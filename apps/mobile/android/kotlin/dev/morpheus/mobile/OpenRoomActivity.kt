package dev.morpheus.mobile

import android.app.Activity
import android.os.Bundle
import java.io.File

/**
 * Where a tapped notification lands: leaves the room to open in a file the
 * app picks up (`apps/mobile/src/push.rs`), brings the app forward and
 * closes. A trampoline because the generated activity cannot be taught to
 * read extras from a new intent.
 */
class OpenRoomActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        intent?.getStringExtra("room_id")?.takeIf { it.isNotEmpty() }?.let { room ->
            File(File(filesDir, "morpheus").apply { mkdirs() }, "open_room").writeText(room)
        }
        packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
            launch.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK or android.content.Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            startActivity(launch)
        }
        finish()
    }
}
