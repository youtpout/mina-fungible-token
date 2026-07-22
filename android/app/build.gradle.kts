import org.gradle.api.tasks.Exec

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

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
val nativeOutput = layout.projectDirectory.dir("src/main/jniLibs")

val buildRustArm64 by tasks.registering(Exec::class) {
    inputs.files(rootProject.fileTree("native/src"), nativeManifest)
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
