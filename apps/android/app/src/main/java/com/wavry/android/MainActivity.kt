package com.wavry.android

import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.wavry.android.ui.AppTab
import com.wavry.android.ui.ConnectionMode
import com.wavry.android.ui.ConnectivityMode
import com.wavry.android.ui.WavryUiState
import com.wavry.android.ui.WavryViewModel
import com.wavry.android.ui.theme.WavryTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            WavryTheme {
                val vm: WavryViewModel = viewModel()
                val state by vm.state.collectAsStateWithLifecycle()
                WavryScreen(
                    state = state,
                    onSetMode = vm::setMode,
                    onSetHost = vm::setHostText,
                    onSetPort = vm::setPortText,
                    onSetDisplayName = vm::setDisplayName,
                    onSetConnectivityMode = vm::setConnectivityMode,
                    onSetAuthServer = vm::setAuthServer,
                    onSetTab = vm::setActiveTab,
                    onLoginCloud = vm::loginCloud,
                    onRegisterCloud = vm::registerCloud,
                    onLogoutCloud = vm::logoutCloud,
                    onRequestCloudConnect = vm::requestCloudConnect,
                    onSaveSettings = vm::saveSettings,
                    onCompleteSetup = vm::completeSetup,
                    onStart = vm::start,
                    onStop = vm::stop,
                )
            }
        }
    }
}

@Composable
@OptIn(ExperimentalMaterial3Api::class)
private fun WavryScreen(
    state: WavryUiState,
    onSetMode: (ConnectionMode) -> Unit,
    onSetHost: (String) -> Unit,
    onSetPort: (String) -> Unit,
    onSetDisplayName: (String) -> Unit,
    onSetConnectivityMode: (ConnectivityMode) -> Unit,
    onSetAuthServer: (String) -> Unit,
    onSetTab: (AppTab) -> Unit,
    onLoginCloud: (String, String) -> Unit,
    onRegisterCloud: (String, String, String) -> Unit,
    onLogoutCloud: () -> Unit,
    onRequestCloudConnect: (String) -> Unit,
    onSaveSettings: () -> Unit,
    onCompleteSetup: (String, ConnectivityMode) -> Unit,
    onStart: () -> Unit,
    onStop: () -> Unit,
) {
    if (!state.setupComplete) {
        SetupFlow(
            state = state,
            onCompleteSetup = onCompleteSetup,
            onSetDisplayName = onSetDisplayName,
            onSetConnectivityMode = onSetConnectivityMode,
        )
        return
    }

    val gradient = Brush.verticalGradient(
        colors = listOf(
            MaterialTheme.colorScheme.background,
            MaterialTheme.colorScheme.surface,
            MaterialTheme.colorScheme.background,
        ),
    )

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
        topBar = {
            CenterAlignedTopAppBar(
                title = {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            text = if (state.isQuestBuild) "Wavry Quest" else "Wavry Android",
                            style = MaterialTheme.typography.titleLarge,
                        )
                        Text(
                            text = "${state.displayName} • Core ${state.version}",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
            )
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(gradient)
                .padding(innerPadding)
                .padding(horizontal = 16.dp, vertical = 12.dp)
                .navigationBarsPadding()
                .imePadding()
                .verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = 760.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    AssistChip(
                        onClick = { },
                        enabled = false,
                        label = { Text(if (state.connectivityMode == ConnectivityMode.WAVRY) "Cloud mode" else "LAN mode") },
                    )
                    AssistChip(
                        onClick = { },
                        enabled = false,
                        label = {
                            Text(if (state.isAuthenticated) "Cloud signed in" else "Cloud signed out")
                        },
                    )
                }

                if (state.isBusy) {
                    LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                }

                AppTabSelector(
                    selected = state.activeTab,
                    onSetTab = onSetTab,
                )

                if (state.activeTab == AppTab.SESSION) {
                    SessionTab(
                        state = state,
                        onSetMode = onSetMode,
                        onSetHost = onSetHost,
                        onSetPort = onSetPort,
                        onRequestCloudConnect = onRequestCloudConnect,
                        onStart = onStart,
                        onStop = onStop,
                    )
                } else {
                    SettingsTab(
                        state = state,
                        onSetDisplayName = onSetDisplayName,
                        onSetConnectivityMode = onSetConnectivityMode,
                        onSetAuthServer = onSetAuthServer,
                        onLoginCloud = onLoginCloud,
                        onRegisterCloud = onRegisterCloud,
                        onLogoutCloud = onLogoutCloud,
                        onSetHost = onSetHost,
                        onSetPort = onSetPort,
                        onSaveSettings = onSaveSettings,
                    )
                }
            }
        }
    }
}

