package com.wavry.android.ui

import android.app.Application
import android.content.Context
import android.os.Build
import android.os.SystemClock
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.wavry.android.BuildConfig
import com.wavry.android.core.AuthApi
import com.wavry.android.core.CloudAuthSession
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
    val portText: String = "4444",
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
    val authServer: String = "https://auth.wavry.dev",
    val isAuthenticated: Boolean = false,
    val authEmail: String = "",
    val authUsername: String = "",
    val isAuthBusy: Boolean = false,
    val authStatusMessage: String = "",
    val authErrorMessage: String = "",
    val stats: SessionStats = SessionStats(),
)

class WavryViewModel(application: Application) : AndroidViewModel(application) {
    private val core = WavryCore(application.applicationContext)
    private val prefs = application.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    private var authToken: String = prefs.getString(KEY_AUTH_TOKEN, "") ?: ""

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
            authServer = prefs.getString(KEY_AUTH_SERVER, DEFAULT_AUTH_SERVER) ?: DEFAULT_AUTH_SERVER,
            isAuthenticated = authToken.isNotBlank(),
            authEmail = prefs.getString(KEY_AUTH_EMAIL, "") ?: "",
            authUsername = prefs.getString(KEY_AUTH_USERNAME, "") ?: "",
            authStatusMessage = if (authToken.isNotBlank()) "Signed in" else "",
        ),
    )
    val state: StateFlow<WavryUiState> = _state.asStateFlow()

    private var statsJob: Job? = null
    private var connectStartedAtMs: Long = 0L
    private var activeTargetLabel: String = ""
    private var activeResolvedHost: String = ""
    private var hasConnectedOnce = false
    private var lastCloudStatus = ""

    init {
        if (authToken.isNotBlank() && _state.value.connectivityMode == ConnectivityMode.WAVRY) {
            connectSignalingInBackground(_state.value.authServer, authToken)
        }
    }

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
        _state.update { it.copy(connectivityMode = value, authErrorMessage = "") }
        saveSetupFields(
            _state.value.displayName.trim().ifEmpty { defaultDisplayName() },
            value,
            _state.value.setupComplete,
        )
        if (value == ConnectivityMode.WAVRY && authToken.isNotBlank()) {
            connectSignalingInBackground(_state.value.authServer, authToken)
        }
    }

    fun setActiveTab(tab: AppTab) {
        _state.update { it.copy(activeTab = tab) }
    }

    fun setAuthServer(value: String) {
        _state.update {
            it.copy(
                authServer = value.trim(),
                authErrorMessage = "",
            )
        }
    }

    fun loginCloud(email: String, password: String) {
        val snapshot = _state.value
        if (snapshot.isAuthBusy) return
        val sanitizedEmail = email.trim()
        if (sanitizedEmail.isEmpty() || password.isBlank()) {
            _state.update { it.copy(authErrorMessage = "Email and password are required.") }
            return
        }

        val server = AuthApi.normalizeServer(snapshot.authServer)
        _state.update {
            it.copy(
                isAuthBusy = true,
                authServer = server,
                authErrorMessage = "",
                authStatusMessage = "Signing in...",
            )
        }

        viewModelScope.launch {
            try {
                val auth = AuthApi.login(server, sanitizedEmail, password)
                authToken = auth.token
                val signalingResult = connectSignalingInBackground(server, auth.token)
                saveAuthSession(auth, server)
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        isAuthenticated = true,
                        authEmail = auth.email,
                        authUsername = auth.username,
                        authStatusMessage = if (signalingResult == 0) {
                            "Signed in and cloud signaling connected."
                        } else {
                            "Signed in. Signaling connection may be unavailable."
                        },
                        authErrorMessage = if (signalingResult == 0) {
                            ""
                        } else {
                            core.describeError(signalingResult)
                        },
                    )
                }
            } catch (error: Exception) {
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        isAuthenticated = false,
                        authStatusMessage = "Sign-in failed",
                        authErrorMessage = error.message ?: "Authentication failed",
                    )
                }
            }
        }
    }

    fun registerCloud(email: String, username: String, password: String) {
        val snapshot = _state.value
        if (snapshot.isAuthBusy) return
        val sanitizedEmail = email.trim()
        val sanitizedUsername = username.trim()
        if (sanitizedEmail.isEmpty() || sanitizedUsername.isEmpty() || password.isBlank()) {
            _state.update {
                it.copy(authErrorMessage = "Email, username, and password are required.")
            }
            return
        }

        val publicKey = core.publicKeyHex()
        if (publicKey.isBlank()) {
            _state.update {
                it.copy(authErrorMessage = "Identity key unavailable. Restart app and try again.")
            }
            return
        }

        val server = AuthApi.normalizeServer(snapshot.authServer)
        _state.update {
            it.copy(
                isAuthBusy = true,
                authServer = server,
                authErrorMessage = "",
                authStatusMessage = "Creating account...",
            )
        }

        viewModelScope.launch {
            try {
                val display = snapshot.displayName.trim().ifEmpty { defaultDisplayName() }
                val auth = AuthApi.register(
                    serverBaseUrl = server,
                    email = sanitizedEmail,
                    password = password,
                    username = sanitizedUsername,
                    displayName = display,
                    publicKeyHex = publicKey,
                )
                authToken = auth.token
                val signalingResult = connectSignalingInBackground(server, auth.token)
                saveAuthSession(auth, server)
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        isAuthenticated = true,
                        authEmail = auth.email,
                        authUsername = auth.username,
                        authStatusMessage = if (signalingResult == 0) {
                            "Account created. Cloud signaling connected."
                        } else {
                            "Account created. Signaling connection may be unavailable."
                        },
                        authErrorMessage = if (signalingResult == 0) {
                            ""
                        } else {
                            core.describeError(signalingResult)
                        },
                    )
                }
            } catch (error: Exception) {
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        authStatusMessage = "Sign-up failed",
                        authErrorMessage = error.message ?: "Unable to create account",
                    )
                }
            }
        }
    }

    fun logoutCloud() {
        authToken = ""
        prefs.edit()
            .remove(KEY_AUTH_TOKEN)
            .remove(KEY_AUTH_EMAIL)
            .remove(KEY_AUTH_USERNAME)
            .apply()
        _state.update {
            it.copy(
                isAuthenticated = false,
                authEmail = "",
                authUsername = "",
                isAuthBusy = false,
                authStatusMessage = "Signed out",
                authErrorMessage = "",
            )
        }
    }

    fun requestCloudConnect(targetUsername: String) {
        val snapshot = _state.value
        if (!snapshot.isAuthenticated) {
            _state.update {
                it.copy(authErrorMessage = "Sign in first to request cloud connection.")
            }
            return
        }

        val target = targetUsername.trim()
        if (target.isEmpty()) {
            _state.update {
                it.copy(authErrorMessage = "Enter a target username.")
            }
            return
        }

        viewModelScope.launch {
            val rc = withContext(Dispatchers.IO) {
                core.sendConnectRequest(target)
            }
            if (rc == 0) {
                hasConnectedOnce = false
                connectStartedAtMs = SystemClock.elapsedRealtime()
                activeTargetLabel = "@$target"
                activeResolvedHost = ""
                lastCloudStatus = ""
                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = true,
                        statusMessage = "Waiting for @$target to acknowledge...",
                        authStatusMessage = "Cloud request sent to @$target.",
                        authErrorMessage = "",
                    )
                }
                startStatsPolling()
            } else {
                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = false,
                        statusMessage = "Cloud request failed",
                        authErrorMessage = core.describeError(rc),
                    )
                }
            }
        }
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
        val normalizedServer = AuthApi.normalizeServer(snapshot.authServer)
        saveSetupFields(snapshot.displayName.trim().ifEmpty { defaultDisplayName() }, snapshot.connectivityMode, snapshot.setupComplete)
        saveConnectionFields(snapshot.hostText.trim(), snapshot.portText)
        prefs.edit().putString(KEY_AUTH_SERVER, normalizedServer).apply()
        persistMode(snapshot.mode)
        _state.update {
            it.copy(
                statusMessage = "Settings saved",
                errorMessage = "",
                authServer = normalizedServer,
            )
        }

        if (snapshot.connectivityMode == ConnectivityMode.WAVRY && authToken.isNotBlank()) {
            connectSignalingInBackground(normalizedServer, authToken)
        }
    }

    fun start() {
        val snapshot = _state.value
        if (snapshot.isBusy || snapshot.isRunning) return

        if (snapshot.connectivityMode == ConnectivityMode.WAVRY && !snapshot.isAuthenticated) {
            _state.update {
                it.copy(
                    errorMessage = "Sign in to Wavry Cloud in Settings before using cloud mode.",
                )
            }
            return
        }

        if (
            snapshot.mode == ConnectionMode.CLIENT &&
                snapshot.connectivityMode == ConnectivityMode.WAVRY &&
                looksLikeCloudUsernameTarget(snapshot.hostText)
        ) {
            requestCloudConnect(snapshot.hostText)
            return
        }

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

            val startup = if (snapshot.mode == ConnectionMode.HOST) {
                val rc = withContext(Dispatchers.IO) { core.startHost(resolvedPort) }
                StartResult(
                    code = rc,
                    connectedPort = resolvedPort,
                    targetLabel = targetLabel,
                )
            } else {
                networkHintForHost(resolvedHost)?.let { hint ->
                    _state.update { current ->
                        current.copy(statusMessage = hint)
                    }
                }
                startClientWithFallback(resolvedHost, resolvedPort)
            }

            if (startup.code == 0) {
                hasConnectedOnce = false
                connectStartedAtMs = SystemClock.elapsedRealtime()
                activeTargetLabel = startup.targetLabel
                activeResolvedHost = resolvedHost
                lastCloudStatus = ""

                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = true,
                        statusMessage = if (snapshot.mode == ConnectionMode.HOST) {
                            "Hosting on ${startup.targetLabel}"
                        } else {
                            if (startup.usedFallback) {
                                "Connected on fallback port ${startup.connectedPort}"
                            } else {
                                "Connecting to ${startup.targetLabel}"
                            }
                        },
                        errorMessage = "",
                    )
                }
                saveConnectionFields(snapshot.hostText.trim(), startup.connectedPort.toString())
                startStatsPolling()
            } else {
                val detailed = startup.errorMessage.ifBlank { core.describeError(startup.code) }
                val hint = networkHintForHost(resolvedHost)
                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = false,
                        errorMessage = if (hint.isNullOrBlank()) detailed else "$detailed\n$hint",
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

            if (rc == 0 || (rc == -1 && !hasConnectedOnce && activeTargetLabel.startsWith("@"))) {
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
                        errorMessage = core.describeError(rc),
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
                    if (snapshot.connectivityMode == ConnectivityMode.WAVRY) {
                        val cloudStatus = withContext(Dispatchers.IO) {
                            core.lastCloudStatus()
                        }
                        if (cloudStatus.isNotBlank() && cloudStatus != lastCloudStatus) {
                            lastCloudStatus = cloudStatus
                            if (!stats.connected && isTerminalCloudFailure(cloudStatus)) {
                                val detail = withContext(Dispatchers.IO) {
                                    core.lastError()
                                }
                                withContext(Dispatchers.IO) {
                                    core.stop()
                                }
                                resetConnectionTracking()
                                val message = if (detail.isBlank()) cloudStatus else "$cloudStatus\n$detail"
                                _state.update {
                                    it.copy(
                                        isBusy = false,
                                        isRunning = false,
                                        statusMessage = "Connection failed",
                                        errorMessage = message,
                                        stats = SessionStats(),
                                    )
                                }
                                stopStatsPolling()
                                return@launch
                            }
                            if (!stats.connected) {
                                _state.update {
                                    it.copy(
                                        statusMessage = cloudStatus,
                                        errorMessage = "",
                                    )
                                }
                            }
                        }
                    }

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
                        val hint = networkHintForHost(activeResolvedHost)
                        val baseMessage = if (activeTargetLabel.startsWith("@")) {
                            "Cloud handshake timed out. Ensure the target user is online and hosting."
                        } else {
                            "Timed out connecting. Verify host IP/port and ensure desktop host is running."
                        }
                        val combined = if (hint.isNullOrBlank()) baseMessage else "$baseMessage\n$hint"
                        _state.update {
                            it.copy(
                                isBusy = false,
                                isRunning = false,
                                statusMessage = "Connection failed",
                                errorMessage = combined,
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
        activeResolvedHost = ""
        hasConnectedOnce = false
        lastCloudStatus = ""
    }

    private fun isTerminalCloudFailure(status: String): Boolean {
        val lower = status.lowercase()
        return lower.contains("rejected") ||
            lower.contains("failed") ||
            lower.contains("invalid")
    }

    private suspend fun startClientWithFallback(host: String, primaryPort: Int): StartResult {
        val primaryRc = withContext(Dispatchers.IO) {
            core.startClient(host, primaryPort)
        }
        if (primaryRc == 0) {
            return StartResult(
                code = 0,
                connectedPort = primaryPort,
                targetLabel = "$host:$primaryPort",
            )
        }

        val primaryError = core.describeError(primaryRc)
        val fallbackPort = suggestedFallbackPort(primaryPort)
        if (!shouldRetryPort(primaryRc, primaryError, fallbackPort)) {
            return StartResult(
                code = primaryRc,
                connectedPort = primaryPort,
                targetLabel = "$host:$primaryPort",
                errorMessage = primaryError,
            )
        }
        val retryPort = fallbackPort ?: primaryPort

        _state.update {
            it.copy(
                statusMessage = "No response on $host:$primaryPort. Retrying on $host:$retryPort...",
                errorMessage = "",
            )
        }

        val retryRc = withContext(Dispatchers.IO) {
            core.startClient(host, retryPort)
        }
        if (retryRc == 0) {
            return StartResult(
                code = 0,
                connectedPort = retryPort,
                targetLabel = "$host:$retryPort",
                usedFallback = true,
            )
        }

        val retryError = core.describeError(retryRc)
        val mergedError = buildString {
            append(primaryError)
            append('\n')
            append("Retried on port $retryPort and also failed.")
            append('\n')
            append(retryError)
        }
        return StartResult(
            code = retryRc,
            connectedPort = retryPort,
            targetLabel = "$host:$retryPort",
            usedFallback = true,
            errorMessage = mergedError,
        )
    }

    private fun suggestedFallbackPort(primaryPort: Int): Int? {
        return when (primaryPort) {
            4444 -> 8000
            8000 -> 4444
            else -> 4444
        }
    }

    private fun shouldRetryPort(code: Int, error: String, fallbackPort: Int?): Boolean {
        if (fallbackPort == null) return false
        if (code != -4 && code != -5) return false
        val lower = error.lowercase()
        return lower.contains("timed out") || lower.contains("startup failed")
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

    private fun networkHintForHost(ip: String): String? {
        val octets = parseIpv4(ip) ?: return null
        val first = octets[0]
        val second = octets[1]

        val isPrivateLan =
            first == 10 ||
                (first == 172 && second in 16..31) ||
                (first == 192 && second == 168)
        if (isPrivateLan) {
            return "Target is a private LAN address. Phone and host must be on the same LAN or VPN."
        }

        val isCarrierGradeNat = first == 100 && second in 64..127
        if (isCarrierGradeNat) {
            return "Target is in 100.64.0.0/10 (VPN/CGNAT range). Ensure phone and host are on the same VPN/tailnet and the host port is reachable."
        }

        return null
    }

    private fun parseIpv4(ip: String): List<Int>? {
        val parts = ip.split('.')
        if (parts.size != 4) return null
        val octets = parts.mapNotNull { it.toIntOrNull() }
        if (octets.size != 4 || octets.any { it !in 0..255 }) return null
        return octets
    }

    private fun looksLikeCloudUsernameTarget(value: String): Boolean {
        val trimmed = value.trim()
        if (trimmed.isEmpty()) return false
        if (trimmed.contains(' ') || trimmed.contains(':') || trimmed.contains('/')) return false
        if (trimmed.contains('.')) return false
        return trimmed.length in 3..32 && trimmed.all { ch ->
            ch.isLetterOrDigit() || ch == '_' || ch == '-'
        }
    }

    private fun connectSignalingInBackground(server: String, token: String): Int {
        val signalingUrl = AuthApi.signalingWsUrl(server)
        return core.connectSignaling(signalingUrl, token)
    }

    private fun saveAuthSession(auth: CloudAuthSession, server: String) {
        prefs.edit()
            .putString(KEY_AUTH_TOKEN, auth.token)
            .putString(KEY_AUTH_EMAIL, auth.email)
            .putString(KEY_AUTH_USERNAME, auth.username)
            .putString(KEY_AUTH_SERVER, server)
            .apply()
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
        private const val KEY_AUTH_SERVER = "auth_server"
        private const val KEY_AUTH_TOKEN = "auth_token"
        private const val KEY_AUTH_EMAIL = "auth_email"
        private const val KEY_AUTH_USERNAME = "auth_username"

        private const val DEFAULT_HOST = "192.168.1.10"
        private const val DEFAULT_PORT = "4444"
        private const val DEFAULT_AUTH_SERVER = "https://auth.wavry.dev"
        private const val CONNECT_TIMEOUT_MS = 12_000L
    }
}

private data class StartResult(
    val code: Int,
    val connectedPort: Int,
    val targetLabel: String,
    val usedFallback: Boolean = false,
    val errorMessage: String = "",
)
