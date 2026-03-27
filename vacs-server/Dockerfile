FROM rust:1.94.1-trixie AS chef

WORKDIR /usr/src/vacs
RUN cargo install cargo-chef --locked

FROM chef AS planner

COPY . .
RUN cargo chef prepare --bin vacs-server --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /usr/src/vacs/recipe.json recipe.json
RUN cargo chef cook --release --bin vacs-server --recipe-path recipe.json

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/usr/local/cargo/git,id=cargo-git \
    cargo build --release --bin vacs-server

FROM debian:13.4-slim AS runtime

LABEL org.opencontainers.image.title="vacs-server" \
      org.opencontainers.image.description="VATSIM ATC Comm System - Signaling Server" \
      org.opencontainers.image.url="https://vacs.network" \
      org.opencontainers.image.documentation="https://github.com/vacs-project/vacs#readme" \
      org.opencontainers.image.source="https://github.com/vacs-project/vacs" \
      org.opencontainers.image.vendor="vacs" \
      org.opencontainers.image.licenses="Apache-2.0 OR MIT"

# Create a non-root user and group
RUN useradd -m -u 10001 -U vacs

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      ca-certificates \
      tzdata \
      curl && \
    rm -rf /var/lib/apt/lists/*

VOLUME ["/etc/vacs-server", "/var/lib/vacs-server/data"]
RUN mkdir -p /etc/vacs-server /var/lib/vacs-server/data/coverage && \
    chown -R vacs:vacs /etc/vacs-server /var/lib/vacs-server/data

COPY --from=builder /usr/src/vacs/target/release/vacs-server /usr/local/bin/vacs-server

EXPOSE 3000
EXPOSE 9200
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
    CMD curl -fsS http://127.0.0.1:3000/health || exit 1

USER vacs
ENTRYPOINT ["/usr/local/bin/vacs-server"]
