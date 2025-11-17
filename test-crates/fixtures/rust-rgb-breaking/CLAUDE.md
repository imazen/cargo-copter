# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

The `rgb` crate provides strongly-typed pixel structs (e.g., `Rgb<u8>`, `Rgba<u16>`, `Bgr<u8>`) for Rust. It serves as a shared type ecosystem enabling no-copy interoperability between crates that work with pixel data. The crate is intentionally color-space agnostic and unopinionated about color management.

**Current Version**: 0.8.91 (transitional version preparing for 1.0)
- v0.8.x: stable, long-term support
- v0.9: transitional version (mostly backwards-compatible with 0.8, forwards-compatible with 1.0)
- v1.0: planned future release with breaking changes

## Development Commands

### Building and Testing

```bash
# Run all tests with all features
cargo test --all --all-features

# Test with no default features (minimal build)
cargo test --no-default-features

# Test individual features
cargo hack test --each-feature --exclude-features unstable-experimental --exclude-all-features --exclude-no-default-features

# Check all feature combinations
cargo hack check --feature-powerset --no-dev-deps --exclude-features unstable-experimental,defmt-03,grb,argb --exclude-all-features --exclude-no-default-features

# Run examples (require specific features)
cargo run --example example
cargo run --example serde --features serde
```

### Documentation

```bash
# Build and view documentation locally
cargo doc --all-features --open

# The README.md is embedded in lib.rs via include_str!, so changes to README.md affect the crate docs
```

## Architecture

### Trait System (The Core Design)

The crate is built around a trait-based architecture with **no inherent methods** on pixel types. All functionality is provided through traits, allowing users to selectively import only what they need.

**Key Traits** (in `src/pixel_traits/`):

1. **`HetPixel`** (`het_pixel.rs`) - The foundational trait implemented by every pixel type
   - Supports heterogeneous pixels where color and alpha components can have different types (e.g., `Rgba<u8, u16>`)
   - Methods: `to_color_array()`, `each_color_mut()`, `alpha_opt()`, `map_colors()`, `map_alpha()`, etc.
   - Uses associated types like `SelfType<U, V>` and `ColorArray<U>` to work around lack of higher-kinded types

2. **`Pixel`** (`pixel.rs`) - Stricter version of `HetPixel`
   - Requires color and alpha components to be the same type (e.g., `Rgba<u8>`)
   - Methods: `to_array()`, `as_array()`, `as_array_mut()`, `map()`, `try_from_components()`, etc.
   - All `Pixel` implementors also implement `HetPixel` via super-trait bound

3. **`GainAlpha`** (`gain_alpha.rs`) - Adding alpha to pixels
   - `with_alpha()`: add/replace alpha channel
   - `with_default_alpha()`: add alpha with a default value

4. **`HasAlpha`** (`has_alpha.rs`) - Only for pixels with alpha
   - `alpha()` and `alpha_mut()` methods
   - Note: Due to deprecated inherent methods, requires fully qualified syntax: `HasAlpha::alpha(&pixel)`

5. **`ArrayLike`** (`arraylike.rs`) - Internal trait for working with fixed-size arrays

### Pixel Format Organization

All pixel formats live in `src/formats/`:
- `rgb.rs`, `rgba.rs` - Standard RGB formats
- `bgr.rs`, `bgra.rs`, `abgr.rs`, `argb.rs`, `grb.rs` - Alternative layouts
- `gray.rs`, `gray_a.rs`, `gray_alpha.rs`, `gray_a44.rs` - Grayscale formats
- `rgbw.rs` - RGBW (RGB + White) format

Each format file defines the struct and implements traits via macros:
- `without_alpha!` macro - implements `HetPixel` and `Pixel` for formats without alpha
- `with_alpha!` macro - implements `HetPixel` and `Pixel` for formats with alpha

### Legacy Module

`src/legacy/` contains backwards-compatibility code:
- Deprecated inherent methods that will be removed in 1.0
- Old trait implementations (`ComponentBytes`, `ComponentSlice`, etc.)
- These exist to ease migration from 0.8 → 0.9 → 1.0

**Important**: When adding new functionality, prefer the new trait system over legacy code.

### Optional Feature Integration

- **`bytemuck`**: Zero-cost conversions to/from `&[u8]` via `Pod` and `Zeroable` traits
  - Implementation in `src/bytemuck_impl.rs`
  - Requires homogeneous pixels (same type for color and alpha)

- **`num-traits`**: Arithmetic traits (`CheckedAdd`, `SaturatingMul`, etc.)
  - Implementation in `src/num_traits.rs`

- **`serde`**: Serialization support via derive macros on each format struct

- **`defmt-03`**: Embedded debugging support

## Migration Considerations (v0.8 → v1.0)

The crate is in a transitional phase. Be aware of:

1. **Type naming**: `RGB` → `Rgb`, `RGBA` → `Rgba` (type aliases with suffixes like `RGB8` remain)
2. **Grayscale types**: Tuple structs (`.0`, `.1`) → named fields (`.v`, `.a`)
3. **Deprecated methods**:
   - `.alpha()` → `.with_alpha()` (to avoid conflict with `HasAlpha::alpha()`)
   - `.map_c()` → `.map_colors()`
4. **Gray types**: Use `Gray_v09` instead of `Gray` for forward compatibility

## Code Style

- `#[no_std]` compatible (with optional `std` feature for `bytemuck` allocation support)
- Heavy use of macros for DRY trait implementations
- Extensive documentation with examples in every public method
- `#[inline]` on performance-critical methods
- Rust edition 2021, MSRV 1.64

## Testing Strategy

Tests are embedded in individual modules:
- Trait implementations tested in `src/pixel_traits/pixel.rs` (see `as_refs()` test)
- Legacy compatibility tested in `src/legacy/mod.rs`
- Feature-gated tests use `#[cfg(feature = "...")]`
