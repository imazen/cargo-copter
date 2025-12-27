# Transitive Version Resolution Scenarios

This document describes the real behavior of cargo version resolution when testing new versions against existing dependents.

## Scenario Types

### 1. Direct Spec Incompatibility (Patch Not Used)

**Error:** `patch ... was not used in the crate graph`

**Cause:** The dependent requires a spec that doesn't match the offered version.

**Example:**
```toml
# Dependent's Cargo.toml
[dependencies]
rgb = "^0.8"  # Requires 0.8.x
```

When trying to test with `rgb:0.9.0`:
- Cargo sees spec `^0.8`
- `0.9.0` doesn't satisfy `^0.8`
- Patch is ignored, uses latest 0.8.x from crates.io

**Solution:** Force mode (`!!`) - directly edit Cargo.toml to replace spec.

### 2. Transitive Spec Incompatibility (Multiple Versions Error)

**Error:** `note: there are multiple different versions of crate 'X' in the dependency graph`

**Cause:** Direct dependency satisfied, but a transitive dependency requires different version.

**Example:**
```
dependent (requires rgb ^0.8, satisfied by 0.9.0 via patch)
└── some-lib (requires rgb =0.8.50)
    └── rgb (stuck at 0.8.50, can't use patched 0.9.0)
```

When testing with `rgb:0.9.0`:
- Direct dep: `^0.8` accepts 0.9.0 via patch
- Transitive dep: `=0.8.50` rejects 0.9.0
- Result: TWO versions of rgb in tree, type mismatch

**Solution:** Recursive patching (`!!!`) - patch all transitive deps too.

### 3. Transitive Patch Incompatibility (Chain Depth 1-4)

When the incompatibility is buried deep:

```
dependent (rgb ^0.8 - ok)
└── lib-a (rgb ^0.8 - ok)
    └── lib-b (rgb ^0.8 - ok)
        └── lib-c (rgb ^0.8 - ok)
            └── lib-d (rgb =0.8.50 - BLOCKS)
```

Patching needs to propagate through ALL intermediate crates to reach lib-d.

## Marker Notation

In cargo-copter output:

| Marker | Meaning |
|--------|---------|
| (none) | Natural resolution - spec satisfied version |
| `!`    | Force mode - spec replaced in Cargo.toml |
| `!!`   | Patch retry - auto-added [patch.crates-io] after multi-version error |
| `!!!`  | Deep patch - recursive transitive patching (depth 2+) |

## Auto-Retry Logic (Implemented)

When `--force-versions` is used and check fails with "multiple versions" error,
cargo-copter now automatically:

1. **Detect conflict**: Parse error for "there are multiple different versions of crate"
2. **Apply patch**: Restore Cargo.toml, re-apply force override WITH `[patch.crates-io]`
3. **Retry check**: Run cargo check again with unified patching
4. **Track depth**: Mark result with `!!` to indicate auto-patching was applied

This auto-retry behavior:
- Makes the explicit `--patch-transitive` flag unnecessary (deprecated, hidden from help)
- Automatically escalates from `!` to `!!` when multi-version conflict detected
- Reports the patching depth in output markers

## Implementation Status

- [x] Conflict detection in error_extract.rs
- [x] Auto-retry in compile.rs (run_three_step_ict)
- [x] Depth tracking in compile.rs (PatchDepth enum, OfferedVersion.patch_depth)
- [x] Marker display in report.rs (OfferedCell uses patch_depth.marker())
- [x] Simple output with markers (print_simple_dependent_result)
- [x] Deprecation of --patch-transitive (hidden from help, shows warning)

## Resolution Strategy

1. **Try natural resolution first** - if spec accepts version, no patching needed
2. **If "patch not used" warning** - apply `!` (force spec replacement)
3. **If "multiple versions" error** - apply `!!!` (recursive patch)
4. **Track depth** - report how deep patching had to go

## Real Example: image crate with rgb:0.8.91-alpha.3

```
REGRESSION: image:0.25.9 with rgb:0.8.91-alpha.3 [forced] - build failed
  error[E0277]: the trait bound `[u8]: AsPixels<rgb::formats::rgb::Rgb<u8>>` is not satisfied

note: there are multiple different versions of crate `rgb` in the dependency graph
   --> rgb-0.8.91-alpha.3/src/legacy/internal/convert/mod.rs:10:1
    | pub trait AsPixels<PixelType> {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this is the required trait
    |
   ::: src/codecs/avif/encoder.rs:21:5
    | use ravif::{BitDepth, Encoder, Img, RGB8, RGBA8};
    |     ----- one version of crate `rgb` used here, as a dependency of crate `ravif`
    | use rgb::AsPixels;
    |     --- one version of crate `rgb` used here, as a direct dependency
    |
   ::: rgb-0.8.52/src/legacy/internal/convert/mod.rs:14:1
    | pub trait AsPixels<PixelType> {
    | ----------------------------- this is the found trait
```

