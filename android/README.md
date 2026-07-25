# Android native transfer app

An Android app that proves a `FungibleToken.transfer` **on the phone** and
broadcasts it to a Mina node. The proving backend is `mina-runtime`/Pickles
compiled as a native Rust shared library (`arm64-v8a` only) — there is no
WebAssembly and no JavaScript in the app.

Everything the prover needs is embedded in the APK, so a fresh clone of this
repository is enough: every Rust dependency resolves from a public git branch,
and no other local checkout is required.

## What is pre-embedded in the APK

The whole point of these assets is that the phone never has to derive them.
They are committed and pulled into the binary by `include_bytes!`, so the app
works offline until it submits the transaction.

| Asset | Size | What it is |
| --- | --- | --- |
| `native/assets/fungible-token-1.1.0.json.gz` | 1.1 MB | the recorded circuits of the eleven `FungibleToken` methods, as exported from o1js |
| `native/assets/fungible-token-1.1.0.cache.b64` | 35 KB | the program's **verifier indexes** — the verification-key side of the compile |
| `native/assets/precomputed/srs-{vesta,pallas}.bin` | 6.4 MB | the two structured reference strings |
| `native/assets/precomputed/lagrange-*.bin` | 1.5 MB | the Lagrange bases, per curve and domain size |

