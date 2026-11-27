package dev.morpheus.app

import dev.morpheus.app.ui.t
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.first
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
        is Restoring -> "restoring"
        is SignedIn -> "signed-in"
    }

    /** Looking for the last session. */
    data object Loading : Screen

    data object SignedOut : Screen

    /** A launch with a session: the last state is drawn from the cache while the session is restored (#309). */
    data class Restoring(val account: Account) : Screen

    data class SignedIn(val account: Account) : Screen
}

/** Signing in, restoring and signing out; the rest of the app lives under [Screen.SignedIn]. */
class AppModel(val morpheus: Morpheus, private val morpheusApp: MorpheusApp? = null) : ViewModel() {
    init {
        People.morpheus = morpheus
    }

    /** The call this account is in, if any (it outlives the screens). */
    val calls: CallModel = CallModel(morpheus, morpheusApp ?: error("an application")).also { call ->
        call.isDm = { id -> sidebar.value?.scopes?.firstOrNull { it.kind == uniffi.morpheus_ffi.ScopeKind.DMS }?.sections?.any { s -> s.rooms.any { it.id == id } } == true }
    }

    /** Calls ringing this account (a card on screen; the phone's notification away). */
    val ringModel = RingModel(morpheusApp ?: error("an application"))
    private var ringWatch: uniffi.morpheus_ffi.TaskHandle? = null

    /** Answer a ring: the call joins, and the ring is done. */
    fun answerRing(ring: uniffi.morpheus_ffi.IncomingRing) {
        ringModel.done(ring)
        startCall(ring.roomId, ring.caller)
    }

    fun declineRing(ring: uniffi.morpheus_ffi.IncomingRing) {
        ringModel.done(ring)
        viewModelScope.launch {
            try {
                morpheus.declineCall(ring.roomId, ring.notification)
            } catch (_: Exception) {
            }
        }
    }

    /** A notification's Answer: the call joins once the account is signed in. */
    private val pendingAnswer = MutableStateFlow<Pair<String, String>?>(null)

    fun answerWhenSignedIn(roomId: String, name: String) {
        pendingAnswer.value = roomId to name
    }

    /** Asked of the activity: the microphone permission, before a call joins. */
    val needMicrophone = MutableStateFlow<Pair<String, String>?>(null)

    /** A person's name for the call lists: their ID's local part until the profile comes. */
    fun nameOf(userId: String, known: Map<String, String>): String = People.nameOf(userId, known)
    val callNames: StateFlow<Map<String, String>> = People.names

    fun wantName(userId: String) = People.want(userId)

    /** Join the call in a room: after the microphone is allowed (asked first when it is not). */
    fun startCall(roomId: String, roomName: String) {
        val app = morpheusApp ?: return
        if (androidx.core.content.ContextCompat.checkSelfPermission(app, android.Manifest.permission.RECORD_AUDIO) == android.content.pm.PackageManager.PERMISSION_GRANTED) {
            calls.join(roomId, roomName)
        } else {
            needMicrophone.value = roomId to roomName
        }
    }

    fun microphoneAnswered(granted: Boolean) {
        val (roomId, roomName) = needMicrophone.value ?: return
        needMicrophone.value = null
        if (granted) calls.join(roomId, roomName)
    }

    /** A room a notification asked for; opened once the channel list has it. */
    private val pendingRoom = MutableStateFlow<String?>(null)

    fun openRoomWhenKnown(roomId: String) {
        pendingRoom.value = roomId
    }

    /** What the app keeps between launches: who, the tab, the server and the room that were open. */
    private val kept = morpheusApp?.getSharedPreferences("ui-state", android.content.Context.MODE_PRIVATE)
    private fun keep(key: String, value: String?) { kept?.edit()?.apply { if (value == null) remove(key) else putString(key, value) }?.apply() }

    // (pages are above)
    private val _screen = MutableStateFlow<Screen>(Screen.Loading)
    val screen: StateFlow<Screen> = _screen.asStateFlow()

    /** Returns once the session is restored or signed in (a room opened from the cache waits for it). */
    suspend fun awaitSession() {
        _screen.first { it is Screen.SignedIn }
    }

    /** The room that is open, if one is. */
    private val _room = MutableStateFlow<RoomItem?>(null)
    val openRoom: StateFlow<RoomItem?> = _room.asStateFlow()

    /** Open a room; a locked one asks for its password first (a page), then opens through [openUnlocked]. */
    fun open(room: RoomItem) {
        if (morpheus.lockState(room.id).locked) {
            _page.value = PageRequest("unlock", room.id, room.name)
            return
        }
        openUnlocked(room)
    }

    fun openUnlocked(room: RoomItem) {
        slide.leaveSign = 0
        _room.value = room
        keep("room", room.id)
        rememberRecent(room.id)
        val record = uniffi.morpheus_ffi.lastChatsOpened(_lastChats.value, room.id, room.isDm, if (room.isDm) null else serverOf(room.id))
        keep("last-chats", record)
        _lastChats.value = record
        AppState.room = room.id
        AppState.screen = "room"
    }

    private val _page = MutableStateFlow<PageRequest?>(null)

    /** The phone page a sheet item opened (invite, create, edit, privacy, overview, reactions, report), drawn over the screen. */
    val page: StateFlow<PageRequest?> = _page.asStateFlow()

    fun openPage(request: PageRequest) {
        _page.value = request
    }

    fun closePage() {
        _page.value = null
    }

    private val _profile = MutableStateFlow<ProfileRequest?>(null)

    /** The profile sheet, if one is open: it opens at once with what is known and fills in as the card arrives (#488, #489). */
    val profile: StateFlow<ProfileRequest?> = _profile.asStateFlow()

