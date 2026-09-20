FROM rust:1.98.1-trixie@sha256:a8a5f0a1e5fe7dfe1d352591e4a1c7dd2c08fd70475cae872cf3458ba0df0546 AS chef

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

# Directories the runtime stage cannot create itself: distroless has no shell.
RUN mkdir -p /etc/vacs-server /var/lib/vacs-server/data/coverage

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:e792ab3d241a468a4fd7519ddbbebe66b49b5f365771716ea688ad40b6c6f1c2 AS runtime

LABEL org.opencontainers.image.title="vacs-server" \
      org.opencontainers.image.description="VATSIM ATC Comm System - Signaling Server" \
      org.opencontainers.image.url="https://vacs.network" \
      org.opencontainers.image.documentation="https://github.com/vacs-project/vacs#readme" \
      org.opencontainers.image.source="https://github.com/vacs-project/vacs" \
      org.opencontainers.image.vendor="vacs" \
      org.opencontainers.image.licenses="Apache-2.0 OR MIT"

# nonroot is uid 65532 in the distroless image.
COPY --from=builder --chown=65532:65532 /etc/vacs-server /etc/vacs-server
COPY --from=builder --chown=65532:65532 /var/lib/vacs-server/data /var/lib/vacs-server/data
VOLUME ["/etc/vacs-server", "/var/lib/vacs-server/data"]

COPY --from=builder /usr/src/vacs/target/release/vacs-server /usr/local/bin/vacs-server

EXPOSE 3000
EXPOSE 9200
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
    CMD ["/usr/local/bin/vacs-server", "healthcheck"]

USER nonroot
ENTRYPOINT ["/usr/local/bin/vacs-server"]
