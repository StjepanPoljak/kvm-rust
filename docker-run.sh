#!/bin/sh

if [ -z ${1} ]; then
    echo "(!) Specify architecture: aarch64, riscv64."
    exit 1
fi

ARCH=${1}

if [ ${ARCH} != aarch64 ] && [ ${ARCH} != riscv64 ]; then
    echo "(!) Unknown architecture: ${ARCH}"
    exit 2
fi

docker run --network host -it --rm \
       -v "$(pwd)":"/home/docker/hearth" -w /home/docker/hearth \
       -e RUSTFLAGS="-C target-feature=+crt-static" \
       ${ARCH}-hearth-build \
       cargo build --release --target=${ARCH}-unknown-linux-gnu
