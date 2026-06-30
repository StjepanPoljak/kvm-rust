#!/bin/sh

docker build --network host \
       --build-arg GID=$(id -g) \
       --build-arg=UID=$(id -u) \
       -t arm-rust-build .
