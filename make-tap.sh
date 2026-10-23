#!/bin/sh
#
# Create a persistent tap device for testing virtio-net.
#
# Usage: sudo ./make-tap.sh [up|down] [tap-name] [host-ip/prefix]
#
# The tap is owned by the user who invoked sudo, so the VMM can attach to it
# without root: ./rust ... --tap tap0
# In the guest: ip addr add 192.168.100.2/24 dev eth0 && ip link set eth0 up

set -e

ACTION=${1:-up}
TAP=${2:-tap0}
HOST_IP=${3:-192.168.100.1/24}
OWNER=${SUDO_USER:-$(id -un)}

if [ "$(id -u)" -ne 0 ]; then
    echo "(!) Run as root: sudo $0 $*" >&2
    exit 1
fi

case "$ACTION" in
    up)
        if ip link show "$TAP" >/dev/null 2>&1; then
            echo "(!) $TAP already exists, run '$0 down $TAP' first." >&2
            exit 2
        fi
        ip tuntap add dev "$TAP" mode tap user "$OWNER"
        ip addr add "$HOST_IP" dev "$TAP"
        ip link set "$TAP" up
        echo "Created $TAP ($HOST_IP), owned by $OWNER."
        ;;
    down)
        ip tuntap del dev "$TAP" mode tap
        echo "Removed $TAP."
        ;;
    *)
        echo "(!) Unknown action: $ACTION (use up or down)." >&2
        exit 3
        ;;
esac
