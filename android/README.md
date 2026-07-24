# Android native transfer benchmark

This application targets 64-bit Android devices (`arm64-v8a`). Its proving
backend is `mina-runtime`/Pickles compiled as a native Rust shared library. It
does not load WebAssembly.

The sender private key is entered in a password field, is never persisted, and
is cleared after each attempt. Do not use a mainnet key for development.

The balance action queries the receiver's derived fungible-token account and
shows its balance in the token's smallest unit. A missing token account is
displayed as a zero balance.

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

## Transfer shape fixture

The exact account-update structure emitted by the fork can be inspected without
connecting to a Mina node or creating a real proof:

```sh
npm run task -- android/tools/export-transfer-shape.ts
npm run task -- android/tools/export-call-data-vector.ts
```

The fixture deploys and exercises the contract on an in-memory local chain. It
uses ephemeral test keys and does not submit a network transaction.

## Network submission

After the witness, proof, and zkApp signatures are generated on-device, the
Rust backend broadcasts the command through the daemon `sendZkapp` GraphQL
mutation on the configured endpoint (the o1js inline-literal format). A
`sent` status with the transaction hash means the node accepted the command
into its pool; inclusion can be verified on
`https://minascan.io/devnet/tx/<hash>`. GraphQL errors and
`failureReason` entries are surfaced as error statuses with the submit
timing populated.

The o1js side of the digest parity check can be recomputed from a command
JSON dump with:

```sh
npm run task -- android/tools/export-account-update-hash.ts <command.json> [updateIndex]
```
