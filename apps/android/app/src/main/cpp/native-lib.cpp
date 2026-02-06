#include <jni.h>

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

extern "C" JNIEXPORT jint JNICALL
Java_com_wavry_android_core_NativeBridge_nativeStartHost(JNIEnv *, jobject, jint port) {
    if (port <= 0 || port > 65535) {
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
