package com.wavry.android

import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Settings
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
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
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.wavry.android.ui.AppTab
import com.wavry.android.ui.AuthFormMode
import com.wavry.android.ui.CloudSignalingState
import com.wavry.android.ui.ConnectionMode
import com.wavry.android.ui.ConnectivityMode
import com.wavry.android.ui.WavryUiState
import com.wavry.android.ui.WavryViewModel
import com.wavry.android.ui.theme.WavryTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            WavryTheme(dynamicColor = true) {
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
                    onSetAuthFormMode = vm::setAuthFormMode,
                    onSetTab = vm::setActiveTab,
                    onLoginCloud = vm::loginCloud,
                    onRegisterCloud = vm::registerCloud,
                    onLogoutCloud = vm::logoutCloud,
                    onReconnectCloudSignaling = vm::reconnectCloudSignaling,
                    onOpenCloudSignIn = { vm.openCloudAuth(AuthFormMode.LOGIN) },
                    onOpenCloudRegister = { vm.openCloudAuth(AuthFormMode.REGISTER) },
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
@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
private fun WavryScreen(
    state: WavryUiState,
    onSetMode: (ConnectionMode) -> Unit,
    onSetHost: (String) -> Unit,
    onSetPort: (String) -> Unit,
    onSetDisplayName: (String) -> Unit,
    onSetConnectivityMode: (ConnectivityMode) -> Unit,
    onSetAuthServer: (String) -> Unit,
    onSetAuthFormMode: (AuthFormMode) -> Unit,
    onSetTab: (AppTab) -> Unit,
    onLoginCloud: (String, String) -> Unit,
    onRegisterCloud: (String, String, String) -> Unit,
    onLogoutCloud: () -> Unit,
    onReconnectCloudSignaling: () -> Unit,
    onOpenCloudSignIn: () -> Unit,
    onOpenCloudRegister: () -> Unit,
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
            onOpenCloudSignIn = onOpenCloudSignIn,
            onOpenCloudRegister = onOpenCloudRegister,
        )
        return
    }

    val navigationItems = listOf(
        NavigationItem(
            tab = AppTab.SESSION,
            label = "Session",
            icon = Icons.Filled.Home,
        ),
        NavigationItem(
            tab = AppTab.SETTINGS,
            label = "Settings",
            icon = Icons.Filled.Settings,
        ),
    )

    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val expandedLayout = maxWidth >= 1000.dp
        val showDrawerButton = !expandedLayout
        val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
        val scope = rememberCoroutineScope()

        ModalNavigationDrawer(
            drawerState = drawerState,
            gesturesEnabled = showDrawerButton,
            drawerContent = {
                if (showDrawerButton) {
                    ModalDrawerSheet {
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = "Navigate",
                            style = MaterialTheme.typography.labelLarge,
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                        )
                        navigationItems.forEach { destination ->
                            NavigationDrawerItem(
                                selected = state.activeTab == destination.tab,
                                onClick = {
                                    onSetTab(destination.tab)
                                    scope.launch { drawerState.close() }
                                },
                                label = { Text(destination.label) },
                                icon = {
                                    Icon(
                                        imageVector = destination.icon,
                                        contentDescription = destination.label,
                                    )
                                },
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                            )
                        }
                    }
                }
            },
        ) {
            Scaffold(
                containerColor = MaterialTheme.colorScheme.surfaceContainerLowest,
                contentWindowInsets = WindowInsets.safeDrawing,
                topBar = {
                    TopAppBar(
                        navigationIcon = {
                            if (showDrawerButton) {
                                IconButton(onClick = { scope.launch { drawerState.open() } }) {
                                    Icon(
                                        imageVector = Icons.Filled.Menu,
                                        contentDescription = "Open navigation",
                                    )
                                }
                            }
                        },
                        title = {
                            Column {
                                Text(
                                    text = if (state.isQuestBuild) "Wavry Quest" else "Wavry Android",
                                    style = MaterialTheme.typography.titleMedium,
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
                Row(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(MaterialTheme.colorScheme.surfaceContainerLowest)
                        .padding(innerPadding)
                        .imePadding(),
                ) {
                    if (expandedLayout) {
                        NavigationRail {
                            navigationItems.forEach { destination ->
                                NavigationRailItem(
                                    selected = state.activeTab == destination.tab,
                                    onClick = { onSetTab(destination.tab) },
                                    label = { Text(destination.label) },
                                    icon = {
                                        Icon(
                                            imageVector = destination.icon,
                                            contentDescription = destination.label,
                                        )
                                    },
                                )
                            }
                        }
                    }

                    Column(
                        modifier = Modifier
                            .weight(1f)
                            .fillMaxSize()
                            .padding(horizontal = 16.dp, vertical = 8.dp)
                            .verticalScroll(rememberScrollState()),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .widthIn(max = 920.dp),
                            verticalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            ConnectionOverviewCard(state = state)

                            if (state.isBusy) {
                                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                            }

                            if (state.activeTab == AppTab.SESSION) {
                                SessionTab(
                                    state = state,
                                    onSetMode = onSetMode,
                                    onSetHost = onSetHost,
                                    onSetPort = onSetPort,
                                    onOpenCloudSignIn = onOpenCloudSignIn,
                                    onOpenCloudRegister = onOpenCloudRegister,
                                    onStart = onStart,
                                    onStop = onStop,
                                )
                            } else {
                                SettingsTab(
                                    state = state,
                                    onSetDisplayName = onSetDisplayName,
                                    onSetConnectivityMode = onSetConnectivityMode,
                                    onSetAuthServer = onSetAuthServer,
                                    onSetAuthFormMode = onSetAuthFormMode,
                                    onLoginCloud = onLoginCloud,
                                    onRegisterCloud = onRegisterCloud,
                                    onLogoutCloud = onLogoutCloud,
                                    onReconnectCloudSignaling = onReconnectCloudSignaling,
                                    onSetHost = onSetHost,
                                    onSetPort = onSetPort,
                                    onSaveSettings = onSaveSettings,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun ConnectionOverviewCard(state: WavryUiState) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        FlowRow(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            AssistChip(
                onClick = { },
                enabled = false,
                label = { Text(if (state.connectivityMode == ConnectivityMode.WAVRY) "Cloud mode" else "LAN mode") },
            )
            AssistChip(
                onClick = { },
                enabled = false,
                label = { Text(if (state.isAuthenticated) "Account signed in" else "Account signed out") },
            )
            if (state.connectivityMode == ConnectivityMode.WAVRY) {
                AssistChip(
                    onClick = { },
                    enabled = false,
                    label = { Text("Signal ${cloudSignalingLabel(state.cloudSignalingState)}") },
                )
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
    onOpenCloudSignIn: () -> Unit,
    onOpenCloudRegister: () -> Unit,
) {
    var step by rememberSaveable { mutableIntStateOf(0) }
    var localName by rememberSaveable { mutableStateOf(state.displayName.ifBlank { "My Android" }) }
    var localConnectivity by rememberSaveable { mutableStateOf(state.connectivityMode) }

    fun finishSetup(mode: ConnectivityMode = localConnectivity) {
        val finalName = localName.trim().ifEmpty { "My Android" }
        onSetDisplayName(finalName)
        onSetConnectivityMode(mode)
        onCompleteSetup(finalName, mode)
    }

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

                Card(
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(20.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
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
                                    Column(
                                        modifier = Modifier.padding(12.dp),
                                        verticalArrangement = Arrangement.spacedBy(8.dp),
                                    ) {
                                        Text(
                                            text = "Client mode is optimized for phone and Quest. Add host IP, connect, and monitor session health in one place.",
                                            style = MaterialTheme.typography.bodyMedium,
                                        )
                                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                            OutlinedButton(
                                                onClick = {
                                                    finishSetup(ConnectivityMode.WAVRY)
                                                    onOpenCloudSignIn()
                                                },
                                                modifier = Modifier.fillMaxWidth(),
                                            ) {
                                                Text("I already have an account")
                                            }
                                            OutlinedButton(
                                                onClick = {
                                                    finishSetup(ConnectivityMode.WAVRY)
                                                    onOpenCloudRegister()
                                                },
                                                modifier = Modifier.fillMaxWidth(),
                                            ) {
                                                Text("Create account")
                                            }
                                        }
                                    }
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
                                if (localConnectivity == ConnectivityMode.WAVRY) {
                                    StatusBanner(
                                        text = "Cloud mode works best with an account. Create one now, or sign in if you already have one.",
                                        isError = false,
                                    )
                                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                        OutlinedButton(
                                            onClick = {
                                                finishSetup(ConnectivityMode.WAVRY)
                                                onOpenCloudRegister()
                                            },
                                        ) {
                                            Text("Create account")
                                        }
                                        OutlinedButton(
                                            onClick = {
                                                finishSetup(ConnectivityMode.WAVRY)
                                                onOpenCloudSignIn()
                                            },
                                        ) {
                                            Text("Sign in")
                                        }
                                    }
                                }
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
                            finishSetup()
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
private fun SessionTab(
    state: WavryUiState,
    onSetMode: (ConnectionMode) -> Unit,
    onSetHost: (String) -> Unit,
    onSetPort: (String) -> Unit,
    onOpenCloudSignIn: () -> Unit,
    onOpenCloudRegister: () -> Unit,
    onStart: () -> Unit,
    onStop: () -> Unit,
) {
    val cloudTargetLooksValid = looksLikeCloudUsername(state.hostText)
    val hasClientTarget = state.mode != ConnectionMode.CLIENT || state.hostText.trim().isNotEmpty()
    val hasValidPort = state.portText.toIntOrNull()?.let { port ->
        if (state.mode == ConnectionMode.HOST) port in 0..65535 else port in 1..65535
    } ?: false
    val requiresCloudAuth =
        state.mode == ConnectionMode.CLIENT &&
            state.connectivityMode == ConnectivityMode.WAVRY &&
            cloudTargetLooksValid &&
            !state.isAuthenticated
    val canStart = !state.isBusy && hasClientTarget && hasValidPort && !requiresCloudAuth
    val startLabel = when {
        state.mode == ConnectionMode.HOST -> "Start Hosting"
        state.connectivityMode == ConnectivityMode.WAVRY && cloudTargetLooksValid -> "Connect via Cloud"
        else -> "Connect"
    }

    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
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
                        "Sign in for username connect, or keep using direct host/IP connection."
                    },
                    isError = !state.isAuthenticated,
                )

                if (!state.isAuthenticated) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(
                            onClick = onOpenCloudSignIn,
                            enabled = !state.isBusy && !state.isRunning,
                        ) {
                            Text("Sign in")
                        }
                        OutlinedButton(
                            onClick = onOpenCloudRegister,
                            enabled = !state.isBusy && !state.isRunning,
                        ) {
                            Text("Create account")
                        }
                    }
                }

                if (state.isAuthenticated && state.cloudSignalingState != CloudSignalingState.CONNECTED) {
                    StatusBanner(
                        text = when (state.cloudSignalingState) {
                            CloudSignalingState.CONNECTING ->
                                "Cloud signaling is connecting. Username requests may take a moment."
                            CloudSignalingState.ERROR ->
                                "Cloud signaling is unavailable. Open Settings and tap Reconnect Signaling."
                            CloudSignalingState.DISCONNECTED ->
                                "Cloud signaling is disconnected. Open Settings and tap Reconnect Signaling."
                            CloudSignalingState.CONNECTED ->
                                ""
                        },
                        isError = state.cloudSignalingState == CloudSignalingState.ERROR,
                    )
                }
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
                                "@username or 192.168.1.20:8000"
                            } else {
                                "192.168.1.20 or host.local:8000"
                            },
                        )
                    },
                    supportingText = {
                        Text(
                            if (state.connectivityMode == ConnectivityMode.WAVRY) {
                                "Use username (optional @) for cloud request, or host/IP for direct fallback."
                            } else {
                                "Hostname or IP. Optional :port supported."
                            },
                        )
                    },
                    enabled = !state.isBusy && !state.isRunning,
                )
            }

            if (state.mode == ConnectionMode.CLIENT && state.connectivityMode == ConnectivityMode.WAVRY) {
                if (state.hostText.isNotBlank() && !cloudTargetLooksValid) {
                    Text(
                        text = "Cloud requests need a username (3-32 chars, optional @ prefix). Use Connect for IP/host targets.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            OutlinedTextField(
                value = state.portText,
                onValueChange = onSetPort,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("UDP Port") },
                supportingText = {
                    Text(
                        if (state.mode == ConnectionMode.HOST) {
                            "Typical: 4444 or 8000. Use 0 for a random host port."
                        } else {
                            "Typical: 4444 (native macOS) or 8000 (desktop host)."
                        },
                    )
                },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                enabled = !state.isBusy && !state.isRunning,
            )

            PortPresetRow(
                selectedPort = state.portText.toIntOrNull(),
                allowRandom = state.mode == ConnectionMode.HOST,
                onSetPort = onSetPort,
                enabled = !state.isBusy && !state.isRunning,
            )

            if (state.mode == ConnectionMode.CLIENT && state.portText == "0") {
                Text(
                    text = "Client mode needs a concrete remote port (for example 4444 or 8000).",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            if (!state.isRunning) {
                Button(
                    onClick = onStart,
                    enabled = canStart,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(startLabel)
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
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(20.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
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
@OptIn(ExperimentalLayoutApi::class)
private fun PortPresetRow(
    selectedPort: Int?,
    allowRandom: Boolean,
    onSetPort: (String) -> Unit,
    enabled: Boolean,
) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (allowRandom) {
            FilterChip(
                selected = selectedPort == 0,
                onClick = { onSetPort("0") },
                enabled = enabled,
                label = { Text("Random") },
            )
        }
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
    onSetAuthFormMode: (AuthFormMode) -> Unit,
    onLoginCloud: (String, String) -> Unit,
    onRegisterCloud: (String, String, String) -> Unit,
    onLogoutCloud: () -> Unit,
    onReconnectCloudSignaling: () -> Unit,
    onSetHost: (String) -> Unit,
    onSetPort: (String) -> Unit,
    onSaveSettings: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
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
                supportingText = { Text("Use 0 for random host port (client mode still requires explicit remote port).") },
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
                onSetAuthFormMode = onSetAuthFormMode,
                onLoginCloud = onLoginCloud,
                onRegisterCloud = onRegisterCloud,
                onLogoutCloud = onLogoutCloud,
                onReconnectCloudSignaling = onReconnectCloudSignaling,
            )
        }
    }
}

@Composable
private fun CloudAccountSection(
    state: WavryUiState,
    onSetAuthServer: (String) -> Unit,
    onSetAuthFormMode: (AuthFormMode) -> Unit,
    onLoginCloud: (String, String) -> Unit,
    onRegisterCloud: (String, String, String) -> Unit,
    onLogoutCloud: () -> Unit,
    onReconnectCloudSignaling: () -> Unit,
) {
    var email by rememberSaveable(state.authEmail, state.isAuthenticated) {
        mutableStateOf(state.authEmail)
    }
    var username by rememberSaveable(state.authUsername, state.isAuthenticated) {
        mutableStateOf(state.authUsername)
    }
    var password by rememberSaveable { mutableStateOf("") }
    val isRegisterMode = state.authFormMode == AuthFormMode.REGISTER
    val trimmedEmail = email.trim()
    val trimmedUsername = username.trim()

    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
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

            if (state.connectivityMode == ConnectivityMode.WAVRY && !state.isAuthenticated) {
                StatusBanner(
                    text = "Cloud mode is selected. Sign in or create an account to connect by username.",
                    isError = false,
                )
            }

            if (state.isAuthenticated) {
                Text(
                    text = "Signed in as ${state.authUsername.ifBlank { state.authEmail }}",
                    style = MaterialTheme.typography.bodyMedium,
                )
                if (state.connectivityMode == ConnectivityMode.WAVRY) {
                    StatusBanner(
                        text = when (state.cloudSignalingState) {
                            CloudSignalingState.CONNECTED -> "Cloud signaling connected."
                            CloudSignalingState.CONNECTING -> "Cloud signaling connecting..."
                            CloudSignalingState.ERROR -> "Cloud signaling unavailable."
                            CloudSignalingState.DISCONNECTED -> "Cloud signaling disconnected."
                        },
                        isError = state.cloudSignalingState == CloudSignalingState.ERROR,
                    )
                    if (state.cloudSignalingState != CloudSignalingState.CONNECTED) {
                        OutlinedButton(
                            onClick = onReconnectCloudSignaling,
                            enabled = !state.isAuthBusy,
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("Reconnect Signaling")
                        }
                    }
                }
                OutlinedButton(
                    onClick = onLogoutCloud,
                    enabled = !state.isAuthBusy,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Sign Out")
                }
            } else {
                SingleChoiceSegmentedButtonRow(
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    SegmentedButton(
                        selected = !isRegisterMode,
                        onClick = { onSetAuthFormMode(AuthFormMode.LOGIN) },
                        enabled = !state.isAuthBusy,
                        shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                        label = { Text("Sign In") },
                    )
                    SegmentedButton(
                        selected = isRegisterMode,
                        onClick = { onSetAuthFormMode(AuthFormMode.REGISTER) },
                        enabled = !state.isAuthBusy,
                        shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                        label = { Text("Create Account") },
                    )
                }

                OutlinedTextField(
                    value = email,
                    onValueChange = { email = it },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = { Text("Email") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                    enabled = !state.isAuthBusy,
                )

                if (isRegisterMode) {
                    OutlinedTextField(
                        value = username,
                        onValueChange = { username = it.take(32) },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        label = { Text("Username") },
                        placeholder = { Text("3-32 chars, letters/numbers/., _, -") },
                        enabled = !state.isAuthBusy,
                    )
                }

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

                if (isRegisterMode) {
                    Text(
                        text = "Password must be at least 8 characters.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                Button(
                    onClick = {
                        if (isRegisterMode) {
                            onRegisterCloud(trimmedEmail, trimmedUsername, password)
                        } else {
                            onLoginCloud(trimmedEmail, password)
                        }
                    },
                    enabled = if (isRegisterMode) {
                        !state.isAuthBusy &&
                            looksLikeEmail(trimmedEmail) &&
                            looksLikeCloudUsername(trimmedUsername) &&
                            password.length >= 8
                    } else {
                        !state.isAuthBusy && looksLikeEmail(trimmedEmail) && password.isNotBlank()
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(if (isRegisterMode) "Create Account" else "Sign In")
                }

                OutlinedButton(
                    onClick = {
                        onSetAuthFormMode(if (isRegisterMode) AuthFormMode.LOGIN else AuthFormMode.REGISTER)
                    },
                    enabled = !state.isAuthBusy,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        if (isRegisterMode) {
                            "Already have an account? Sign in"
                        } else {
                            "Need an account? Create one"
                        },
                    )
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

private data class NavigationItem(
    val tab: AppTab,
    val label: String,
    val icon: ImageVector,
)

private fun cloudSignalingLabel(state: CloudSignalingState): String {
    return when (state) {
        CloudSignalingState.CONNECTED -> "online"
        CloudSignalingState.CONNECTING -> "connecting"
        CloudSignalingState.ERROR -> "offline"
        CloudSignalingState.DISCONNECTED -> "disconnected"
    }
}

private fun looksLikeCloudUsername(value: String): Boolean {
    val trimmed = value.trim().removePrefix("@").trim()
    if (trimmed.length !in 3..32) return false
    return trimmed.all { ch ->
        ch.isLetterOrDigit() || ch == '.' || ch == '_' || ch == '-'
    }
}

private fun looksLikeEmail(value: String): Boolean {
    val trimmed = value.trim()
    val atIndex = trimmed.indexOf('@')
    if (atIndex <= 0 || atIndex >= trimmed.length - 1) return false
    val dotAfterAt = trimmed.indexOf('.', atIndex + 1)
    return dotAfterAt > atIndex + 1 && dotAfterAt < trimmed.length - 1
}

@Composable
private fun StatusBanner(text: String, isError: Boolean) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (isError) {
                MaterialTheme.colorScheme.errorContainer
            } else {
                MaterialTheme.colorScheme.surfaceContainerHigh
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
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
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
