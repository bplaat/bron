# Build container
FROM rust:1.100-slim-bookworm AS builder

ARG TARGETARCH

WORKDIR /src

# hadolint ignore=DL3008
RUN apt-get update && \
    apt-get install -y --no-install-recommends curl make perl musl-tools && \
    curl -fsSL https://deb.nodesource.com/setup_26.x -o nodesource_setup.sh && \
    bash nodesource_setup.sh && \
    apt-get install -y --no-install-recommends nodejs && \
    rm -rf /var/lib/apt/lists/*

COPY rust-toolchain.toml .

RUN if [ "$TARGETARCH" = "arm64" ]; then \
        rustup target add aarch64-unknown-linux-musl; \
    else \
        rustup target add x86_64-unknown-linux-musl; \
    fi

COPY . .

RUN if [ "$TARGETARCH" = "arm64" ]; then \
        cargo build --release --target aarch64-unknown-linux-musl --bin plaatnotes && \
        cp target/aarch64-unknown-linux-musl/release/plaatnotes /plaatnotes; \
    else \
        cargo build --release --target x86_64-unknown-linux-musl --bin plaatnotes && \
        cp target/x86_64-unknown-linux-musl/release/plaatnotes /plaatnotes; \
    fi

# Run container
FROM scratch

LABEL org.opencontainers.image.source="https://github.com/bplaat/bron"
LABEL org.opencontainers.image.description="A simple note-taking app"
LABEL org.opencontainers.image.licenses=MIT

COPY --from=builder /plaatnotes /app/plaatnotes

WORKDIR /app

EXPOSE 8080

VOLUME ["/data"]

ENV DATA_PATH=/data

ENTRYPOINT ["/app/plaatnotes"]