@Composable
private fun SetupFlow(
    state: WavryUiState,
    onCompleteSetup: (String, ConnectivityMode) -> Unit,
    onSetDisplayName: (String) -> Unit,
    onSetConnectivityMode: (ConnectivityMode) -> Unit,
) {
    var step by rememberSaveable { mutableIntStateOf(0) }
    var localName by rememberSaveable { mutableStateOf(state.displayName.ifBlank { "My Android" }) }
    var localConnectivity by rememberSaveable { mutableStateOf(state.connectivityMode) }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .padding(horizontal = 16.dp, vertical = 20.dp)
                .verticalScroll(rememberScrollState())
                .imePadding(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = 640.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Text(
                    text = "Welcome to Wavry",
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = "Step ${step + 1} of 3",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                ElevatedCard(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
                ) {
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Text(
                            text = when (step) {
                                0 -> "Get ready"
                                1 -> "Device identity"
                                else -> "Connectivity"
                            },
                            style = MaterialTheme.typography.titleLarge,
                        )

                        Text(
                            text = when (step) {
                                0 -> "Low-latency remote desktop streaming on Android and Quest with secure transport."
                                1 -> "Choose the name shown in your session list."
                                else -> "Choose the default route for connections."
                            },
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )

                        when (step) {
                            0 -> {
                                Card(
                                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
                                ) {
                                    Text(
                                        text = "Client mode is optimized for phone and Quest. Add host IP, connect, and monitor session health in one place.",
                                        modifier = Modifier.padding(12.dp),
                                        style = MaterialTheme.typography.bodyMedium,
                                    )
                                }
                            }

                            1 -> {
                                OutlinedTextField(
                                    value = localName,
                                    onValueChange = { localName = it.take(48) },
                                    modifier = Modifier.fillMaxWidth(),
                                    singleLine = true,
                                    label = { Text("Device Name") },
                                    placeholder = { Text("My Android") },
                                )
                            }

                            else -> {
                                ConnectivitySelector(
                                    selected = localConnectivity,
                                    enabled = true,
                                    onSetConnectivityMode = { localConnectivity = it },
                                )
                                Text(
                                    text = "You can change this later in Settings.",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }

                if (step > 0) {
                    OutlinedButton(
                        onClick = { step -= 1 },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("Back")
                    }
                }

                Button(
                    onClick = {
                        if (step < 2) {
                            if (step == 1 && localName.trim().isEmpty()) {
                                localName = "My Android"
                            }
                            step += 1
                        } else {
                            val finalName = localName.trim().ifEmpty { "My Android" }
                            onSetDisplayName(finalName)
                            onSetConnectivityMode(localConnectivity)
                            onCompleteSetup(finalName, localConnectivity)
                        }
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(if (step < 2) "Continue" else "Finish Setup")
                }
            }
        }
    }
}

@Composable
private fun AppTabSelector(
    selected: AppTab,
    onSetTab: (AppTab) -> Unit,
) {
    val tabs = listOf(AppTab.SESSION, AppTab.SETTINGS)
    SingleChoiceSegmentedButtonRow(
        modifier = Modifier.fillMaxWidth(),
    ) {
        tabs.forEachIndexed { index, tab ->
            SegmentedButton(
                selected = selected == tab,
                onClick = { onSetTab(tab) },
                shape = SegmentedButtonDefaults.itemShape(index = index, count = tabs.size),
                label = { Text(if (tab == AppTab.SESSION) "Session" else "Settings") },
            )
        }
    }
}

@Composable
private fun SessionTab(
    state: WavryUiState,
    onSetMode: (ConnectionMode) -> Unit,
    onSetHost: (String) -> Unit,
    onSetPort: (String) -> Unit,
    onRequestCloudConnect: (String) -> Unit,
    onStart: () -> Unit,
    onStop: () -> Unit,
) {
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = "Session",
                style = MaterialTheme.typography.titleLarge,
            )

            if (state.connectivityMode == ConnectivityMode.WAVRY) {
                StatusBanner(
                    text = if (state.isAuthenticated) {
                        "Cloud signaling is active. Use username lookup, or connect directly by IP."
                    } else {
                        "Sign in from Settings to use cloud signaling."
                    },
                    isError = !state.isAuthenticated,
                )
            }

            ModeSelector(
                selected = state.mode,
                supportsHost = state.supportsHost,
                onSetMode = onSetMode,
                enabled = !state.isBusy && !state.isRunning,
            )

            if (state.mode == ConnectionMode.CLIENT) {
                OutlinedTextField(
                    value = state.hostText,
                    onValueChange = onSetHost,
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = {
                        Text(if (state.connectivityMode == ConnectivityMode.WAVRY) "Target" else "Host")
                    },
                    placeholder = {
                        Text(
                            if (state.connectivityMode == ConnectivityMode.WAVRY) {
                                "username or 192.168.1.20:8000"
                            } else {
                                "192.168.1.20 or host.local:8000"
                            },
                        )
                    },
                    supportingText = {
                        Text(
                            if (state.connectivityMode == ConnectivityMode.WAVRY) {
                                "Use username for cloud request, or host/IP for direct fallback."
                            } else {
                                "Hostname or IP. Optional :port supported."
                            },
                        )
                    },
                    enabled = !state.isBusy && !state.isRunning,
                )
            }

            if (state.mode == ConnectionMode.CLIENT && state.connectivityMode == ConnectivityMode.WAVRY) {
                OutlinedButton(
                    onClick = { onRequestCloudConnect(state.hostText) },
                    enabled = state.isAuthenticated && !state.isBusy && !state.isRunning && state.hostText.isNotBlank(),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Request Cloud Connect")
                }
            }

            OutlinedTextField(
                value = state.portText,
                onValueChange = onSetPort,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("UDP Port") },
                supportingText = { Text("Typical: 4444 (native macOS) or 8000 (desktop host).") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                enabled = !state.isBusy && !state.isRunning,
            )

            PortPresetRow(
                selectedPort = state.portText.toIntOrNull(),
                onSetPort = onSetPort,
                enabled = !state.isBusy && !state.isRunning,
            )

            if (!state.isRunning) {
                Button(
                    onClick = onStart,
                    enabled = !state.isBusy,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(if (state.mode == ConnectionMode.HOST) "Start Hosting" else "Connect")
                }
            } else {
                OutlinedButton(
                    onClick = onStop,
                    enabled = !state.isBusy,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Stop Session")
                }
            }

            if (state.errorMessage.isNotBlank()) {
                StatusBanner(
                    text = state.errorMessage,
                    isError = true,
                )
            }

            StatusBanner(
                text = state.statusMessage,
                isError = false,
            )
        }
    }

    if (state.isQuestBuild) {
        ElevatedCard(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text(
                    text = "Quest VR",
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    text = "Quest flavor enables VR launch metadata and hand-tracking capability flags.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    StatsCard(state)
}

@Composable
private fun PortPresetRow(
    selectedPort: Int?,
    onSetPort: (String) -> Unit,
    enabled: Boolean,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilterChip(
            selected = selectedPort == 4444,
            onClick = { onSetPort("4444") },
            enabled = enabled,
            label = { Text("4444") },
        )
        FilterChip(
            selected = selectedPort == 8000,
            onClick = { onSetPort("8000") },
            enabled = enabled,
            label = { Text("8000") },
        )
    }
}

@Composable
private fun SettingsTab(
    state: WavryUiState,
    onSetDisplayName: (String) -> Unit,
    onSetConnectivityMode: (ConnectivityMode) -> Unit,
    onSetAuthServer: (String) -> Unit,
    onLoginCloud: (String, String) -> Unit,
    onRegisterCloud: (String, String, String) -> Unit,
    onLogoutCloud: () -> Unit,
    onSetHost: (String) -> Unit,
    onSetPort: (String) -> Unit,
    onSaveSettings: () -> Unit,
) {
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = "Settings",
                style = MaterialTheme.typography.titleLarge,
            )

            OutlinedTextField(
                value = state.displayName,
                onValueChange = onSetDisplayName,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("Device Name") },
                placeholder = { Text("My Android") },
            )

            ConnectivitySelector(
                selected = state.connectivityMode,
                enabled = true,
                onSetConnectivityMode = onSetConnectivityMode,
            )

            OutlinedTextField(
                value = state.hostText,
                onValueChange = onSetHost,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("Default Host") },
                placeholder = { Text("192.168.1.20") },
            )

            OutlinedTextField(
                value = state.portText,
                onValueChange = onSetPort,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("Default Port") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            )

            Button(
                onClick = onSaveSettings,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Save Settings")
            }

            Text(
                text = "Cloud mode keeps parity with desktop account flows; direct mode is lowest-friction LAN pairing.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            CloudAccountSection(
                state = state,
                onSetAuthServer = onSetAuthServer,
                onLoginCloud = onLoginCloud,
                onRegisterCloud = onRegisterCloud,
                onLogoutCloud = onLogoutCloud,
            )
        }
    }
}

