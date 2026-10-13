# Build box for the TrimUI Brick: Rust plus TrimUI's own TG5040 cross toolchain
# (gcc 7.5 / glibc 2.25, so binaries run on stock firmware). Used by docker-build.sh.
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl git file make cmake python3 xz-utils gcc libc6-dev \
 && rm -rf /var/lib/apt/lists/*

ARG TOOLCHAIN_URL=https://github.com/trimui/toolchain_sdk_smartpro/releases/download/20231018/aarch64-linux-gnu-7.5.0-linaro.tgz
ARG TOOLCHAIN_SHA256=604d97c1f5fadc88d6ebff577ea648d0fbc650411af98a3bd8783dbe4e3ed154
RUN curl -fsSL -o /tmp/tc.tgz "$TOOLCHAIN_URL" \
 && echo "$TOOLCHAIN_SHA256  /tmp/tc.tgz" | sha256sum -c - \
 && tar -xzf /tmp/tc.tgz -C /opt \
 && rm /tmp/tc.tgz

# Matches rust-toolchain.toml, so rustup never needs to download at build time.
ENV RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:/opt/aarch64-linux-gnu-7.5.0-linaro/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | CARGO_HOME=/usr/local/cargo sh -s -- -y --no-modify-path \
      --profile minimal --default-toolchain 1.96 -c rustfmt,clippy -t aarch64-unknown-linux-gnu \
 && chmod -R a+rX /usr/local/rustup /usr/local/cargo

ENV BRICK_TOOLCHAIN=/opt/aarch64-linux-gnu-7.5.0-linaro \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
