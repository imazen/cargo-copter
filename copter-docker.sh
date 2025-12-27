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
IMAGE_NAME="${COPTER_DOCKER_IMAGE:-ghcr.io/imazen/cargo-copter:latest}"
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

# Check if we're in development mode (local script exists in current dir)
is_dev_mode() {
    [ -f "./copter-docker.sh" ] || [ "${COPTER_DEV_MODE:-}" = "1" ]
}

# Build or pull Docker image if needed
build_image() {
    if docker image inspect "$IMAGE_NAME" &>/dev/null; then
        info "Using existing Docker image: $IMAGE_NAME"
        return 0
    fi

    # For default ghcr.io image in non-dev mode, try to pull first
    if [[ "$IMAGE_NAME" == ghcr.io/* ]] && ! is_dev_mode; then
        info "Pulling Docker image: $IMAGE_NAME"
        if docker pull "$IMAGE_NAME"; then
            info "Docker image pulled successfully"
            return 0
        fi
        warn "Failed to pull image from registry, building locally instead"
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

    # Parse --path argument from args
    local user_specified_crate=false
    local user_specified_path=""
    local i=0
    while [ $i -lt ${#args[@]} ]; do
        case "${args[$i]}" in
            --crate|--crate=*|-c) user_specified_crate=true ;;
            --path=*) user_specified_path="${args[$i]#--path=}" ;;
            -p=*) user_specified_path="${args[$i]#-p=}" ;;
            --path|-p)
                if [ $((i+1)) -lt ${#args[@]} ]; then
                    user_specified_path="${args[$((i+1))]}"
                fi
                ;;
        esac
        i=$((i+1))
    done

    # Resolve the path to absolute
    local resolved_path=""
    local container_path=""
    if [ -n "$user_specified_path" ]; then
        resolved_path="$(cd "$(dirname "$user_specified_path")" 2>/dev/null && pwd)/$(basename "$user_specified_path")"
        resolved_path="$(realpath "$user_specified_path" 2>/dev/null || echo "$resolved_path")"
    fi

    # Check if we have a local Cargo.toml and should use it
    local use_local_crate=false
    if [ -f "$WORKSPACE/Cargo.toml" ] && [ "$user_specified_crate" = false ] && [ -z "$user_specified_path" ]; then
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

    # Add workspace mount if we're using a local crate (implicit path)
    if [ "$use_local_crate" = true ]; then
        docker_opts+=(--volume "$WORKSPACE:/workspace:ro")
        container_path="/workspace"
    fi

    # Add mount for explicit --path argument
    if [ -n "$resolved_path" ]; then
        # Mount the path's parent directory to handle both file and directory paths
        local mount_dir="$resolved_path"
        if [ -f "$resolved_path" ]; then
            mount_dir="$(dirname "$resolved_path")"
        fi
        docker_opts+=(--volume "$mount_dir:/external-crate:ro")
        # Compute container path
        if [ -f "$resolved_path" ]; then
            container_path="/external-crate/$(basename "$resolved_path")"
        else
            container_path="/external-crate"
        fi
    fi

    # Build the cargo-copter command with appropriate path flag
    local copter_cmd="cargo-copter --staging-dir /copter/staging"
    if [ -n "$container_path" ]; then
        copter_cmd="$copter_cmd --path $container_path"
    fi

    # Filter out --path from args since we're handling it ourselves
    local filtered_args=()
    local skip_next=false
    for arg in "${args[@]}"; do
        if [ "$skip_next" = true ]; then
            skip_next=false
            continue
        fi
        case "$arg" in
            --path=*|-p=*) continue ;;
            --path|-p) skip_next=true; continue ;;
            *) filtered_args+=("$arg") ;;
        esac
    done

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
        $copter_cmd ${filtered_args[*]:-}
    " || copter_exit=$?

    # Copy reports to workspace
    # Handle both new directory structure (copter-report/) and old flat files
    echo ""
    mkdir -p "$WORKSPACE/copter-report"
    if [ -d "$COPTER_DIR/copter-report" ]; then
        cp -r "$COPTER_DIR/copter-report/"* "$WORKSPACE/copter-report/" 2>/dev/null
        info "Reports copied to: $WORKSPACE/copter-report/"
    elif [ -f "$COPTER_DIR/copter-report.md" ] || [ -f "$COPTER_DIR/copter-report.json" ]; then
        # Old flat file structure (pre-0.3 cargo-copter)
        cp "$COPTER_DIR/copter-report.md" "$WORKSPACE/copter-report/report.md" 2>/dev/null
        cp "$COPTER_DIR/copter-report.json" "$WORKSPACE/copter-report/report.json" 2>/dev/null
        info "Reports copied to: $WORKSPACE/copter-report/"
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
