#!/usr/bin/env bash
#
# Builds the prover for whatever Xcode is currently targeting, and refreshes the
# form defaults. This is the project's pre-build phase; run it by hand only to
# see what Xcode would do.
#
#   PLATFORM_NAME=iphoneos ios/build-rust.sh
#
# The first build of a new triple compiles the whole proof-system stack and
# takes tens of minutes. Later ones are incremental, per triple.
#
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
native="$(cd "$here/.." && pwd)/shared/native"

# Xcode's PATH does not include a rustup install.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

case "${PLATFORM_NAME:-macosx}" in
  iphoneos) triple=aarch64-apple-ios ;;
  iphonesimulator) triple=aarch64-apple-ios-sim ;;
  macosx) triple=$(uname -m)-apple-darwin ;;
  *) echo "unknown PLATFORM_NAME ${PLATFORM_NAME}" >&2; exit 1 ;;
esac

"$here/generate-defaults.sh"

# Xcode sets deployment-target variables that cargo would otherwise inherit and
# apply to the host build scripts, which then fail to run.
unset IPHONEOS_DEPLOYMENT_TARGET MACOSX_DEPLOYMENT_TARGET SDKROOT

echo "building $triple"
cargo build --release --lib --locked --manifest-path "$native/Cargo.toml" --target "$triple"
