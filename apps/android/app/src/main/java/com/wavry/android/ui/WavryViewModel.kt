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
import java.net.URL
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

enum class AuthFormMode {
    LOGIN,
    REGISTER,
}

enum class CloudSignalingState {
    DISCONNECTED,
    CONNECTING,
    CONNECTED,
    ERROR,
}

private enum class SignalingRefreshReason {
    RESTORE,
    MODE_SWITCH,
    SETTINGS_SAVE,
    MANUAL_RETRY,
}

data class SessionEntry(
    val id: String = java.util.UUID.randomUUID().toString(),
    val target: String,
    val timestamp: Long = System.currentTimeMillis(),
    val durationMs: Long = 0,
    val success: Boolean = true,
)

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
    val authFormMode: AuthFormMode = AuthFormMode.LOGIN,
    val cloudSignalingState: CloudSignalingState = CloudSignalingState.DISCONNECTED,
    val stats: SessionStats = SessionStats(),
    val sessionHistory: List<SessionEntry> = emptyList(),
)

class WavryViewModel(application: Application) : AndroidViewModel(application) {
    private val core = WavryCore(application.applicationContext)
    private val prefs = application.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    private var authToken: String = prefs.getString(KEY_AUTH_TOKEN, "") ?: ""
    private val initialConnectivityMode = loadConnectivityMode()

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
            connectivityMode = initialConnectivityMode,
            authServer = prefs.getString(KEY_AUTH_SERVER, DEFAULT_AUTH_SERVER) ?: DEFAULT_AUTH_SERVER,
            isAuthenticated = authToken.isNotBlank(),
            authEmail = prefs.getString(KEY_AUTH_EMAIL, "") ?: "",
            authUsername = prefs.getString(KEY_AUTH_USERNAME, "") ?: "",
            authStatusMessage = if (authToken.isNotBlank()) "Signed in" else "",
            cloudSignalingState = if (authToken.isNotBlank() && initialConnectivityMode == ConnectivityMode.WAVRY) {
                CloudSignalingState.CONNECTING
            } else {
                CloudSignalingState.DISCONNECTED
            },
            sessionHistory = loadSessionHistory(),
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
            refreshCloudSignaling(
                server = _state.value.authServer,
                token = authToken,
                reason = SignalingRefreshReason.RESTORE,
            )
        }
    }

    fun setMode(mode: ConnectionMode) {
        if (mode == ConnectionMode.HOST && !_state.value.supportsHost) {
            _state.update { it.copy(errorMessage = "Hosting is not available on Android builds") }
            return
        }
        _state.update { current ->
            val normalizeClientPort = mode == ConnectionMode.CLIENT && current.portText == "0"
            current.copy(
                mode = mode,
                portText = if (normalizeClientPort) DEFAULT_PORT else current.portText,
                statusMessage = if (normalizeClientPort) {
                    "Client mode uses explicit remote ports. Port reset to $DEFAULT_PORT."
                } else {
                    current.statusMessage
                },
                errorMessage = "",
            )
        }
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
        _state.update {
            it.copy(
                connectivityMode = value,
                authStatusMessage = if (value == ConnectivityMode.WAVRY && !it.isAuthenticated) {
                    "Cloud mode enabled. Sign in for username connect, or use IP/host for direct fallback."
                } else if (value == ConnectivityMode.DIRECT) {
                    "Direct mode enabled."
                } else {
                    it.authStatusMessage
                },
                authErrorMessage = if (value == ConnectivityMode.DIRECT) "" else it.authErrorMessage,
                cloudSignalingState = if (value == ConnectivityMode.WAVRY && it.isAuthenticated) {
                    CloudSignalingState.CONNECTING
                } else {
                    CloudSignalingState.DISCONNECTED
                },
            )
        }
        saveSetupFields(
            _state.value.displayName.trim().ifEmpty { defaultDisplayName() },
            value,
            _state.value.setupComplete,
        )
        if (value == ConnectivityMode.WAVRY && authToken.isNotBlank()) {
            refreshCloudSignaling(
                server = _state.value.authServer,
                token = authToken,
                reason = SignalingRefreshReason.MODE_SWITCH,
            )
        }
    }

    fun setActiveTab(tab: AppTab) {
        _state.update { it.copy(activeTab = tab) }
    }

    fun setAuthFormMode(mode: AuthFormMode) {
        _state.update { it.copy(authFormMode = mode, authErrorMessage = "") }
    }

    fun openCloudAuth(mode: AuthFormMode) {
        _state.update {
            it.copy(
                activeTab = AppTab.SETTINGS,
                authFormMode = mode,
                authStatusMessage = if (mode == AuthFormMode.REGISTER) {
                    "Create an account to connect by username."
                } else {
                    "Sign in to enable cloud username connect."
                },
                authErrorMessage = "",
            )
        }
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
        val wantsCloudSignaling = snapshot.connectivityMode == ConnectivityMode.WAVRY
        if (sanitizedEmail.isEmpty() || password.isBlank()) {
            _state.update { it.copy(authErrorMessage = "Email and password are required.") }
            return
        }
        if (!isValidEmail(sanitizedEmail)) {
            _state.update { it.copy(authErrorMessage = "Enter a valid email address.") }
            return
        }

        val server = AuthApi.normalizeServer(snapshot.authServer)
        if (!isValidAuthServer(server)) {
            _state.update {
                it.copy(authErrorMessage = "Auth server must start with http:// or https://.")
            }
            return
        }
        _state.update {
            it.copy(
                isAuthBusy = true,
                authServer = server,
                authErrorMessage = "",
                authStatusMessage = "Signing in...",
                authFormMode = AuthFormMode.LOGIN,
                cloudSignalingState = if (wantsCloudSignaling) {
                    CloudSignalingState.CONNECTING
                } else {
                    CloudSignalingState.DISCONNECTED
                },
            )
        }

        viewModelScope.launch {
            try {
                val auth = AuthApi.login(server, sanitizedEmail, password)
                authToken = auth.token
                val signalingResult = if (wantsCloudSignaling) {
                    connectSignalingWithRetry(server, auth.token)
                } else {
                    0
                }
                saveAuthSession(auth, server)
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        isAuthenticated = true,
                        authEmail = auth.email,
                        authUsername = auth.username,
                        authStatusMessage = if (!wantsCloudSignaling) {
                            "Signed in. Enable Cloud mode when you want username connect."
                        } else if (signalingResult == 0) {
                            "Signed in and cloud signaling connected."
                        } else {
                            "Signed in. Signaling connection may be unavailable."
                        },
                        authErrorMessage = if (!wantsCloudSignaling || signalingResult == 0) {
                            ""
                        } else {
                            normalizeCloudConnectError(core.describeError(signalingResult))
                        },
                        cloudSignalingState = if (!wantsCloudSignaling) {
                            CloudSignalingState.DISCONNECTED
                        } else if (signalingResult == 0) {
                            CloudSignalingState.CONNECTED
                        } else {
                            CloudSignalingState.ERROR
                        },
                    )
                }
            } catch (error: Exception) {
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        isAuthenticated = false,
                        authStatusMessage = "Sign-in failed",
                        authErrorMessage = normalizeAuthError(error),
                        cloudSignalingState = CloudSignalingState.DISCONNECTED,
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
        val wantsCloudSignaling = snapshot.connectivityMode == ConnectivityMode.WAVRY
        if (sanitizedEmail.isEmpty() || sanitizedUsername.isEmpty() || password.isBlank()) {
            _state.update {
                it.copy(authErrorMessage = "Email, username, and password are required.")
            }
            return
        }
        if (!isValidEmail(sanitizedEmail)) {
            _state.update { it.copy(authErrorMessage = "Enter a valid email address.") }
            return
        }
        if (!isValidCloudUsername(sanitizedUsername, allowDot = true)) {
            _state.update {
                it.copy(authErrorMessage = "Username must be 3-32 characters and use letters, numbers, ., _, or -.")
            }
            return
        }
        if (password.length < 8) {
            _state.update { it.copy(authErrorMessage = "Password must be at least 8 characters.") }
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
        if (!isValidAuthServer(server)) {
            _state.update {
                it.copy(authErrorMessage = "Auth server must start with http:// or https://.")
            }
            return
        }
        _state.update {
            it.copy(
                isAuthBusy = true,
                authServer = server,
                authErrorMessage = "",
                authStatusMessage = "Creating account...",
                authFormMode = AuthFormMode.REGISTER,
                cloudSignalingState = if (wantsCloudSignaling) {
                    CloudSignalingState.CONNECTING
                } else {
                    CloudSignalingState.DISCONNECTED
                },
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
                val signalingResult = if (wantsCloudSignaling) {
                    connectSignalingWithRetry(server, auth.token)
                } else {
                    0
                }
                saveAuthSession(auth, server)
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        isAuthenticated = true,
                        authEmail = auth.email,
                        authUsername = auth.username,
                        authStatusMessage = if (!wantsCloudSignaling) {
                            "Account created. Enable Cloud mode when you want username connect."
                        } else if (signalingResult == 0) {
                            "Account created. Cloud signaling connected."
                        } else {
                            "Account created. Signaling connection may be unavailable."
                        },
                        authFormMode = AuthFormMode.LOGIN,
                        authErrorMessage = if (!wantsCloudSignaling || signalingResult == 0) {
                            ""
                        } else {
                            normalizeCloudConnectError(core.describeError(signalingResult))
                        },
                        cloudSignalingState = if (!wantsCloudSignaling) {
                            CloudSignalingState.DISCONNECTED
                        } else if (signalingResult == 0) {
                            CloudSignalingState.CONNECTED
                        } else {
                            CloudSignalingState.ERROR
                        },
                    )
                }
            } catch (error: Exception) {
                _state.update {
                    it.copy(
                        isAuthBusy = false,
                        authStatusMessage = "Sign-up failed",
                        authErrorMessage = normalizeAuthError(error),
                        cloudSignalingState = CloudSignalingState.DISCONNECTED,
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
                authFormMode = AuthFormMode.LOGIN,
                cloudSignalingState = CloudSignalingState.DISCONNECTED,
            )
        }
    }

    fun reconnectCloudSignaling() {
        val snapshot = _state.value
        if (snapshot.isAuthBusy || !snapshot.isAuthenticated || authToken.isBlank()) {
            return
        }
        refreshCloudSignaling(
            server = snapshot.authServer,
            token = authToken,
            reason = SignalingRefreshReason.MANUAL_RETRY,
        )
    }

    fun requestCloudConnect(targetUsername: String) {
        val snapshot = _state.value
        if (snapshot.isBusy || snapshot.isRunning) return
        if (!snapshot.isAuthenticated) {
            _state.update {
                it.copy(
                    activeTab = AppTab.SETTINGS,
                    authFormMode = AuthFormMode.LOGIN,
                    authErrorMessage = "Sign in first to request cloud connection.",
                )
            }
            return
        }

        val target = normalizeCloudTarget(targetUsername)
        if (target.isEmpty()) {
            _state.update {
                it.copy(errorMessage = "Enter a target username.")
            }
            return
        }
        if (!isValidCloudUsername(target, allowDot = true)) {
            _state.update {
                it.copy(errorMessage = "Cloud connect requires a username (3-32 chars, letters/numbers/., _, -).")
            }
            return
        }

        _state.update {
            it.copy(
                isBusy = true,
                errorMessage = "",
                authErrorMessage = "",
                statusMessage = "Sending cloud request to @$target...",
            )
        }

        viewModelScope.launch {
            if (_state.value.cloudSignalingState != CloudSignalingState.CONNECTED) {
                _state.update {
                    it.copy(
                        statusMessage = "Reconnecting cloud signaling...",
                        cloudSignalingState = CloudSignalingState.CONNECTING,
                    )
                }
                val signalingRc = connectSignalingWithRetry(_state.value.authServer, authToken)
                if (signalingRc != 0) {
                    _state.update {
                        it.copy(
                            isBusy = false,
                            isRunning = false,
                            statusMessage = "Cloud request failed",
                            errorMessage = normalizeCloudConnectError(core.describeError(signalingRc)),
                            cloudSignalingState = CloudSignalingState.ERROR,
                        )
                    }
                    return@launch
                }
                _state.update {
                    it.copy(
                        authErrorMessage = "",
                        cloudSignalingState = CloudSignalingState.CONNECTED,
                    )
                }
            }

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
                        statusMessage = "Cloud request sent to @$target. Waiting for response...",
                        authStatusMessage = "Cloud request sent to @$target",
                        authErrorMessage = "",
                        errorMessage = "",
                        cloudSignalingState = CloudSignalingState.CONNECTED,
                    )
                }
                startStatsPolling()
            } else {
                val detail = normalizeCloudConnectError(core.describeError(rc))
                _state.update {
                    it.copy(
                        isBusy = false,
                        isRunning = false,
                        statusMessage = "Cloud request failed",
                        authErrorMessage = "",
                        errorMessage = detail,
                        cloudSignalingState = CloudSignalingState.ERROR,
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
                statusMessage = if (mode == ConnectivityMode.WAVRY && !it.isAuthenticated) {
                    "Setup complete. Create an account to use cloud username connect."
                } else {
                    "Setup complete"
                },
                errorMessage = "",
                activeTab = if (mode == ConnectivityMode.WAVRY && !it.isAuthenticated) {
                    AppTab.SETTINGS
                } else {
                    AppTab.SESSION
                },
                authFormMode = if (mode == ConnectivityMode.WAVRY && !it.isAuthenticated) {
                    AuthFormMode.REGISTER
                } else {
                    it.authFormMode
                },
                authStatusMessage = if (mode == ConnectivityMode.WAVRY && !it.isAuthenticated) {
                    "Create an account or sign in to enable cloud connect."
                } else {
                    it.authStatusMessage
                },
                authErrorMessage = "",
            )
        }
        saveSetupFields(sanitizedName, mode, true)
        saveConnectionFields(_state.value.hostText, _state.value.portText)
    }

    fun saveSettings() {
        val snapshot = _state.value
        val port = snapshot.portText.toIntOrNull()
        if (port == null || port !in 0..65535) {
            _state.update {
                it.copy(errorMessage = "Port must be between 0 and 65535 (0 = random host port)")
            }
            return
        }
        val normalizedServer = AuthApi.normalizeServer(snapshot.authServer)
        if (!isValidAuthServer(normalizedServer)) {
            _state.update {
                it.copy(errorMessage = "Auth server URL must start with http:// or https://.")
            }
            return
        }
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
            refreshCloudSignaling(
                server = normalizedServer,
                token = authToken,
                reason = SignalingRefreshReason.SETTINGS_SAVE,
            )
        }
    }

    fun start() {
        val snapshot = _state.value
        if (snapshot.isBusy || snapshot.isRunning) return

        val wantsCloudUsernameConnect =
            snapshot.mode == ConnectionMode.CLIENT &&
                snapshot.connectivityMode == ConnectivityMode.WAVRY &&
                looksLikeCloudUsernameTarget(snapshot.hostText)

        if (wantsCloudUsernameConnect && !snapshot.isAuthenticated) {
            _state.update {
                it.copy(
                    activeTab = AppTab.SETTINGS,
                    authFormMode = AuthFormMode.LOGIN,
                    errorMessage = "Sign in first to connect by username, or enter host/IP for direct connect.",
                )
            }
            return
        }

        if (wantsCloudUsernameConnect) {
            requestCloudConnect(snapshot.hostText)
            return
        }

        val fallbackPort = snapshot.portText.toIntOrNull()
        if (fallbackPort == null) {
            _state.update { it.copy(errorMessage = "Port must be numeric") }
            return
        }
        if (snapshot.mode == ConnectionMode.HOST) {
            if (fallbackPort !in 0..65535) {
                _state.update {
                    it.copy(errorMessage = "Host port must be between 0 and 65535 (0 = random)")
                }
                return
            }
        } else if (fallbackPort !in 1..65535) {
            _state.update {
                it.copy(errorMessage = "Client port must be between 1 and 65535")
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
            val (resolvedHost, resolvedPort, _) =
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
                val runtimePort = if (rc == 0) {
                    val status = withContext(Dispatchers.IO) { core.lastCloudStatus() }
                    parseHostedPortFromStatus(status) ?: resolvedPort
                } else {
                    resolvedPort
                }
                StartResult(
                    code = rc,
                    connectedPort = runtimePort,
                    targetLabel = if (runtimePort == 0) {
                        "UDP random"
                    } else {
                        "UDP $runtimePort"
                    },
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
                val portToPersist = if (snapshot.mode == ConnectionMode.HOST) {
                    snapshot.portText
                } else {
                    startup.connectedPort.toString()
                }
                saveConnectionFields(snapshot.hostText.trim(), portToPersist)
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
                if (hasConnectedOnce) {
                    val duration = SystemClock.elapsedRealtime() - connectStartedAtMs
                    recordSessionInHistory(activeTargetLabel, true, duration)
                }
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

    private fun recordSessionInHistory(target: String, success: Boolean, durationMs: Long) {
        val entry = SessionEntry(target = target, success = success, durationMs = durationMs)
        _state.update { current ->
            val newHistory = (listOf(entry) + current.sessionHistory).take(20)
            saveSessionHistory(newHistory)
            current.copy(sessionHistory = newHistory)
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
                            val friendlyStatus = normalizeCloudProgressStatus(cloudStatus)
                            if (!stats.connected && isTerminalCloudFailure(cloudStatus)) {
                                val detail = withContext(Dispatchers.IO) {
                                    core.lastError()
                                }
                                withContext(Dispatchers.IO) {
                                    core.stop()
                                }
                                recordSessionInHistory(activeTargetLabel, false, 0)
                                resetConnectionTracking()
                                val message = normalizeCloudFailure(cloudStatus, detail)
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
                                        statusMessage = friendlyStatus,
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
                        recordSessionInHistory(activeTargetLabel, false, elapsedMs)
                        resetConnectionTracking()
                        val hint = networkHintForHost(activeResolvedHost)
                        val baseMessage = if (activeTargetLabel.startsWith("@")) {
                            "Connection timed out. Check that the remote host is online, hosting, and ready to accept cloud requests."
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
            lower.contains("invalid") ||
            lower.contains("timed out") ||
            lower.contains("timeout")
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

    private fun parseHostedPortFromStatus(status: String): Int? {
        val marker = "Hosting on UDP "
        val idx = status.indexOf(marker)
        if (idx < 0) return null
        val digits = buildString {
            for (ch in status.substring(idx + marker.length)) {
                if (ch.isDigit()) append(ch) else break
            }
        }
        val parsed = digits.toIntOrNull() ?: return null
        return parsed.takeIf { it in 0..65535 }
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

    private fun normalizeCloudTarget(raw: String): String {
        return raw.trim().removePrefix("@").trim()
    }

    private fun looksLikeCloudUsernameTarget(value: String): Boolean {
        return isValidCloudUsername(normalizeCloudTarget(value), allowDot = false)
    }

    private fun isValidCloudUsername(value: String, allowDot: Boolean): Boolean {
        val trimmed = value.trim()
        if (trimmed.isEmpty()) return false
        if (trimmed.contains(' ') || trimmed.contains(':') || trimmed.contains('/')) return false
        if (!allowDot && trimmed.contains('.')) return false
        return trimmed.length in 3..32 && trimmed.all { ch ->
            ch.isLetterOrDigit() || ch == '_' || ch == '-' || (allowDot && ch == '.')
        }
    }

    private fun isValidEmail(value: String): Boolean {
        if (value.isBlank()) return false
        val atIndex = value.indexOf('@')
        if (atIndex <= 0 || atIndex == value.length - 1) return false
        val dotAfterAt = value.indexOf('.', startIndex = atIndex + 1)
        return dotAfterAt > atIndex + 1 && dotAfterAt < value.length - 1
    }

    private fun isValidAuthServer(server: String): Boolean {
        return try {
            val url = URL(server)
            (url.protocol == "http" || url.protocol == "https") && !url.host.isNullOrBlank()
        } catch (_: Exception) {
            false
        }
    }

    private fun normalizeAuthError(error: Throwable): String {
        val raw = error.message?.trim().orEmpty()
        if (raw.isEmpty()) return "Authentication failed."
        val lower = raw.lowercase()
        return when {
            lower.contains("timed out") || lower.contains("timeout") ->
                "The auth request timed out. Check your connection and try again."
            lower.contains("unable to resolve host") ||
                lower.contains("failed to connect") ||
                lower.contains("connection refused") ->
                "Could not reach the auth server. Verify the server URL and network."
            lower.contains("invalid credentials") ||
                lower.contains("invalid email or password") ->
                "Incorrect email or password."
            lower.contains("already exists") ||
                lower.contains("already registered") ->
                "This account already exists. Sign in instead."
            else -> raw
        }
    }

    private fun normalizeCloudProgressStatus(status: String): String {
        val trimmed = status.trim()
        if (trimmed.isEmpty()) return trimmed
        val lower = trimmed.lowercase()
        return when {
            lower.contains("request sent") ->
                "Cloud request sent. Waiting for host acknowledgment..."
            lower.contains("host acknowledged request") ->
                "Host acknowledged. Preparing secure connection..."
            lower.contains("host acknowledged. starting direct session") ->
                "Host acknowledged. Starting direct session..."
            lower.contains("direct route unavailable") ->
                "Direct route unavailable. Trying relay route..."
            lower.contains("relay allocated") ->
                "Relay route allocated. Starting secure session..."
            lower.contains("host acknowledged. establishing secure session") ->
                "Host accepted. Establishing secure session..."
            lower.contains("cloud request rejected") ->
                "The host rejected this request."
            lower.contains("relay request failed") ->
                "Relay setup failed."
            lower.contains("relay response invalid") ->
                "Relay response was invalid."
            lower.contains("cloud connect failed") ->
                "Cloud connection failed."
            else -> trimmed
        }
    }

    private fun normalizeCloudFailure(status: String, detail: String): String {
        val base = normalizeCloudProgressStatus(status)
        val normalizedDetail = normalizeCloudConnectError(detail)
        if (normalizedDetail.isBlank()) return base
        return if (normalizedDetail.equals(base, ignoreCase = true)) {
            base
        } else {
            "$base\n$normalizedDetail"
        }
    }

    private fun normalizeCloudConnectError(raw: String): String {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return ""
        val lower = trimmed.lowercase()
        return when {
            lower.contains("timed out") || lower.contains("timeout") ->
                "Connection timed out. Check that the remote host is online and reachable."
            lower.contains("cloud request rejected") || lower.contains("rejected") ->
                "The host rejected this request."
            lower.contains("relay request failed") || lower.contains("relay response invalid") ->
                "Relay routing failed. Retry, or use direct IP/host connect."
            lower.contains("signaling is not connected") ->
                "Cloud signaling is disconnected. Sign in again or check network."
            lower.contains("not logged in") ->
                "Sign in to use username-based cloud connect."
            lower.contains("already active") ->
                "A client session is already running. Stop the current session and retry."
            else -> trimmed
        }
    }

    private suspend fun connectSignaling(server: String, token: String): Int {
        val signalingUrl = AuthApi.signalingWsUrl(server)
        return withContext(Dispatchers.IO) {
            core.connectSignaling(signalingUrl, token)
        }
    }

    private suspend fun connectSignalingWithRetry(
        server: String,
        token: String,
        maxAttempts: Int = 3,
        initialBackoffMs: Long = 400L,
    ): Int {
        var lastCode = -1
        for (attempt in 1..maxAttempts) {
            val rc = connectSignaling(server, token)
            if (rc == 0) {
                return 0
            }
            lastCode = rc
            if (attempt < maxAttempts && shouldRetrySignaling(rc)) {
                val delayMs = initialBackoffMs * (1L shl (attempt - 1))
                delay(delayMs)
            } else {
                break
            }
        }
        return lastCode
    }

    private fun shouldRetrySignaling(code: Int): Boolean {
        if (code in setOf(-3, -4, -5, -11)) {
            return true
        }
        val detail = core.describeError(code).lowercase()
        return detail.contains("timed out") ||
            detail.contains("timeout") ||
            detail.contains("failed to connect") ||
            detail.contains("connection refused") ||
            detail.contains("unreachable") ||
            detail.contains("network")
    }

    private fun refreshCloudSignaling(
        server: String,
        token: String,
        reason: SignalingRefreshReason,
    ) {
        if (token.isBlank()) {
            _state.update {
                it.copy(
                    cloudSignalingState = CloudSignalingState.DISCONNECTED,
                )
            }
            return
        }

        val attempts = when (reason) {
            SignalingRefreshReason.RESTORE -> 2
            SignalingRefreshReason.MODE_SWITCH -> 3
            SignalingRefreshReason.SETTINGS_SAVE -> 3
            SignalingRefreshReason.MANUAL_RETRY -> 3
        }

        _state.update { current ->
            current.copy(
                cloudSignalingState = CloudSignalingState.CONNECTING,
                authStatusMessage = when (reason) {
                    SignalingRefreshReason.RESTORE -> current.authStatusMessage.ifBlank { "Restoring cloud signaling..." }
                    SignalingRefreshReason.MODE_SWITCH -> "Connecting cloud signaling..."
                    SignalingRefreshReason.SETTINGS_SAVE -> "Reconnecting cloud signaling..."
                    SignalingRefreshReason.MANUAL_RETRY -> "Retrying cloud signaling..."
                },
            )
        }

        viewModelScope.launch {
            val rc = connectSignalingWithRetry(server, token, maxAttempts = attempts)
            if (_state.value.isAuthenticated.not()) return@launch

            if (rc == 0) {
                _state.update { current ->
                    val nextStatus = when (reason) {
                        SignalingRefreshReason.RESTORE ->
                            "Signed in"
                        SignalingRefreshReason.MODE_SWITCH ->
                            "Cloud signaling connected."
                        SignalingRefreshReason.SETTINGS_SAVE ->
                            "Settings saved. Cloud signaling connected."
                        SignalingRefreshReason.MANUAL_RETRY ->
                            "Cloud signaling connected."
                    }
                    val nextError = if (current.authErrorMessage.contains("signaling", ignoreCase = true)) {
                        ""
                    } else {
                        current.authErrorMessage
                    }
                    current.copy(
                        authStatusMessage = nextStatus,
                        authErrorMessage = nextError,
                        cloudSignalingState = CloudSignalingState.CONNECTED,
                    )
                }
            } else {
                val detail = normalizeCloudConnectError(core.describeError(rc))
                _state.update { current ->
                    val nextStatus = when (reason) {
                        SignalingRefreshReason.RESTORE ->
                            "Signed in. Cloud signaling reconnect failed."
                        SignalingRefreshReason.MODE_SWITCH ->
                            "Cloud signaling unavailable."
                        SignalingRefreshReason.SETTINGS_SAVE ->
                            "Settings saved. Cloud signaling unavailable."
                        SignalingRefreshReason.MANUAL_RETRY ->
                            "Cloud signaling unavailable."
                    }
                    current.copy(
                        authStatusMessage = nextStatus,
                        authErrorMessage = detail,
                        cloudSignalingState = CloudSignalingState.ERROR,
                    )
                }
            }
        }
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
        private const val KEY_SESSION_HISTORY = "session_history"

        private const val DEFAULT_HOST = "192.168.1.10"
        private const val DEFAULT_PORT = "4444"
        private const val DEFAULT_AUTH_SERVER = "https://auth.wavry.dev"
        private const val CONNECT_TIMEOUT_MS = 12_000L
    }

    private fun loadSessionHistory(): List<SessionEntry> {
        val json = prefs.getString(KEY_SESSION_HISTORY, "[]") ?: "[]"
        return try {
            val listType = object : com.google.gson.reflect.TypeToken<List<SessionEntry>>() {}.type
            com.google.gson.Gson().fromJson(json, listType)
        } catch (e: Exception) {
            emptyList()
        }
    }

    private fun saveSessionHistory(history: List<SessionEntry>) {
        val json = com.google.gson.Gson().toJson(history.take(20))
        prefs.edit().putString(KEY_SESSION_HISTORY, json).apply()
    }
}

private data class StartResult(
    val code: Int,
    val connectedPort: Int,
    val targetLabel: String,
    val usedFallback: Boolean = false,
    val errorMessage: String = "",
)
