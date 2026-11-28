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
        AppState.welcome = false // the first-open welcome would cover the screens the other tests open; "welcome" turns it on
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
                request.optLong("ms", 300), request.optBoolean("fling", false), request.optLong("hold", 0), request.optLong("rest", 150),
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
        "haptics_reset" -> { AppState.hapticLog.clear(); true }
        "slide_reset" -> { AppState.slideLog.clear(); true }
        "clock_offset" -> { AppState.clockOffsetMs = request.getLong("ms"); true }
        "older_delay" -> { AppState.olderDelayMs = request.getLong("ms"); true }
        "profile_delay" -> { AppState.profileDelayMs = request.getLong("ms"); true }
        "image_delay" -> { AppState.imageDelayMs = request.getLong("ms"); true }
        "frames_reset" -> onMain { FrameWatch.reset(); true }
        "stages_reset" -> { AppState.stages.clear(); true }
        "warm" -> { AppState.warmEnabled = request.getBoolean("on"); true }
        // A tap (or the system back) and the time until something shows or goes: {tag or text or x,y, back, until, gone, timeout_ms} -> ms from the finger lifting to the frame that has it (flow timing).
        "timed" -> timed(request)
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
        "update_feed" -> {
            android.system.Os.setenv("MORPHEUS_UPDATE_FEED", request.getString("url"), true)
            (AppState.recheckUpdate ?: error("not signed in"))()
            true
        }
        "welcome" -> { AppState.welcome = request.getBoolean("on"); true }
        "manage_url" -> { AppState.manageUrl = request.getString("url"); true }
        "paste_image" -> {
            // As a keyboard or a paste hands a picture over: a content URI in a clip, offered to the window's receive-content listener.
            val dir = java.io.File(app.cacheDir, "opened").apply { mkdirs() }
            val file = java.io.File(dir, request.getString("name"))
            file.writeBytes(android.util.Base64.decode(request.getString("b64"), android.util.Base64.DEFAULT))
            val uri = androidx.core.content.FileProvider.getUriForFile(app, "${app.packageName}.files", file)
            onMain {
                val view = activity?.currentFocus ?: root() ?: error("no window")
                val clip = android.content.ClipData("picture", arrayOf("image/png"), android.content.ClipData.Item(uri))
                val info = androidx.core.view.ContentInfoCompat.Builder(clip, androidx.core.view.ContentInfoCompat.SOURCE_CLIPBOARD).build()
                var left = androidx.core.view.ViewCompat.performReceiveContent(view, info)
                if (left != null) {
                    // The keyboard's own way in: the text input session's commitContent.
                    val editor = android.view.inputmethod.EditorInfo()
                    val connection = view.onCreateInputConnection(editor) ?: error("no text input is open")
                    val content = androidx.core.view.inputmethod.InputContentInfoCompat(uri, android.content.ClipDescription("picture", arrayOf("image/png")), null)
                    val taken = androidx.core.view.inputmethod.InputConnectionCompat.commitContent(connection, editor, content, androidx.core.view.inputmethod.InputConnectionCompat.INPUT_CONTENT_GRANT_READ_URI_PERMISSION, null)
                    if (!taken) error("nothing took the picture")
                }
            }
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
            if (request.has("tone")) { dev.morpheus.app.MicTest.tone = request.getBoolean("tone"); dev.morpheus.app.MicPump.tone = request.getBoolean("tone") }
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
                .put("keep_screen_on", AppState.keepScreenOn)
                .put("pip_armed", AppState.pipArmed)
                .put("call_people", dev.morpheus.app.CallModel.current?.snapshot?.value?.people?.joinToString { "${it.userId}:${if (it.speaking) "S" else "-"}${if (it.muted) "m" else ""}" } ?: "none")
                .put("voice_stats", uniffi.morpheus_ffi.callVoiceStats())
                .put("call_speakers", dev.morpheus.app.CallModel.speakers.value.let { it.first + "=" + it.second.joinToString() })
                .put("call_watchers", dev.morpheus.app.CallModel.current?.snapshot?.value?.watchers?.toLong() ?: JSONObject.NULL)
                .put("screen_frames", AppState.screenFrames)
                .put("noise_applied", dev.morpheus.app.CallModel.current?.noiseSuppressionApplied() ?: JSONObject.NULL)
                .put("gate_open", (app as dev.morpheus.app.MorpheusApp).morpheus.gateReading().open)
                .put("gate_kept", (app as dev.morpheus.app.MorpheusApp).morpheus.inputSensitivity().thresholdDb.toDouble())
                .put("gate_threshold", (app as dev.morpheus.app.MorpheusApp).morpheus.gateReading().thresholdDb.toDouble())
                .put("strong_frames", uniffi.morpheus_ffi.callMicFramesCleaned().toLong())
                .put("mic_pump_frames", dev.morpheus.app.CallModel.current?.micPump?.frames ?: 0L)
                .put("noise_applied_key", dev.morpheus.app.CallModel.current?.noiseApplied() ?: JSONObject.NULL)
                .put("last_link", AppState.lastLink)
                .put("last_copy", AppState.lastCopy)
                .put("last_share", AppState.lastShare)
                .put("last_dismiss_ms", AppState.lastDismissMs)
                .put("last_pane_ms", AppState.lastPaneMs)
                .put("colour_source", AppState.colourSource)
                .put("sdk", android.os.Build.VERSION.SDK_INT)
                .put("colour_pairs", JSONArray(AppState.colourPairs.map { JSONArray().put(it.first).put(it.second.toLong() and 0xFFFFFF).put(it.third.toLong() and 0xFFFFFF) }))
                .put("room", AppState.room)
                .put("haptics", AppState.haptics)
                .put("haptic_log", JSONArray(AppState.hapticLog.toList()))
                .put("older_scrolls", AppState.olderScrolls)
                .put("stages", JSONObject(AppState.stages.toMap()))
                .put("overscroll_effects", JSONArray(AppState.overscrollEffects.toList()))
                .put("sheet_rows_lag_ms", if (AppState.sheetRowsAt > 0 && AppState.sheetAt > 0) maxOf(0L, AppState.sheetRowsAt - AppState.sheetAt) else -1)
                .put("sheet_lag_ms", if (AppState.sheetAt >= AppState.touchDownAt && AppState.sheetAt > 0) AppState.sheetAt - AppState.touchDownAt else -1)
                .put("image_fetches", AppState.imageFetches)
                .put("emotes_drawn", AppState.emotesDrawn)
                .put("ime_shown", AppState.imeShown)
                .put("ime_hidden", AppState.imeHidden)
                .put("ime_show_lag_ms", AppState.imeShownAt - AppState.imeLiftAt)
                .put("ime_hide_lag_ms", AppState.imeHiddenAt - AppState.imeLiftAt)
                .put("ime_visible", root()?.let { androidx.core.view.ViewCompat.getRootWindowInsets(it)?.isVisible(androidx.core.view.WindowInsetsCompat.Type.ime()) } ?: false)
                .put("slide_log", JSONArray(AppState.slideLog.map { JSONArray().put(it.first).put(it.second.toDouble()) }))
                .put("avatars_drawn", AppState.avatarsDrawn)
                .put("max_frame_gap_ms", FrameWatch.max)
                .put("frame_count", FrameWatch.frames)
                .put("frame_janks", FrameWatch.janks)
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
            .put("spans", spansOf(node))
    }

    /** The styles of a text node's annotated string, as `bold:0-4`, `italic`, `code`, `strike`, `underline` and `link` (UTF-16 offsets), so the tests can tell a formatted message from its marks. */
    private fun spansOf(node: SemanticsNode): JSONArray {
        val out = JSONArray()
        val text = node.config.getOrNull(SemanticsProperties.Text)?.firstOrNull() ?: return out
        for (span in text.spanStyles) {
            val range = "${span.start}-${span.end}"
            val style = span.item
            if (style.fontWeight == androidx.compose.ui.text.font.FontWeight.Bold) out.put("bold:$range")
            if (style.fontStyle == androidx.compose.ui.text.font.FontStyle.Italic) out.put("italic:$range")
            if (style.fontFamily == androidx.compose.ui.text.font.FontFamily.Monospace) out.put("code:$range")
            style.textDecoration?.let {
                if (it.contains(androidx.compose.ui.text.style.TextDecoration.LineThrough)) out.put("strike:$range")
                if (it.contains(androidx.compose.ui.text.style.TextDecoration.Underline)) out.put("underline:$range")
            }
        }
        for (link in text.getStringAnnotations("URL", 0, text.length)) out.put("link:${link.start}-${link.end}")
        for (inline in text.getStringAnnotations("androidx.compose.foundation.text.inlineContent", 0, text.length)) out.put("emote:${inline.start}-${inline.end}")
        return out
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

    /** Something with this tag (or, for `text:`, with this text in it) is on the screen. */
    private fun visible(what: String): Boolean = onMain {
        val shown = if (what.startsWith("text:")) nodes().firstOrNull { it.boundsInWindow.height > 0f && textOf(it).contains(what.removePrefix("text:")) }
        else nodes().firstOrNull { it.config.getOrNull(SemanticsProperties.TestTag) == what }
        shown != null && shown.boundsInWindow.height > 0f
    }

    private fun afterFrames(count: Int) {
        val done = java.util.concurrent.CountDownLatch(1)
        var left = count
        main.post {
            val choreographer = android.view.Choreographer.getInstance()
            choreographer.postFrameCallback(object : android.view.Choreographer.FrameCallback {
                override fun doFrame(frameTimeNanos: Long) {
                    if (--left <= 0) done.countDown() else choreographer.postFrameCallback(this)
                }
            })
        }
        done.await(2, java.util.concurrent.TimeUnit.SECONDS)
    }

    private fun timed(request: JSONObject): JSONObject {
        val until = request.optString("until").ifEmpty { null }
        val gone = request.optString("gone").ifEmpty { null }
        val timeout = request.optLong("timeout_ms", 5000)
        val point: Pair<Float, Float>? = if (request.optBoolean("back")) null else if (request.has("x")) request.getDouble("x").toFloat() to request.getDouble("y").toFloat() else onMain { boxOf(find(request)).center.let { it.x to it.y } }
        val down = SystemClock.uptimeMillis()
        var start: Long
        if (point == null) {
            start = SystemClock.uptimeMillis()
            onMain { pressBack() }
        } else {
            touch(MotionEvent.ACTION_DOWN, down, point.first, point.second)
            Thread.sleep(80) // a human tap lasts about this long
            start = SystemClock.uptimeMillis()
            touch(MotionEvent.ACTION_UP, down, point.first, point.second)
        }
        val deadline = start + timeout
        while (SystemClock.uptimeMillis() < deadline) {
            val ok = if (until != null) visible(until) else if (gone != null) !visible(gone) else true
            if (ok) {
                afterFrames(1)
                return JSONObject().put("ms", SystemClock.uptimeMillis() - start).put("at", start).put("ok", true)
            }
            Thread.sleep(2)
        }
        return JSONObject().put("ms", timeout).put("ok", false)
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

    private fun swipe(x1: Float, y1: Float, x2: Float, y2: Float, ms: Long, fling: Boolean, hold: Long, rest: Long = 150) {
        val down = SystemClock.uptimeMillis()
        val steps = 12
        AppState.touchDownAt = down
        AppState.sheetAt = 0
        AppState.sheetRowsAt = 0
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
        if (!fling) Thread.sleep(rest)
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

    /** Frames since the reset, and how many came more than two refresh intervals after the one before (dropped frames). */
    @Volatile
    var frames: Int = 0

    @Volatile
    var janks: Int = 0

    private var last = 0L
    private var running = false

    fun reset() {
        max = 0
        frames = 0
        janks = 0
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
                if (last != 0L) { frames++; if (now - last > 34) janks++ }
                last = now
                choreographer.postFrameCallback(this)
            }
        })
    }
}
