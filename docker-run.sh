#!/bin/sh

docker run --network host -it --rm \
       -v "$(pwd)":"/home/docker/kvm-rust" \
       arm-rust-build \
       cargo build --release --target=aarch64-unknown-linux-gnu
