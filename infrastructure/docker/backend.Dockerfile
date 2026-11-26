# syntax=docker/dockerfile:1.7
#
# SOLPRINT backend image. Build context: ./backend (never the repository root,
# so the root .env can never enter the build context or a layer).

ARG RUST_IMAGE=rust:1.99.1-slim-trixie
ARG RUNTIME_IMAGE=debian:trixie-20261123-slim

# ---- build ----------------------------------------------------------------
FROM ${RUST_IMAGE} AS build
WORKDIR /src

# Optional, validated by the application (hex only) before being exposed.
ARG SOLPRINT_GIT_COMMIT=""
ENV SOLPRINT_GIT_COMMIT=${SOLPRINT_GIT_COMMIT} \
    CARGO_TERM_COLOR=never

COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked --bin solprint \
 && install -D -m 0755 target/release/solprint /out/solprint

# ---- runtime --------------------------------------------------------------
FROM ${RUNTIME_IMAGE} AS runtime

RUN groupadd --system --gid 10001 solprint \
 && useradd --system --uid 10001 --gid solprint --no-create-home \
        --home-dir /nonexistent --shell /usr/sbin/nologin solprint

COPY --from=build /out/solprint /usr/local/bin/solprint

USER 10001:10001
ENV APP_HOST=0.0.0.0 \
    APP_ALLOW_NON_LOOPBACK=true \
    APP_PORT=8080 \
    LOG_FORMAT=json
EXPOSE 8080
STOPSIGNAL SIGTERM

HEALTHCHECK --interval=10s --timeout=5s --start-period=30s --retries=3 \
    CMD ["/usr/local/bin/solprint", "healthcheck"]

ENTRYPOINT ["/usr/local/bin/solprint"]
