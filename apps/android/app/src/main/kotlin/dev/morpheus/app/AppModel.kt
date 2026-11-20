package dev.morpheus.app

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.cancel
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
class AppModel(val morpheus: Morpheus, private val morpheusApp: MorpheusApp? = null) : ViewModel() {
    /** A room a notification asked for; opened once the channel list has it. */
    private val pendingRoom = MutableStateFlow<String?>(null)

    fun openRoomWhenKnown(roomId: String) {
        pendingRoom.value = roomId
    }

    private val _screen = MutableStateFlow<Screen>(Screen.Loading)
    val screen: StateFlow<Screen> = _screen.asStateFlow()

    /** The room that is open, if one is. */
    private val _room = MutableStateFlow<RoomItem?>(null)
    val openRoom: StateFlow<RoomItem?> = _room.asStateFlow()

    fun open(room: RoomItem) {
        _room.value = room
        AppState.screen = "room"
    }

    private val _newOpen = MutableStateFlow(false)
    val newOpen: StateFlow<Boolean> = _newOpen.asStateFlow()

    fun openNew() {
        _newOpen.value = true
        AppState.screen = "new"
    }

    fun closeNew() {
        _newOpen.value = false
        AppState.screen = _screen.value.name()
    }

    /** A room just made, joined or messaged: leave the New screen and open it once the list has it. */
    fun openNewRoom(roomId: String) {
        closeNew()
        openRoomWhenKnown(roomId)
    }

    fun answerInvite(roomId: String, accept: Boolean) {
        viewModelScope.launch {
            try {
                if (accept) morpheus.acceptInvite(roomId) else morpheus.declineInvite(roomId)
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    fun closeRoom() {
        _room.value = null
        AppState.screen = _screen.value.name()
    }

    /** Encryption: made at sign-in, stopped at sign-out. */
    private val _security = MutableStateFlow<SecurityModel?>(null)
    val security: StateFlow<SecurityModel?> = _security.asStateFlow()
    private var securityScope: kotlinx.coroutines.CoroutineScope? = null

    private val _securityOpen = MutableStateFlow(false)
    val securityOpen: StateFlow<Boolean> = _securityOpen.asStateFlow()

    fun openSecurity() {
        _securityOpen.value = true
        AppState.screen = "security"
    }

    fun closeSecurity() {
        _securityOpen.value = false
        AppState.screen = _screen.value.name()
    }

    private fun signedInChanged(screen: Screen) {
        securityScope?.cancel()
        _securityOpen.value = false
        if (screen is Screen.SignedIn) {
            val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.Main)
            securityScope = scope
            _security.value = SecurityModel(morpheus, scope) { openSecurity() }
        } else {
            securityScope = null
            _security.value = null
        }
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
        _screen.onEach { AppState.screen = it.name(); signedInChanged(it) }.launchIn(viewModelScope)
        kotlinx.coroutines.flow.combine(pendingRoom, sidebar) { id, bar -> id to bar }.onEach { (id, bar) ->
            val room = bar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == id }
            if (room != null) {
                pendingRoom.value = null
                open(room)
            }
        }.launchIn(viewModelScope)
        viewModelScope.launch {
            _screen.value = try {
                morpheus.restoreSession()?.let { morpheusApp?.registerPushNow(); Screen.SignedIn(it) } ?: Screen.SignedOut
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
                morpheusApp?.registerPushNow()
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
        _newOpen.value = false
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
