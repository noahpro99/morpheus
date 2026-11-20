package dev.morpheus.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.ViewModel
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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        setContent {
            MorpheusTheme { Root(model) }
        }
        if (savedInstanceState == null) dev.morpheus.app.push.Push.start(this)
        openRoomOf(intent)
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        openRoomOf(intent)
    }

    /** A tap on a notification names the room to show. */
    private fun openRoomOf(intent: android.content.Intent?) {
        intent?.getStringExtra(dev.morpheus.app.push.Push.EXTRA_ROOM)?.let { model.openRoomWhenKnown(it) }
    }
}
