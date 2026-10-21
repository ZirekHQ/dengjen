plugins { id("com.android.library") }

android {
    namespace = "io.github.zirekhq.dengjen.android.core"
    compileSdk = 35
    defaultConfig {
        minSdk = 27
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
