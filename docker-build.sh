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

docker build --network host     \
       --build-arg GID=$(id -g) \
       --build-arg=UID=$(id -u) \
       --build-arg=ARCH=${ARCH} \
       -t ${ARCH}-rust-build .
