# Apple front end (macOS now, iOS next)

The same prover as the Android app, with the same form on top of it. Nothing in
`shared/native` is Android-specific: the crate builds a `cdylib` for Android, a
`staticlib` for the Apple targets, and a command-line binary for either
desktop. The three interfaces sit side by side —
`shared/native/src/lib.rs` for JNI, `shared/native/src/ffi.rs` for C, and
`shared/native/src/bin/mina.rs` for the terminal — over one prover, one witness
solver and one set of embedded assets.

```
shared/native/          the prover, the assets, JNI + C + CLI entry points
shared/tools/           the o1js-side export and fixture scripts
android/                the Gradle app (Kotlin, one Activity)
ios/Sources/            the SwiftUI screen — macOS and iOS from one source
ios/include/mina.h      the C interface as Swift sees it
ios/build-macos.sh      cargo → swiftc → .app, no Xcode project
ios/xcodeproj.sh        generates the Xcode project for a device build
ios/build-rust.sh       the project's pre-build phase: cargo for the right triple
ios/generate-defaults.sh  form defaults from .env.local, shared by both builds
ios/snapshot.sh         renders the form to a PNG, no window needed
```

## Run it on this Mac

```sh
./ios/build-macos.sh --run
```

That builds `shared/native` for the host, generates the form defaults from
`.env.local` (same file and same keys the Gradle build reads), links one SwiftUI
binary against `libmina_token_mobile.a` and wraps it in
`ios/build/MinaTokenTransfer.app`. Drop `--run` to only print the bundle path.

Prerequisites: Rust (`rustup`) and the Xcode command-line tools
(`xcode-select --install`) — no Xcode project, no signing, no provisioning. The
bundle is ad-hoc signed and runs from `ios/build/`.

The first build is the slow one: it clones and compiles the whole proof-system
stack, which takes tens of minutes. After that the script takes seconds unless
the Rust sources change.

### The screen

Field for field the Android form (`android/.../activity_main.xml` and
`MainActivity.kt`): the same order, labels, `#101018` background and `#8C66FF`
accent, the same two progress bars — the header one scrolls out of sight during
a long proving run, so it is mirrored under the send button — the same key that
locks after a transfer and is only replaced through *Use a different key*, and
the same monospaced result and five-line timing block.

The Rust calls run on one serial queue, which stands in for the Android app's
single-thread executor. Compile is a per-process `OnceLock`, so a second
transfer in the same session pays only the proving time.

Both actions carry an explicit fill (`FormButtonStyle`): the macOS default
bordered style draws its chrome in a near-black grey that vanishes against
`#101018`, which made them invisible for the app's first second, while they are
still disabled. The fields follow `values/transfer_input.xml` in the same
spirit — no box, a `#8C66FF` underline, white text, `#888596` placeholder —
because the stock rounded-border field is a white slab on this form.

Each field also opts out of autocorrection and autocapitalisation, and asks for
the keyboard its content needs. That is not cosmetic on iOS: the default
behaviour capitalises the first letter, which silently invalidates a `B62q…`
address. Android sets `textNoSuggestions` in the layout for the same reason.

### Narrow screens

Three things made the form hang off an iPhone 13 mini's 375 pt, all fixed and
worth knowing before adding to it:

- `.frame(minWidth: 520)` on the root view. On macOS that is a window hint; on
  iOS it is a hard constraint on the view, so every `maxWidth: .infinity`
  control inside was laid out 520 pt wide. It is now behind `#if os(macOS)`.
- The response JSON. A compact JSON payload is one long line with nowhere to
  break, so as a plain `Text` it demanded its full width and the enclosing
  `VStack` — and every control in it — grew to match. It is now re-indented for
  display and sits in a horizontal `ScrollView`, which takes the width it is
  given.
- Single-line truncation on the status and balance lines, where a node error
  message or a transaction hash needs to wrap. `.wrapping()` in
  `FormModifiers.swift`.

Verified on an iPhone 13 mini simulator, backend loaded and a real balance query
answered.

`MainActivity.kt` is the reference for behaviour. Anything visible in
`ios/Sources/TransferView.swift` should match it.

