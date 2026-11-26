plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// One version for every Suppa: the one in the project's Cargo.toml.
val suppaVersion: String = run {
    val toml = rootProject.file("../Cargo.toml").readText()
    Regex("""(?m)^version\s*=\s*"([^"]+)"""").find(toml)?.groupValues?.get(1) ?: "0.0.0"
}
val versionParts = suppaVersion.split(".").map { it.toIntOrNull() ?: 0 } + listOf(0, 0, 0)

android {
    namespace = "com.suppa.browser"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.suppa.browser"
        minSdk = 29
        targetSdk = 35
        versionName = suppaVersion
        versionCode = versionParts[0] * 1_000_000 + versionParts[1] * 1_000 + versionParts[2]
    }

    // build.bat passes the signing key in the environment (never stored in the project).
    val ks = System.getenv("SUPPA_KEYSTORE")
    signingConfigs {
        if (ks != null && file(ks).exists()) {
            create("release") {
                storeFile = file(ks)
                storePassword = System.getenv("SUPPA_KEY_PASSWORD")
                keyAlias = "suppa"
                keyPassword = System.getenv("SUPPA_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    // The interface is the same scripts folder as on Windows and Linux
    // (../../scripts), plus the phone parts in ../scripts.
    sourceSets {
        getByName("main") {
            assets.srcDirs("../../scripts", "../scripts")
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
        buildConfig = true
    }
    packaging {
        resources.excludes += setOf("META-INF/**", "kotlin/**", "DebugProbesKt.bin")
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.12.1")
}
