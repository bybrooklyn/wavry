package com.wavry.android.core

import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

data class CloudAuthSession(
    val token: String,
    val userId: String,
    val email: String,
    val username: String,
)

class AuthApiException(message: String) : Exception(message)

object AuthApi {
    private const val DEFAULT_SERVER = "https://auth.wavry.dev"
    private const val CONNECT_TIMEOUT_MS = 10_000
    private const val READ_TIMEOUT_MS = 10_000

    suspend fun register(
        serverBaseUrl: String,
        email: String,
        password: String,
        username: String,
        displayName: String,
        publicKeyHex: String,
    ): CloudAuthSession {
        val payload = JSONObject()
            .put("email", email)
            .put("password", password)
            .put("username", username)
            .put("display_name", displayName)
            .put("public_key", publicKeyHex)
        return requestAuth(serverBaseUrl, "/auth/register", payload)
    }

    suspend fun login(
        serverBaseUrl: String,
        email: String,
        password: String,
    ): CloudAuthSession {
        val payload = JSONObject()
            .put("email", email)
            .put("password", password)
        return requestAuth(serverBaseUrl, "/auth/login", payload)
    }

    fun normalizeServer(raw: String): String {
        val trimmed = raw.trim().ifEmpty { DEFAULT_SERVER }
        return trimmed.trimEnd('/')
    }

    fun signalingWsUrl(serverBaseUrl: String): String {
        val normalized = normalizeServer(serverBaseUrl)
        return try {
            val url = URL(normalized)
            val scheme = if (url.protocol.equals("http", ignoreCase = true)) "ws" else "wss"
            val hostPort =
                if (url.port > 0) "${url.host}:${url.port}" else url.host
            "$scheme://$hostPort/ws"
        } catch (_: Exception) {
            "wss://auth.wavry.dev/ws"
        }
    }

    private suspend fun requestAuth(
        serverBaseUrl: String,
        path: String,
        payload: JSONObject,
    ): CloudAuthSession = withContext(Dispatchers.IO) {
        val server = normalizeServer(serverBaseUrl)
        val url = URL("$server$path")
        val conn = (url.openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            connectTimeout = CONNECT_TIMEOUT_MS
            readTimeout = READ_TIMEOUT_MS
            doOutput = true
            setRequestProperty("Content-Type", "application/json")
            setRequestProperty("Accept", "application/json")
        }

        try {
            conn.outputStream.use { out ->
                out.write(payload.toString().toByteArray(Charsets.UTF_8))
            }

            val statusCode = conn.responseCode
            val body = readBody(
                if (statusCode in 200..299) conn.inputStream else conn.errorStream,
            )
            if (statusCode !in 200..299) {
                throw AuthApiException(extractErrorMessage(body, statusCode))
            }
            parseAuthSession(body)
        } finally {
            conn.disconnect()
        }
    }

    private fun parseAuthSession(body: String): CloudAuthSession {
        val root = JSONObject(body)

        val tokenFromFlat = root.optString("token")
        val userIdFromFlat = root.optString("user_id")
        val emailFromFlat = root.optString("email")
        val usernameFromFlat = root.optString("username")
        if (
            tokenFromFlat.isNotBlank() &&
                userIdFromFlat.isNotBlank() &&
                emailFromFlat.isNotBlank() &&
                usernameFromFlat.isNotBlank()
        ) {
            return CloudAuthSession(
                token = tokenFromFlat,
                userId = userIdFromFlat,
                email = emailFromFlat,
                username = usernameFromFlat,
            )
        }

        val sessionObj = root.optJSONObject("session")
        val userObj = root.optJSONObject("user")
        if (sessionObj != null && userObj != null) {
            val token = sessionObj.optString("token")
            val userId = userObj.optString("id")
            val email = userObj.optString("email")
            val username = userObj.optString("username")
            if (
                token.isNotBlank() &&
                    userId.isNotBlank() &&
                    email.isNotBlank() &&
                    username.isNotBlank()
            ) {
                return CloudAuthSession(
                    token = token,
                    userId = userId,
                    email = email,
                    username = username,
                )
            }
        }

        throw AuthApiException("Server returned an unexpected auth payload.")
    }

    private fun extractErrorMessage(body: String, statusCode: Int): String {
        if (body.isNotBlank()) {
            runCatching {
                val root = JSONObject(body)
                val message = root.optString("error")
                if (message.isNotBlank()) {
                    return message
                }
            }
        }
        return "Authentication request failed (HTTP $statusCode)."
    }

    private fun readBody(stream: InputStream?): String {
        if (stream == null) return ""
        return stream.bufferedReader(Charsets.UTF_8).use { it.readText() }
    }
}
