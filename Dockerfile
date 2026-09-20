FROM debian:12

ENV DEBIAN_FRONTEND=noninteractive

ARG ARCH=aarch64

RUN set -e;                                                     \
    case "$ARCH" in                                             \
        aarch64) kernel_arch=arm64 ;;                           \
        riscv64) kernel_arch=riscv64 ;;                         \
        *) echo "Unsupported ARCH: $ARCH" >&2; exit 1 ;;        \
    esac;                                                       \
    apt-get update -y &&                                        \
    apt-get install -y --no-install-recommends                  \
        ca-certificates curl build-essential bison              \
        libclang-dev python3 bc flex libssl-dev                 \
        gcc-${ARCH}-linux-gnu                                   \
        linux-libc-dev-${kernel_arch}-cross;                    \
    rm -rf /var/lib/apt/lists/*

ARG USER=docker
ARG UID
ARG GID

ENV HOME /home/${USER}

RUN groupadd -g ${GID} ${USER} \
    && useradd -m -u ${UID} -g ${GID} ${USER}

USER ${USER}

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

ENV PATH="$HOME/.cargo/bin:${PATH}"

RUN set -e;                                                     \
    case "$ARCH" in                                             \
        aarch64) rust_target=aarch64-unknown-linux-gnu ;;       \
        riscv64) rust_target=riscv64gc-unknown-linux-gnu ;;     \
    esac;                                                       \
    rustup target add "$rust_target"

RUN mkdir -p ${HOME}/kvm-rust
WORKDIR ${HOME}/kvm-rust

CMD [ "/bin/bash" ]