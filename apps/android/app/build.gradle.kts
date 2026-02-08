plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val releaseStoreFile = System.getenv("WAVRY_ANDROID_RELEASE_STORE_FILE")
val releaseStorePassword = System.getenv("WAVRY_ANDROID_RELEASE_STORE_PASSWORD")
val releaseKeyAlias = System.getenv("WAVRY_ANDROID_RELEASE_KEY_ALIAS")
val releaseKeyPassword = System.getenv("WAVRY_ANDROID_RELEASE_KEY_PASSWORD")
val hasReleaseSigningEnv =
    !releaseStoreFile.isNullOrBlank() &&
        !releaseStorePassword.isNullOrBlank() &&
        !releaseKeyAlias.isNullOrBlank() &&
        !releaseKeyPassword.isNullOrBlank()

android {
    namespace = "com.wavry.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.wavry.android"
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = "0.0.1-canary"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            abiFilters += listOf("arm64-v8a")
        }

        externalNativeBuild {
            cmake {
                cppFlags += listOf("-std=c++17")
                abiFilters += listOf("arm64-v8a")
            }
        }

        manifestPlaceholders["appLabel"] = "Wavry Android"
    }

    flavorDimensions += "device"
    productFlavors {
        create("mobile") {
            dimension = "device"
            applicationIdSuffix = ".mobile"
            versionNameSuffix = "-mobile"
            manifestPlaceholders["appLabel"] = "Wavry Android"
            buildConfigField("boolean", "IS_QUEST", "false")
            buildConfigField("boolean", "SUPPORTS_HOST", "false")
            ndk {
                abiFilters += listOf("x86_64")
            }
            externalNativeBuild {
                cmake {
                    abiFilters += listOf("x86_64")
                }
            }
        }

        create("quest") {
            dimension = "device"
            applicationIdSuffix = ".quest"
            versionNameSuffix = "-quest"
            manifestPlaceholders["appLabel"] = "Wavry Quest"
            buildConfigField("boolean", "IS_QUEST", "true")
            buildConfigField("boolean", "SUPPORTS_HOST", "false")
        }
    }

    signingConfigs {
        create("release") {
            if (hasReleaseSigningEnv) {
                storeFile = file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // For local testing, make release APK installable by signing with the debug key.
            // For production, set WAVRY_ANDROID_RELEASE_* env vars to use your real keystore.
            signingConfig =
                if (hasReleaseSigningEnv) {
                    signingConfigs.getByName("release")
                } else {
                    signingConfigs.getByName("debug")
                }
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.09.00")

    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.4")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.4")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.4")
    implementation("androidx.activity:activity-compose:1.9.2")

    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("com.google.android.material:material:1.12.0")
    implementation("com.google.code.gson:gson:2.11.0")

    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
}
