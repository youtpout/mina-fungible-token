#!/usr/bin/env bash
#
# Builds the macOS front end of the mobile prover: `shared/native` compiled
# for the host as a static archive, one SwiftUI screen linked against it, and
# the pair wrapped in a .app bundle. No Xcode project and no signing — the
# bundle runs from the build directory.
#
#   ios/build-macos.sh          build, then print the bundle path
#   ios/build-macos.sh --run    build and launch it
#
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/.." && pwd)
native="$root/shared/native"
build="$here/build"
app="$build/MinaTokenTransfer.app"
name=MinaTokenTransfer
# Matched to the host, which is what rustc targets by default: a lower Swift
# deployment target makes the linker warn about every prebuilt object in the
# archive. Pin MACOSX_DEPLOYMENT_TARGET for cargo too if you need an older one.
target="$(uname -m)-apple-macos$(sw_vers -productVersion)"

# 1. The prover. `--lib` is the staticlib crate-type; the cdylib in the same
#    list is what Android loads, and building both here costs nothing.
echo "==> cargo build --release --lib  (this is the slow one on a cold target/)"
cargo build --release --lib --manifest-path "$native/Cargo.toml"
archive="$native/target/release/libmina_token_mobile.a"
[ -f "$archive" ] || { echo "no static archive at $archive" >&2; exit 1; }

# 2. Form defaults, from .env.local — shared with the Xcode pre-build phase.
"$here/generate-defaults.sh"

# 3. The bundle skeleton.
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>$name</string>
  <key>CFBundleIdentifier</key><string>com.lumina.minatokennative</string>
  <key>CFBundleName</key><string>Mina token transfer</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>$(sw_vers -productVersion)</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# 4. The Swift screen, linked against the archive — by path, since `-l` next
#    to it would pick the Android cdylib sitting in the same directory and the
#    app would then want that .dylib at runtime. `--print native-static-libs`
#    is how the system libraries the crate needs are discovered rather than
#    guessed; it lands in the compiler's stderr as a note.
echo "==> resolving the archive's native dependencies"
link_note=$(cargo rustc --release --lib --manifest-path "$native/Cargo.toml" \
  -- --print native-static-libs 2>&1 | sed -n 's/.*native-static-libs: //p' | tail -n 1 || true)
# swiftc passes -lSystem itself, so drop it here to keep the linker quiet.
read -r -a link_flags <<< "$(printf '%s' "${link_note:--lc -lm}" | sed 's/-lSystem//g')"

echo "==> swiftc $name"
swiftc \
  -O -whole-module-optimization \
  -target "$target" \
  -sdk "$(xcrun --show-sdk-path --sdk macosx)" \
  -import-objc-header "$here/include/mina.h" \
  -o "$app/Contents/MacOS/$name" \
  "$here"/Sources/*.swift \
  "$archive" \
  "${link_flags[@]}"

# Ad-hoc signing keeps Gatekeeper and the keychain quiet on a local build.
codesign --force --sign - "$app" >/dev/null 2>&1 || true

echo "==> $app"
if [ "${1-}" = "--run" ]; then
  open "$app"
fi