`native/build.rs` scans `assets/precomputed/` and emits the embedding table, so
the payloads are optional: an empty directory just means they get recomputed on
first use and the app still builds. With them in place, startup compile drops
from 6.3 s to 0.55 s on a desktop (see [Compile cost](#compile-cost)).

The verification key is **not** trusted from the cache blindly — the export test
fails if the payload would change it, and at runtime a stale payload is ignored
and the program compiles normally.

## Prerequisites

Versions matter here; the ones below are what the project is pinned to.

- **JDK 17.** Newer JDKs break the Android Gradle plugin used here.
- **Android SDK, platform 35** and **NDK 27.3.13750724** exactly (the version
  is pinned in `app/build.gradle.kts`; install that build from the SDK manager,
  or edit `ndkVersion` if you have another NDK 27 patch release).
- **Rust** with the Android target and `cargo-ndk`:

  ```sh
  rustup target add aarch64-linux-android
  cargo install cargo-ndk
  ```

- **adb** (`platform-tools`), to install onto the phone.
- A 64-bit Android device, API 28 or newer, with at least ~1 GB of free RAM for
  the prover. The native heap peaks around 560 MB during proving.

Gradle itself does not need to be installed — the wrapper downloads 8.13.

### Pointing Gradle at the SDK

Either export the standard variable:

```sh
export ANDROID_HOME=$HOME/Android/Sdk
```

or write `android/local.properties` (untracked, per-machine):

```
sdk.dir=/absolute/path/to/Android/Sdk
```

If your JDK is not the default one, export it for the build:

```sh
export JAVA_HOME=/path/to/jdk-17
```

## Build the APK

```sh
git clone -b mobile https://github.com/youtpout/mina-fungible-token
cd mina-fungible-token/android
./gradlew assembleDebug
```

The Gradle build runs `cargo ndk` first (see the `buildRustArm64` task), so the
single command produces both the native library and the APK:

```
app/build/outputs/apk/debug/app-debug.apk   (~31 MB)
```

The first build is slow — it clones and compiles the whole proof-system stack
for `aarch64`, which takes tens of minutes and needs network access. Later
builds reuse `native/target/` and take seconds unless the Rust sources change.

`assembleDebug` is signed with the standard Android debug key, so it installs
directly. `assembleRelease` produces an **unsigned** APK — usable only if you
add your own signing config.

## Copy it to the phone

**Over USB.** Enable *Developer options → USB debugging*, plug the phone in,
accept the authorization prompt, then:

```sh
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

**Over Wi-Fi**, which avoids USB permission problems on Linux (no udev rule
needed). On the phone: *Developer options → Wireless debugging → Pair device
with pairing code*, then from the desktop:

```sh
adb pair <phone-ip>:<pairing-port>
```

```sh
adb connect <phone-ip>:<debug-port>
```

and run the same `adb install -r` command.

**Without adb at all.** Copy `app-debug.apk` to the phone by any means (USB
file transfer, cloud drive, `python3 -m http.server` on the same network) and
open it from the file manager. Android will ask you to allow installing from
that app; the APK is self-contained, so nothing else needs to be side-loaded.

## Using the app

The form takes the sender's private key, the receiver's address, the amount in
the token's smallest unit, the token contract address, and a node GraphQL
endpoint. Press *Send* and the app compiles the program, builds the witness,
proves, signs and submits, reporting each step's timing.

The private key is entered in a password field and **never persisted**. After a
transfer it stays in place so the next one needs no retyping, but the field
locks: replacing it is a deliberate act through *Use a different key*, which
empties the field first. **Do not use a mainnet key.**

The *Check balance* action queries the receiver's derived fungible-token account
and shows its balance in the smallest unit; a missing token account reads as
zero.

Expected timings, for calibration:

| | CPU | RAM | compile | proving | total |
| --- | --- | --- | --- | --- | --- |
| Pixel 3, Android 12 | Snapdragon 845 (4× Kryo 385 Gold 2.8 GHz + 4× Silver 1.77 GHz) | 4 GB | 44.6 s | 13.7 s | 59.1 s |
| desktop reference | AMD Ryzen 9 7950X (16 cores / 32 threads) | 32 GB | 6.3 s | 2.1 s | 9.1 s |

Both rows were measured **before** the verifier-index cache and the SRS payloads
were embedded, so they show the raw cost of the phone against a desktop: about
7× on compile and 6.5× on proving. The desktop compile has since dropped to
547 ms with the assets in place (see [Compile cost](#compile-cost)); the phone
has not been re-measured since, so treat its 44.6 s as an upper bound rather
than what a current build does.

Proving is unaffected by the assets and stays around 13.7 s on the Pixel 3.
Compile is a per-process `OnceLock`, so a second transfer in the same session
skips it entirely and only pays the proving time.

### Prefilling the form at build time

If a `.env.local` file exists at the repository root, the build prefills the
form from it. Copy the template and edit it:

```sh
cp .env.example .env.local
```

It sets `MINA_PRIVATE_KEY`, `MINA_RECEIVER_ADDRESS`, `MINA_TRANSFER_AMOUNT`,
`MINA_TOKEN_ADDRESS` and `MINA_GRAPHQL_URL`. The file stays untracked, but the
values are compiled into the APK resources — **only put throwaway Devnet keys
there**, and do not distribute an APK built this way.

## Network submission

Once the witness, proof and zkApp signatures are generated on-device, the Rust
backend broadcasts the command through the daemon `sendZkapp` GraphQL mutation
(the o1js inline-literal format). A `sent` status with a transaction hash means
the node accepted the command into its pool; inclusion can be checked on
`https://minascan.io/devnet/tx/<hash>`. GraphQL errors and `failureReason`
entries surface as error statuses with the submit timing populated.

Pick the endpoint with care: the minascan Devnet node hangs on `sendZkapp`
(60 s, then 502). `https://mina-devnet-graphql.aurowallet.com/graphql` works and
is the built-in default.

## Regenerating the embedded assets

Both exports are `#[ignore]`d tests, run on the desktop, and both write into
`native/assets/`.

**The verifier-index cache**, whenever the embedded program or the pickles
compiler changes:

```sh
cd android/native && MINA_CACHE_OUT=assets/fungible-token-1.1.0.cache.b64 cargo test --release export_program_cache -- --ignored --nocapture
```

It prints the payload size and the cold/warm compile times, and fails if the
cache would change the verification key.

**The SRS and Lagrange bases**, which are the same for every Mina circuit and
therefore change only with the proof system itself:

```sh
cd android/native && MINA_SRS_OUT=assets/precomputed cargo test --release export_srs_payloads -- --ignored --nocapture
```

These use the compact binary layout (`raw: true`): each curve point is stored as
its two 32-byte coordinates, 64 bytes per point against about 169 in the o1js
`Cache` JSON, which spells them out in decimal. That takes the set from 27 MB to
7.9 MB and, since decoding bytes beats parsing decimal, it also shaves the
startup compile. The trade is that jsoo cannot read these files — which costs
nothing here, as the rust and jsoo caches are not interchangeable anyway.

## Compile cost

Proving needs only the `transfer` branch, but the contract's verification key is
derived from a wrap shared by all eleven methods, so every branch is compiled to
its step verifier. The split is measurable with:

```sh
cargo test --release decompose_compile_time -- --ignored --nocapture
```

Cumulatively, on the desktop reference (AMD Ryzen 9 7950X, 16 cores / 32
threads, 32 GB):

| | compile |
| --- | --- |
| original | 6259 ms |
| pickles stops building a per-branch wrap it then drops | 4576 ms |
| verifier-index cache embedded | 3469 ms |
| SRS and Lagrange bases embedded too | 547 ms |

On a Pixel 3 (Snapdragon 845, 4 GB) only the first row has been measured, at
44 615 ms. The phone tracked the desktop at a steady ~7× on every earlier
benchmark, which would put a current build near 4 s — but that is extrapolation,
not a measurement. Reproduce it on a connected device with a transfer from the
app itself: the reported `compile` timing is this same number, and the run needs
no network until it submits.

Measure the desktop row on any change with:

```sh
cd android/native && cargo test --release measure_startup_compile -- --ignored --nocapture
```

## Fixtures and tools

The account-update structure emitted by the fork can be inspected without a Mina
node or a real proof. The fixture deploys and exercises the contract on an
in-memory local chain with ephemeral keys, and submits nothing:

```sh
npm run task -- android/tools/export-transfer-shape.ts
```

```sh
npm run task -- android/tools/export-call-data-vector.ts
```

The o1js side of the digest parity check can be recomputed from a command JSON
dump with:

```sh
npm run task -- android/tools/export-account-update-hash.ts <command.json> [updateIndex]
```

## Troubleshooting

- **`NDK not configured` / wrong NDK version** — install
  `27.3.13750724` from the SDK manager, or set `ndkVersion` in
  `app/build.gradle.kts` to the NDK you have.
- **`cargo-ndk` not found** — the Gradle task shells out to it; install it and
  make sure `~/.cargo/bin` is on the `PATH` Gradle inherits.
- **`INSTALL_FAILED_NO_MATCHING_ABIS`** — the device or emulator is not
  `arm64-v8a`. The app ships no other ABI on purpose; a 32-bit device cannot
  address enough memory for the prover anyway.
- **The app dies during proving** — out of memory. Close other apps; proving
  peaks near 560 MB of native heap.
- **`adb` cannot see the phone on Linux** — udev rules are often missing for USB
  debugging. Use wireless debugging instead of chasing the rule.
