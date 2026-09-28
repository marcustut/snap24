#!/usr/bin/env bash
# Builds the snap24-app Rust binary for the architecture Xcode is targeting and
# places it where the app bundle expects its executable.
#
# Adapted from Bevy's `examples/mobile/build_rust_deps.sh`
# (https://github.com/bevyengine/bevy, MIT OR Apache-2.0).

set -eux

PATH=$PATH:$HOME/.cargo/bin

PROFILE=debug
RELFLAG=
if [[ "$CONFIGURATION" != "Debug" ]]; then
    PROFILE=release
    RELFLAG=--release
fi

# Homebrew is not on Xcode's PATH; some build tooling needs it.
export PATH="$PATH:/opt/homebrew/bin"

# Keep cargo artifacts in the workspace target dir so repeat builds are fast and
# the same cache is shared with desktop builds.
export CARGO_TARGET_DIR="$SRCROOT/../../../target"

# Xcode puts its toolchain first on PATH, which breaks `ld: library 'System' not
# found` for Rust link steps (<https://github.com/rust-lang/rust/issues/80817>).
# Reset PATH so the system `cc` is used.
export PATH="/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

IS_SIMULATOR=0
if [ "${LLVM_TARGET_TRIPLE_SUFFIX-}" = "-simulator" ]; then
  IS_SIMULATOR=1
fi

EXECUTABLES=
for arch in $ARCHS; do
  case "$arch" in
    x86_64)
      if [ $IS_SIMULATOR -eq 0 ]; then
        echo "Building for x86_64, but not a simulator build. What's going on?" >&2
        exit 2
      fi
      export CFLAGS_x86_64_apple_ios="-target x86_64-apple-ios"
      TARGET=x86_64-apple-ios
      ;;
    arm64)
      if [ $IS_SIMULATOR -eq 0 ]; then
        TARGET=aarch64-apple-ios
      else
        TARGET=aarch64-apple-ios-sim
      fi
      ;;
  esac

  cargo build $RELFLAG --manifest-path "$SRCROOT/../Cargo.toml" --target $TARGET --bin snap24-app

  EXECUTABLES="$EXECUTABLES $CARGO_TARGET_DIR/$TARGET/$PROFILE/snap24-app"
done

lipo -create -output "$TARGET_BUILD_DIR/$EXECUTABLE_PATH" $EXECUTABLES
