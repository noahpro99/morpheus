package dev.morpheus.app

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.Morpheus
import uniffi.morpheus_ffi.RecoveryKind
import uniffi.morpheus_ffi.SecurityStatus
import uniffi.morpheus_ffi.VerificationAction
import uniffi.morpheus_ffi.VerificationFlow
import uniffi.morpheus_ffi.VerificationPanel

/** Encryption on this device: its status, the recovery key and verifying with another device. */
class SecurityModel(private val morpheus: Morpheus, private val scope: CoroutineScope, private val onAsked: () -> Unit) {
    private val _status = MutableStateFlow<SecurityStatus?>(null)
    val status: StateFlow<SecurityStatus?> = _status.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    private val _busy = MutableStateFlow(false)
    val busy: StateFlow<Boolean> = _busy.asStateFlow()

    /** A recovery key just made, to be shown once. */
    private val _newKey = MutableStateFlow<String?>(null)
    val newKey: StateFlow<String?> = _newKey.asStateFlow()

    private val _flow = MutableStateFlow<VerificationFlow?>(null)
    val flow: StateFlow<VerificationFlow?> = _flow.asStateFlow()

    private val _panel = MutableStateFlow<VerificationPanel?>(null)
    val panel: StateFlow<VerificationPanel?> = _panel.asStateFlow()

    private var watching: Job? = null

    /** Whether the screen has anything to ask of the person. */
    val needsAttention: Boolean get() = _status.value?.let { !it.deviceVerified || it.recovery == RecoveryKind.NEEDS_KEY } ?: false

    init {
        scope.launch {
            while (true) {
                try {
                    _status.value = morpheus.securityStatus()
                } catch (_: Exception) {
                }
                delay(if (needsAttention) 3_000 else 30_000)
            }
        }
        scope.launch {
            try {
                morpheus.incomingVerifications().collect { show(it) }
            } catch (_: Exception) {
            }
        }
    }

    private suspend fun refresh() {
        _status.value = morpheus.securityStatus()
    }

    fun recover(key: String) {
        if (_busy.value) return
        _busy.value = true
        _error.value = null
        scope.launch {
            try {
                morpheus.recover(key.trim())
                refresh()
            } catch (e: Exception) {
                _error.value = e.sentence()
            } finally {
                _busy.value = false
            }
        }
    }

    fun setUpRecovery() {
        if (_busy.value) return
        _busy.value = true
        _error.value = null
        scope.launch {
            try {
                _newKey.value = morpheus.enableRecovery()
                refresh()
            } catch (e: Exception) {
                _error.value = e.sentence()
            } finally {
                _busy.value = false
            }
        }
    }

    fun keySaved() {
        _newKey.value = null
    }

    fun startVerification() {
        if (_flow.value != null) return
        _error.value = null
        scope.launch {
            try {
                show(morpheus.requestVerification())
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    private fun show(flow: VerificationFlow) {
        watching?.cancel()
        _flow.value = flow
        _panel.value = null
        onAsked()
        watching = scope.launch {
            try {
                flow.panels().collect { _panel.value = it }
            } catch (_: Exception) {
            }
        }
    }

    fun act(action: VerificationAction) {
        val flow = _flow.value ?: return
        if (action == VerificationAction.DISMISS) {
            closeFlow()
            return
        }
        scope.launch {
            try {
                flow.act(action)
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
            if (action == VerificationAction.CANCEL) closeFlow()
        }
    }

    private fun closeFlow() {
        watching?.cancel()
        _flow.value = null
        _panel.value = null
        scope.launch { refresh() }
    }
}
