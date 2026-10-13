#!/bin/sh
set -eu
: "${BRICK_TOOLCHAIN:?Set BRICK_TOOLCHAIN to the extracted TrimUI toolchain directory}"
cd "$(dirname "$0")/.."
export PATH="$BRICK_TOOLCHAIN/bin:$PATH"
MGBA=e31759b24e7a4e3899285ff720d7b573ac328ae7
GPSP=7b2dd37394e2822110cacef321e9bb291a305b77
mkdir -p vendor brick/core-src
[ -f vendor/mgba-src.tar.gz ] || curl -fL "https://github.com/libretro/mgba/archive/$MGBA.tar.gz" -o vendor/mgba-src.tar.gz
[ -f vendor/gpsp-src.tar.gz ] || curl -fL "https://github.com/libretro/gpsp/archive/$GPSP.tar.gz" -o vendor/gpsp-src.tar.gz
mgba_src="$PWD/brick/core-src/mgba-$MGBA"
gpsp_src="$PWD/brick/core-src/gpsp-$GPSP"
if [ ! -d "$mgba_src" ]; then
    tar --no-same-owner -xzf vendor/mgba-src.tar.gz -C brick/core-src
    for p in cores/mgba/*.patch; do git -C "$mgba_src" apply "$PWD/$p"; done
fi
if [ ! -d "$gpsp_src" ]; then
    tar --no-same-owner -xzf vendor/gpsp-src.tar.gz -C brick/core-src
    for p in cores/gpsp/*.patch brick/gpsp-old-binutils.patch; do git -C "$gpsp_src" apply "$PWD/$p"; done
fi
cmake -S "$mgba_src" -B brick/core-build/mgba \
    -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=aarch64 \
    -DCMAKE_C_COMPILER=aarch64-linux-gnu-gcc -DCMAKE_CXX_COMPILER=aarch64-linux-gnu-g++ \
    -DLIBMGBA_ONLY=ON -DBUILD_LIBRETRO=ON -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_FLAGS=-mcpu=cortex-a53 \
    -DCMAKE_DISABLE_FIND_PACKAGE_PNG=ON -DCMAKE_DISABLE_FIND_PACKAGE_ZLIB=ON \
    -DCMAKE_DISABLE_FIND_PACKAGE_LibZip=ON -DCMAKE_POLICY_VERSION_MINIMUM=3.5
cmake --build brick/core-build/mgba --target mgba_libretro --parallel 8
make -C "$gpsp_src" platform=arm64 CC=aarch64-linux-gnu-gcc CXX=aarch64-linux-gnu-g++ AR=aarch64-linux-gnu-ar -j8
cp brick/core-build/mgba/mgba_libretro.so vendor/
cp "$gpsp_src/gpsp_libretro.so" vendor/
