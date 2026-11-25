package dev.morpheus.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.ViewModel
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch
import androidx.lifecycle.ViewModelProvider
import dev.morpheus.app.ui.MorpheusTheme
import dev.morpheus.app.ui.Root

class MainActivity : ComponentActivity() {
    private var modelMade = false
    private val model: AppModel by viewModels {
        object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T =
                AppModel((application as MorpheusApp).morpheus.also { AppState.mark("app model: making") }, application as MorpheusApp).also { modelMade = true; AppState.mark("app model made") } as T
        }
    }

    private val microphone = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { model.microphoneAnswered(it) }

    override fun onCreate(savedInstanceState: Bundle?) {
        AppState.mark("activity: onCreate")
        super.onCreate(savedInstanceState)
        // Nothing here waits for the Rust core (it starts on a thread of its own, see MorpheusApp): the model is
        // made when the first frame composes, by when the core is usually up (#317).
        dev.morpheus.app.ui.MaterialYou.refresh(this)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        setContent {
            val app = model
            androidx.compose.runtime.LaunchedEffect(Unit) {
                launch { app.calls.needCamera.collect { if (it) camera.launch(android.Manifest.permission.CAMERA) } }
                launch { app.needMicrophone.collect { if (it != null) microphone.launch(android.Manifest.permission.RECORD_AUDIO) } }
            }
            val light = dev.morpheus.app.ui.Palette.IsLight
            // (onCreate set the dark bars already: only a change of look needs this again.)
            val appliedLight = androidx.compose.runtime.remember { booleanArrayOf(false) }
            androidx.compose.runtime.LaunchedEffect(light) {
                if (light == appliedLight[0]) return@LaunchedEffect
                appliedLight[0] = light
                val bars = if (light) SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT) else SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
                enableEdgeToEdge(statusBarStyle = bars, navigationBarStyle = bars)
            }
            AppState.mark("activity: setContent")
            MorpheusTheme { Root(app) }
        }
        // The push registration and the permission prompt wait until the first frame is up.
        if (savedInstanceState == null) android.os.Looper.myQueue().addIdleHandler {
            dev.morpheus.app.push.Push.start(this)
            false
        }
        if (savedInstanceState == null) ShareInbox.receive(this, intent)
        openRoomOf(intent)
    }

    private val camera = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { model.calls.cameraAnswered(it) }

    override fun onStop() {
        super.onStop()
        // The camera is for a call on screen: it stops when the app is left, unless picture-in-picture keeps the call in view.
        if (modelMade && !isInPictureInPictureMode) model.calls.appLeft()
    }

    override fun onStart() {
        super.onStart()
        // (The first start has nothing to bring back: the model is not made yet.)
        if (modelMade) {
            model.calls.appBack()
            model.ringModel.onScreen()
        }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        ShareInbox.receive(this, intent)
        openRoomOf(intent)
    }

    /** A tap on a notification names the room to show. */
    private fun openRoomOf(intent: android.content.Intent?) {
        intent?.getStringExtra(Ringer.EXTRA_RING)?.let {
            // A ring that fills the screen shows over the lock screen.
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        }
        intent?.getStringExtra(Ringer.EXTRA_ANSWER)?.let { model.answerWhenSignedIn(it, intent.getStringExtra(Ringer.EXTRA_ANSWER_NAME).orEmpty()) }
        intent?.getStringExtra(dev.morpheus.app.push.Push.EXTRA_ROOM)?.let { model.openRoomWhenKnown(it) }
    }
}
