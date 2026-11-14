# syntax=docker/dockerfile:1
# The website in one static binary. Needs public/winxp, see offline/README.md.

# Static site
FROM node:26-alpine AS site
WORKDIR /src
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY index.html vite.config.js ./
COPY src src
COPY public public
RUN npm run build

# Server, statically linked against musl
FROM rust:1.100-alpine AS server
# hadolint ignore=DL3018
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY server .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked && cp target/release/bastiaan /bastiaan

FROM scratch
LABEL org.opencontainers.image.source="https://github.com/bplaat/bastiaan"
LABEL org.opencontainers.image.description="Bastiaan van der Plaat's personal website"
LABEL org.opencontainers.image.licenses=MIT
COPY --from=server /bastiaan /app/bastiaan
COPY --from=site /src/dist /app/dist
WORKDIR /app
ENV DIST_PATH=/app/dist
EXPOSE 8080
USER 65534:65534
ENTRYPOINT ["/app/bastiaan"]
