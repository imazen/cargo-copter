# Running Cargo Crusader in Docker

**SECURITY WARNING**: Cargo Crusader executes arbitrary untrusted code from crates.io. Always run it in an isolated environment like Docker, especially in CI/CD pipelines.

This guide shows how to safely run cargo-crusader inside Docker containers for maximum security isolation.

## Why Docker?

Running cargo-crusader in Docker provides:

1. **Filesystem isolation**: Untrusted code can't access your host filesystem
2. **Network isolation**: Optional `--network none` prevents network access
3. **User isolation**: Runs as non-root user inside container
4. **Cleanup**: Container is destroyed after run, leaving no artifacts
5. **Reproducibility**: Same environment every time

## Quick Start

### Using the provided script

```bash
# Make the script executable
chmod +x crusader-docker.sh

# Run crusader on your current crate
./crusader-docker.sh

# Limit to 10 dependents
./crusader-docker.sh --top-dependents 10

# Test specific crate version
./crusader-docker.sh --path ./my-crate
```

The script will:
1. Build a Docker image with Rust and cargo-crusader
2. Mount your current directory (read-only)
3. Run crusader with the specified options
4. Copy reports to `./crusader-output/`

### Manual Docker usage

```bash
# Build the image
docker build -t cargo-crusader:latest -f Dockerfile .

# Run crusader (current directory must be a Rust crate)
docker run --rm \
  --user $(id -u):$(id -g) \
  --volume "$PWD:/workspace:ro" \
  --volume "$PWD/crusader-output:/output:rw" \
  --env CARGO_HOME=/tmp/cargo \
  cargo-crusader:latest \
  cargo-crusader --top-dependents 5
```

## Dockerfile

Create a `Dockerfile` in your project (or use the one in this repo):

```dockerfile
FROM rust:1.75-slim

# Install dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      pkg-config \
      libssl-dev \
      ca-certificates \
      git && \
    rm -rf /var/lib/apt/lists/*

# Install cargo-crusader from crates.io
# (or copy local binary if preferred)
RUN cargo install cargo-crusader

# Create non-root user
RUN useradd -m -u 1000 crusader && \
    mkdir -p /workspace /output && \
    chown crusader:crusader /workspace /output

USER crusader
WORKDIR /workspace

ENTRYPOINT ["cargo-crusader"]
CMD ["--help"]
```

## Security Best Practices

### 1. Read-only workspace mount

Always mount your source code as read-only:

```bash
--volume "$PWD:/workspace:ro"
```

This prevents any malicious code from modifying your source files.

### 2. Non-root user

Run as a non-privileged user:

```bash
--user $(id -u):$(id -g)
```

This prevents privilege escalation attacks.

### 3. Network isolation (optional)

For maximum isolation, disable network after downloading dependencies:

```bash
docker run --rm \
  --network none \
  # ... other options ...
```

**Note**: This requires pre-downloading dependencies in a separate step.

### 4. Fresh CARGO_HOME

Use a temporary cargo home inside the container:

```bash
--env CARGO_HOME=/tmp/cargo
```

This ensures no credential files are persisted.

### 5. Resource limits

Limit CPU and memory to prevent DoS:

```bash
docker run --rm \
  --cpus=2 \
  --memory=4g \
  --memory-swap=4g \
  # ... other options ...
```

### 6. Read-only root filesystem

Make the root filesystem read-only with explicit write mounts:

```bash
docker run --rm \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid \
  --tmpfs /output:rw \
  # ... other options ...
```

## Complete Secure Example

```bash
#!/bin/bash
set -euo pipefail

# Configuration
IMAGE="cargo-crusader:latest"
WORKSPACE="$(pwd)"
OUTPUT="$WORKSPACE/crusader-output"

# Create output directory
mkdir -p "$OUTPUT"

# Build image if needed
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "Building Docker image..."
  docker build -t "$IMAGE" .
fi

# Run crusader with full security isolation
docker run --rm \
  --user "$(id -u):$(id -g)" \
  --volume "$WORKSPACE:/workspace:ro" \
  --volume "$OUTPUT:/output:rw" \
  --workdir /workspace \
  --env CARGO_HOME=/tmp/cargo \
  --cpus=4 \
  --memory=8g \
  --memory-swap=8g \
  --network bridge \
  "$IMAGE" \
  --top-dependents "${1:-5}" \
  --output /output/crusader-report.html

echo ""
echo "Reports generated in: $OUTPUT"
echo "  - crusader-report.html"
echo "  - crusader-analysis.md"
```

## GitHub Actions Integration

See `.github/workflows/crusader-docker.yml` for a complete example of using Docker in CI.

Key points:
- Build fresh image in the workflow
- Mount workspace read-only
- Output to separate volume
- Upload reports as artifacts
- No secret mounting

## Troubleshooting

### Permission errors

If you get permission errors accessing reports:

```bash
# Run with your user/group IDs
docker run --user $(id -u):$(id -g) ...
```

### Network timeouts

If you get network timeouts downloading crates:

```bash
# Increase timeout or use network bridge
docker run --network bridge ...
```

### Out of disk space

Docker images can be large. Clean up periodically:

```bash
docker system prune -a
```

### Reports not appearing

Ensure the output directory exists and is writable:

```bash
mkdir -p ./crusader-output
chmod 755 ./crusader-output
```

## Advanced Usage

### Custom cargo registry

```bash
docker run --rm \
  --env CARGO_REGISTRIES_MYCRATES_INDEX="https://my-registry.com/index" \
  # ... other options ...
```

### Offline mode with pre-downloaded dependencies

```bash
# Step 1: Download dependencies
docker run --rm \
  --volume "$PWD:/workspace:ro" \
  --volume "$PWD/cargo-cache:/cargo:rw" \
  --env CARGO_HOME=/cargo \
  rust:1.75-slim \
  cargo fetch --manifest-path /workspace/Cargo.toml

# Step 2: Run crusader offline
docker run --rm \
  --network none \
  --volume "$PWD:/workspace:ro" \
  --volume "$PWD/cargo-cache:/cargo:ro" \
  --env CARGO_HOME=/cargo \
  cargo-crusader:latest \
  --top-dependents 5
```

### Multi-stage build for smaller images

```dockerfile
# Builder stage
FROM rust:1.75-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      pkg-config \
      libssl-dev \
      ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/cargo-crusader /usr/local/bin/
RUN useradd -m -u 1000 crusader

USER crusader
WORKDIR /workspace
ENTRYPOINT ["cargo-crusader"]
```

## See Also

- [GitHub Actions Examples](../.github/workflows/)
- [Security Considerations](../README.md#security)
- [crusader-docker.sh](../crusader-docker.sh) - Ready-to-use wrapper script
