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

## Build-time form defaults

If a `.env.local` file exists at the repository root (see `.env.example`),
the build prefills the transfer form with its `MINA_PRIVATE_KEY`,
`MINA_RECEIVER_ADDRESS`, `MINA_TRANSFER_AMOUNT`, `MINA_TOKEN_ADDRESS`, and
`MINA_GRAPHQL_URL` values. The file itself stays untracked, but the values
are compiled into the APK resources — only use throwaway Devnet keys.

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

## Regenerating the compile cache

`native/assets/fungible-token-1.1.0.cache.b64` holds the program's verifier
indexes. The app hands it to the backend, which rebuilds the prover indexes
around it instead of committing every circuit's fixed columns again. Refresh
it whenever the embedded program or the pickles compiler changes:

```sh
cd android/native && MINA_CACHE_OUT=assets/fungible-token-1.1.0.cache.b64 cargo test --release export_program_cache -- --ignored --nocapture
```

The command prints the payload size and the cold/warm compile times, and
fails if the cache would change the verification key. A stale payload is
never fatal at runtime: the backend ignores it and compiles normally.

## Embedding the SRS and Lagrange bases

The remaining startup cost is building the SRS and the Lagrange bases. Those
are the same for every Mina circuit, so they can be exported once and seeded
at startup:

```sh
cd android/native && MINA_SRS_OUT=assets/precomputed cargo test --release export_srs_payloads -- --ignored --nocapture
```

`build.rs` picks up whatever lands in `native/assets/precomputed/` and embeds
it; an empty directory just means they get recomputed on first use, so the
project builds without them. They stay out of version control — about 27 MB,
with the Vesta SRS alone over the limit — and they are reproducible from the
command above.

Note the payloads use the o1js `Cache` format, which writes each curve point
as two decimal strings in JSON: about 169 bytes per point, against 64 bytes
for the same point in a compact binary encoding. The size is encoding
overhead for jsoo interoperability, not extra data.

## Compile cost

Proving needs only the `transfer` branch, but the contract's verification key
is derived from a wrap shared by all eleven methods, so every branch is
compiled to its step verifier. The split is measurable with:

```sh
cargo test --release decompose_compile_time -- --ignored --nocapture
```

On a desktop, cumulatively:

| | compile |
| --- | --- |
| original | 6259 ms |
| pickles stops building a per-branch wrap it then drops | 4576 ms |
| verifier-index cache embedded | 3469 ms |
| SRS and Lagrange bases embedded too | 641 ms |

Measure the last row on any change with:

```sh
cd android/native && cargo test --release measure_startup_compile -- --ignored --nocapture
```

`compiled_token` also caches the program for the life of the process, so a
second transfer in the same session skips compilation entirely.
