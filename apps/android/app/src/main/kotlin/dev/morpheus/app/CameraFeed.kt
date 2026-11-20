package dev.morpheus.app

import android.content.Context
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import uniffi.morpheus_ffi.CameraDriver
import uniffi.morpheus_ffi.callCameraFrame

/** Which of a phone's cameras to use: the one facing the wanted way, else any. */
object CameraChoice {
    /** The index in [facings] (CameraCharacteristics.LENS_FACING_* values) to open, or null with no camera. */
    fun pick(facings: List<Int>, front: Boolean): Int? {
        val wanted = if (front) CameraCharacteristics.LENS_FACING_FRONT else CameraCharacteristics.LENS_FACING_BACK
        return facings.indexOf(wanted).takeIf { it >= 0 } ?: facings.indices.firstOrNull()
    }

    /** Whether there is a camera on each side to switch between. */
    fun canSwitch(facings: List<Int>): Boolean =
        facings.contains(CameraCharacteristics.LENS_FACING_FRONT) && facings.contains(CameraCharacteristics.LENS_FACING_BACK)
}

/**
 * The phone's camera in a call (Camera2): the call asks it to start and stop
 * through [CameraDriver]; each picture goes back to Rust (`callCameraFrame`),
 * which sends it and shows it on my own tile. Starts on the front camera.
 */
class CameraFeed(private val context: Context) : CameraDriver {
    private var thread: HandlerThread? = null
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null

    @Volatile
    var front = true
        private set

    private fun facings(manager: CameraManager, ids: List<String>) =
        ids.map { manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) ?: -1 }

    /** Whether the phone has cameras on both sides. */
    fun canSwitch(): Boolean {
        val manager = context.getSystemService(CameraManager::class.java) ?: return false
        return try { CameraChoice.canSwitch(facings(manager, manager.cameraIdList.toList())) } catch (_: Exception) { false }
    }

    /** Use the other camera, now when this one is on. */
    @Synchronized
    fun flip() {
        front = !front
        if (device != null) start()
    }

    @Synchronized
    override fun start(): Boolean {
        stop()
        val manager = context.getSystemService(CameraManager::class.java) ?: return false
        val ids = try { manager.cameraIdList.toList() } catch (_: Exception) { return false }
        val index = CameraChoice.pick(facings(manager, ids), front) ?: return false
        val id = ids[index]
        val characteristics = manager.getCameraCharacteristics(id)
        front = characteristics.get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_FRONT
        val sensor = characteristics.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0

        val worker = HandlerThread("morpheus-camera").also { it.start() }
        val handler = Handler(worker.looper)
        val images = ImageReader.newInstance(WIDTH, HEIGHT, ImageFormat.YUV_420_888, 2)
        images.setOnImageAvailableListener({ source ->
            val image = source.acquireLatestImage() ?: return@setOnImageAvailableListener
            try {
                val planes = image.planes
                callCameraFrame(
                    bytes(planes[0].buffer), bytes(planes[1].buffer), bytes(planes[2].buffer),
                    planes[0].rowStride.toUInt(), planes[1].rowStride.toUInt(), planes[1].pixelStride.toUInt(),
                    image.width.toUInt(), image.height.toUInt(), sensor.toUInt(),
                )
            } finally {
                image.close()
            }
        }, handler)

        val opened = CountDownLatch(1)
        var ok = false
        try {
            manager.openCamera(id, object : CameraDevice.StateCallback() {
                override fun onOpened(camera: CameraDevice) {
                    device = camera
                    @Suppress("DEPRECATION")
                    camera.createCaptureSession(
                        listOf(images.surface),
                        object : CameraCaptureSession.StateCallback() {
                            override fun onConfigured(s: CameraCaptureSession) {
                                session = s
                                val request = camera.createCaptureRequest(CameraDevice.TEMPLATE_RECORD)
                                request.addTarget(images.surface)
                                request.set(CaptureRequest.CONTROL_MODE, CaptureRequest.CONTROL_MODE_AUTO)
                                s.setRepeatingRequest(request.build(), null, handler)
                                ok = true
                                opened.countDown()
                            }

                            override fun onConfigureFailed(s: CameraCaptureSession) {
                                Log.w(TAG, "camera session not configured")
                                opened.countDown()
                            }
                        },
                        handler,
                    )
                }

                override fun onDisconnected(camera: CameraDevice) {
                    camera.close()
                    opened.countDown()
                }

                override fun onError(camera: CameraDevice, error: Int) {
                    Log.w(TAG, "camera error $error")
                    camera.close()
                    opened.countDown()
                }
            }, handler)
        } catch (err: SecurityException) {
            Log.w(TAG, "no camera permission: $err")
            worker.quitSafely()
            images.close()
            return false
        }
        opened.await(10, TimeUnit.SECONDS)
        thread = worker
        reader = images
        if (!ok) stop()
        return ok
    }

    @Synchronized
    override fun stop() {
        try { session?.close() } catch (_: Exception) {}
        try { device?.close() } catch (_: Exception) {}
        try { reader?.close() } catch (_: Exception) {}
        thread?.quitSafely()
        session = null
        device = null
        reader = null
        thread = null
    }

    private fun bytes(buffer: java.nio.ByteBuffer): ByteArray {
        val out = ByteArray(buffer.remaining())
        buffer.duplicate().get(out)
        return out
    }

    companion object {
        private const val TAG = "MorpheusCamera"
        private const val WIDTH = 640
        private const val HEIGHT = 480
    }
}
