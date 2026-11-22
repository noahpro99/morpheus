@file:OptIn(androidx.compose.ui.InternalComposeUiApi::class)

package dev.morpheus.app

import android.app.Activity
import android.app.Application
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.node.RootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.AnnotatedString
import org.json.JSONArray
import org.json.JSONObject
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/**
 * The e2e driver's way into the app (debug builds only; morpheus-e2e's
 * `native_android.rs` is the other end, through `adb forward`).
 *
 * Plain JSON lines over loopback TCP on [PORT]: one request per line,
 * `{"id":1,"op":"tree"}`, one answer per line, `{"id":1,"ok":true,"result":...}`.
 *
 * - `tree`: every node of Compose's merged semantics tree, with its text, test
 *   tag, screen bounds and what it can do (click, edit, scroll).
 * - `tap` (x, y) and `swipe` (x1, y1, x2, y2, ms): real touches, sent to the
 *   Compose view as the system would.
 * - `click` (tag or text): the node's own click action (a tap on its centre
 *   when it has none).
 * - `type` (tag, text): sets a field's text.
 * - `key` (code): a key press, e.g. 66 (Enter); `back`.
 * - `state`: the app's own state (`AppState`) and the focused field.
 */
class TestBridge(private val app: Application) {
    private val main = Handler(Looper.getMainLooper())

    @Volatile
    private var activity: Activity? = null

