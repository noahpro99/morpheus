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
import androidx.activity.findViewTreeOnBackPressedDispatcherOwner
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
        // Tagged nodes of the unmerged tree (a clickable row merges its icon into itself): [{tag, x, y, width, height}] for tags with this prefix.
        "unmerged" -> onMain {
            val prefix = request.getString("prefix")
            val out = JSONArray()
            for (win in windows()) {
                fun walk(node: SemanticsNode) {
                    val tag = node.config.getOrNull(SemanticsProperties.TestTag)
                    if (tag != null && tag.startsWith(prefix)) {
                        val box = node.boundsInWindow.translate(win.dx.toFloat(), win.dy.toFloat())
                        out.put(JSONObject().put("tag", tag).put("x", box.left).put("y", box.top).put("width", box.width).put("height", box.height))
                    }
                    node.children.forEach { walk(it) }
                }
                walk(win.root.semanticsOwner.unmergedRootSemanticsNode)
            }
            out
        }
        "tap" -> {
            tap(request.getDouble("x").toFloat(), request.getDouble("y").toFloat())
            true
        }
        "swipe" -> {
            // `via`: [[x, y], ...] the finger passes on the way (one gesture that turns back).
            val via = request.optJSONArray("via")
            if (via != null) {
                val pts = (0 until via.length()).map { via.getJSONArray(it).let { p -> p.getDouble(0).toFloat() to p.getDouble(1).toFloat() } }
                swipePath(listOf(request.getDouble("x1").toFloat() to request.getDouble("y1").toFloat()) + pts + (request.getDouble("x2").toFloat() to request.getDouble("y2").toFloat()), request.optLong("ms", 300))
            } else swipe(
                request.getDouble("x1").toFloat(), request.getDouble("y1").toFloat(),
                request.getDouble("x2").toFloat(), request.getDouble("y2").toFloat(),
                request.optLong("ms", 300), request.optBoolean("fling", false), request.optLong("hold", 0),
            )
            true
        }
        "quick_tap" -> {
            // A finger down and up at once (30 ms), as a person taps a row.
            tapAt(request.getDouble("x").toFloat(), request.getDouble("y").toFloat())
            true
        }
        "double_tap" -> {
            val (x, y) = request.getDouble("x").toFloat() to request.getDouble("y").toFloat()
            tapAt(x, y)
            Thread.sleep(request.optLong("gap", 70))
            tapAt(x, y)
            true
        }
        "sample" -> {
            // A quick tap (when `x` and `y` are given), then every 10 ms for `ms` where these tags are: [x, width] or null.
            if (request.has("x")) tapAt(request.getDouble("x").toFloat(), request.getDouble("y").toFloat())
            // Or the system back (`back`: true) in place of the tap.
            if (request.optBoolean("back")) onMain { pressBack() }
            // Or a drag (`swipe`: x1, y1, x2, y2, ms) that runs while the samples are taken.
            request.optJSONObject("swipe")?.let { s ->
                Thread {
                    val via = s.optJSONArray("via")
                    if (via != null) {
                        val pts = (0 until via.length()).map { via.getJSONArray(it).let { p -> p.getDouble(0).toFloat() to p.getDouble(1).toFloat() } }
                        swipePath(listOf(s.getDouble("x1").toFloat() to s.getDouble("y1").toFloat()) + pts + (s.getDouble("x2").toFloat() to s.getDouble("y2").toFloat()), s.getLong("ms"))
                    } else swipe(s.getDouble("x1").toFloat(), s.getDouble("y1").toFloat(), s.getDouble("x2").toFloat(), s.getDouble("y2").toFloat(), s.getLong("ms"), false, 0)
                }.start()
            }
            val tags = request.getJSONArray("tags")
            val until = SystemClock.uptimeMillis() + request.optLong("ms", 900)
            val out = JSONArray()
            while (SystemClock.uptimeMillis() < until) {
                out.put(onMain {
                    val all = nodes()
                    val row = JSONArray()
                    for (i in 0 until tags.length()) {
                        val node = all.firstOrNull { it.config.getOrNull(SemanticsProperties.TestTag) == tags.getString(i) }
                        row.put(if (node == null) JSONObject.NULL else JSONObject().put("x", node.positionInWindow.x.toDouble()).put("w", node.boundsInWindow.width))
                    }
                    row
                })
                Thread.sleep(10)
            }
            out
        }
        "windows" -> onMain { JSONArray(windows().map { JSONObject().put("view", it.view.javaClass.name).put("over", it.over).put("modal", it.modal).put("w", it.view.width).put("h", it.view.height).put("x", it.dx).put("y", it.dy) }) }
        "slide_reset" -> { AppState.slideLog.clear(); true }
        "clock_offset" -> { AppState.clockOffsetMs = request.getLong("ms"); true }
        "older_delay" -> { AppState.olderDelayMs = request.getLong("ms"); true }
        "profile_delay" -> { AppState.profileDelayMs = request.getLong("ms"); true }
        "frames_reset" -> onMain { FrameWatch.reset(); true }
        "click" -> onMain { click(find(request)) }
        "type" -> onMain {
            val node = find(request)
            val action = node.config.getOrNull(SemanticsActions.SetText)?.action
                ?: error("${request.opt("tag") ?: request.opt("text")} is not an editable field")
            action(AnnotatedString(request.getString("text")))
        }
        // The text selection (start, end; equal for a cursor) of the editable field with this tag, and a way to set it.
        "selection" -> onMain {
            val range = find(request).config.getOrNull(SemanticsProperties.TextSelectionRange) ?: error("no selection")
            JSONArray().put(range.start).put(range.end)
        }
        "select" -> onMain {
            val node = find(request)
            val action = node.config.getOrNull(SemanticsActions.SetSelection)?.action ?: error("not a field with a selection")
            action(request.getInt("start"), request.getInt("end"), true)
        }
        "key" -> onMain {
            val view = root() ?: error("no window")
            val code = request.getInt("code")
            view.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code))
            view.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code))
        }
        "back" -> onMain { pressBack() }
        "attach" -> {
            val hook = RoomHooks.attach ?: error("no room is open")
            hook(request.getString("name"), request.getString("mime"), request.getString("text").toByteArray())
            true
        }
        "pick_picture" -> {
            val hook = RoomHooks.picture ?: error("no page takes a picture")
            hook(request.getString("name"), android.util.Base64.decode(request.getString("b64"), android.util.Base64.DEFAULT))
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
        // The ongoing call notification (#494): its action titles and importance; or fire the action with this title, as a tap in the shade would.
        "call_notification" -> {
            val manager = app.getSystemService(android.app.NotificationManager::class.java)
            val n = manager.activeNotifications.firstOrNull { it.id == CallService.ID }?.notification ?: error("no call notification")
            val titles = (n.actions ?: emptyArray()).map { it.title.toString() }
            request.optString("fire").takeIf { it.isNotEmpty() }?.let { want ->
                (n.actions ?: emptyArray()).first { it.title.toString() == want }.actionIntent.send()
            }
            JSONObject().put("actions", JSONArray(titles)).put("importance", manager.getNotificationChannel(n.channelId).importance)
                .put("big_content", n.extras.getCharSequence(android.app.Notification.EXTRA_BIG_TEXT) != null)
        }
        // The call sounds played so far (#439), by key; "clear" forgets them first.
        "call_sounds" -> {
            if (request.optBoolean("clear")) CallSounds.played.clear()
            JSONObject().put("played", JSONArray(CallSounds.played.toList()))
        }
        // Make someone a friend (or not) of the signed-in account, for tests that start from a friends list.
        "friend" -> {
            kotlinx.coroutines.runBlocking { (app as MorpheusApp).morpheus.setFriend(request.getString("user"), request.getBoolean("on")) }
            true
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
        // The sound of someone's shared screen as it plays here: `heard` false while there is none.
        "stream_heard" -> {
            val heard = (app as MorpheusApp).morpheus.streamHeard(request.getString("user"))
            JSONObject().put("heard", heard != null).put("level", heard?.level?.toDouble() ?: 0.0).put("volume", heard?.volume?.toDouble() ?: 0.0)
        }
        // The microphone test on the Voice & Video page: `tone` true makes it hear a tone (the emulator's microphone is silent); `running` and `level` say how it is.
        "mic_test" -> {
            if (request.has("tone")) dev.morpheus.app.MicTest.tone = request.getBoolean("tone")
            val test = dev.morpheus.app.ui.MicTestState.current
            JSONObject().put("running", test?.running == true).put("level", (test?.level ?: 0f).toDouble())
        }
        // The camera preview on that page: how many pictures so far (the latest one's sequence number) and its size.
        "camera_preview" -> {
            val frame = uniffi.morpheus_ffi.cameraPreviewFrame()
            JSONObject().put("seq", frame?.seq?.toLong() ?: 0L).put("width", frame?.width?.toInt() ?: 0).put("height", frame?.height?.toInt() ?: 0)
        }
        // The input type the focused field asks the keyboard for (EditorInfo.inputType): what an IME would see.
        "input_type" -> onMain {
            val view = root()?.findFocus() ?: error("nothing is focused")
            val info = android.view.inputmethod.EditorInfo()
            view.onCreateInputConnection(info)
            JSONObject().put("input_type", info.inputType).put("caps_sentences", info.inputType and android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES != 0)
        }
        // What is on the clipboard: its types, its text, and for a picture the size of what its address serves.
        "clipboard" -> onMain {
            val manager = app.getSystemService(android.content.ClipboardManager::class.java)
            val clip = manager.primaryClip
            val item = clip?.getItemAt(0)
            val size = item?.uri?.let { uri -> app.contentResolver.openInputStream(uri)?.use { it.readBytes().size } }
            JSONObject()
                .put("mime", JSONArray((0 until (clip?.description?.mimeTypeCount ?: 0)).map { clip!!.description.getMimeType(it) }))
                .put("text", item?.text?.toString())
                .put("picture_bytes", size ?: JSONObject.NULL)
        }
        "state" -> onMain {
            JSONObject()
                .put("screen", AppState.screen)
                .put("pip", AppState.pip)
                .put("pip_armed", AppState.pipArmed)
                .put("call_watchers", dev.morpheus.app.CallModel.current?.snapshot?.value?.watchers?.toLong() ?: JSONObject.NULL)
                .put("screen_frames", AppState.screenFrames)
                .put("noise_applied", dev.morpheus.app.CallModel.current?.noiseSuppressionApplied() ?: JSONObject.NULL)
                .put("last_link", AppState.lastLink)
                .put("colour_source", AppState.colourSource)
                .put("sdk", android.os.Build.VERSION.SDK_INT)
                .put("colour_pairs", JSONArray(AppState.colourPairs.map { JSONArray().put(it.first).put(it.second.toLong() and 0xFFFFFF).put(it.third.toLong() and 0xFFFFFF) }))
                .put("room", AppState.room)
                .put("haptics", AppState.haptics)
                .put("image_fetches", AppState.imageFetches)
                .put("ime_shown", AppState.imeShown)
                .put("ime_hidden", AppState.imeHidden)
                .put("ime_show_lag_ms", AppState.imeShownAt - AppState.imeLiftAt)
                .put("ime_hide_lag_ms", AppState.imeHiddenAt - AppState.imeLiftAt)
                .put("ime_visible", root()?.let { androidx.core.view.ViewCompat.getRootWindowInsets(it)?.isVisible(androidx.core.view.WindowInsetsCompat.Type.ime()) } ?: false)
                .put("slide_log", JSONArray(AppState.slideLog.map { JSONArray().put(it.first).put(it.second.toDouble()) }))
                .put("avatars_drawn", AppState.avatarsDrawn)
                .put("max_frame_gap_ms", FrameWatch.max)
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

    /** A window of the app with Compose in it: the activity's, and the dialogs and popups over it (a sheet, an alert, a menu). */
    private class Win(val view: View, val root: RootForTest, val dx: Int, val dy: Int, val over: Boolean, val modal: Boolean, val touchable: Boolean = true)

    /** The app's windows with Compose in them, the one underneath first. */
    private fun windows(): List<Win> {
        val decor = root()
        val views: List<View> = if (android.os.Build.VERSION.SDK_INT >= 29) android.view.inspector.WindowInspector.getGlobalWindowViews() else listOfNotNull(decor)
        return views.filter { it.isShown }.mapNotNull { view ->
            val compose = composeRoot(view) ?: return@mapNotNull null
            val at = IntArray(2).also { view.getLocationOnScreen(it) }
            val type = (view.layoutParams as? android.view.WindowManager.LayoutParams)?.type ?: 1
            val over = view !== decor
            // Modal: a dialog, or a window that covers the screen (a sheet). A small popup (a menu) is not.
            val touchable = (view.layoutParams as? android.view.WindowManager.LayoutParams)?.flags?.and(android.view.WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE) == 0
            val covers = decor != null && view.width >= decor.width * 0.9 && view.height >= decor.height * 0.8
            Win(view, compose, at[0], at[1], over, modal = over && touchable && (type == android.view.WindowManager.LayoutParams.TYPE_APPLICATION || covers), touchable = touchable)
        }
    }

    /** Where each node's window is on the screen (the node's own bounds are in its window). */
    private val offsets = java.util.IdentityHashMap<SemanticsNode, IntArray>()

    private fun nodes(): List<SemanticsNode> {
        val out = ArrayList<SemanticsNode>()
        offsets.clear()
        fun walk(node: SemanticsNode, at: IntArray) {
            out.add(node)
            offsets[node] = at
            node.children.forEach { walk(it, at) }
        }
        for (win in windows()) win.root.semanticsOwner.rootSemanticsNode.let { walk(it, intArrayOf(win.dx, win.dy)) }
        return out
    }

    /** The node's box on the screen. */
    private fun boxOf(node: SemanticsNode): androidx.compose.ui.geometry.Rect {
        val at = offsets[node] ?: intArrayOf(0, 0)
        return node.boundsInWindow.translate(at[0].toFloat(), at[1].toFloat())
    }

    private fun textOf(node: SemanticsNode): String {
        val config = node.config
        config.getOrNull(SemanticsProperties.EditableText)?.let { return it.text }
        return config.getOrNull(SemanticsProperties.Text)?.joinToString("\n") { it.text } ?: ""
    }

    private fun describe(node: SemanticsNode): JSONObject {
        val config = node.config
        // What is visible of it: a row cut off by its list is reported (and tapped) where it can be seen.
        val box = boxOf(node)
        return JSONObject()
            .put("id", node.id)
            .put("tag", config.getOrNull(SemanticsProperties.TestTag) ?: if (config.getOrNull(SemanticsProperties.ContentDescription)?.firstOrNull() == "Close sheet") "sheet-scrim" else null)
            .put("text", textOf(node))
            .put("description", config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(" "))
            .put("state", config.getOrNull(SemanticsProperties.StateDescription) ?: config.getOrNull(SemanticsProperties.ToggleableState)?.let { if (it == androidx.compose.ui.state.ToggleableState.On) "On" else "Off" } ?: config.getOrNull(SemanticsProperties.Selected)?.let { if (it) "On" else "Off" })
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
        val box = boxOf(node)
        tap(box.center.x, box.center.y)
        return true
    }

    // ---- touches

    /** The window a touch at (x, y) goes to: a dialog over the activity takes every touch (it is modal), a popup the ones on it. */
    private fun windowAt(x: Float, y: Float): View {
        val all = windows()
        all.lastOrNull { it.modal }?.let { return it.view }
        for (win in all.reversed()) {
            if (win.view === root()) break
            if (!win.touchable) continue
            val hit = win.root.semanticsOwner.rootSemanticsNode.let { r -> r.children.any { c -> c.boundsInWindow.translate(win.dx.toFloat(), win.dy.toFloat()).contains(androidx.compose.ui.geometry.Offset(x, y)) } }
            if (hit) return win.view
        }
        return root() ?: error("no window")
    }

    /** The window a gesture's DOWN went to: the rest of the finger's events go there too, as on a phone, even when a sheet opens over it mid-press. */
    private var gestureWindow: View? = null

    private fun touch(action: Int, down: Long, x: Float, y: Float) = onMain {
        val view = if (action == MotionEvent.ACTION_DOWN || gestureWindow?.isAttachedToWindow != true) windowAt(x, y).also { gestureWindow = it } else gestureWindow!!
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

    /** The system back, as the gesture or the button sends it: to the window over the activity (a sheet, an alert) if there is one, else the activity. Runs on the main thread. */
    private fun pressBack() {
        windows().lastOrNull { it.over && it.touchable }?.let { win ->
            // The window's own back dispatcher (from API 33 a dialog gets no back key, the system calls its callback).
            val dispatcher = win.view.findViewTreeOnBackPressedDispatcherOwner()?.onBackPressedDispatcher
            if (dispatcher != null) { dispatcher.onBackPressed(); return }
        }
        (activity as? androidx.activity.ComponentActivity)?.onBackPressedDispatcher?.onBackPressed() ?: error("no window")
    }

    private fun tapAt(x: Float, y: Float) {
        val down = SystemClock.uptimeMillis()
        touch(MotionEvent.ACTION_DOWN, down, x, y)
        Thread.sleep(30)
        touch(MotionEvent.ACTION_UP, down, x, y)
    }

    /** One touch that goes through the points in turn (down at the first, up at the last), settling a moment at each turn. */
    private fun swipePath(points: List<Pair<Float, Float>>, ms: Long) {
        val down = SystemClock.uptimeMillis()
        touch(MotionEvent.ACTION_DOWN, down, points[0].first, points[0].second)
        for (leg in 1 until points.size) {
            val (a, b) = points[leg - 1] to points[leg]
            for (i in 1..8) {
                Thread.sleep(ms / 8)
                touch(MotionEvent.ACTION_MOVE, down, a.first + (b.first - a.first) * i / 8, a.second + (b.second - a.second) * i / 8)
            }
            Thread.sleep(80)
        }
        Thread.sleep(150)
        touch(MotionEvent.ACTION_UP, down, points.last().first, points.last().second)
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

/** The longest time between two frames since the last reset: a stall of the main thread shows here (the e2e reads it). */
object FrameWatch {
    @Volatile
    var max: Long = 0

    private var last = 0L
    private var running = false

    fun reset() {
        max = 0
        last = 0
        start()
    }

    private fun start() {
        if (running) return
        running = true
        val choreographer = android.view.Choreographer.getInstance()
        choreographer.postFrameCallback(object : android.view.Choreographer.FrameCallback {
            override fun doFrame(frameTimeNanos: Long) {
                val now = frameTimeNanos / 1_000_000
                if (last != 0L && now - last > max) max = now - last
                last = now
                choreographer.postFrameCallback(this)
            }
        })
    }
}
