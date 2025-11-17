#!/bin/bash
# crusader-docker.sh - Safe Docker wrapper for cargo-crusader
#
# This script runs cargo-crusader inside a Docker container with proper
# security isolation to prevent untrusted dependency code from accessing
# your system.
#
# Usage:
#   ./crusader-docker.sh [cargo-crusader options]
#
# Examples:
#   ./crusader-docker.sh --top-dependents 10
#   ./crusader-docker.sh --dependents serde,tokio
#   ./crusader-docker.sh --baseline-path ./v1.0.0

set -euo pipefail

# Configuration
IMAGE_NAME="${CRUSADER_DOCKER_IMAGE:-cargo-crusader:local}"
WORKSPACE="$(pwd)"
OUTPUT_DIR="${CRUSADER_OUTPUT_DIR:-$WORKSPACE/crusader-output}"
CARGO_HOME_CACHE="${CRUSADER_CARGO_CACHE:-$WORKSPACE/.crusader/docker-cargo}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Helper functions
info() {
    echo -e "${GREEN}==>${NC} $*"
}

warn() {
    echo -e "${YELLOW}Warning:${NC} $*" >&2
}

error() {
    echo -e "${RED}Error:${NC} $*" >&2
    exit 1
}

# Check prerequisites
check_prerequisites() {
    if ! command -v docker &> /dev/null; then
        error "Docker is not installed. Please install Docker first."
    fi

    if [ ! -f "Cargo.toml" ]; then
        error "No Cargo.toml found in current directory. Please run from a Rust crate directory."
    fi
}

# Build Docker image if needed
build_image() {
    if docker image inspect "$IMAGE_NAME" &>/dev/null; then
        info "Using existing Docker image: $IMAGE_NAME"
        return 0
    fi

    info "Building Docker image: $IMAGE_NAME"

    # Create temporary Dockerfile
    TEMP_DOCKERFILE=$(mktemp)
    trap "rm -f $TEMP_DOCKERFILE" EXIT

    cat > "$TEMP_DOCKERFILE" <<'DOCKERFILE'
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

# Create non-root user
RUN useradd -m -u 1000 crusader && \
    mkdir -p /workspace /output /cargo-cache && \
    chown crusader:crusader /workspace /output /cargo-cache

USER crusader
WORKDIR /workspace

# Note: cargo-crusader will be installed from mounted source
# This keeps the image generic and reusable
DOCKERFILE

    docker build -t "$IMAGE_NAME" -f "$TEMP_DOCKERFILE" . || \
        error "Failed to build Docker image"

    info "Docker image built successfully"
}

# Prepare output directory
prepare_output() {
    mkdir -p "$OUTPUT_DIR"
    chmod 755 "$OUTPUT_DIR"

    # Clean old reports
    rm -f "$OUTPUT_DIR/crusader-report.html" \
          "$OUTPUT_DIR/crusader-analysis.md"
}

# Prepare cargo cache directory
prepare_cargo_cache() {
    mkdir -p "$CARGO_HOME_CACHE"
    chmod 755 "$CARGO_HOME_CACHE"
}

# Run cargo-crusader in Docker
run_crusader() {
    local args=("$@")

    info "Running cargo-crusader in Docker container..."
    info "Workspace: $WORKSPACE (read-only)"
    info "Output: $OUTPUT_DIR"
    info "Cargo cache: $CARGO_HOME_CACHE"

    # Security settings
    local docker_opts=(
        --rm
        --user "$(id -u):$(id -g)"
        --volume "$WORKSPACE:/workspace:ro"
        --volume "$OUTPUT_DIR:/output:rw"
        --volume "$CARGO_HOME_CACHE:/cargo-cache:rw"
        --workdir /workspace
        --env CARGO_HOME=/cargo-cache
        --env RUST_BACKTRACE=1
        --cpus=4
        --memory=8g
        --memory-swap=8g
        --network bridge
        --security-opt=no-new-privileges
    )

    # Run the container
    docker run "${docker_opts[@]}" "$IMAGE_NAME" bash -c "
        set -e

        echo '=== Installing cargo-crusader from source ==='
        cargo build --release --quiet

        echo '=== Running cargo-crusader ==='
        ./target/release/cargo-crusader ${args[*]:-} || EXIT_CODE=\$?

        echo '=== Copying reports to output ==='
        cp -f crusader-report.html /output/ 2>/dev/null || echo 'No HTML report generated'
        cp -f crusader-analysis.md /output/ 2>/dev/null || echo 'No markdown report generated'

        echo '=== Complete ==='
        ls -lh /output/

        exit \${EXIT_CODE:-0}
    " || EXIT_CODE=$?

    return ${EXIT_CODE:-0}
}

# Display results
display_results() {
    echo ""
    info "Reports generated:"

    if [ -f "$OUTPUT_DIR/crusader-report.html" ]; then
        echo "  📄 HTML: $OUTPUT_DIR/crusader-report.html"
    fi

    if [ -f "$OUTPUT_DIR/crusader-analysis.md" ]; then
        echo "  📝 Markdown: $OUTPUT_DIR/crusader-analysis.md"

        # Show summary if available
        if grep -q "## Summary Statistics" "$OUTPUT_DIR/crusader-analysis.md"; then
            echo ""
            info "Summary:"
            sed -n '/## Summary Statistics/,/^## /p' "$OUTPUT_DIR/crusader-analysis.md" | head -15
        fi
    fi

    echo ""
}

# Main execution
main() {
    info "Cargo Crusader Docker Runner"
    echo ""

    check_prerequisites
    build_image
    prepare_output
    prepare_cargo_cache

    if run_crusader "$@"; then
        display_results
        info "Success! All tests completed."
        exit 0
    else
        EXIT_CODE=$?
        display_results
        error "Crusader found issues. Exit code: $EXIT_CODE"
    fi
}

# Show help if requested
if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
    cat <<'EOF'
crusader-docker.sh - Safe Docker wrapper for cargo-crusader

Usage:
  ./crusader-docker.sh [OPTIONS]

This script runs cargo-crusader inside a Docker container with security
isolation. All cargo-crusader options are supported.

Examples:
  ./crusader-docker.sh --top-dependents 10
  ./crusader-docker.sh --dependents serde,tokio
  ./crusader-docker.sh --baseline-path ./v1.0.0

Environment Variables:
  CRUSADER_DOCKER_IMAGE    Docker image name (default: cargo-crusader:local)
  CRUSADER_OUTPUT_DIR      Output directory (default: ./crusader-output)
  CRUSADER_CARGO_CACHE     Cargo cache directory (default: ./.crusader/docker-cargo)

Security Features:
  - Read-only workspace mount
  - Non-root user execution
  - Resource limits (4 CPUs, 8GB RAM)
  - Isolated cargo cache
  - Container removed after execution

For more information, see docs/docker.md
EOF
    exit 0
fi

main "$@"
