# syntax=docker/dockerfile:1
#
# docker build -t bugsink-rs .
# docker run -e DATABASE_URL=postgres://... -e BASE_URL=https://bugsink.example.com -p 8000:8000 bugsink-rs
#
# The image runs `bugsink serve` (HTTP on 0.0.0.0:8000 plus in-process workers, serving the built frontend). Every
# command bootstraps or verifies the database first; other commands: `docker run ... bugsink-rs <command>`.

FROM node:22-slim AS frontend
WORKDIR /src
COPY package.json package-lock.json ./
COPY frontend/package.json frontend/
RUN --mount=type=cache,target=/root/.npm npm ci
COPY frontend frontend
RUN npm run build

# musl, so the binary is static and runs on scratch.
FROM rust:1.99-alpine AS backend
RUN apk add --no-cache musl-dev ca-certificates
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY bugsink-db bugsink-db
COPY bugsink-server bugsink-server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin bugsink \
    && cp target/release/bugsink /bugsink

FROM scratch
# Messaging webhooks verify TLS against the system's roots.
COPY --from=backend /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=backend /bugsink /bugsink
COPY --from=frontend /src/frontend/dist /frontend
ENV BUGSINK_BIND=0.0.0.0:8000 \
    BUGSINK_STATIC_DIR=/frontend \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
USER 10001:10001
EXPOSE 8000
ENTRYPOINT ["/bugsink"]
CMD ["serve"]
