package dev.morpheus.app

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.Account
import uniffi.morpheus_ffi.Morpheus
import uniffi.morpheus_ffi.RoomItem
import uniffi.morpheus_ffi.Sidebar

/** Which screen the app is on. */
sealed interface Screen {
    fun name(): String = when (this) {
        Loading -> "loading"
        SignedOut -> "signed-out"
        is SignedIn -> "signed-in"
    }

    /** Looking for the last session. */
    data object Loading : Screen

    data object SignedOut : Screen

    data class SignedIn(val account: Account) : Screen
}

/** Signing in, restoring and signing out; the rest of the app lives under [Screen.SignedIn]. */
class AppModel(val morpheus: Morpheus) : ViewModel() {
    private val _screen = MutableStateFlow<Screen>(Screen.Loading)
    val screen: StateFlow<Screen> = _screen.asStateFlow()

    /** The room that is open, if one is. */
    private val _room = MutableStateFlow<RoomItem?>(null)
    val openRoom: StateFlow<RoomItem?> = _room.asStateFlow()

    fun open(room: RoomItem) {
        _room.value = room
        AppState.screen = "room"
    }

    fun closeRoom() {
        _room.value = null
        AppState.screen = _screen.value.name()
    }

    private val _signingIn = MutableStateFlow(false)
    val signingIn: StateFlow<Boolean> = _signingIn.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    /** Which rail entry the channel list shows (`ScopeItem.key`). */
    private val _scopeKey = MutableStateFlow("rooms")
    val scopeKey: StateFlow<String> = _scopeKey.asStateFlow()

    fun selectScope(key: String) {
        _scopeKey.value = key
    }

    /** The channel list while signed in; the Rust side stops pushing when nobody collects. */
    @OptIn(ExperimentalCoroutinesApi::class)
    val sidebar: StateFlow<Sidebar?> = _screen
        .flatMapLatest { if (it is Screen.SignedIn) morpheus.sidebars() else emptyFlow() }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    init {
        _screen.onEach { AppState.screen = it.name() }.launchIn(viewModelScope)
        viewModelScope.launch {
            _screen.value = try {
                morpheus.restoreSession()?.let { Screen.SignedIn(it) } ?: Screen.SignedOut
            } catch (e: Exception) {
                _error.value = e.sentence()
                Screen.SignedOut
            }
        }
    }

    fun signIn(homeserver: String, username: String, password: String) {
        if (_signingIn.value) return
        _signingIn.value = true
        _error.value = null
        viewModelScope.launch {
            try {
                _screen.value = Screen.SignedIn(morpheus.login(homeserver.trim(), username.trim(), password))
            } catch (e: Exception) {
                _error.value = e.sentence()
            } finally {
                _signingIn.value = false
            }
        }
    }

    fun signOut() {
        // The screen goes first: its listeners stop with it, and the sign-in
        // screen is there while the server is told.
        _room.value = null
        _screen.value = Screen.SignedOut
        viewModelScope.launch {
            try {
                morpheus.logout()
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }
}

/** What the e2e bridge reports as the app's state (debug builds read it; nothing else does). */
object AppState {
    @Volatile
    var screen: String = "loading"
}
