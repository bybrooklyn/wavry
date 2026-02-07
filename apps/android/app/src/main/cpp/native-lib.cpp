#include <jni.h>
#include <string>

#include "wavry.h"

extern "C" JNIEXPORT void JNICALL
Java_com_wavry_android_core_NativeBridge_nativeInit(JNIEnv *, jobject) {
    wavry_init();
}

extern "C" JNIEXPORT jint JNICALL
Java_com_wavry_android_core_NativeBridge_nativeInitIdentity(
    JNIEnv *env,
    jobject,
    jstring storage_path
) {
    if (storage_path == nullptr) {
        return -1;
    }
    const char *storage_chars = env->GetStringUTFChars(storage_path, nullptr);
    if (storage_chars == nullptr) {
        return -2;
    }
    int rc = wavry_init_identity(storage_chars);
    env->ReleaseStringUTFChars(storage_path, storage_chars);
    return rc;
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_wavry_android_core_NativeBridge_nativeVersion(JNIEnv *env, jobject) {
    const char *version = wavry_version();
    if (version == nullptr) {
        return env->NewStringUTF("unknown");
    }
    return env->NewStringUTF(version);
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_wavry_android_core_NativeBridge_nativeGetPublicKeyHex(JNIEnv *env, jobject) {
    uint8_t key[32] = {0};
    if (wavry_get_public_key(key) != 0) {
        return env->NewStringUTF("");
    }

    static const char kHex[] = "0123456789abcdef";
    std::string out;
    out.reserve(64);
    for (uint8_t byte : key) {
        out.push_back(kHex[(byte >> 4) & 0x0F]);
        out.push_back(kHex[byte & 0x0F]);
    }
    return env->NewStringUTF(out.c_str());
}

extern "C" JNIEXPORT jint JNICALL
Java_com_wavry_android_core_NativeBridge_nativeStartHost(JNIEnv *, jobject, jint port) {
    if (port < 0 || port > 65535) {
        return -10;
    }
    return wavry_start_host(static_cast<uint16_t>(port));
}

extern "C" JNIEXPORT jint JNICALL
Java_com_wavry_android_core_NativeBridge_nativeStartClient(
    JNIEnv *env,
    jobject,
    jstring host,
    jint port
) {
    if (host == nullptr || port <= 0 || port > 65535) {
        return -10;
    }

    const char *host_chars = env->GetStringUTFChars(host, nullptr);
    if (host_chars == nullptr) {
        return -11;
    }

    int rc = wavry_start_client(host_chars, static_cast<uint16_t>(port));
    env->ReleaseStringUTFChars(host, host_chars);
    return rc;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_wavry_android_core_NativeBridge_nativeConnectSignaling(
    JNIEnv *env,
    jobject,
    jstring url,
    jstring token
) {
    if (url == nullptr || token == nullptr) {
        return -10;
    }

    const char *url_chars = env->GetStringUTFChars(url, nullptr);
    if (url_chars == nullptr) {
        return -11;
    }
    const char *token_chars = env->GetStringUTFChars(token, nullptr);
    if (token_chars == nullptr) {
        env->ReleaseStringUTFChars(url, url_chars);
        return -11;
    }

    int rc = wavry_connect_signaling_with_url(url_chars, token_chars);
    env->ReleaseStringUTFChars(token, token_chars);
    env->ReleaseStringUTFChars(url, url_chars);
    return rc;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_wavry_android_core_NativeBridge_nativeSendConnectRequest(
    JNIEnv *env,
    jobject,
    jstring username
) {
    if (username == nullptr) {
        return -10;
    }

    const char *username_chars = env->GetStringUTFChars(username, nullptr);
    if (username_chars == nullptr) {
        return -11;
    }
    int rc = wavry_send_connect_request(username_chars);
    env->ReleaseStringUTFChars(username, username_chars);
    return rc;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_wavry_android_core_NativeBridge_nativeStop(JNIEnv *, jobject) {
    return wavry_stop();
}

extern "C" JNIEXPORT jlongArray JNICALL
Java_com_wavry_android_core_NativeBridge_nativeGetStats(JNIEnv *env, jobject) {
    WavryStats stats{};
    int rc = wavry_get_stats(&stats);
    if (rc != 0) {
        return nullptr;
    }

    jlong values[6] = {
        static_cast<jlong>(stats.connected ? 1 : 0),
        static_cast<jlong>(stats.fps),
        static_cast<jlong>(stats.rtt_ms),
        static_cast<jlong>(stats.bitrate_kbps),
        static_cast<jlong>(stats.frames_encoded),
        static_cast<jlong>(stats.frames_decoded),
    };

    jlongArray arr = env->NewLongArray(6);
    if (arr == nullptr) {
        return nullptr;
    }
    env->SetLongArrayRegion(arr, 0, 6, values);
    return arr;
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_wavry_android_core_NativeBridge_nativeLastError(JNIEnv *env, jobject) {
    char buffer[512] = {0};
    int copied = wavry_copy_last_error(buffer, sizeof(buffer));
    if (copied <= 0) {
        return env->NewStringUTF("");
    }
    return env->NewStringUTF(buffer);
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_wavry_android_core_NativeBridge_nativeLastCloudStatus(JNIEnv *env, jobject) {
    char buffer[512] = {0};
    int copied = wavry_copy_last_cloud_status(buffer, sizeof(buffer));
    if (copied <= 0) {
        return env->NewStringUTF("");
    }
    return env->NewStringUTF(buffer);
}