    fun openProfile(userId: String, name: String, avatar: uniffi.morpheus_ffi.MediaHandle?, roomId: String? = null) {
        _profile.value = ProfileRequest(userId, name, avatar, roomId)
    }

    fun closeProfile() {
        _profile.value = null
    }

    private var sheetIds = 0
    private val _sheet = MutableStateFlow<SheetRequest?>(null)

    /** The long-press sheet, if one is open (every long press opens this one). */
    val sheet: StateFlow<SheetRequest?> = _sheet.asStateFlow()

    /**
     * Open the sheet with the rows `load` makes (the desktop's menu for the thing pressed). `onOutcome` is told what only
     * the screen can do (reply, edit, a reaction, delete). A tap that opens it (not a long press) has no haptic.
     */
    fun openSheet(
        title: String,
        longPress: Boolean = true,
        startFull: Boolean = false,
        quickReactions: List<String> = emptyList(),
        onOutcome: (uniffi.morpheus_ffi.MenuOutcome) -> Unit = {},
        load: suspend () -> List<uniffi.morpheus_ffi.MenuRow>,
    ) {
        viewModelScope.launch {
            val request = SheetRequest(++sheetIds, title, emptyList(), longPress, startFull, quickReactions, onOutcome, loading = true)
            _sheet.value = request
            try {
                request.rows = load()
                request.loading = false
            } catch (e: Exception) {
                if (_sheet.value === request) _sheet.value = null
                _error.value = e.sentence()
            }
        }
    }

    fun closeSheet() {
        _sheet.value = null
    }

    /** A quick reaction of a message sheet. */
    fun chooseReaction(key: String) {
        val request = _sheet.value ?: return
        closeSheet()
        request.onOutcome(uniffi.morpheus_ffi.MenuOutcome.React(key))
    }

