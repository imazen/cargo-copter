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

## Auto-Retry Logic (Planned)

When `--force` is used and check fails with "multiple versions" error:

1. **Detect conflict**: Parse error for "there are multiple different versions of crate"
2. **Extract deps**: Find which crates are using the wrong version (e.g., `ravif`)
3. **Apply patch**: Add `[patch.crates-io]` section to unify all transitive deps
4. **Retry check**: Run cargo check again with patching
5. **Track depth**: If still fails, identify deeper transitive deps and repeat

This auto-retry will:
- Deprecate the explicit `--patch-transitive` flag
- Automatically escalate from `!` to `!!` to `!!!` as needed
- Report the final patching depth in output

## Implementation Status

- [x] Conflict detection in error_extract.rs
- [ ] Auto-retry in compile.rs
- [ ] Depth tracking in types.rs
- [ ] Marker display in report.rs
- [ ] Deprecation of --patch-transitive

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

## Semver Implications

For crates to work with new rgb versions, they need:

| Current Spec | New Spec Needed | Notes |
|--------------|-----------------|-------|
| `=0.8.50`    | `^0.8`          | Allow any 0.8.x |
| `^0.8.50`    | `^0.8`          | Already compatible |
| `0.8`        | `>=0.8`         | If supporting 0.9+ |

## Test Fixtures

See the `transitive-depth-*` directories for test fixtures demonstrating:
- `transitive-depth-1/` - Direct transitive with strict spec
- `transitive-depth-2/` - 2 levels deep
- `transitive-depth-3/` - 3 levels deep
- `transitive-depth-4/` - 4 levels deep
- `dependent-transitive-conflict/` - Mixed direct + transitive conflict
