FROM debian:11

ENV DEBIAN_FRONTEND=noninteractive

RUN apt update -y                       \
    && apt install -y                   \
           curl                         \
           build-essential              \
           libclang-dev                 \
           linux-libc-dev-arm64-cross   \
           gcc-aarch64-linux-gnu        \
    && rm -rf /var/lib/apt/lists/*

ARG USER=docker
ARG UID
ARG GID

ENV HOME /home/${USER}

RUN groupadd -g ${GID} ${USER}
RUN useradd -m -u ${UID} -g ${GID} ${USER}
RUN mkdir -p ${HOME}
RUN chown ${USER}:${USER} ${HOME}

USER ${USER}

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# RUN rustup default stable

ENV PATH="$HOME/.cargo/bin:${PATH}"

RUN rustup target add aarch64-unknown-linux-gnu

RUN mkdir -p ${HOME}/kvm-rust
WORKDIR ${HOME}/kvm-rust

CMD [ /bin/bash ]