#!/bin/sh

docker run --network host -it --rm \
       -e CROSS_COMPILE=aarch64-linux-gnu- -e ARCH=arm64 \
       -v "$(pwd)":"/home/docker/hearth" -w "/home/docker/hearth/linux-7.1.3" \
       aarch64-hearth-build /bin/bash -c 'make defconfig && make'
