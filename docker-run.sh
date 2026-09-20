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
       -v "$(pwd)":"/home/docker/kvm-rust" \
       ${ARCH}-rust-build \
       cargo build --release --target=${ARCH}-unknown-linux-gnu
