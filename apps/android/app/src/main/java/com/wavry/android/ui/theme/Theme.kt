package com.wavry.android.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable

private val WavryDark = darkColorScheme(
    primary = AccentPrimary,
    onPrimary = TextPrimary,
    secondary = AccentSuccess,
    error = AccentDanger,
    background = BgBase,
    onBackground = TextPrimary,
    surface = BgBase,
    onSurface = TextPrimary,
    surfaceVariant = BgElevation1,
    onSurfaceVariant = TextSecondary,
    outlineVariant = BgElevation3,
)

@Composable
fun WavryTheme(
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = WavryDark,
        typography = Typography,
        content = content,
    )
}