@Composable
private fun CloudAccountSection(
    state: WavryUiState,
    onSetAuthServer: (String) -> Unit,
    onLoginCloud: (String, String) -> Unit,
    onRegisterCloud: (String, String, String) -> Unit,
    onLogoutCloud: () -> Unit,
) {
    var email by rememberSaveable(state.authEmail, state.isAuthenticated) {
        mutableStateOf(state.authEmail)
    }
    var username by rememberSaveable(state.authUsername, state.isAuthenticated) {
        mutableStateOf(state.authUsername)
    }
    var password by rememberSaveable { mutableStateOf("") }

    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(
                text = "Account",
                style = MaterialTheme.typography.titleMedium,
            )

            OutlinedTextField(
                value = state.authServer,
                onValueChange = onSetAuthServer,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("Auth Server") },
                placeholder = { Text("https://auth.wavry.dev") },
                enabled = !state.isAuthBusy,
            )

            if (state.isAuthenticated) {
                Text(
                    text = "Signed in as ${state.authUsername.ifBlank { state.authEmail }}",
                    style = MaterialTheme.typography.bodyMedium,
                )
                OutlinedButton(
                    onClick = onLogoutCloud,
                    enabled = !state.isAuthBusy,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Sign Out")
                }
            } else {
                OutlinedTextField(
                    value = email,
                    onValueChange = { email = it.trim() },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = { Text("Email") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                    enabled = !state.isAuthBusy,
                )
                OutlinedTextField(
                    value = username,
                    onValueChange = { username = it.take(32) },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = { Text("Username (for sign-up)") },
                    enabled = !state.isAuthBusy,
                )
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = { Text("Password") },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    enabled = !state.isAuthBusy,
                )

                Button(
                    onClick = { onLoginCloud(email, password) },
                    enabled = !state.isAuthBusy && email.isNotBlank() && password.isNotBlank(),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Sign In")
                }
                OutlinedButton(
                    onClick = { onRegisterCloud(email, username, password) },
                    enabled = !state.isAuthBusy && email.isNotBlank() && username.isNotBlank() && password.isNotBlank(),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Create Account")
                }
            }

            if (state.authStatusMessage.isNotBlank()) {
                StatusBanner(
                    text = state.authStatusMessage,
                    isError = false,
                )
            }
            if (state.authErrorMessage.isNotBlank()) {
                StatusBanner(
                    text = state.authErrorMessage,
                    isError = true,
                )
            }
        }
    }
}

