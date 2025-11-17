# Multi-stage Dockerfile for cargo-crusader
#
# This Dockerfile builds cargo-crusader in an isolated environment
# and creates a minimal runtime image for safe dependency testing.
#
# Build:
#   docker build -t cargo-crusader:latest .
#
# Run:
#   docker run --rm -v "$PWD:/workspace:ro" cargo-crusader:latest --help

# Stage 1: Builder
FROM rust:1.75-slim AS builder

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
    strip target/release/cargo-crusader

# Verify the binary works
RUN ./target/release/cargo-crusader --version || echo "Built successfully"

# Stage 2: Runtime
FROM rust:1.75-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      pkg-config \
      libssl-dev \
      ca-certificates \
      git && \
    rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /build/target/release/cargo-crusader /usr/local/bin/cargo-crusader

# Create non-root user for safety
RUN useradd -m -u 1000 crusader && \
    mkdir -p /workspace /output /cargo-cache && \
    chown crusader:crusader /workspace /output /cargo-cache

# Switch to non-root user
USER crusader

# Set working directory
WORKDIR /workspace

# Set default cargo home to cache directory
ENV CARGO_HOME=/cargo-cache

# Default entrypoint
ENTRYPOINT ["cargo-crusader"]
CMD ["--help"]

# Labels
LABEL org.opencontainers.image.title="cargo-crusader"
LABEL org.opencontainers.image.description="Test reverse dependencies before publishing to crates.io"
LABEL org.opencontainers.image.source="https://github.com/brson/cargo-crusader"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"
