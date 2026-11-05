package dev.morpheus.mobile

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

/**
 * The phone's camera in a call: the front camera (else the back one), each
 * picture handed to Rust (`camera_frame` in `apps/mobile/src/main.rs`),
 * which sends it to the call. Started and stopped from Rust through JNI.
 */
object CameraFeed {
    private const val TAG = "MorpheusCamera"
    private const val WIDTH = 640
    private const val HEIGHT = 480

    private var thread: HandlerThread? = null
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null

    /** Each picture's planes; `rotation` turns it upright (degrees). */
    @JvmStatic
    external fun frame(
        y: java.nio.ByteBuffer, u: java.nio.ByteBuffer, v: java.nio.ByteBuffer,
        yStride: Int, uvStride: Int, uvPixelStride: Int,
        width: Int, height: Int, rotation: Int,
    )

    /** Open the camera and start sending; whether it opened. */
    @JvmStatic
    @Synchronized
    fun start(context: Context): Boolean {
        stop()
        val manager = context.getSystemService(CameraManager::class.java) ?: return false
        val ids = try { manager.cameraIdList.toList() } catch (err: Exception) { return false }
        val facing = { id: String ->
            manager.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING)
        }
        val id = ids.firstOrNull { facing(it) == CameraCharacteristics.LENS_FACING_FRONT }
            ?: ids.firstOrNull() ?: return false
        val sensor = manager.getCameraCharacteristics(id)
            .get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0

        val worker = HandlerThread("morpheus-camera").also { it.start() }
        val handler = Handler(worker.looper)
        val images = ImageReader.newInstance(WIDTH, HEIGHT, ImageFormat.YUV_420_888, 2)
        images.setOnImageAvailableListener({ source ->
            val image = source.acquireLatestImage() ?: return@setOnImageAvailableListener
            try {
                val planes = image.planes
                frame(
                    planes[0].buffer, planes[1].buffer, planes[2].buffer,
                    planes[0].rowStride, planes[1].rowStride, planes[1].pixelStride,
                    image.width, image.height, sensor,
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

    @JvmStatic
    @Synchronized
    fun stop() {
        try { session?.close() } catch (_: Exception) {}
        try { device?.close() } catch (_: Exception) {}
        try { reader?.close() } catch (_: Exception) {}
        thread?.quitSafely()
        session = null
        device = null
        reader = null
        thread = null
    }
}