### Looking at the layout without a screen

```sh
./ios/snapshot.sh          # 620×900 into ios/build/snapshot.png
```

Renders the form through `ImageRenderer` — no window, no display, no screen
recording permission, which is what makes it usable over ssh or from a
sandboxed shell. Text fields, the progress bar and the checkbox come out as
yellow placeholder blocks because they are AppKit-backed; the layout around
them and whether a control is legible at all is what the image is for.

`ImageRenderer` draws a `ScrollView` as an empty rectangle, which is why
`TransferView.form` sits outside its scroll container.

## Read a machine's proving budget

The quickest measurement needs no keys, no network and no UI:

```sh
cd shared/native
cargo run --release --bin mina
```

```
compile : 367 ms
witness :  47 ms
proving : 1449 ms
total   : 1863 ms
proof   : 32248 bytes of transaction proof
```

Those are an **Apple M4** (4 P-cores + 6 E-cores, 32 GB, macOS 26.3), idle and
cool. For scale, against the reference desktop (AMD Ryzen 9 7950X, 16 cores /
32 threads):

| | compile | witness | proving |
| --- | --- | --- | --- |
| Apple M4 | 367 ms | 47 ms | 1449 ms |
| Ryzen 9 7950X | 488 ms | 60 ms | 1546 ms |
| **iPhone 13 (A15)** | **~500 ms** | *included* | **~3000 ms** |
| Alldocube tablet (Cortex-A78) | 1561 ms | ~105 ms | 6644 ms |
| Pixel 3 (Snapdragon 845) | 2369 ms | ~200 ms | 10 992 ms (witness included) |

Two results worth separating.

**A 10-core laptop chip matches a 32-thread desktop.** Proving is the parallel
stage, the one that should favour the desktop, yet the M4 and the 7950X land
within 7 %. Per-core throughput is carrying it.

**A phone matches that desktop on compile.** The iPhone 13's ~500 ms is the
7950X's 488 ms, on a chip in a pocket with no fan. Proving is where the phone
pays — ~3 s against 1.5 — but that is 2.2× faster than the A78 tablet and 3.7×
the Pixel 3, and it puts a full on-device transfer inside four seconds of
compute. The A15 figures come from the app itself, so `proving` includes witness
solving; the CLI is what tells the two apart.

**Measure on an idle machine.** The first figures taken here were 617 / 73 /
2951 ms — 1.8× off across every stage, because the run followed half an hour of
`cargo build` saturating all ten cores. Nothing about the prover changed. See
*Temperature dominates everything* in
[../android/README.md](../android/README.md): the same effect, the same size,
and the largest one measured on this app.

The three stages are timed apart on purpose: the app reports witness solving
inside its `Proving` figure, so this is how the two are told apart.

To perform a real transfer from the terminal instead of the form, pass the
request the app's form would collect:

```sh
cargo run --release --bin mina -- --transfer request.json
```

```json
{
  "senderPrivateKey": "EKE...",
  "receiver": "B62q...",
  "amount": "1000000000",
  "tokenAddress": "B62q...",
  "graphqlUrl": "https://mina-devnet-graphql.aurowallet.com/graphql",
  "fundReceiver": true
}
```

It prints the same JSON response the app displays, timings included, and
submits to the node — the code path is literally the one `nativeTransfer`
calls.

A real Devnet transfer from the macOS app, same M4:

```
Compile:   365 ms
Proving:  1521 ms
Signature:   3 ms
Submit:    521 ms
Total:    2679 ms
```

`Total` runs wider than the four stages it lists — here by 269 ms. That gap is
the preflight: parsing the request, deriving the keys and the token id, and the
`fetch_network_snapshot` round trip for the fee-payer nonce and the receiver's
account, all of which happen before the compile timer opens. It is network
latency, not prover time.

## The C interface

`shared/native/src/ffi.rs` mirrors the JNI entry points as `extern "C"`
functions — `mina_backend_info`, `mina_transfer`, `mina_token_balance` and
`mina_string_free`. Each takes a JSON request and returns a JSON response, the
returned pointer is owned by Rust, and a panic inside the prover is caught at
the boundary and turned into `{"status":"error",…}` rather than crossing it.