@Composable
private fun StatusBanner(text: String, isError: Boolean) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (isError) {
                MaterialTheme.colorScheme.errorContainer
            } else {
                MaterialTheme.colorScheme.surfaceVariant
            },
        ),
    ) {
        Text(
            text = text,
            modifier = Modifier.padding(10.dp),
            style = MaterialTheme.typography.bodyMedium,
            color = if (isError) {
                MaterialTheme.colorScheme.onErrorContainer
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        )
    }
}

@Composable
private fun ModeSelector(
    selected: ConnectionMode,
    supportsHost: Boolean,
    onSetMode: (ConnectionMode) -> Unit,
    enabled: Boolean,
) {
    val options = if (supportsHost) {
        listOf(ConnectionMode.CLIENT, ConnectionMode.HOST)
    } else {
        listOf(ConnectionMode.CLIENT)
    }

    SingleChoiceSegmentedButtonRow(
        modifier = Modifier.fillMaxWidth(),
    ) {
        options.forEachIndexed { index, mode ->
            SegmentedButton(
                selected = selected == mode,
                onClick = { onSetMode(mode) },
                enabled = enabled,
                shape = SegmentedButtonDefaults.itemShape(index = index, count = options.size),
                label = {
                    Text(if (mode == ConnectionMode.CLIENT) "Client" else "Host")
                },
            )
        }
    }
}

@Composable
private fun ConnectivitySelector(
    selected: ConnectivityMode,
    enabled: Boolean,
    onSetConnectivityMode: (ConnectivityMode) -> Unit,
) {
    val options = listOf(ConnectivityMode.DIRECT, ConnectivityMode.WAVRY)
    SingleChoiceSegmentedButtonRow(
        modifier = Modifier.fillMaxWidth(),
    ) {
        options.forEachIndexed { index, mode ->
            SegmentedButton(
                selected = selected == mode,
                onClick = { onSetConnectivityMode(mode) },
                enabled = enabled,
                shape = SegmentedButtonDefaults.itemShape(index = index, count = options.size),
                label = {
                    Text(if (mode == ConnectivityMode.DIRECT) "LAN" else "Cloud")
                },
            )
        }
    }
}

@Composable
private fun StatsCard(state: WavryUiState) {
    ElevatedCard(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = "Session Stats",
                style = MaterialTheme.typography.titleMedium,
            )
            StatRow("Connected", if (state.stats.connected) "Yes" else "No")
            StatRow("FPS", state.stats.fps.toString())
            StatRow("RTT", "${state.stats.rttMs} ms")
            StatRow("Bitrate", "${state.stats.bitrateKbps} kbps")
            StatRow("Frames Encoded", state.stats.framesEncoded.toString())
            StatRow("Frames Decoded", state.stats.framesDecoded.toString())
        }
    }
}

@Composable
private fun StatRow(label: String, value: String) {
    Row(modifier = Modifier.fillMaxWidth()) {
        Text(
            text = label,
            modifier = Modifier.width(150.dp),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.SemiBold,
        )
    }
}
