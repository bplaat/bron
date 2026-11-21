# The website and its LAN relay in one static binary, built from the root of the repository.
# Needs apps/web/bastiaan/web/public/winxp, see winxp-image/README.md.

# Static site, without the Windows XP image, which the last layer adds so a new image leaves the builds cached
FROM node:26-alpine AS site
WORKDIR /src
COPY --exclude=apps/web/bastiaan/web/public/winxp . .
RUN --mount=type=cache,target=/root/.npm npm ci --workspace bastiaan && npm run build --workspace bastiaan

# Server, statically linked against musl
FROM rust:1.100-alpine AS server
WORKDIR /src
COPY --exclude=apps/web/bastiaan/web . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p bastiaan && cp target/release/bastiaan /bastiaan

FROM scratch
LABEL org.opencontainers.image.source="https://github.com/bplaat/bron"
LABEL org.opencontainers.image.description="Bastiaan van der Plaat's personal website"
LABEL org.opencontainers.image.licenses=MIT
COPY --from=server /bastiaan /app/bastiaan
COPY --from=site /src/apps/web/bastiaan/web/dist /app/dist
COPY apps/web/bastiaan/web/public/winxp /app/dist/winxp
WORKDIR /app
ENV DIST_PATH=/app/dist
EXPOSE 8080
USER 65534:65534
ENTRYPOINT ["/app/bastiaan"]
