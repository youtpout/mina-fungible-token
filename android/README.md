# Android native transfer benchmark

This application targets 64-bit Android devices (`arm64-v8a`). Its proving
backend is `mina-runtime`/Pickles compiled as a native Rust shared library. It
does not load WebAssembly.

The sender private key is entered in a password field, is never persisted, and
is cleared after each attempt. Do not use a mainnet key for development.

## Prerequisites

- Android SDK 35 and NDK 27 or newer;
- JDK 17;
- Rust target `aarch64-linux-android`;
- `cargo-ndk`.

## Build

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk
cd android
./gradlew assembleRelease
```

The APK is produced under `app/build/outputs/apk/release/`.

The current UI and JNI transport deliberately keep network submission disabled
inside the Rust backend until the native `FungibleToken.transfer` witness,
proof, and zkApp signatures have all passed parity tests against o1js.
