package com.wavry.android.ui

import android.app.Application
import android.content.Context
import android.os.Build
import android.os.SystemClock
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.wavry.android.BuildConfig
import com.wavry.android.core.SessionStats
import com.wavry.android.core.WavryCore
import java.net.Inet4Address
import java.net.InetAddress
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

enum class ConnectionMode {
    HOST,
    CLIENT,
}

enum class ConnectivityMode {
    WAVRY,
    DIRECT,
}

enum class AppTab {
    SESSION,
    SETTINGS,
}

data class WavryUiState(
    val mode: ConnectionMode = ConnectionMode.CLIENT,
    val hostText: String = "192.168.1.10",
    val portText: String = "8000",
    val isRunning: Boolean = false,
    val isBusy: Boolean = false,
    val statusMessage: String = "Ready",
    val errorMessage: String = "",
    val version: String = "",
    val isQuestBuild: Boolean = false,
    val supportsHost: Boolean = false,
    val setupComplete: Boolean = false,
    val displayName: String = "",
    val connectivityMode: ConnectivityMode = ConnectivityMode.DIRECT,
    val activeTab: AppTab = AppTab.SESSION,
    val stats: SessionStats = SessionStats(),
)

class WavryViewModel(application: Application) : AndroidViewModel(application) {
    private val core = WavryCore(application.applicationContext)
    private val prefs = application.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    private val _state = MutableStateFlow(
        WavryUiState(
            mode = loadMode(),
            hostText = prefs.getString(KEY_HOST, DEFAULT_HOST) ?: DEFAULT_HOST,
            portText = prefs.getString(KEY_PORT, DEFAULT_PORT) ?: DEFAULT_PORT,
            version = core.version(),
            statusMessage = "Native core: ${core.version()}",
            isQuestBuild = BuildConfig.IS_QUEST,
            supportsHost = BuildConfig.SUPPORTS_HOST,
            setupComplete = prefs.getBoolean(KEY_SETUP_COMPLETE, false),
            displayName = prefs.getString(KEY_DISPLAY_NAME, defaultDisplayName()) ?: defaultDisplayName(),
            connectivityMode = loadConnectivityMode(),
        ),
    )
    val state: StateFlow<WavryUiState> = _state.asStateFlow()

    private var statsJob: Job? = null
    private var connectStartedAtMs: Long = 0L
    private var activeTargetLabel: String = ""
    private var hasConnectedOnce = false

    fun setMode(mode: ConnectionMode) {
        if (mode == ConnectionMode.HOST && !_state.value.supportsHost) {
            _state.update { it.copy(errorMessage = "Hosting is not available on Android builds") }
            return
        }
        _state.update { it.copy(mode = mode, errorMessage = "") }
        persistMode(mode)
    }

    fun setHostText(value: String) {
        _state.update { it.copy(hostText = value, errorMessage = "") }
    }

    fun setPortText(value: String) {
        _state.update { it.copy(portText = value.filter { ch -> ch.isDigit() }.take(5), errorMessage = "") }
    }

    fun setDisplayName(value: String) {
        _state.update { it.copy(displayName = value.take(48), errorMessage = "") }
    }

    fun setConnectivityMode(value: ConnectivityMode) {
        _state.update { it.copy(connectivityMode = value) }
    }

    fun setActiveTab(tab: AppTab) {
        _state.update { it.copy(activeTab = tab) }
    }

    fun completeSetup(displayName: String, mode: ConnectivityMode) {
        val sanitizedName = displayName.trim().ifEmpty { defaultDisplayName() }
        _state.update {
            it.copy(
                setupComplete = true,
                displayName = sanitizedName,
                connectivityMode = mode,
                statusMessage = "Setup complete",
                errorMessage = "",
            )
        }
        saveSetupFields(sanitizedName, mode, true)
        saveConnectionFields(_state.value.hostText, _state.value.portText)
    }

    fun saveSettings() {
        val snapshot = _state.value
        val port = snapshot.portText.toIntOrNull()
        if (port == null || port !in 1..65535) {
            _state.update {
                it.copy(errorMessage = "Port must be between 1 and 65535")
            }
            return
        }
        saveSetupFields(snapshot.displayName.trim().ifEmpty { defaultDisplayName() }, snapshot.connectivityMode, snapshot.setupComplete)
        saveConnectionFields(snapshot.hostText.trim(), snapshot.portText)
        persistMode(snapshot.mode)
        _state.update {
            it.copy(
                statusMessage = "Settings saved",
                errorMessage = "",
            )
        }
    }

