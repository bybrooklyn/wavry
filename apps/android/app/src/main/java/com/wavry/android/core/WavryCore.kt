package com.wavry.android.core

import android.content.Context

class WavryCore(
    context: Context,
    private val native: NativeBridge = NativeBridge(),
) {
    init {
        native.nativeInit()
        native.nativeInitIdentity(context.filesDir.absolutePath)
    }

    fun version(): String = native.nativeVersion()

    fun publicKeyHex(): String = native.nativeGetPublicKeyHex()

    fun startHost(port: Int): Int = native.nativeStartHost(port)

    fun startClient(host: String, port: Int): Int = native.nativeStartClient(host, port)

    fun connectSignaling(url: String, token: String): Int = native.nativeConnectSignaling(url, token)

    fun sendConnectRequest(username: String): Int = native.nativeSendConnectRequest(username)

    fun stop(): Int = native.nativeStop()

    fun lastError(): String = native.nativeLastError().trim()

    fun lastCloudStatus(): String = native.nativeLastCloudStatus().trim()

    fun describeError(code: Int): String {
        val base = messageForCode(code)
        val detail = lastError()
        if (detail.isBlank()) {
            return base
        }
        return if (detail.equals(base, ignoreCase = true)) {
            base
        } else {
            "$base\n$detail"
        }
    }

    fun stats(): SessionStats {
        val raw = native.nativeGetStats() ?: return SessionStats()
        if (raw.size < 6) return SessionStats()

        return SessionStats(
            connected = raw[0] != 0L,
            fps = raw[1],
            rttMs = raw[2],
            bitrateKbps = raw[3],
            framesEncoded = raw[4],
            framesDecoded = raw[5],
        )
    }

    companion object {
        fun messageForCode(code: Int): String {
            return when (code) {
                0 -> "Success"
                -1 -> "A session is already active"
                -2 -> "Invalid argument or failed to initialize runtime"
                -3 -> "Runtime or channel failure"
                -4 -> "Session startup failed"
                -5 -> "Startup failed"
                -10 -> "Invalid host/port values"
                -11 -> "String conversion failure"
                else -> "Operation failed (code $code)"
            }
        }
    }
}
