import org.gradle.api.tasks.Exec

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Build-time defaults for the transfer form, read from the repository-root
// .env.local (never committed). The values end up inside the APK, so only
// use throwaway Devnet keys there.
val envLocalFile = rootProject.file("../.env.local")
val envLocalDefaults: Map<String, String> = if (envLocalFile.exists()) {
    envLocalFile.readLines()
        .map { it.trim() }
        .filter { it.isNotEmpty() && !it.startsWith("#") && it.contains("=") }
        .associate { it.substringBefore("=") to it.substringAfter("=") }
} else {
    emptyMap()
}

fun envLocalDefault(name: String, fallback: String = ""): String =
    envLocalDefaults[name] ?: fallback

android {
    namespace = "com.lumina.minatokennative"
    compileSdk = 35
    ndkVersion = "27.3.13750724"

    defaultConfig {
        applicationId = "com.lumina.minatokennative"
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += "arm64-v8a"
        }

        resValue("string", "default_sender_private_key", envLocalDefault("MINA_PRIVATE_KEY"))
        resValue("string", "default_receiver", envLocalDefault("MINA_RECEIVER_ADDRESS"))
        resValue("string", "default_amount", envLocalDefault("MINA_TRANSFER_AMOUNT", "1000000000"))
        resValue("string", "default_token_address", envLocalDefault("MINA_TOKEN_ADDRESS"))
        resValue(
            "string",
            "default_graphql_url",
            envLocalDefault(
                "MINA_GRAPHQL_URL",
                "https://mina-devnet-graphql.aurowallet.com/graphql",
            ),
        )
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions.jvmTarget = "17"
}

val nativeManifest = rootProject.file("native/Cargo.toml")
val nativeLock = rootProject.file("native/Cargo.lock")
val nativeOutput = layout.projectDirectory.dir("src/main/jniLibs")

val buildRustArm64 by tasks.registering(Exec::class) {
    inputs.files(rootProject.fileTree("native/src"), nativeManifest, nativeLock)
    outputs.dir(nativeOutput.dir("arm64-v8a"))
    workingDir(rootProject.file("native"))
    commandLine(
        "cargo", "ndk",
        "--target", "arm64-v8a",
        "--output-dir", nativeOutput.asFile.absolutePath,
        "build", "--release", "--locked",
    )
}

tasks.named("preBuild").configure { dependsOn(buildRustArm64) }
