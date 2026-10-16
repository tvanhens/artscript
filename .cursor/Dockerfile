FROM ubuntu:24.04

ARG BUN_VERSION=1.4.3
ARG UV_VERSION=0.13.1

# skia-safe links against libstdc++, freetype and fontconfig.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
       build-essential ca-certificates curl git pkg-config python3 sudo unzip \
       libfontconfig-dev libfreetype-dev \
    && rm -rf /var/lib/apt/lists/* \
    && echo 'ubuntu ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/ubuntu \
    && curl -LsSf "https://astral.sh/uv/${UV_VERSION}/install.sh" \
       | env UV_INSTALL_DIR=/usr/local/bin UV_NO_MODIFY_PATH=1 sh

USER ubuntu
RUN curl -fsSL https://sh.rustup.rs \
       | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable -c rustfmt -c clippy \
    && curl -fsSL https://bun.sh/install | bash -s "bun-v${BUN_VERSION}"

# install and start run in login shells that never read ~/.bashrc, so tools must be on the default PATH.
USER root
RUN ln -sf /home/ubuntu/.cargo/bin/* /usr/local/bin/ \
    && ln -sf /home/ubuntu/.bun/bin/bun /usr/local/bin/bun \
    && ln -sf /home/ubuntu/.bun/bin/bun /usr/local/bin/bunx

USER ubuntu
