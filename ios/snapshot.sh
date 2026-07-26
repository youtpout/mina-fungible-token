#!/usr/bin/env bash
#
# Renders the transfer form to a PNG, no window and no screen permissions —
# see ios/tools/snapshot.swift for what the image is and is not good for.
#
#   ios/snapshot.sh                        620×900 into ios/build/snapshot.png
#   ios/snapshot.sh 520 1200 wide.png
#
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
native="$(cd "$here/.." && pwd)/shared/native"
archive="$native/target/release/libmina_token_mobile.a"
build="$here/build"
mkdir -p "$build"

width=${1:-620}
height=${2:-900}
out=${3:-$build/snapshot.png}

# The app's own build produces the archive and the generated defaults; this
# only borrows them, so run build-macos.sh first on a fresh checkout.
[ -f "$archive" ] || { echo "no archive at $archive — run ios/build-macos.sh first" >&2; exit 1; }
[ -f "$here/Sources/Defaults.generated.swift" ] \
  || { echo "no generated defaults — run ios/build-macos.sh first" >&2; exit 1; }

link_note=$(cargo rustc --release --lib --manifest-path "$native/Cargo.toml" \
  -- --print native-static-libs 2>&1 | sed -n 's/.*native-static-libs: //p' | tail -n 1 || true)
read -r -a link_flags <<< "$(printf '%s' "${link_note:--lc -lm}" | sed 's/-lSystem//g')"

swiftc \
  -target "$(uname -m)-apple-macos$(sw_vers -productVersion)" \
  -sdk "$(xcrun --show-sdk-path --sdk macosx)" \
  -import-objc-header "$here/include/mina.h" \
  -o "$build/snapshot" \
  "$here/tools/snapshot.swift" \
  "$here"/Sources/TransferView.swift \
  "$here"/Sources/FormButtonStyle.swift \
  "$here"/Sources/FormModifiers.swift \
  "$here"/Sources/MinaBackend.swift \
  "$here"/Sources/Defaults.generated.swift \
  "$archive" \
  "${link_flags[@]}"

"$build/snapshot" "$width" "$height" "$out"
