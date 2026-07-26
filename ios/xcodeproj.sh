#!/usr/bin/env bash
#
# Generates ios/MinaTokenTransfer.xcodeproj for a device or simulator build.
# See ios/xcodeproj.rb for what goes into it.
#
#   ios/xcodeproj.sh && open ios/MinaTokenTransfer.xcodeproj
#
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if ! ruby -e "require 'xcodeproj'" >/dev/null 2>&1; then
  echo "the xcodeproj gem is missing: gem install xcodeproj" >&2
  exit 1
fi

# The project references it, and a missing file reference is a build error even
# though the pre-build phase would write it.
"$here/generate-defaults.sh"

ruby "$here/xcodeproj.rb"