    /** The row at `path` of the open sheet was chosen: do it here, or tell the screen. */
    fun chooseSheetItem(path: String) {
        val request = _sheet.value ?: return
        closeSheet()
        if (path == "reorder") { openReorder(); return }
        if (path.startsWith("reorder-channels:")) { openPage(PageRequest("channel-order", path.removePrefix("reorder-channels:"))); return }
        if (path == "select-messages") { request.onOutcome(uniffi.morpheus_ffi.MenuOutcome.Page("select-messages", "", "")); return }
        viewModelScope.launch {
            try {
                when (val outcome = morpheus.runMenuItem(path)) {
                    is uniffi.morpheus_ffi.MenuOutcome.Done -> refreshRail()
                    is uniffi.morpheus_ffi.MenuOutcome.Copy -> {
                        val clipboard = morpheusApp?.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as? android.content.ClipboardManager
                        clipboard?.setPrimaryClip(android.content.ClipData.newPlainText("", outcome.text))
                    }
                    is uniffi.morpheus_ffi.MenuOutcome.CopyImage -> {
                        val copied = morpheusApp?.copyImage(outcome.bytes, outcome.mime) == true
                        dev.morpheus.app.ui.Toasts.say(if (copied) t("Image copied") else t("Could not copy the image"), "image-copied")
                    }
                    is uniffi.morpheus_ffi.MenuOutcome.ShowTheme -> {
                        _themeCodeToTry.value = outcome.code
                        closeRoom() // Settings is under an open chat
                        openSettings()
                        openSettingsPage("Appearance")
                    }
                    is uniffi.morpheus_ffi.MenuOutcome.Leave -> leaveRoomUndoable(outcome.roomId, outcome.name)
                    is uniffi.morpheus_ffi.MenuOutcome.Switched -> accountChanged()
                    is uniffi.morpheus_ffi.MenuOutcome.AddAccount -> beginAddAccount()
                    is uniffi.morpheus_ffi.MenuOutcome.SignOut -> signOut()
                    is uniffi.morpheus_ffi.MenuOutcome.Page -> _page.value = PageRequest(outcome.kind, outcome.roomId, outcome.extra)
                    is uniffi.morpheus_ffi.MenuOutcome.NotYet -> _error.value = t("%{name} is not on the phone yet.", "name" to outcome.label)
                    else -> request.onOutcome(outcome)
                }
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    /** A theme code a message offered: Settings > Appearance has it ready to use (null once used or left). */
    private val _themeCodeToTry = MutableStateFlow<String?>(null)
    val themeCodeToTry: StateFlow<String?> = _themeCodeToTry.asStateFlow()

    fun themeCodeTried() {
        _themeCodeToTry.value = null
    }

    private val _settingsOpen = MutableStateFlow(false)
    val settingsOpen: StateFlow<Boolean> = _settingsOpen.asStateFlow()

    /** The Settings group open (`Account`, `Appearance`, ...), or null on the list of groups; the setting a search found. */
    private val _settingsPage = MutableStateFlow<String?>(null)
    val settingsPage: StateFlow<String?> = _settingsPage.asStateFlow()
    private val _settingsAnchor = MutableStateFlow<String?>(null)
    val settingsAnchor: StateFlow<String?> = _settingsAnchor.asStateFlow()

    fun openSettingsPage(group: String?, anchor: String? = null) {
        _settingsPage.value = group
        _settingsAnchor.value = anchor
    }

    fun openSettings() {
        _settingsPage.value = null
        _settingsOpen.value = true
        AppState.screen = "settings"
    }

    private val _bridgesOpen = MutableStateFlow(false)
    val bridgesOpen: StateFlow<Boolean> = _bridgesOpen.asStateFlow()

    fun openBridges() {
        _bridgesOpen.value = true
        AppState.screen = "bridges"
    }

    fun closeBridges() {
        _bridgesOpen.value = false
        AppState.screen = "settings"
    }

    fun closeSettings() {
        // Back from a group is the list of groups.
        if (_settingsPage.value != null && !_bridgesOpen.value) {
            _settingsPage.value = null
            return
        }
        _bridgesOpen.value = false
        _settingsOpen.value = false
        AppState.screen = _screen.value.name()
    }

    /** The look in use (a theme, or a custom colour); the palette it makes is what every screen draws with. */
    private val _appearance = MutableStateFlow(morpheus.appearance().also { dev.morpheus.app.ui.Palette.use(it.palette) })
    val appearance: StateFlow<uniffi.morpheus_ffi.Appearance> = _appearance.asStateFlow()

    private val _uiZoom = MutableStateFlow(morpheus.uiZoom())

    /** The zoom of the app's sizes and text (#542), kept on this device. */
    val uiZoom: StateFlow<uniffi.morpheus_ffi.UiZoom> = _uiZoom.asStateFlow()

    /** A step up (1), down (-1) or back to 100% (0). */
    fun stepZoom(direction: Int) { _uiZoom.value = morpheus.stepUiZoom(direction) }

    fun useAppearance(look: uniffi.morpheus_ffi.Appearance) {
        dev.morpheus.app.ui.Palette.use(look.palette)
        _appearance.value = look
    }

    /** The language picked; screens read it so their words change with it. */
    private val _language = MutableStateFlow(run { morpheus.startLanguage(); morpheus.startClock(); dev.morpheus.app.ui.Words.morpheus = morpheus; dev.morpheus.app.ui.Words.changed(); morpheus.languageChoice() })
    val language: StateFlow<String> = _language.asStateFlow()

    fun useLanguage(code: String) {
        morpheus.setLanguage(code)
        dev.morpheus.app.ui.Words.changed()
        _language.value = code
    }

    /** The words for `key` in the language in use. */
    fun t(key: String): String = morpheus.translate(key)

    private val _reorderOpen = MutableStateFlow(false)
    val reorderOpen: StateFlow<Boolean> = _reorderOpen.asStateFlow()

    fun openReorder() {
        _reorderOpen.value = true
        refreshRail()
        AppState.screen = "reorder-servers"
    }

    fun closeReorder() {
        _reorderOpen.value = false
        AppState.screen = _screen.value.name()
    }

    private val _railRows = MutableStateFlow<List<uniffi.morpheus_ffi.RailRowItem>>(emptyList())

    /** The rail as the account keeps it: servers and folders of servers, in order. */
    val railRows: StateFlow<List<uniffi.morpheus_ffi.RailRowItem>> = _railRows.asStateFlow()

    fun refreshRail() {
        viewModelScope.launch {
            try {
                _railRows.value = morpheus.railRows()
            } catch (_: Exception) {
            }
        }
    }

    /** Change the account's rail, then show what it is now. */
    private fun changeRail(change: suspend () -> Unit) {
        viewModelScope.launch {
            try {
                change()
                _railRows.value = morpheus.railRows()
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    fun moveToFolder(spaceId: String, folderId: String?) = changeRail { morpheus.moveServerToFolder(spaceId, folderId) }
    fun moveOutOfFolder(spaceId: String) = changeRail { morpheus.moveServerOutOfFolder(spaceId) }
    fun renameFolder(folderId: String, name: String) = changeRail { morpheus.renameFolder(folderId, name) }

    /** Put the rail's entries in this order (a server's ID, or `folder:<id>`) on every device. */
    fun setRailOrder(ids: List<String>) {
        viewModelScope.launch {
            try {
                morpheus.setRailOrder(ids)
                _railRows.value = morpheus.railRows()
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    /** The quick switcher (the bottom search bar of each list, #500, #455), over whatever is on screen. */
    private val _switcherOpen = MutableStateFlow(false)
    val switcherOpen: StateFlow<Boolean> = _switcherOpen.asStateFlow()

    fun openSwitcher() { _switcherOpen.value = true }

    fun closeSwitcher() { _switcherOpen.value = false }

    /** What the switcher shows for `query`, best first (the core's palette match, shared with the desktop's Ctrl+K). */
    fun switcherSearch(query: String): List<uniffi.morpheus_ffi.SwitcherHit> =
        morpheus.switcherSearch(query, recentRooms(), _room.value?.id, false)

    /** The rooms opened last, newest first (kept across launches). */
    private fun recentRooms(): List<String> = kept?.getString("recent-rooms", null).orEmpty().split('\n').filter { it.isNotEmpty() }

    private fun rememberRecent(roomId: String) {
        keep("recent-rooms", (listOf(roomId) + recentRooms().filter { it != roomId }).take(20).joinToString("\n"))
    }

    /** What a row of the switcher does. */
    fun pickSwitcherHit(hit: uniffi.morpheus_ffi.SwitcherHit) {
        closeSwitcher()
        when (hit.kind) {
            uniffi.morpheus_ffi.SwitcherKind.SERVER -> { selectScope("space:" + hit.id); selectTab(Tab.Servers) }
            uniffi.morpheus_ffi.SwitcherKind.CHANNEL, uniffi.morpheus_ffi.SwitcherKind.DM -> openRoomWhenKnown(hit.id)
            uniffi.morpheus_ffi.SwitcherKind.SETTING -> openSettings()
            uniffi.morpheus_ffi.SwitcherKind.ACTION -> when {
                hit.id == "Inbox" -> openInbox()
                hit.id.startsWith("Settings") -> openSettings()
                else -> openNew()
            }
        }
    }

    /** The inbox the bell opens: invites, and the rooms with news. */
    private val _inboxOpen = MutableStateFlow(false)
    val inboxOpen: StateFlow<Boolean> = _inboxOpen.asStateFlow()

    fun openInbox() {
        _inboxOpen.value = true
        AppState.screen = "inbox"
    }

    fun closeInbox() {
        _inboxOpen.value = false
        AppState.screen = _screen.value.name()
    }

    /** The first-time hint that the lists are a swipe apart (shown once, #500). */
    private val _swipeHint = MutableStateFlow(kept?.getString("swipe-hint-seen", null) == null)
    val swipeHint: StateFlow<Boolean> = _swipeHint.asStateFlow()

    fun dismissSwipeHint() {
        if (!_swipeHint.value) return
        keep("swipe-hint-seen", "1")
        _swipeHint.value = false
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

    /** Mark these rooms read (the open list's "mark all read"). */
    fun markRoomsRead(roomIds: List<String>) {
        viewModelScope.launch {
            try {
                morpheus.markRoomsRead(roomIds)
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    /** Pin a DM to the top, or close it until a new message (on every device). */
    fun setDmMark(roomId: String, kind: String, on: Boolean) {
        viewModelScope.launch {
            try {
                morpheus.setDmMark(roomId, kind, on)
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    /** Mute a room for `ms` milliseconds (0: until turned back on), or turn its notifications back on (null). */
    fun muteRoom(roomId: String, ms: ULong?) {
        viewModelScope.launch {
            try {
                if (ms == null) morpheus.unmuteRoom(roomId) else morpheus.muteRoom(roomId, ms)
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    /** Ignore someone (their messages fold away) or stop, on every device. */
    fun setIgnored(userId: String, ignored: Boolean) {
        People.ignored.value = if (ignored) People.ignored.value + userId else People.ignored.value - userId
        viewModelScope.launch {
            try {
                morpheus.setIgnored(userId, ignored)
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
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


    /** The open chat's place between the screen's edges (#464). */
    val slide = ChatSlide()

    /** What a swipe along `list` would do now (the last chats are kept across launches). */
    fun swipeStep(list: uniffi.morpheus_ffi.SwipeList, fingerLeft: Boolean): uniffi.morpheus_ffi.SwipeStep {
        val step = uniffi.morpheus_ffi.listSwipe(_lastChats.value, list, fingerLeft, _scopeKey.value)
        // A server nothing was opened in yet opens its default channel (#506), the desktop's pick (view-model default_channel).
        if (step is uniffi.morpheus_ffi.SwipeStep.Nothing && list == uniffi.morpheus_ffi.SwipeList.SERVERS && fingerLeft) {
            val server = sidebar.value?.scopes?.firstOrNull { it.key == _scopeKey.value && it.kind == uniffi.morpheus_ffi.ScopeKind.SERVER }
            val rooms = server?.sections?.flatMap { it.rooms }.orEmpty()
            uniffi.morpheus_ffi.defaultChannel(rooms.map { it.isVoice })?.let { return uniffi.morpheus_ffi.SwipeStep.Open(rooms[it.toInt()].id) }
        }
        return step
    }

    /** The rail entry (server) a channel belongs to: the one shown when it is in that, else the first that lists it (#499). */
    private fun serverOf(roomId: String): String {
        val servers = sidebar.value?.scopes.orEmpty().filter { it.kind != uniffi.morpheus_ffi.ScopeKind.DMS && it.sections.any { s -> s.rooms.any { r -> r.id == roomId } } }
        return servers.firstOrNull { it.key == _scopeKey.value }?.key ?: servers.firstOrNull()?.key ?: _scopeKey.value
    }

    /** The channel last opened in this server (its row is marked in the list). */
    fun lastChannelIn(server: String, record: String): String? = uniffi.morpheus_ffi.lastChatInServer(record, server)

    /** The chats opened last, as the view-model keeps them (a server's last channel is marked in its list; #499). */
    private val _lastChats = MutableStateFlow(kept?.getString("last-chats", null).orEmpty())
    val lastChats: StateFlow<String> = _lastChats.asStateFlow()

    /** The chat a drag along a list is bringing in opens under the finger now (it follows it); false when it did not open (a locked room asks first). */
    fun peekChat(roomId: String): Boolean {
        val bar = sidebar.value ?: return false
        val room = bar.scopes.flatMap { it.sections }.flatMap { it.rooms }.firstOrNull { it.id == roomId } ?: return false
        open(room)
        return _room.value?.id == roomId
    }

    fun closeRoom() {
        _room.value = null
        keep("room", null)
        AppState.room = ""
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
        ringWatch?.cancel()
        ringWatch = null
        if (screen is Screen.SignedIn) viewModelScope.launch {
            try {
                ringWatch = morpheus.watchRings(ringModel)
            } catch (_: Exception) {
            }
        }
        pendingAnswer.value?.let { (room, name) -> if (screen is Screen.SignedIn) { pendingAnswer.value = null; startCall(room, name) } }
        if (screen is Screen.SignedIn) viewModelScope.launch {
            try {
                People.ignored.value = morpheus.ignoredPeople().toSet()
            } catch (_: Exception) {
            }
        } else People.ignored.value = emptySet()
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
    private val _scopeKey = MutableStateFlow(kept?.getString("scope", null) ?: "rooms")
    val scopeKey: StateFlow<String> = _scopeKey.asStateFlow()

    fun selectScope(key: String) {
        keep("scope", key)
        _scopeKey.value = key
        if (AppState.welcome && key.startsWith("space:") && morpheus.takeWelcome(key.removePrefix("space:"))) openPage(PageRequest("overview", key.removePrefix("space:")))
    }

    /** The bottom bar's tab the signed-in home shows. */
    private val _tab = MutableStateFlow(kept?.getString("tab", null)?.let { name -> Tab.entries.firstOrNull { it.name == name } } ?: Tab.Servers)
    val tab: StateFlow<Tab> = _tab.asStateFlow()

    fun selectTab(tab: Tab) {
        dismissSwipeHint()
        keep("tab", tab.name)
        // The bar on a room's channels pane: a tab leaves the room for it.
        if (_room.value != null) closeRoom()
        _tab.value = tab
    }
    /** The list of the last run, read once: the first frame draws it (no skeleton for data that is kept; #317). */
    private val startBar: Sidebar? = if (kept?.getString("account", null)?.split("\n")?.size == 3) morpheus.cachedSidebar() else null

    /** The channel list while signed in; the Rust side stops pushing when nobody collects. */
    @OptIn(ExperimentalCoroutinesApi::class)
    private val rawSidebar: StateFlow<Sidebar?> = _screen
        .flatMapLatest {
            when (it) {
                is Screen.SignedIn -> morpheus.sidebars()
                // The list of the last run, until the session is back.
                is Screen.Restoring -> kotlinx.coroutines.flow.flow { startBar?.let { bar -> emit(bar) } }
                else -> emptyFlow()
            }
        }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), startBar)

    /** The messages that mentioned me since the app started, newest first (the Inbox's Mentions, #448). */
    @OptIn(ExperimentalCoroutinesApi::class)
    private val rawMentions: StateFlow<List<uniffi.morpheus_ffi.MentionItem>> = _screen
        .flatMapLatest { if (it is Screen.SignedIn) morpheus.mentions() else emptyFlow() }
        .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    private val seenMentions = MutableStateFlow<Set<String>>(emptySet())

    /** The mentions not opened yet, leaving out chats locked with a password. */
    val mentions: StateFlow<List<uniffi.morpheus_ffi.MentionItem>> = kotlinx.coroutines.flow.combine(rawMentions, seenMentions) { all, seen ->
        all.filter { it.eventId !in seen && !morpheus.lockState(it.roomId).locked }
    }.stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    /** Open the chat a mention is in; the mention is read. */
    fun openMention(item: uniffi.morpheus_ffi.MentionItem) {
        seenMentions.value += item.eventId
        closeInbox()
        openRoomWhenKnown(item.roomId)
    }

    private val _linkPreview = MutableStateFlow<uniffi.morpheus_ffi.LinkTarget?>(null)

    /** The room a Matrix link named that I am not in: its preview page reads it (#454). */
    val linkPreview: StateFlow<uniffi.morpheus_ffi.LinkTarget?> = _linkPreview.asStateFlow()

    /** Open a Matrix link (matrix.to or matrix:): a person, a chat I am in, or a preview of a room with Join or Ask to join. Returns false when `url` is not one. */
    fun openMatrixLink(url: String): Boolean {
        if (_screen.value !is Screen.SignedIn || !morpheus.isMatrixLink(url)) return false
        viewModelScope.launch {
            try {
                val target = morpheus.resolveLink(url)
                when (target.kind) {
                    "user" -> openProfile(target.id, target.id, null)
                    "chat" -> openNewRoom(morpheus.startDm(target.id))
                    "room" -> openRoomWhenKnown(target.id)
                    "preview" -> { _linkPreview.value = target; openPage(PageRequest("link-preview", target.id)) }
                }
            } catch (e: Exception) { _error.value = e.sentence() }
        }
        return true
    }

    /** Rooms just left, out of the lists while "Undo" is on offer (#415). */
    private val leftRooms = MutableStateFlow<Set<String>>(emptySet())

    val sidebar: StateFlow<Sidebar?> = kotlinx.coroutines.flow.combine(rawSidebar, leftRooms) { bar, left ->
        if (bar == null || left.isEmpty()) bar
        else bar.copy(scopes = bar.scopes.map { scope -> scope.copy(sections = scope.sections.map { section -> section.copy(rooms = section.rooms.filter { it.id !in left }) }) })
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), startBar)

    /** Leave a room with a few seconds to take it back: it goes from the lists now and is left when the toast has gone. */
    fun leaveRoomUndoable(roomId: String, name: String) {
        leftRooms.value = leftRooms.value + roomId
        if (_room.value?.id == roomId) closeRoom()
        dev.morpheus.app.ui.Toasts.undoable(
            t("Left #%{name}", "name" to name),
            onUndo = { leftRooms.value = leftRooms.value - roomId },
            onCommit = {
                viewModelScope.launch {
                    try { morpheus.leaveRoom(roomId) } catch (e: Exception) { _error.value = e.sentence() }
                    leftRooms.value = leftRooms.value - roomId
                }
            },
        )
    }

    /** A link that started the app or came while it was closed: opened once I am signed in. */
    fun openLinkWhenSignedIn(url: String) {
        viewModelScope.launch { _screen.first { it is Screen.SignedIn }; openMatrixLink(url) }
    }

    init {
        AppState.matrixLinks = { openMatrixLink(it) }
        _screen.onEach { AppState.screen = it.name(); if (it is Screen.SignedIn) { AppState.sessionBack(); loadOwnProfile(); startUpdateChecks(); Streamer.update(morpheus) }; signedInChanged(it) }.launchIn(viewModelScope)
        kotlinx.coroutines.flow.combine(pendingRoom, sidebar) { id, bar -> id to bar }.onEach { (id, bar) ->
            val room = bar?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == id }
            if (room != null) {
                pendingRoom.value = null
                open(room)
            }
        }.launchIn(viewModelScope)
        // Time passes for the password locks: an open one closes after the time away from it.
        viewModelScope.launch {
            while (true) {
                kotlinx.coroutines.delay(500)
                morpheus.lockTick(_room.value?.id)
            }
        }
        // A session and a list from the last run: draw them now, restore the session behind them.
        kept?.getString("account", null)?.split("\n")?.takeIf { it.size == 3 }?.let { (user, server, device) ->
            val bar = startBar ?: return@let
            AppState.mark("cached list read")
            _screen.value = Screen.Restoring(Account(userId = user, homeserver = server, deviceId = device))
            // The room that was open is drawn from its kept rows too (#313); it goes live once the session is back.
            kept?.getString("room", null)?.let { id ->
                bar.scopes.flatMap { it.sections }.flatMap { it.rooms }.firstOrNull { it.id == id }?.let { room ->
                    if (!morpheus.lockState(room.id).locked) {
                        _room.value = room
                    AppState.room = room.id
                    }
                }
            }
        }
        viewModelScope.launch {
            _screen.value = try {
                morpheus.restoreSession()?.let {
                    morpheusApp?.registerPushNow()
                    keep("account", listOf(it.userId, it.homeserver, it.deviceId).joinToString("\n"))
                    Screen.SignedIn(it)
                } ?: Screen.SignedOut
            } catch (e: Exception) {
                _error.value = e.sentence()
                Screen.SignedOut
            }
            // Where the last run ended: the room that was open comes back.
            if (_screen.value is Screen.SignedIn) kept?.getString("room", null)?.let { openRoomWhenKnown(it) } else _room.value = null
        }
    }

    fun signIn(homeserver: String, username: String, password: String) {
        if (_signingIn.value) return
        _signingIn.value = true
        _error.value = null
        viewModelScope.launch {
            try {
                val account = morpheus.login(homeserver.trim(), username.trim(), password)
                _adding.value = false
                keep("account", listOf(account.userId, account.homeserver, account.deviceId).joinToString("\n"))
                _screen.value = Screen.SignedIn(account)
                morpheusApp?.registerPushNow()
            } catch (e: Exception) {
                _error.value = e.sentence()
            } finally {
                _signingIn.value = false
            }
        }
    }

    /** Sign in through the homeserver's single sign-on page (#450): the browser opens it and comes back to the app. */
    fun signInSso(homeserver: String) {
        if (_signingIn.value) return
        _signingIn.value = true
        _error.value = null
        viewModelScope.launch {
            try {
                val opener = object : uniffi.morpheus_ffi.UrlLauncher {
                    override fun openUrl(url: String) = morpheusApp?.openLink(url) == null
                }
                val account = morpheus.loginSso(homeserver.trim(), opener)
                _adding.value = false
                keep("account", listOf(account.userId, account.homeserver, account.deviceId).joinToString("\n"))
                _screen.value = Screen.SignedIn(account)
                morpheusApp?.registerPushNow()
            } catch (e: Exception) {
                _error.value = e.sentence()
            } finally {
                _signingIn.value = false
            }
        }
    }

    private val _adding = MutableStateFlow(false)

    /** The sign-in screen is up to add another account, over the signed-in one (back cancels it). */
    val addingAccount: StateFlow<Boolean> = _adding.asStateFlow()

    fun beginAddAccount() { _adding.value = true }

    fun cancelAddAccount() { _adding.value = false }

    private val _own = MutableStateFlow<uniffi.morpheus_ffi.OwnProfile?>(null)

    /** Who is signed in, as they show (name and picture). */
    val ownProfile: StateFlow<uniffi.morpheus_ffi.OwnProfile?> = _own.asStateFlow()

    private val _ownKey = MutableStateFlow(kept?.getString("own-avatar-key", null))

    /** The key of my picture, kept: it is drawn from the disk at once at the next launch. */
    val ownAvatarKey: StateFlow<String?> = _ownKey.asStateFlow()

    private val _status = MutableStateFlow("")

    /** Keep a note about someone (also when the card that held it has gone). */
    fun saveNote(userId: String, note: String) {
        viewModelScope.launch {
            try { morpheus.setNote(userId, note) } catch (e: Exception) { _error.value = e.sentence() }
        }
    }

    private val _nicknameVersion = MutableStateFlow(0)

    /** Counts the nickname changes made here: lists read once (the members) read again on it. */
    val nicknameVersion: StateFlow<Int> = _nicknameVersion.asStateFlow()

    fun nicknamesChanged() {
        _nicknameVersion.value += 1
    }

    /** My status message (empty for none). */
    val ownStatus: StateFlow<String> = _status.asStateFlow()

    fun refreshStatus() {
        viewModelScope.launch {
            try { _status.value = morpheus.ownStatus() } catch (_: Exception) {}
        }
    }

    private val _update = MutableStateFlow<uniffi.morpheus_ffi.UpdateOffer?>(null)

    /** A newer release than this one, when the release feed names one (#447). */
    val update: StateFlow<uniffi.morpheus_ffi.UpdateOffer?> = _update.asStateFlow()

    private var updateChecks: kotlinx.coroutines.Job? = null

    /** Ask the release feed now and then (every six hours while the app is signed in). */
    private fun startUpdateChecks() {
        AppState.recheckUpdate = { viewModelScope.launch { checkForUpdate() } }
        updateChecks?.cancel()
        updateChecks = viewModelScope.launch {
            while (true) {
                checkForUpdate()
                kotlinx.coroutines.delay(6 * 60 * 60 * 1000L)
            }
        }
    }

    /** Build the bug report page (the diagnostics filled in) and hand it to `open`. */
    fun reportBug(os: String, device: String, open: (String) -> Unit) {
        viewModelScope.launch { try { open(morpheus.bugReportUrl(os, device)) } catch (e: Exception) { _error.value = e.sentence() } }
    }

    /** Ask the release feed whether a newer release is out. */
    suspend fun checkForUpdate() {
        try { _update.value = morpheus.checkForUpdate() } catch (_: Exception) {}
    }

    /** Read my name and picture again (after they were changed). */
    fun refreshOwnProfile() = loadOwnProfile()

    private fun loadOwnProfile() {
        viewModelScope.launch {
            try {
                val profile = morpheus.ownProfile()
                _own.value = profile
                _ownKey.value = profile.avatarKey
                keep("own-avatar-key", profile.avatarKey)
            } catch (_: Exception) {
            }
        }
    }

    /** Another account is the signed-in one now (the ffi switched): start from its lists. */
    fun accountChanged() {
        val account = morpheus.account() ?: return
        _room.value = null
        _newOpen.value = false
        AppState.room = ""
        _own.value = null
        _ownKey.value = null
        listOf("room", "last-chats", "scope", "own-avatar-key").forEach { keep(it, null) }
        keep("account", listOf(account.userId, account.homeserver, account.deviceId).joinToString("\n"))
        _screen.value = Screen.SignedIn(account)
        viewModelScope.launch { morpheusApp?.registerPushNow() }
    }

    /** The next stored account, after this one (a double tap on You); nothing when there is only this one. */
    fun nextAccount() {
        viewModelScope.launch {
            try {
                val accounts = morpheus.savedAccounts()
                if (accounts.size < 2) return@launch
                val at = accounts.indexOfFirst { it.active }
                morpheus.switchAccount(accounts[(at + 1) % accounts.size].key)
                accountChanged()
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    fun signOut() {
        // The screen goes first: its listeners stop with it, and the sign-in
        // screen is there while the server is told.
        _newOpen.value = false
        _room.value = null
        AppState.room = ""
        _screen.value = Screen.SignedOut
        listOf("account", "room", "last-chats", "scope", "tab").forEach { keep(it, null) }
        viewModelScope.launch {
            try {
                morpheus.logout()
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }
}

/** What the account says about people, for every screen: who is ignored. */
object People {
    val ignored = MutableStateFlow<Set<String>>(emptySet())

    /** People's names as the profiles say them, fetched once each (the screens show the ID's local part until then). */
    val names = MutableStateFlow<Map<String, String>>(emptyMap())

    /** Their pictures (a person with none is not in the map), fetched with the name. */
    val avatars = MutableStateFlow<Map<String, uniffi.morpheus_ffi.MediaHandle>>(emptyMap())
    private val asked = HashSet<String>()
    var morpheus: Morpheus? = null
    private val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.Main)

    fun nameOf(userId: String, known: Map<String, String>): String = known[userId] ?: userId.trimStart('@').substringBefore(':')

    /** Ask for someone's name, once. */
    fun want(userId: String) {
        val app = morpheus ?: return
        if (!asked.add(userId)) return
        scope.launch {
            names.value = names.value + (userId to app.displayName(userId))
            app.personAvatar(userId)?.let { avatars.value = avatars.value + (userId to it) }
        }
    }
}

/** What the e2e bridge reports as the app's state (debug builds read it; nothing else does). */
object AppState {
    /** Opens a Matrix link in the app (set by the model; `Context.openLink` asks it first). */
    @Volatile
    var matrixLinks: ((String) -> Boolean)? = null

    /** Asks the release feed again now (the debug bridge calls it after pointing the feed at a test server). */
    @Volatile
    var recheckUpdate: (() -> Unit)? = null

    /** A server's overview shows the first time it is opened (#429); the debug bridge turns it off for the other tests. */
    @Volatile
    var welcome: Boolean = true

    /** A stand-in for the homeserver's account page address (the debug bridge sets it; null means ask the homeserver). */
    @Volatile
    var manageUrl: String? = null

    /** The app is in picture-in-picture (the debug bridge reads it). */
    @Volatile
    var pip: Boolean = false

    /** A video of the call is being watched, so leaving the app goes into picture-in-picture (#529; the debug bridge reads it). */
    @Volatile
    var pipArmed: Boolean = false

    /** Pictures of the phone's screen handed to the call since the app started (#432; the debug bridge reads it). */
    @Volatile
    var screenFrames: Long = 0

    /** What the debug bridge adds to the clock the room screen reads, to see a day roll over (0 otherwise). */
    @Volatile
    var clockOffsetMs: Long = 0L

    /** What the debug bridge adds to every request for older messages, to see the list while one is slow (0 otherwise). */
    @Volatile
    var olderDelayMs: Long = 0L

    /** What the debug bridge adds before a profile card is fetched, to see the sheet open while it loads (0 otherwise; #489). */
    @Volatile
    var profileDelayMs: Long = 0L

    @Volatile
    var screen: String = "loading"

    /** The Material 3 colour scheme in use as (role, fill, foreground) ARGB, and where it came from (`palette` or `dynamic`): the debug bridge reports them. */
    @Volatile
    var colourPairs: List<Triple<String, Int, Int>> = emptyList()

    @Volatile
    var colourSource: String = "palette"

    /** The last web link opened from a message (the debug bridge reports it). */
    @Volatile
    var lastLink: String = ""

    /** The call screen keeps the display on (video is being watched; the debug bridge reports it). */
    @Volatile
    var keepScreenOn: Boolean = false

    /** The last text a button copied for sharing (an invite link; the debug bridge reports it). */
    @Volatile
    var lastCopy: String = ""

    /** Times the keyboard was asked for and put away by a swipe (the debug bridge reports them; #344). */
    @Volatile
    var imeShown: Int = 0

    @Volatile
    var imeHidden: Int = 0

    /** When (uptime ms) the finger last left the room pager, and when the keyboard was last asked for and put away: the debug bridge reports the lags (#379). */
    @Volatile
    var imeLiftAt: Long = 0

    @Volatile
    var imeShownAt: Long = 0

    @Volatile
    var imeHiddenAt: Long = 0

    /** Where the room pane (1) and the lists pane (0) were at each layout while they slid (the debug bridge reports it; #333). */
    val slideLog = java.util.concurrent.ConcurrentLinkedQueue<Pair<Int, Float>>()

    fun slide(pane: Int, x: Float) {
        slideLog.add(pane to x)
        while (slideLog.size > 300) slideLog.poll()
    }

    /** Pictures fetched over the network (not found on the disk), and avatars drawn (the debug bridge reports both; #334). */
    @Volatile
    var imageFetches: Int = 0

    /** Custom emoji pictures drawn so far (the e2e checks a message's emoji came out as pictures). */
    @Volatile
    var emotesDrawn: Int = 0

    @Volatile
    var avatarsDrawn: Int = 0

    /** Long-press haptics fired (the debug bridge reports it; #325). */
    @Volatile
    var haptics: Int = 0

    /** What each haptic was (tap, commit, tick, grab, long_press), in order (the debug bridge reports it; #485). */
    val hapticLog: MutableList<String> = java.util.Collections.synchronizedList(ArrayList())

    /** When the debug bridge's last touch went down, and when a long press opened a sheet (uptime ms; #530). */
    @Volatile
    var touchDownAt: Long = 0

    @Volatile
    var sheetAt: Long = 0

    /** The room on screen (the debug bridge reports it). */
    @Volatile
    var room: String = ""

    /** A spinner or skeleton was drawn since the process started (the debug bridge reports it; #309). */
    @Volatile
    var loadingSeen: Boolean = false

    /** Milliseconds from the process start to the first channel list drawn with content (0: none yet). */
    @Volatile
    var firstListMs: Long = 0

    /** Milliseconds from the process start to the first room drawn with messages (0: none yet; #313). */
    @Volatile
    var firstRoomMs: Long = 0

    /** Milliseconds from the process start to the session being restored (0: not yet). */
    @Volatile
    var sessionMs: Long = 0

    fun roomDrawn() {
        if (firstRoomMs == 0L) firstRoomMs = android.os.SystemClock.elapsedRealtime() - android.os.Process.getStartElapsedRealtime()
    }

    fun sessionBack() {
        if (sessionMs == 0L) { sessionMs = android.os.SystemClock.elapsedRealtime() - android.os.Process.getStartElapsedRealtime(); mark("session restored") }
    }

    fun listDrawn() {
        if (firstListMs == 0L) { firstListMs = android.os.SystemClock.elapsedRealtime() - android.os.Process.getStartElapsedRealtime(); mark("first list drawn") }
    }

    /** Named points of the start, in ms from the process start (the debug bridge reports them; #317). */
    val marks = java.util.concurrent.ConcurrentLinkedQueue<Pair<String, Long>>()

    fun mark(name: String) {
        if (marks.none { it.first == name }) marks.add(name to android.os.SystemClock.elapsedRealtime() - android.os.Process.getStartElapsedRealtime())
    }
}

/** The three places of the bottom bar. */
enum class Tab { Servers, Dms, Profile }


/** An open long-press sheet: its rows and what to do with what only the screen can. */
/** Whose profile sheet is open: what is known before the card is fetched. */
class ProfileRequest(val userId: String, val name: String, val avatar: uniffi.morpheus_ffi.MediaHandle?, val roomId: String?)

class SheetRequest(
    val id: Int,
    val title: String,
    rows: List<uniffi.morpheus_ffi.MenuRow>,
    val longPress: Boolean,
    val startFull: Boolean,
    val quickReactions: List<String>,
    val onOutcome: (uniffi.morpheus_ffi.MenuOutcome) -> Unit,
    loading: Boolean = false,
) {
    /** The rows come after the sheet is up (the menu may ask the server: the sheet opens at the long-press timeout, #530). */
    var rows by androidx.compose.runtime.mutableStateOf(rows)
    var loading by androidx.compose.runtime.mutableStateOf(loading)
}

/** A page of the phone for a sheet item: what kind, which room (or server, or folder), a name where it matters, the message it is about. */
class PageRequest(val kind: String, val roomId: String, val extra: String = "", val message: uniffi.morpheus_ffi.MessageItem? = null)

/**
 * Where the open chat is between the screen's edges (#464): 1 covers the screen, 0 is off it at the chat's edge (`view_model::list_swipes::chat_edge`:
 * a DM at the left, a channel at the right). A finger drag sets it while the finger is down (`dragging`); otherwise Root animates it to where the
 * chat is (open or not).
 */
class ChatSlide {
    /** Which edge a chat that a drag is sending off goes to: -1 left, 1 right, 0 its own (a DM left, a channel right; #469). */
    var leaveSign by androidx.compose.runtime.mutableIntStateOf(0)
    val f = androidx.compose.animation.core.Animatable(0f)
    var dragging by androidx.compose.runtime.mutableStateOf(false)
}
