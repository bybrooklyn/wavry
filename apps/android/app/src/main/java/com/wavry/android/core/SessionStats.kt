package com.wavry.android.core

data class SessionStats(
    val connected: Boolean = false,
    val fps: Long = 0,
    val rttMs: Long = 0,
    val bitrateKbps: Long = 0,
    val framesEncoded: Long = 0,
    val framesDecoded: Long = 0,
    val jitterMs: Long = 0,
    val packetLoss: Float = 0f,
)
