#!/bin/sh

docker run --network host -it --rm \
       -v "$(pwd)":"/home/docker/hearth" -w /home/docker/hearth \
       aarch64-hearth-build /bin/bash -c 'cd samples && ./build-arm.sh'
