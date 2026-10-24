#!/bin/sh
#
# Build an x86_64 guest kernel with what this VMM and containers (k3s) need:
# x86_64_defconfig plus guest-kernel.config, everything built in.
#
# Usage: ./make-guest-kernel.sh <linux-source> [build-dir]
#
# The kernel is built out of tree (O=build-dir, default build/linux), so the
# source tree must be clean: no .config or build output in it. To build from
# a tree you also develop in, export its HEAD first:
#   git -C ~/linux archive --prefix=linux-src/ HEAD | tar -x -C build
#
# Build dependencies: gcc, make, flex, bison, bc, libelf-dev, libssl-dev.
# Without them on the host, build in a container as your own user:
#   docker run --rm -v "$(pwd)":/work -w /work debian:13 sh -c \
#     "apt-get update && apt-get install -y build-essential flex bison bc \
#      libelf-dev libssl-dev && setpriv --reuid=$(id -u) --regid=$(id -g) \
#      --clear-groups ./make-guest-kernel.sh build/linux-src"

set -e

if [ -z "$1" ]; then
    echo "(!) Usage: $0 <linux-source> [build-dir]" >&2
    exit 1
fi

SRC=$(realpath "$1")
FRAGMENT=$(realpath "$(dirname "$0")/guest-kernel.config")
mkdir -p "${2:-build/linux}"
OUT=$(realpath "${2:-build/linux}")

make -C "$SRC" O="$OUT" x86_64_defconfig
"$SRC"/scripts/kconfig/merge_config.sh -m -O "$OUT" "$OUT/.config" "$FRAGMENT"
make -C "$SRC" O="$OUT" olddefconfig

# olddefconfig silently drops options whose dependencies are not met.
MISSING=$(grep '^CONFIG_' "$FRAGMENT" | while read -r opt; do
    grep -qx "$opt" "$OUT/.config" || echo "$opt"
done)
if [ -n "$MISSING" ]; then
    echo "(!) Options from $FRAGMENT that did not stick:" >&2
    echo "$MISSING" >&2
    exit 2
fi

make -C "$SRC" O="$OUT" -j"$(nproc)" vmlinux

echo "Built $OUT/vmlinux."
