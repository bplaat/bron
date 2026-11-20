FROM debian:bookworm-slim
# hadolint ignore=DL3008
RUN apt-get update && apt-get install -y --no-install-recommends \
        bison build-essential ca-certificates curl flex gcc-mingw-w64-i686 make texinfo xz-utils \
    && rm -rf /var/lib/apt/lists/*
