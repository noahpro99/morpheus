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
    private val model: AppModel by viewModels {
        object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T =
                AppModel((application as MorpheusApp).morpheus, application as MorpheusApp) as T
        }
    }

    private val microphone = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { model.microphoneAnswered(it) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        lifecycleScope.launch { model.calls.needCamera.collect { if (it) camera.launch(android.Manifest.permission.CAMERA) } }
        lifecycleScope.launch { model.needMicrophone.collect { if (it != null) microphone.launch(android.Manifest.permission.RECORD_AUDIO) } }
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        setContent {
            val light = dev.morpheus.app.ui.Palette.IsLight
            androidx.compose.runtime.LaunchedEffect(light) {
                val bars = if (light) SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT) else SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
                enableEdgeToEdge(statusBarStyle = bars, navigationBarStyle = bars)
            }
            MorpheusTheme { Root(model) }
        }
        if (savedInstanceState == null) dev.morpheus.app.push.Push.start(this)
        openRoomOf(intent)
    }

    private val camera = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { model.calls.cameraAnswered(it) }

    override fun onStop() {
        super.onStop()
        // The camera is for a call on screen: it stops when the app is left, unless picture-in-picture keeps the call in view.
        if (!isInPictureInPictureMode) model.calls.appLeft()
    }

    override fun onStart() {
        super.onStart()
        model.calls.appBack()
        model.ringModel.onScreen()
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
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
