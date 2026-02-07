package com.wavry.android.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

private val WavryDark = darkColorScheme(
    primary = AccentPrimary,
    onPrimary = TextPrimary,
    primaryContainer = Color(0xFF0D4A93),
    onPrimaryContainer = Color(0xFFD8E3FF),
    secondary = AccentSuccess,
    onSecondary = Color(0xFF00391E),
    secondaryContainer = Color(0xFF14552E),
    onSecondaryContainer = Color(0xFFC4F8D0),
    error = AccentDanger,
    background = BgBase,
    onBackground = TextPrimary,
    surface = BgElevation1,
    onSurface = TextPrimary,
    surfaceVariant = BgElevation2,
    onSurfaceVariant = TextSecondary,
    outline = BgElevation3,
    outlineVariant = BgElevation3,
)

private val WavryLight = lightColorScheme(
    primary = Color(0xFF0B57D0),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFD8E3FF),
    onPrimaryContainer = Color(0xFF001A41),
    secondary = Color(0xFF136C2E),
    onSecondary = Color(0xFFFFFFFF),
    secondaryContainer = Color(0xFFB7F2C3),
    onSecondaryContainer = Color(0xFF00210D),
    error = Color(0xFFB3261E),
    onError = Color(0xFFFFFFFF),
    errorContainer = Color(0xFFF9DEDC),
    onErrorContainer = Color(0xFF410E0B),
    background = Color(0xFFF7F9FC),
    onBackground = Color(0xFF191C21),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF191C21),
    surfaceVariant = Color(0xFFDFE3EC),
    onSurfaceVariant = Color(0xFF434852),
    outline = Color(0xFF737782),
    outlineVariant = Color(0xFFC3C7D0),
)

@Composable
fun WavryTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit,
) {
    val context = LocalContext.current
    val colors = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> WavryDark
        else -> WavryLight
    }

    MaterialTheme(
        colorScheme = colors,
        typography = Typography,
        content = content,
    )
}
