package dev.morpheus.mobile

import android.Manifest
import android.app.Activity
import android.app.Application
import android.app.NotificationManager
import android.content.ContentProvider
import android.content.ContentValues
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.os.Bundle
import org.unifiedpush.android.connector.UnifiedPush

/**
 * Runs at process start (Android creates content providers before any
 * activity), so push setup needs no hook in the generated activity: the first
 * time an activity shows, ask for the notification permission (Android 13+)
 * and register with the phone's UnifiedPush app (Sunup, ntfy, …).
 */
class PushStartup : ContentProvider() {
    override fun onCreate(): Boolean {
        val app = context?.applicationContext as? Application ?: return true
        app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            private var done = false

            override fun onActivityResumed(activity: Activity) {
                // The app itself, not the call screen or a pass-through.
                if (!isMain(activity)) return
                visible = true
                CallRinger.appOnScreen(activity)
                if (done) return
                done = true
                askForNotifications(activity)
                UnifiedPush.tryUseCurrentOrDefaultDistributor(activity) { found ->
                    if (found) UnifiedPush.register(activity, messageForDistributor = "Morpheus")
                }
            }

            override fun onActivityCreated(activity: Activity, state: Bundle?) {
                appStarted = true
            }
            override fun onActivityStarted(activity: Activity) {}
            override fun onActivityPaused(activity: Activity) {
                if (!isMain(activity)) return
                visible = false
                CallRinger.appLeftScreen(activity)
            }
            override fun onActivityStopped(activity: Activity) {}
            override fun onActivitySaveInstanceState(activity: Activity, state: Bundle) {}
            override fun onActivityDestroyed(activity: Activity) {}
        })
        return true
    }

    /** The app's own screen (Dioxus's activity), not one of ours. */
    private fun isMain(activity: Activity) = activity.javaClass.name == "dev.dioxus.main.MainActivity"

    private fun askForNotifications(activity: Activity) {
        if (Build.VERSION.SDK_INT >= 33 &&
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7301)
        }
    }

    companion object {
        /**
         * Whether a Morpheus screen is showing. Process priority is no guide:
         * delivering a push raises the app to the foreground briefly.
         */
        @Volatile
        var visible = false

        /**
         * Whether the app (and so its Rust side, syncing) started in this
         * process. A push can start the process on its own, with only the
         * push service running.
         */
        @Volatile
        var appStarted = false
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, args: Array<out String>?, sort: String?): Cursor? = null
    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, args: Array<out String>?): Int = 0
    override fun update(uri: Uri, values: ContentValues?, selection: String?, args: Array<out String>?): Int = 0
}
