#!/bin/sh
set -eu
: "${BRICK_TOOLCHAIN:?Set BRICK_TOOLCHAIN to the extracted aarch64-linux-gnu-7.5.0-linaro directory}"
cd "$(dirname "$0")/.."
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$BRICK_TOOLCHAIN/bin/aarch64-linux-gnu-gcc"
export CC_aarch64_unknown_linux_gnu="$BRICK_TOOLCHAIN/bin/aarch64-linux-gnu-gcc"
export AR_aarch64_unknown_linux_gnu="$BRICK_TOOLCHAIN/bin/aarch64-linux-gnu-ar"
"$BRICK_TOOLCHAIN/bin/aarch64-linux-gnu-gcc" -Os -s brick/slot-filecheck.c -o brick/card/Slot/System/slot-filecheck
cargo +1.96.0 build --locked --profile device --target aarch64-unknown-linux-gnu -p slot --no-default-features --features brick
