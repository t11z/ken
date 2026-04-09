# Multi-stage build for the Ken server.
# Stage 1: Build the binary
# Stage 2: Copy into a minimal runtime image

FROM rust:1.94-slim AS builder

WORKDIR /build
COPY . .

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN cargo build --release -p ken-server

# Runtime stage: distroless with C runtime (needed by sqlx/sqlite)
FROM gcr.io/distroless/cc-debian12

COPY --from=builder /build/target/release/ken-server /usr/local/bin/ken-server
COPY --from=builder /build/crates/ken-server/static /usr/local/share/ken/static

EXPOSE 8443 8444

ENTRYPOINT ["/usr/local/bin/ken-server", "--config", "/etc/ken/ken.toml"]
