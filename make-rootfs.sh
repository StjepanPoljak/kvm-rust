#!/bin/sh
#
# Build a small ext4 busybox rootfs image for testing virtio-blk and virtio-net.
#
# Usage: ./make-rootfs.sh [output.img] [size-in-MiB]
#
# BUSYBOX can point to a static busybox for another guest architecture
# (default: the host's /usr/bin/busybox, which must be statically linked).
# No root privileges are needed: the image is populated with mkfs.ext4 -d.

set -e

OUT=${1:-rootfs.img}
SIZE_MB=${2:-16}
BUSYBOX=${BUSYBOX:-/usr/bin/busybox}

if ! file -L "$BUSYBOX" | grep -q "statically linked"; then
    echo "(!) $BUSYBOX is not statically linked." >&2
    exit 1
fi

STAGING=$(mktemp -d)
trap 'rm -rf "$STAGING"' EXIT

mkdir -p "$STAGING"/bin "$STAGING"/sbin "$STAGING"/etc "$STAGING"/proc \
         "$STAGING"/sys "$STAGING"/dev "$STAGING"/tmp "$STAGING"/root \
         "$STAGING"/test

cp "$BUSYBOX" "$STAGING"/bin/busybox
for applet in $("$BUSYBOX" --list); do
    [ "$applet" = busybox ] || ln -s busybox "$STAGING/bin/$applet"
done

# Known data for read tests: 1 MiB of pseudo-random bytes and its checksum,
# large enough to span many virtio-blk requests and descriptor chains.
dd if=/dev/urandom of="$STAGING"/test/random.bin bs=64K count=16 2>/dev/null
(cd "$STAGING"/test && sha256sum random.bin > random.bin.sha256)

cat > "$STAGING"/sbin/init <<'EOF'
#!/bin/sh

mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null

echo "=== virtio-blk rootfs booted ==="
cat /sys/block/vda/size 2>/dev/null | sed 's/^/vda sectors: /'

echo "--- read test"
cd /test && sha256sum -c random.bin.sha256 && cd /

echo "--- write test"
if mount -o remount,rw /; then
    dd if=/dev/urandom of=/test/written.bin bs=4K count=64 2>/dev/null
    sha256sum /test/written.bin > /tmp/written.sha256
    sync
    echo 3 > /proc/sys/vm/drop_caches
    sha256sum -c /tmp/written.sha256
    echo "(remove /test/written.bin and sync before powering off)"
else
    echo "remount rw failed"
fi

# Addresses match the make-tap.sh defaults.
echo "--- net test"
if [ -e /sys/class/net/eth0 ]; then
    ip link set lo up
    ip addr add 192.168.100.2/24 dev eth0
    ip link set eth0 up
    ping -c 3 -W 2 192.168.100.1
else
    echo "no eth0 (start with --tap)"
fi

echo "=== dropping to shell (poweroff -f to exit) ==="
exec setsid cttyhack sh
EOF
chmod +x "$STAGING"/sbin/init

rm -f "$OUT"
mkfs.ext4 -q -F -L rootfs -E root_owner=0:0 -d "$STAGING" "$OUT" "${SIZE_MB}M"

echo "Created $OUT (${SIZE_MB} MiB)."