    fun start() {
        app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            override fun onActivityResumed(a: Activity) {
                activity = a
            }

            override fun onActivityPaused(a: Activity) {
                if (activity === a) activity = null
            }

            override fun onActivityCreated(a: Activity, b: Bundle?) = Unit
            override fun onActivityStarted(a: Activity) = Unit
            override fun onActivityStopped(a: Activity) = Unit
            override fun onActivitySaveInstanceState(a: Activity, b: Bundle) = Unit
            override fun onActivityDestroyed(a: Activity) = Unit
        })
        thread(name = "e2e-bridge", isDaemon = true) {
            val server = ServerSocket(PORT, 8, InetAddress.getByName("127.0.0.1"))
            Log.i(TAG, "listening on $PORT")
            while (true) {
                val client = server.accept()
                thread(name = "e2e-bridge-client", isDaemon = true) { serve(client) }
            }
        }
    }

    private fun serve(socket: Socket) {
        socket.use {
            val reader = it.getInputStream().bufferedReader()
            val writer = it.getOutputStream().bufferedWriter()
            while (true) {
                val line = reader.readLine() ?: break
                val reply = JSONObject()
                try {
                    val request = JSONObject(line)
                    reply.put("id", request.opt("id"))
                    reply.put("result", handle(request))
                    reply.put("ok", true)
                } catch (e: Throwable) {
                    reply.put("ok", false)
                    reply.put("error", e.message ?: e.toString())
                }
                writer.write(reply.toString())
                writer.write("\n")
                writer.flush()
            }
        }
    }

    private fun <T> onMain(block: () -> T): T {
        val done = CompletableFuture<T>()
        main.post {
            try {
                done.complete(block())
            } catch (e: Throwable) {
                done.completeExceptionally(e)
            }
        }
        try {
            return done.get(10, TimeUnit.SECONDS)
        } catch (e: java.util.concurrent.ExecutionException) {
            throw e.cause ?: e
        }
    }

    private fun handle(request: JSONObject): Any? = when (val op = request.getString("op")) {
        "ping" -> "pong"
        "tree" -> onMain { JSONArray(nodes().map { describe(it) }) }
        "tap" -> {
            tap(request.getDouble("x").toFloat(), request.getDouble("y").toFloat())
            true
        }
        "swipe" -> {
            swipe(
                request.getDouble("x1").toFloat(), request.getDouble("y1").toFloat(),
                request.getDouble("x2").toFloat(), request.getDouble("y2").toFloat(),
                request.optLong("ms", 300), request.optBoolean("fling", false), request.optLong("hold", 0),
            )
            true
        }
        "click" -> onMain { click(find(request)) }
        "type" -> onMain {
            val node = find(request)
            val action = node.config.getOrNull(SemanticsActions.SetText)?.action
                ?: error("${request.opt("tag") ?: request.opt("text")} is not an editable field")
            action(AnnotatedString(request.getString("text")))
        }
        "key" -> onMain {
            val view = root() ?: error("no window")
            val code = request.getInt("code")
            view.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code))
            view.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code))
        }
        "back" -> onMain {
            (activity as? androidx.activity.ComponentActivity)?.onBackPressedDispatcher?.onBackPressed() ?: error("no window")
        }
        "attach" -> {
            val hook = RoomHooks.attach ?: error("no room is open")
            hook(request.getString("name"), request.getString("mime"), request.getString("text").toByteArray())
            true
        }
        "push" -> {
            dev.morpheus.app.push.Push.visible = false // as if the app were in the background
            val bytes = request.getString("body").toByteArray()
            thread { dev.morpheus.app.push.Push.onPush(app, bytes) }
            true
        }
        "notifications" -> {
            val manager = app.getSystemService(android.app.NotificationManager::class.java)
            JSONArray(manager.activeNotifications.map {
                JSONObject()
                    .put("title", it.notification.extras.getCharSequence(android.app.Notification.EXTRA_TITLE)?.toString())
                    .put("text", it.notification.extras.getCharSequence(android.app.Notification.EXTRA_TEXT)?.toString())
            })
        }
        "gain" -> {
            // What a person's voice plays at now, and the kept microphone and speaker volumes.
            val morpheus = (app as MorpheusApp).morpheus
            val volumes = morpheus.callVolumes()
            JSONObject()
                .put("gain", morpheus.personGain(request.getString("user")).toDouble())
                .put("input", volumes.input.toDouble())
                .put("output", volumes.output.toDouble())
        }
        "state" -> onMain {
            JSONObject()
                .put("screen", AppState.screen)
                .put("last_link", AppState.lastLink)
                .put("room", AppState.room)
                .put("haptics", AppState.haptics)
                .put("loading_seen", AppState.loadingSeen)
                .put("first_list_ms", AppState.firstListMs)
                .put("first_room_ms", AppState.firstRoomMs)
                .put("session_ms", AppState.sessionMs)
                .put("marks", JSONArray(AppState.marks.map { JSONArray().put(it.first).put(it.second) }))
                .put("version", BuildConfig.VERSION_NAME)
        }
        else -> error("unknown op $op")
    }

    // ---- the tree

    private fun root(): View? = activity?.window?.decorView

    private fun composeRoot(view: View?): RootForTest? {
        if (view is RootForTest) return view
        if (view is ViewGroup) {
            for (i in 0 until view.childCount) composeRoot(view.getChildAt(i))?.let { return it }
        }
        return null
    }

    private fun nodes(): List<SemanticsNode> {
        val root = composeRoot(root())?.semanticsOwner?.rootSemanticsNode ?: return emptyList()
        val out = ArrayList<SemanticsNode>()
        fun walk(node: SemanticsNode) {
            out.add(node)
            node.children.forEach(::walk)
        }
        walk(root)
        return out
    }

    private fun textOf(node: SemanticsNode): String {
        val config = node.config
        config.getOrNull(SemanticsProperties.EditableText)?.let { return it.text }
        return config.getOrNull(SemanticsProperties.Text)?.joinToString("\n") { it.text } ?: ""
    }

    private fun describe(node: SemanticsNode): JSONObject {
        val config = node.config
        // What is visible of it: a row cut off by its list is reported (and tapped) where it can be seen.
        val box = node.boundsInWindow
        return JSONObject()
            .put("id", node.id)
            .put("tag", config.getOrNull(SemanticsProperties.TestTag))
            .put("text", textOf(node))
            .put("description", config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(" "))
            .put("x", box.left).put("y", box.top)
            .put("width", box.width).put("height", box.height)
            .put("clickable", config.contains(SemanticsActions.OnClick))
            .put("editable", config.contains(SemanticsActions.SetText))
            .put("scrollable", config.contains(SemanticsActions.ScrollBy))
            .put("focused", config.getOrNull(SemanticsProperties.Focused) == true)
            .put("enabled", !config.contains(SemanticsProperties.Disabled))
            .put("password", config.contains(SemanticsProperties.Password))
    }

    private fun find(request: JSONObject): SemanticsNode {
        val tag = request.optString("tag").ifEmpty { null }
        val text = request.optString("text").ifEmpty { null }
        val all = nodes()
        return (if (tag != null) all.firstOrNull { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
        else all.firstOrNull { textOf(it).contains(text ?: "") })
            ?: error("no element ${tag?.let { "tagged $it" } ?: "with text \"$text\""}")
    }

    private fun click(node: SemanticsNode): Boolean {
        val action = node.config.getOrNull(SemanticsActions.OnClick)?.action
        if (action != null) return action() ?: true
        val box = node.boundsInWindow
        tap(box.center.x, box.center.y)
        return true
    }

    // ---- touches

    private fun touch(action: Int, down: Long, x: Float, y: Float) = onMain {
        val view = root() ?: error("no window")
        val at = IntArray(2).also { view.getLocationOnScreen(it) }
        val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, x - at[0], y - at[1], 0)
        event.source = android.view.InputDevice.SOURCE_TOUCHSCREEN
        view.dispatchTouchEvent(event)
        event.recycle()
    }

    private fun tap(x: Float, y: Float) {
        val down = SystemClock.uptimeMillis()
        touch(MotionEvent.ACTION_DOWN, down, x, y)
        Thread.sleep(40)
        touch(MotionEvent.ACTION_UP, down, x, y)
    }

    private fun swipe(x1: Float, y1: Float, x2: Float, y2: Float, ms: Long, fling: Boolean, hold: Long) {
        val down = SystemClock.uptimeMillis()
        val steps = 12
        touch(MotionEvent.ACTION_DOWN, down, x1, y1)
        // Held still after the touch-down first: a long press, then the drag.
        if (hold > 0) Thread.sleep(hold)
        for (i in 1..steps) {
            Thread.sleep(ms / steps)
            touch(MotionEvent.ACTION_MOVE, down, x1 + (x2 - x1) * i / steps, y1 + (y2 - y1) * i / steps)
        }
        // Held still a moment before lifting: no fling, so a drag moves the
        // content by what it covers and no more.
        // A flick (`fling`) lifts while still moving, so the content gets the finger's velocity.
        if (!fling) Thread.sleep(150)
        touch(MotionEvent.ACTION_UP, down, x2, y2)
    }

    companion object {
        const val PORT = 47100
        private const val TAG = "MorpheusBridge"
    }
}
