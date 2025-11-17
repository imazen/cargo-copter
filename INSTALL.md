# Installation Guide

## Quick Install (cargo-binstall)

The fastest way to install cargo-copter is using [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), which downloads pre-built binaries:

```bash
# Install cargo-binstall first (if not already installed)
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash

# Then install cargo-copter
cargo binstall cargo-copter
```

## Install from crates.io

```bash
cargo install cargo-copter
```

## Install from source

```bash
git clone https://github.com/imazen/cargo-copter
cd cargo-copter
cargo install --path .
```

## Supported Platforms

Pre-built binaries are available for:

- **Linux**
  - x86_64-unknown-linux-gnu
  - x86_64-unknown-linux-musl (static binary)
  - aarch64-unknown-linux-gnu (ARM64)

- **macOS**
  - x86_64-apple-darwin (Intel)
  - aarch64-apple-darwin (Apple Silicon)

- **Windows**
  - x86_64-pc-windows-msvc

## Verify Installation

```bash
cargo-copter --version
```

## Usage

See [README.md](README.md) for usage examples.
