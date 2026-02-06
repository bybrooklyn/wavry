package com.wavry.android.core

class NativeBridge {
    external fun nativeInit()
    external fun nativeInitIdentity(storagePath: String): Int
    external fun nativeVersion(): String
    external fun nativeStartHost(port: Int): Int
    external fun nativeStartClient(host: String, port: Int): Int
    external fun nativeStop(): Int
    external fun nativeGetStats(): LongArray?

    companion object {
        init {
            System.loadLibrary("wavry_android")
        }
    }
}