Swift reaches them through `ios/include/mina.h`, imported as a bridging header
by the build script; `ios/Sources/MinaBackend.swift` is the whole of the glue.

## On the iPhone

The Swift is the same; a device needs a signed bundle, which means an Xcode
target. The project is generated rather than committed — it would otherwise
carry a team id and a machine's paths:

```sh
./ios/xcodeproj.sh
```

```sh
open ios/MinaTokenTransfer.xcodeproj
```

In Xcode, once: select the **MinaTokenTransfer** target → *Signing &
Capabilities* → tick *Automatically manage signing* and pick your team. A free
Apple ID works — add it under *Settings → Accounts* if it is not there. Then
pick your iPhone in the device menu and ⌘R.

On the phone, the first launch of a free-provisioned build needs *Settings →
General → VPN & Device Management → <your Apple ID> → Trust*. Such builds expire
after seven days; rebuilding from Xcode renews them.

Everything else is wired up already:

- The pre-build phase runs `ios/build-rust.sh`, which picks the triple from
  `PLATFORM_NAME` — `aarch64-apple-ios` for the phone,
  `aarch64-apple-ios-sim` for the simulator — and regenerates the form
  defaults. The first build of a new triple compiles the whole proof-system
  stack and takes tens of minutes; the device one is already done here.
- The target links the archive **by path**, per SDK, for the reason in
  *Troubleshooting* below.
- `ios/include/mina.h` is the bridging header, and `ios/Sources/*.swift` are
  the sources — the same files the macOS build uses.

Verified on this checkout: `xcodebuild -sdk iphoneos -configuration Release`
builds and links, producing an 18 MB arm64 bundle with the prover and its
embedded assets inside and no dynamic library to chase.

Measured on an iPhone 13 (A15, 6 cores, 4 GB): **~500 ms compile, ~3 s
proving** — the compile of a 32-thread desktop, and proving between the M4 and
the A78 tablet. See [Read a machine's proving budget](#read-a-machines-proving-budget)
for the full comparison.

Two things to watch on a phone: proving peaks near 560 MB of native heap, which
is comfortable at 4 GB but not free, and a phone throttles — read
[../android/README.md](../android/README.md) on temperature before comparing any
two runs.

### The simulator

Pick one in Xcode's device menu, or drive it from the command line:

```sh
xcrun simctl create "iPhone 13 mini" \
  com.apple.CoreSimulator.SimDeviceType.iPhone-13-mini \
  com.apple.CoreSimulator.SimRuntime.iOS-26-3
```

```sh
xcodebuild -project ios/MinaTokenTransfer.xcodeproj -scheme MinaTokenTransfer \
  -sdk iphonesimulator -configuration Release \
  -derivedDataPath ios/build/DerivedData \
  -destination 'platform=iOS Simulator,name=iPhone 13 mini' \
  CODE_SIGNING_ALLOWED=NO build
```

Useful for layout on a small screen, which is what it was used for here. It runs
at Mac speed, so it measures the Mac and not the phone.

## Troubleshooting

- **`Library not loaded: …libmina_token_mobile.dylib`** — something linked the
  Android `cdylib` instead of the archive. The script passes the `.a` by path
  for exactly this reason; `-lmina_token_mobile` would find the `.dylib` first.
- **A control is invisible on the form** — a stock macOS control style against
  `#101018`. Render it with `ios/snapshot.sh` and give it an explicit fill, the
  way `FormButtonStyle` does for the two actions.
- **`couldn't read …/assets/precomputed/*.bin`** — `build.rs` bakes absolute
  paths into the embedding table, so a moved checkout leaves them stale.
  `touch shared/native/assets/precomputed` reruns it.
- **`was built for newer 'macOS' version`** — the Swift deployment target is
  below the one rustc built the archive for. The script pins both to the host;
  pass `MACOSX_DEPLOYMENT_TARGET` to cargo if you need an older floor.
