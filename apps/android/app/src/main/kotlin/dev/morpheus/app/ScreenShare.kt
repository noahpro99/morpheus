package dev.morpheus.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.ImageReader
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import dev.morpheus.app.ui.t
import uniffi.morpheus_ffi.ScreenDriver
import uniffi.morpheus_ffi.callScreenFrame

/**
 * Sharing the phone's screen into a call (#432), with MediaProjection. The order the system asks for: the person is
 * asked (the system's own dialog, from the activity), a foreground service of type mediaProjection is started
 * ([ScreenShareService]), and only then is the projection taken from the permission's result. The Rust core then
 * starts the capture through [ScreenDriver]; each picture goes back to it ([callScreenFrame]). The system's chip, the
 * notification's Stop sharing and the call's own button all end it. The share has no sound of its own yet (Android
 * would need the playback-capture API, for apps that allow it).
 */
class ScreenShare(private val context: Context) : ScreenDriver {
    private var projection: MediaProjection? = null
    private var display: VirtualDisplay? = null
    private var reader: ImageReader? = null
    private var thread: HandlerThread? = null

    /** The projection to capture from, taken once the service is in the foreground. */
    @Synchronized
    fun take(resultCode: Int, data: Intent) {
        val manager = context.getSystemService(MediaProjectionManager::class.java)
        val taken = manager.getMediaProjection(resultCode, data)
        // The system ends it too (the status bar chip, the notification, another app's share): the call stops sending.
        taken.registerCallback(object : MediaProjection.Callback() {
            override fun onStop() {
                projection = null
                CallModel.current?.screenEnded()
            }
        }, Handler(context.mainLooper))
        projection = taken
    }

    @Synchronized
    override fun start(): List<UInt> {
        val taken = projection ?: return emptyList()
        stop(keepProjection = true)
        val metrics = context.resources.displayMetrics
        // Sent at the size and speed chosen in Voice & Video (#544): even sizes, the screen's own proportions.
        val setup = (context.applicationContext as MorpheusApp).morpheus.shareSetup(metrics.widthPixels.toUInt(), metrics.heightPixels.toUInt())
        val width = setup.width.toInt()
        val height = setup.height.toInt()
        val gap = 1000L / setup.fps.toLong()
        var last = 0L
        val worker = HandlerThread("morpheus-screen").also { it.start() }
        val images = ImageReader.newInstance(width, height, PixelFormat.RGBA_8888, 2)
        images.setOnImageAvailableListener({ source ->
            // (A share ending closes the reader while a picture is being read: that picture is let go, not a crash.)
            val image = try { source.acquireLatestImage() } catch (_: IllegalStateException) { null } ?: return@setOnImageAvailableListener
            val now = android.os.SystemClock.elapsedRealtime()
            if (now - last < gap) { image.close(); return@setOnImageAvailableListener }
            last = now
            try {
                val plane = image.planes[0]
                val buffer = plane.buffer
                val bytes = ByteArray(buffer.remaining())
                buffer.get(bytes)
                callScreenFrame(bytes, plane.rowStride.toUInt(), image.width.toUInt(), image.height.toUInt())
                AppState.screenFrames++
            } catch (_: IllegalStateException) {
            } finally {
                image.close()
            }
        }, Handler(worker.looper))
        display = try {
            taken.createVirtualDisplay("morpheus-share", width, height, metrics.densityDpi, DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, images.surface, null, null)
        } catch (e: Exception) {
            android.util.Log.w("MorpheusCalls", "screen capture did not start: $e")
            worker.quitSafely()
            images.close()
            return emptyList()
        }
        reader = images
        thread = worker
        return listOf(width.toUInt(), height.toUInt())
    }

    override fun stop() = stop(keepProjection = false)

    @Synchronized
    private fun stop(keepProjection: Boolean) {
        display?.release()
        display = null
        reader?.setOnImageAvailableListener(null, null)
        reader?.close()
        reader = null
        thread?.quitSafely()
        thread = null
        if (!keepProjection) {
            // A projection is used once (Android 14): a next share asks again.
            projection?.stop()
            projection = null
            ScreenShareService.stop(context)
        }
    }
}

/** The foreground service the system wants for a screen capture (type mediaProjection), with Stop sharing in its notification. */
class ScreenShareService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            STOP -> {
                CallModel.current?.stopScreenShare()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
            else -> {
                val manager = getSystemService(NotificationManager::class.java)
                manager.createNotificationChannel(NotificationChannel(CHANNEL, t("Screen sharing"), NotificationManager.IMPORTANCE_LOW).apply { setSound(null, null) })
                val stop = PendingIntent.getService(this, 5, Intent(this, ScreenShareService::class.java).setAction(STOP), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
                val open = PendingIntent.getActivity(this, 6, Intent(this, MainActivity::class.java).setFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
                val notification = Notification.Builder(this, CHANNEL)
                    .setSmallIcon(applicationInfo.icon)
                    .setContentTitle(t("Sharing your screen"))
                    .setOngoing(true)
                    .setContentIntent(open)
                    .addAction(Notification.Action.Builder(null, t("Stop sharing"), stop).build())
                    .build()
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION)
                } else {
                    startForeground(ID, notification)
                }
                // In the foreground now: the projection may be taken.
                CallModel.current?.projectionServiceUp()
            }
        }
        return START_NOT_STICKY
    }

    companion object {
        private const val CHANNEL = "screen-share"
        private const val ID = 4242
        private const val STOP = "dev.morpheus.app.SCREEN_SHARE_STOP"

        fun start(context: Context) {
            context.startForegroundService(Intent(context, ScreenShareService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, ScreenShareService::class.java))
        }
    }
}
