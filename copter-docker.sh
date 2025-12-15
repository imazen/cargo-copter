#! /bin/bash


# copter-docker.sh - Safe Docker wrapper for cargo-copter
#
# This script runs cargo-copter inside a Docker container with proper
# security isolation to prevent untrusted dependency code from accessing
# your system.
#
# Usage:
#   ./copter-docker.sh [cargo-copter options]
#
# Examples:
#   ./copter-docker.sh --top-dependents 10
#   ./copter-docker.sh --dependents serde tokio
#   ./copter-docker.sh --crate rgb --test-versions "0.8.50 0.8.51"

set -euo pipefail

# Configuration
IMAGE_NAME="${COPTER_DOCKER_IMAGE:-cargo-copter:local}"
WORKSPACE="$(pwd)"
COPTER_DIR="${COPTER_DIR:-$WORKSPACE/.copter}"
CARGO_HOME_CACHE="${COPTER_CARGO_CACHE:-$COPTER_DIR/docker-cargo}"

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
}

# Build Docker image if needed
build_image() {
    if docker image inspect "$IMAGE_NAME" &>/dev/null; then
        info "Using existing Docker image: $IMAGE_NAME"
        return 0
    fi

    info "Building Docker image: $IMAGE_NAME (this may take a few minutes)"

    # Create temporary Dockerfile
    TEMP_DOCKERFILE=$(mktemp)
    trap "rm -f $TEMP_DOCKERFILE" EXIT

    cat > "$TEMP_DOCKERFILE" <<'DOCKERFILE'
FROM rust:1.92-slim

# Install dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      pkg-config \
      libssl-dev \
      ca-certificates \
      git && \
    rm -rf /var/lib/apt/lists/*

# Create directories with proper permissions
RUN mkdir -p /workspace /copter /cargo-cache && \
    chmod 777 /workspace /copter /cargo-cache

WORKDIR /workspace
DOCKERFILE

    docker build -t "$IMAGE_NAME" -f "$TEMP_DOCKERFILE" . || \
        error "Failed to build Docker image"

    info "Docker image built successfully"
}

# Prepare directories
prepare_directories() {
    mkdir -p "$COPTER_DIR/staging"
    mkdir -p "$CARGO_HOME_CACHE"
}

# Run cargo-copter in Docker
run_copter() {
    local args=("$@")

    info "Running cargo-copter in Docker container..."
    echo "  Workspace: $WORKSPACE"
    echo "  Staging: $COPTER_DIR/staging"
    echo ""

    # Check if user specified --crate or --path in args
    local user_specified_crate=false
    local user_specified_path=false
    for arg in "${args[@]}"; do
        case "$arg" in
            --crate|--crate=*|-c) user_specified_crate=true ;;
            --path|--path=*|-p) user_specified_path=true ;;
        esac
    done

    # Check if we have a local Cargo.toml and should use it
    local use_local_crate=false
    if [ -f "$WORKSPACE/Cargo.toml" ] && [ "$user_specified_crate" = false ] && [ "$user_specified_path" = false ]; then
        use_local_crate=true
    fi

    # Determine TTY flags - only use -it if we have a terminal
    local tty_opts=""
    if [ -t 0 ] && [ -t 1 ]; then
        tty_opts="-it"
    fi

    # Security settings
    local docker_opts=(
        --rm
        $tty_opts
        --user "$(id -u):$(id -g)"
        --volume "$COPTER_DIR:/copter:rw"
        --volume "$CARGO_HOME_CACHE:/cargo-cache:rw"
        --workdir /copter
        --env CARGO_HOME=/cargo-cache
        --env RUST_BACKTRACE=1
        --cpus=4
        --memory=8g
        --network bridge
        --security-opt=no-new-privileges
    )

    # Add workspace mount if we're using a local crate
    if [ "$use_local_crate" = true ]; then
        docker_opts+=(--volume "$WORKSPACE:/workspace:ro")
    fi

    # Build the cargo-copter command with appropriate path flag
    local copter_cmd="cargo-copter --staging-dir /copter/staging"
    if [ "$use_local_crate" = true ]; then
        copter_cmd="$copter_cmd --path /workspace"
    fi

    # Run cargo-copter (install if needed)
    # Capture exit code - non-zero means regressions were found (which is expected)
    local copter_exit=0
    docker run "${docker_opts[@]}" "$IMAGE_NAME" bash -c "
        set -e
        export PATH=\"\$CARGO_HOME/bin:\$PATH\"

        # Install cargo-copter if not already installed
        if ! command -v cargo-copter &> /dev/null; then
            echo '==> Installing cargo-copter from crates.io...'
            cargo install cargo-copter --quiet 2>/dev/null || cargo install cargo-copter
        fi

        echo ''
        $copter_cmd ${args[*]:-}
    " || copter_exit=$?

    # Copy reports from .copter to workspace
    echo ""
    local reports_copied=false
    for report in copter-report.md copter-report.json; do
        if [ -f "$COPTER_DIR/$report" ]; then
            cp "$COPTER_DIR/$report" "$WORKSPACE/" 2>/dev/null && reports_copied=true
        fi
    done
    if [ "$reports_copied" = true ]; then
        info "Reports copied to: $WORKSPACE/"
    fi

    return $copter_exit
}

# Main execution
main() {
    check_prerequisites
    build_image
    prepare_directories
    run_copter "$@"
}

# Show help if requested
if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
    cat <<'EOF'
copter-docker.sh - Safe Docker wrapper for cargo-copter

Usage:
  ./copter-docker.sh [OPTIONS]

This script runs cargo-copter inside a Docker container with security
isolation. All cargo-copter options are supported.

Examples:
  # Test local crate against top dependents
  ./copter-docker.sh --top-dependents 10

  # Test specific published crate versions
  ./copter-docker.sh --crate rgb --test-versions "0.8.50 0.8.51"

  # Test against specific dependents
  ./copter-docker.sh --dependents image serde

Environment Variables:
  COPTER_DOCKER_IMAGE    Docker image name (default: cargo-copter:local)
  COPTER_DIR             Copter data directory (default: ./.copter)
  COPTER_CARGO_CACHE     Cargo cache directory (default: ./.copter/docker-cargo)

Security Features:
  - Read-only workspace mount (your source code is protected)
  - Resource limits (4 CPUs, 8GB RAM)
  - Isolated cargo cache (faster subsequent runs)
  - Container removed after execution
  - No new privileges allowed

The staging directory (.copter/staging) is preserved between runs for
faster builds. Use --clean to purge it.
EOF
    exit 0
fi

main "$@"