The conflict:
- `image` directly depends on `rgb` (forced to 0.8.91-alpha.3)
- `ravif` (image's dependency) also depends on `rgb` (stuck at 0.8.52)
- Two different `AsPixels` traits in the type system

To resolve: need to patch `ravif` to also use rgb:0.8.91-alpha.3

## Semver Spec Requirements for Adoption

For crates to naturally adopt new versions (without force/patch), their dependency
specs must be compatible. Here's what different specs allow:

### Version Spec Compatibility Matrix

| Spec in Cargo.toml | Allows 0.8.50 | Allows 0.8.91 | Allows 0.9.0 | Notes |
|--------------------|---------------|---------------|--------------|-------|
| `=0.8.50`          | ✓             | ✗             | ✗            | Exact version pin - blocks all upgrades |
| `^0.8.50`          | ✓             | ✓             | ✗            | Caret allows 0.8.x ≥ 0.8.50 |
| `^0.8`             | ✓             | ✓             | ✗            | Caret allows any 0.8.x |
| `>=0.8, <0.9`      | ✓             | ✓             | ✗            | Explicit range, same as ^0.8 |
| `>=0.8`            | ✓             | ✓             | ✓            | Open-ended, allows any ≥0.8 |
| `*`                | ✓             | ✓             | ✓            | Wildcard, allows any version |

### Recommended Actions by Scenario

#### For Crate Authors (Publishing New Versions)

1. **Breaking change to 0.9.x?**
   - Dependents with `^0.8` will NOT get 0.9.x automatically
   - cargo-copter will show these as regressions with `!` marker
   - Use `--force-versions 0.9.0` to test impact

2. **Semver-compatible patch (0.8.x)?**
   - Dependents with `^0.8` should naturally upgrade
   - No force/patch needed for testing
   - If regressions occur, it's a breaking change disguised as patch

#### For Dependent Crate Maintainers

| Current Spec | Problem | Recommended Change |
|--------------|---------|-------------------|
| `=0.8.50`    | Blocks all updates | Use `^0.8.50` unless exact match required |
| `^0.8.50`    | Good for 0.8.x | Already optimal for patch updates |
| `>=0.8, <0.9` | Verbose | Simplify to `^0.8` |

#### For Transitive Dependencies

The key insight: **ALL crates in the dependency tree must have compatible specs**
for natural resolution to work.

```
app
├── lib-a (rgb = "^0.8")      ✓ Will accept 0.8.91
└── lib-b
    └── lib-c (rgb = "=0.8.50") ✗ Blocks 0.8.91
```

In this case:
- `lib-c` must update their spec from `=0.8.50` to `^0.8`
- Until they do, cargo-copter will show `!!` (auto-patched) results
- The `!!` marker indicates force + transitive patching was needed

### Spec Compatibility for Alpha/Pre-release Versions

| Base Spec | Alpha Version | Allowed? | Notes |
|-----------|---------------|----------|-------|
| `^0.8`    | `0.8.91-alpha.3` | ✓ | Semver pre-release is ≥ than 0.8.0 |
| `^0.8.50` | `0.8.91-alpha.3` | ✓ | Pre-release of 0.8.91 satisfies ^0.8.50 |
| `=0.8.50` | `0.8.91-alpha.3` | ✗ | Exact pin rejects everything else |

### How cargo-copter Helps

1. **Baseline test**: Shows what works with naturally resolved versions
2. **Force test (`!`)**: Shows what WOULD work if specs allowed the version
3. **Patch retry (`!!`)**: Shows if transitive conflicts can be resolved via patching
4. **Report**: Identifies which crates in tree are blocking adoption

Example output interpretation:
```
OK: image:0.25.9 - passed with rgb:0.8.91-alpha.3 [!!]
```
Means: image would work with rgb:0.8.91-alpha.3, but ONLY with force mode and
transitive patching. To adopt naturally, transitive deps (e.g., ravif) need
updated specs.

## Test Fixtures

See the `transitive-depth-*` directories for test fixtures demonstrating:
- `transitive-depth-1/` - Direct transitive with strict spec
- `transitive-depth-2/` - 2 levels deep
- `transitive-depth-3/` - 3 levels deep
- `transitive-depth-4/` - 4 levels deep
- `dependent-transitive-conflict/` - Mixed direct + transitive conflict
