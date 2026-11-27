package dev.morpheus.app

import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import uniffi.morpheus_ffi.Morpheus
import uniffi.morpheus_ffi.Sidebar
import uniffi.morpheus_ffi.SidebarListener
import uniffi.morpheus_ffi.SyncListener

// The Rust side pushes updates to a listener and hands back a TaskHandle; a
// callbackFlow turns that into a Flow that starts when collected and stops
// the Rust task when it is not collected any more (docs/ANDROID.md).

/** The sync state ("running", "offline", ...), now and whenever it changes. */
fun Morpheus.syncStates(): Flow<String> = callbackFlow {
    val handle = watchSync(object : SyncListener {
        override fun onSyncState(state: String) {
            trySend(state)
        }
    })
    awaitClose { handle.cancel() }
}

/** The channel list (servers, rooms, counts), now and whenever it changes. */
fun Morpheus.sidebars(): Flow<Sidebar> = callbackFlow {
    val handle = watchSidebar(object : SidebarListener {
        override fun onSidebar(sidebar: Sidebar) {
            trySend(sidebar)
        }
    })
    awaitClose { handle.cancel() }
}

/** What to tell a person about a failed call: the core's own sentence when it gave one. */
fun Throwable.sentence(): String = when (this) {
    is uniffi.morpheus_ffi.FfiException.Failed -> reason
    is uniffi.morpheus_ffi.FfiException.SignedOut -> "Not signed in"
    else -> message ?: toString()
}

/** The panel a verification shows, now and at each step. */
fun uniffi.morpheus_ffi.VerificationFlow.panels(): Flow<uniffi.morpheus_ffi.VerificationPanel> = callbackFlow {
    val handle = watch(object : uniffi.morpheus_ffi.VerificationListener {
        override fun onPanel(panel: uniffi.morpheus_ffi.VerificationPanel) {
            trySend(panel)
        }
    })
    awaitClose { handle.cancel() }
}

/** Verification requests from the account's other devices. */
fun Morpheus.incomingVerifications(): Flow<uniffi.morpheus_ffi.VerificationFlow> = callbackFlow {
    val handle = watchIncomingVerifications(object : uniffi.morpheus_ffi.IncomingVerificationListener {
        override fun onIncoming(flow: uniffi.morpheus_ffi.VerificationFlow) {
            trySend(flow)
        }
    })
    awaitClose { handle.cancel() }
}

/** The recent mentions (newest first), now and as new ones arrive. */
fun Morpheus.mentions(): Flow<List<uniffi.morpheus_ffi.MentionItem>> = callbackFlow {
    val handle = watchMentions(object : uniffi.morpheus_ffi.MentionListener {
        override fun onMentions(items: List<uniffi.morpheus_ffi.MentionItem>) {
            trySend(items)
        }
    })
    awaitClose { handle.cancel() }
}
