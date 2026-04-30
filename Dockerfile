# syntax=docker/dockerfile:1.7

FROM rust:1.95.0-slim-bookworm AS builder

ARG DEBIAN_FRONTEND=noninteractive

ENV CARGO_HOME=/workspace/.app_cache/cargo \
    CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_TARGET_DIR=/workspace/.app_cache/target

WORKDIR /workspace

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        clang \
        cmake \
        libcurl4-openssl-dev \
        libssl-dev \
        libsasl2-dev \
        libzstd-dev \
        pkg-config \
        zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock

RUN mkdir -p src && echo "fn main() {}" > src/main.rs

RUN --mount=type=cache,target=/workspace/.app_cache/cargo,sharing=locked \
    --mount=type=cache,target=/workspace/.app_cache/target,sharing=locked \
    cargo build --release --locked

COPY src src

RUN --mount=type=cache,target=/workspace/.app_cache/cargo,sharing=locked \
    --mount=type=cache,target=/workspace/.app_cache/target,sharing=locked \
    touch src/main.rs \
    && cargo build --release --locked \
    && cp /workspace/.app_cache/target/release/mir-azure /workspace/mir-azure-bin

FROM debian:bookworm-slim

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        ghostscript \
        libssl3 \
        libreoffice-writer \
        python3 \
        python3-pip \
        python3-venv \
    && rm -rf /var/lib/apt/lists/*

RUN python3 -m venv /opt/markitdown \
    && /opt/markitdown/bin/pip install --no-cache-dir "markitdown[all]==0.1.5" \
    && ln -s /opt/markitdown/bin/markitdown /usr/local/bin/markitdown

COPY --from=builder /workspace/mir-azure-bin /usr/local/bin/mir-azure

WORKDIR /workspace

ENTRYPOINT ["mir-azure"]
