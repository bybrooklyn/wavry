package com.wavry.android.core

class NativeBridge {
    external fun nativeInit()
    external fun nativeInitIdentity(storagePath: String): Int
    external fun nativeGetPublicKeyHex(): String
    external fun nativeVersion(): String
    external fun nativeStartHost(port: Int): Int
    external fun nativeStartClient(host: String, port: Int): Int
    external fun nativeConnectSignaling(url: String, token: String): Int
    external fun nativeSendConnectRequest(username: String): Int
    external fun nativeStop(): Int
    external fun nativeGetStats(): LongArray?
    external fun nativeLastError(): String
    external fun nativeLastCloudStatus(): String

    companion object {
        init {
            System.loadLibrary("wavry_android")
        }
    }
}
