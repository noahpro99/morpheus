package dev.morpheus.mobile

import android.app.Activity
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.view.Gravity
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import java.lang.ref.WeakReference

/**
 * The full-screen incoming call, over the lock screen too: who is calling,
 * Decline and Answer. Closes once the ring is over.
 */
class IncomingCallActivity : Activity() {
    private var roomId = ""

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setShowWhenLocked(true)
        setTurnScreenOn(true)
        roomId = intent.getStringExtra("room_id").orEmpty()
        val caller = intent.getStringExtra("caller").orEmpty().ifEmpty { "Someone" }
        if (CallRinger.ringFor(roomId) == null) {
            finish()
            return
        }
        current = WeakReference(this)
        val density = resources.displayMetrics.density
        fun dp(value: Int) = (value * density).toInt()
        fun round(label: String, color: Int, onTap: () -> Unit) = Button(this).apply {
            text = label
            contentDescription = label
            setTextColor(Color.WHITE)
            isAllCaps = false
            textSize = 16f
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(color)
            }
            layoutParams = LinearLayout.LayoutParams(dp(88), dp(88)).apply { setMargins(dp(28), 0, dp(28), 0) }
            setOnClickListener { onTap() }
        }
        val name = TextView(this).apply {
            text = caller
            textSize = 30f
            setTextColor(Color.WHITE)
            gravity = Gravity.CENTER
        }
        val what = TextView(this).apply {
            text = "Incoming call"
            textSize = 16f
            setTextColor(Color.parseColor("#b5bac1"))
            gravity = Gravity.CENTER
            setPadding(0, dp(8), 0, dp(160))
        }
        val buttons = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            addView(round("Decline", Color.parseColor("#da373c")) {
                CallRinger.decline(this@IncomingCallActivity, roomId)
                finish()
            })
            addView(round("Answer", Color.parseColor("#23a559")) {
                CallRinger.answer(this@IncomingCallActivity, roomId)
                finish()
            })
        }
        setContentView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setBackgroundColor(Color.parseColor("#1a1e29"))
            addView(name)
            addView(what)
            addView(buttons)
        })
    }

    override fun onDestroy() {
        if (current?.get() === this) current = null
        super.onDestroy()
    }

    companion object {
        private var current: WeakReference<IncomingCallActivity>? = null

        /** The ring in `roomId` is over: close its screen, if showing. */
        fun close(roomId: String) {
            current?.get()?.let { activity ->
                if (activity.roomId == roomId) activity.runOnUiThread { activity.finish() }
            }
        }
    }
}
