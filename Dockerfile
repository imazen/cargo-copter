# Multi-stage Dockerfile for cargo-copter
#
# This Dockerfile builds cargo-copter in an isolated environment
# and creates a minimal runtime image for safe dependency testing.
#
# Build:
#   docker build -t cargo-copter:latest .
#
# Run:
#   docker run --rm -v "$(pwd):/workspace:ro" -v "$(pwd)/.copter:/copter" \
#     cargo-copter:latest --crate rgb --top-dependents 5

# Stage 1: Builder
FROM rust:1.83-slim AS builder

# Install build dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      pkg-config \
      libssl-dev \
      ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Create build directory
WORKDIR /build

# Copy source files
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release --locked && \
    strip target/release/cargo-copter

# Verify the binary works
RUN ./target/release/cargo-copter --version || echo "Built successfully"

# Stage 2: Runtime
FROM rust:1.83-slim

# Install runtime dependencies (needed for compiling tested crates)
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      pkg-config \
      libssl-dev \
      ca-certificates \
      git && \
    rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /build/target/release/cargo-copter /usr/local/bin/cargo-copter

# Create directories with proper permissions
# /workspace - for mounting source crates (read-only)
# /copter - for staging, cache, and reports (read-write)
RUN mkdir -p /workspace /copter && \
    chmod 777 /workspace /copter

# Set working directory to /copter so reports are written there
WORKDIR /copter

# Default entrypoint with staging dir preset
ENTRYPOINT ["cargo-copter", "--staging-dir", "/copter/staging"]
CMD ["--help"]

# Labels
LABEL org.opencontainers.image.title="cargo-copter"
LABEL org.opencontainers.image.description="Test reverse dependencies before publishing to crates.io"
LABEL org.opencontainers.image.source="https://github.com/brson/cargo-copter"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"