    fun start() {
        val snapshot = _state.value
        if (snapshot.isBusy || snapshot.isRunning) return

        val fallbackPort = snapshot.portText.toIntOrNull()
        if (fallbackPort == null || fallbackPort !in 1..65535) {
            _state.update {
                it.copy(errorMessage = "Port must be between 1 and 65535")
            }
            return
        }

        if (snapshot.mode == ConnectionMode.HOST && !snapshot.supportsHost) {
            _state.update {
                it.copy(errorMessage = "Hosting is currently disabled on Android")
            }
            return
        }

        _state.update {
            it.copy(
                isBusy = true,
                errorMessage = "",
                statusMessage = if (snapshot.mode == ConnectionMode.HOST) {
                    "Starting host..."
                } else {
                    "Resolving host..."
                },
            )
        }

        viewModelScope.launch {
            val (resolvedHost, resolvedPort, targetLabel) =
                if (snapshot.mode == ConnectionMode.CLIENT) {
                    when (val parsed = parseClientTarget(snapshot.hostText, fallbackPort)) {
                        null -> {
                            _state.update {
                                it.copy(
                                    isBusy = false,
                                    isRunning = false,
                                    errorMessage = "Host is required. Use IP or host:port.",
                                )
                            }
                            return@launch
                        }

                        else -> {
                            val resolved = withContext(Dispatchers.IO) {
                                resolveHost(parsed.first)
                            }
                            if (resolved == null) {
                                _state.update {
                                    it.copy(
                                        isBusy = false,
                                        isRunning = false,
                                        errorMessage = "Unable to resolve host '${parsed.first}'.",
                                    )
                                }
                                return@launch
                            }
                            Triple(resolved, parsed.second, "${parsed.first}:${parsed.second}")
                        }
                    }
                } else {
                    Triple("", fallbackPort, "UDP $fallbackPort")
                }

            val rc = withContext(Dispatchers.IO) {
                if (snapshot.mode == ConnectionMode.HOST) {
                    core.startHost(resolvedPort)
                } else {
                    core.startClient(resolvedHost, resolvedPort)
                }
            }

            if (rc == 0) {
                hasConnectedOnce = false
                connectStartedAtMs = SystemClock.elapsedRealtime()
                activeTargetLabel = targetLabel

                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = true,
                        statusMessage = if (snapshot.mode == ConnectionMode.HOST) {
                            "Hosting on $targetLabel"
                        } else {
                            "Connecting to $targetLabel"
                        },
                        errorMessage = "",
                    )
                }
                saveConnectionFields(snapshot.hostText.trim(), resolvedPort.toString())
                startStatsPolling()
            } else {
                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = false,
                        errorMessage = WavryCore.messageForCode(rc),
                    )
                }
            }
        }
    }

    fun stop() {
        val snapshot = _state.value
        if (snapshot.isBusy) return

        _state.update { it.copy(isBusy = true, errorMessage = "") }

        viewModelScope.launch {
            val rc = withContext(Dispatchers.IO) {
                core.stop()
            }

            if (rc == 0) {
                resetConnectionTracking()
                stopStatsPolling()
                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = false,
                        statusMessage = "Session stopped",
                        errorMessage = "",
                        stats = SessionStats(),
                    )
                }
            } else {
                _state.update {
                    it.copy(
                        isBusy = false,
                        errorMessage = WavryCore.messageForCode(rc),
                    )
                }
            }
        }
    }

    private fun startStatsPolling() {
        stopStatsPolling()
        statsJob = viewModelScope.launch {
            while (true) {
                val stats = withContext(Dispatchers.IO) {
                    core.stats()
                }

                val snapshot = _state.value
                if (snapshot.isRunning && snapshot.mode == ConnectionMode.CLIENT) {
                    val elapsedMs = SystemClock.elapsedRealtime() - connectStartedAtMs
                    if (stats.connected) {
                        if (!hasConnectedOnce) {
                            hasConnectedOnce = true
                            _state.update {
                                it.copy(
                                    statusMessage = "Connected to $activeTargetLabel",
                                    errorMessage = "",
                                )
                            }
                        }
                    } else if (!hasConnectedOnce && elapsedMs > CONNECT_TIMEOUT_MS) {
                        withContext(Dispatchers.IO) {
                            core.stop()
                        }
                        resetConnectionTracking()
                        _state.update {
                            it.copy(
                                isBusy = false,
                                isRunning = false,
                                statusMessage = "Connection failed",
                                errorMessage = "Timed out connecting. Verify host IP/port and ensure desktop host is running.",
                                stats = SessionStats(),
                            )
                        }
                        stopStatsPolling()
                        return@launch
                    }
                }

                _state.update {
                    it.copy(stats = stats)
                }
                delay(1000)
            }
        }
    }

    private fun stopStatsPolling() {
        statsJob?.cancel()
        statsJob = null
    }

    private fun resetConnectionTracking() {
        connectStartedAtMs = 0L
        activeTargetLabel = ""
        hasConnectedOnce = false
    }

    private fun parseClientTarget(hostInput: String, defaultPort: Int): Pair<String, Int>? {
        val trimmed = hostInput.trim()
        if (trimmed.isEmpty()) {
            return null
        }

        val colonCount = trimmed.count { it == ':' }
        if (colonCount > 1) {
            _state.update {
                it.copy(errorMessage = "IPv6 targets are not supported yet. Use IPv4 or hostname.")
            }
            return null
        }

        if (colonCount == 1) {
            val split = trimmed.split(':', limit = 2)
            val hostPart = split[0].trim()
            val portPart = split[1].trim()
            if (hostPart.isEmpty()) {
                _state.update { it.copy(errorMessage = "Host is required before ':'") }
                return null
            }
            val parsedPort = portPart.toIntOrNull()
            if (parsedPort == null || parsedPort !in 1..65535) {
                _state.update { it.copy(errorMessage = "Invalid port in host field") }
                return null
            }
            return hostPart to parsedPort
        }

        return trimmed to defaultPort
    }

    private fun resolveHost(host: String): String? {
        return try {
            val candidates = InetAddress.getAllByName(host)
            val preferred = candidates.firstOrNull { it is Inet4Address } ?: candidates.firstOrNull()
            preferred?.hostAddress
        } catch (_: Exception) {
            null
        }
    }

    private fun saveSetupFields(displayName: String, mode: ConnectivityMode, setupComplete: Boolean) {
        prefs.edit()
            .putString(KEY_DISPLAY_NAME, displayName)
            .putString(KEY_CONNECTIVITY_MODE, connectivityModeKey(mode))
            .putBoolean(KEY_SETUP_COMPLETE, setupComplete)
            .apply()
    }

    private fun saveConnectionFields(host: String, port: String) {
        prefs.edit()
            .putString(KEY_HOST, host.ifEmpty { DEFAULT_HOST })
            .putString(KEY_PORT, port.ifEmpty { DEFAULT_PORT })
            .apply()
    }

    private fun persistMode(mode: ConnectionMode) {
        prefs.edit()
            .putString(KEY_MODE, if (mode == ConnectionMode.HOST) "host" else "client")
            .apply()
    }

    private fun loadMode(): ConnectionMode {
        if (!BuildConfig.SUPPORTS_HOST) {
            return ConnectionMode.CLIENT
        }
        return when (prefs.getString(KEY_MODE, "client")) {
            "host" -> ConnectionMode.HOST
            else -> ConnectionMode.CLIENT
        }
    }

    private fun loadConnectivityMode(): ConnectivityMode {
        return when (prefs.getString(KEY_CONNECTIVITY_MODE, "direct")) {
            "wavry" -> ConnectivityMode.WAVRY
            else -> ConnectivityMode.DIRECT
        }
    }

    private fun connectivityModeKey(mode: ConnectivityMode): String {
        return when (mode) {
            ConnectivityMode.WAVRY -> "wavry"
            ConnectivityMode.DIRECT -> "direct"
        }
    }

    private fun defaultDisplayName(): String {
        return Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android Device"
    }

    override fun onCleared() {
        stopStatsPolling()
        super.onCleared()
    }

    companion object {
        private const val PREFS_NAME = "wavry_android_prefs"
        private const val KEY_HOST = "host"
        private const val KEY_PORT = "port"
        private const val KEY_MODE = "mode"
        private const val KEY_DISPLAY_NAME = "display_name"
        private const val KEY_CONNECTIVITY_MODE = "connectivity_mode"
        private const val KEY_SETUP_COMPLETE = "setup_complete"

        private const val DEFAULT_HOST = "192.168.1.10"
        private const val DEFAULT_PORT = "8000"
        private const val CONNECT_TIMEOUT_MS = 12_000L
    }
}
