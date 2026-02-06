package com.wavry.android

import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.text.font.FontWeight
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
                    onSetTab = vm::setActiveTab,
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
    onSetTab: (AppTab) -> Unit,
    onSaveSettings: () -> Unit,
    onCompleteSetup: (String, ConnectivityMode) -> Unit,
    onStart: () -> Unit,
    onStop: () -> Unit,
) {
    val gradient = Brush.verticalGradient(
        colors = listOf(
            MaterialTheme.colorScheme.surface,
            MaterialTheme.colorScheme.surfaceVariant,
            MaterialTheme.colorScheme.surface,
        ),
    )

    if (!state.setupComplete) {
        SetupFlow(
            state = state,
            onCompleteSetup = onCompleteSetup,
            onSetDisplayName = onSetDisplayName,
            onSetConnectivityMode = onSetConnectivityMode,
        )
        return
    }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.surface,
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(if (state.isQuestBuild) "Wavry Quest" else "Wavry Android")
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
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                AssistChip(
                    onClick = { },
                    enabled = false,
                    label = { Text("${if (state.connectivityMode == ConnectivityMode.WAVRY) "Cloud" else "LAN"} mode") },
                )
                AssistChip(
                    onClick = { },
                    enabled = false,
                    label = {
                        Text(
                            if (state.isRunning) {
                                if (state.stats.connected) "Connected" else "Running"
                            } else {
                                "Idle"
                            },
                        )
                    },
                )
            }

            TabRow(
                selectedTabIndex = if (state.activeTab == AppTab.SESSION) 0 else 1,
                containerColor = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Tab(
                    selected = state.activeTab == AppTab.SESSION,
                    onClick = { onSetTab(AppTab.SESSION) },
                    text = { Text("Session") },
                )
                Tab(
                    selected = state.activeTab == AppTab.SETTINGS,
                    onClick = { onSetTab(AppTab.SETTINGS) },
                    text = { Text("Settings") },
                )
            }

            if (state.activeTab == AppTab.SESSION) {
                SessionTab(
                    state = state,
                    onSetMode = onSetMode,
                    onSetHost = onSetHost,
                    onSetPort = onSetPort,
                    onStart = onStart,
                    onStop = onStop,
                )
            } else {
                SettingsTab(
                    state = state,
                    onSetDisplayName = onSetDisplayName,
                    onSetConnectivityMode = onSetConnectivityMode,
                    onSetHost = onSetHost,
                    onSetPort = onSetPort,
                    onSaveSettings = onSaveSettings,
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
) {
    var step by rememberSaveable { mutableIntStateOf(0) }
    var localName by rememberSaveable { mutableStateOf(state.displayName.ifBlank { "My Android" }) }
    var localConnectivity by rememberSaveable { mutableStateOf(state.connectivityMode) }

    Surface(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Card(
            modifier = Modifier.fillMaxSize(),
            shape = RoundedCornerShape(24.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(22.dp),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    Surface(
                        modifier = Modifier
                            .width(56.dp)
                            .height(56.dp),
                        shape = CircleShape,
                        color = MaterialTheme.colorScheme.primary,
                    ) {
                        Column(
                            modifier = Modifier.fillMaxSize(),
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Text(
                                text = "W",
                                modifier = Modifier.padding(start = 20.dp),
                                style = MaterialTheme.typography.titleLarge,
                                color = MaterialTheme.colorScheme.onPrimary,
                                fontWeight = FontWeight.SemiBold,
                            )
                        }
                    }

                    Text(
                        text = "Welcome to Wavry",
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.SemiBold,
                    )

                    Text(
                        text = when (step) {
                            0 -> "Low-latency remote desktop streaming on Android."
                            1 -> "Choose the device name shown in session screens."
                            else -> "Pick your default connectivity path."
                        },
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )

                    when (step) {
                        0 -> {
                            Card(
                                modifier = Modifier.fillMaxWidth(),
                                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                            ) {
                                Text(
                                    text = "This app is client-focused on Android and Quest. Enter host IP, connect, and monitor session health from one screen.",
                                    modifier = Modifier.padding(14.dp),
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
                        }
                    }
                }

                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    if (step > 0) {
                        Button(onClick = { step -= 1 }) {
                            Text("Back")
                        }
                    }
                    Spacer(modifier = Modifier.weight(1f))
                    if (step < 2) {
                        Button(
                            onClick = {
                                if (step == 1 && localName.trim().isEmpty()) {
                                    localName = "My Android"
                                }
                                step += 1
                            },
                        ) {
                            Text("Continue")
                        }
                    } else {
                        Button(
                            onClick = {
                                val finalName = localName.trim().ifEmpty { "My Android" }
                                onSetDisplayName(finalName)
                                onSetConnectivityMode(localConnectivity)
                                onCompleteSetup(finalName, localConnectivity)
                            },
                        ) {
                            Text("Finish Setup")
                        }
                    }
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
    onStart: () -> Unit,
    onStop: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(
                text = "Session Control",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )

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
                    label = { Text("Host") },
                    placeholder = { Text("192.168.1.20 or host.local:8000") },
                    enabled = !state.isBusy && !state.isRunning,
                )
            }

            OutlinedTextField(
                value = state.portText,
                onValueChange = onSetPort,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("UDP Port") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                enabled = !state.isBusy && !state.isRunning,
            )

            if (!state.isRunning) {
                Button(onClick = onStart, enabled = !state.isBusy) {
                    Text(if (state.mode == ConnectionMode.HOST) "Start Hosting" else "Connect")
                }
            } else {
                Button(onClick = onStop, enabled = !state.isBusy) {
                    Text("Stop")
                }
            }

            if (state.errorMessage.isNotBlank()) {
                Text(
                    text = state.errorMessage,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }

            Text(
                text = state.statusMessage,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }

    if (state.isQuestBuild) {
        QuestVrCard()
    }

    StatsCard(state)
}

@Composable
private fun SettingsTab(
    state: WavryUiState,
    onSetDisplayName: (String) -> Unit,
    onSetConnectivityMode: (ConnectivityMode) -> Unit,
    onSetHost: (String) -> Unit,
    onSetPort: (String) -> Unit,
    onSaveSettings: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text(
                text = "Profile & Defaults",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
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

            Button(onClick = onSaveSettings) {
                Text("Save Settings")
            }

            Text(
                text = "Cloud mode retains the desktop parity path, but Android currently uses direct host connection flow.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun ModeSelector(
    selected: ConnectionMode,
    supportsHost: Boolean,
    onSetMode: (ConnectionMode) -> Unit,
    enabled: Boolean,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        if (supportsHost) {
            FilterChip(
                selected = selected == ConnectionMode.HOST,
                onClick = { onSetMode(ConnectionMode.HOST) },
                label = { Text("Host") },
                enabled = enabled,
            )
        }
        FilterChip(
            selected = selected == ConnectionMode.CLIENT,
            onClick = { onSetMode(ConnectionMode.CLIENT) },
            label = { Text("Client") },
            enabled = enabled,
        )
    }
}

@Composable
private fun ConnectivitySelector(
    selected: ConnectivityMode,
    enabled: Boolean,
    onSetConnectivityMode: (ConnectivityMode) -> Unit,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        FilterChip(
            selected = selected == ConnectivityMode.DIRECT,
            onClick = { onSetConnectivityMode(ConnectivityMode.DIRECT) },
            label = { Text("LAN") },
            enabled = enabled,
        )
        FilterChip(
            selected = selected == ConnectivityMode.WAVRY,
            onClick = { onSetConnectivityMode(ConnectivityMode.WAVRY) },
            label = { Text("Cloud") },
            enabled = enabled,
        )
    }
}

@Composable
private fun QuestVrCard() {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                text = "Quest VR",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = "Quest build keeps VR launch metadata and hand-tracking capability flags enabled.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun StatsCard(state: WavryUiState) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = "Session Stats",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
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
            modifier = Modifier.width(140.dp),
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
