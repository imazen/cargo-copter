# Patching State Machine

This document explains how cargo-copter patches dependencies to test specific versions.

## Overview

When testing a specific version of a crate (e.g., testing `rgb:0.8.91` against `image`), cargo-copter may need to override Cargo's normal dependency resolution. This document describes the state machine that handles this.

## The Problem

Cargo resolves dependencies based on semver constraints. If `image` depends on `rgb = "^0.8"` and the published versions are 0.8.50, 0.8.51, ..., 0.8.91, Cargo will pick a compatible version based on its resolver algorithm.

We want to test specific versions:
- **Baseline**: What Cargo naturally chooses (no override)
- **Specific version**: Force Cargo to use exactly 0.8.91
- **WIP/Local**: Force Cargo to use a local path

## Patching Depths (`PatchDepth` enum)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           PATCHING STRATEGIES                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  PatchDepth::None   → No override, Cargo's natural resolution              │
│                       Used for: Baseline testing                            │
│                       Marker: (none)                                        │
│                                                                             │
│  PatchDepth::Force  → Modify Cargo.toml dependency directly                 │
│                       Used for: Force-testing incompatible versions         │
│                       Marker: !                                             │
│                       Example: rgb = "0.8.91" → rgb = "=0.7.0"             │
│                                                                             │
│  PatchDepth::Patch  → Add [patch.crates-io] section                         │
│                       Used for: Unifying multiple versions in tree          │
│                       Marker: !!                                            │
│                       Example: [patch.crates-io]                            │
│                                rgb = { path = "/path/to/rgb" }              │
│                                                                             │
│  PatchDepth::DeepPatch → Deep transitive conflicts (not auto-resolved)      │
│                          Used for: When patch.crates-io isn't enough        │
│                          Marker: !!!                                        │
│                          Shows advice about blocking crates                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## State Machine Flow

```
                              START
                                │
                                ▼
                     ┌──────────────────────┐
                     │  Has override_path?  │
                     └──────────────────────┘
                           │         │
                      NO   │         │  YES
                           ▼         ▼
                    ┌──────────┐   ┌──────────────────┐
                    │  BASELINE │   │  patch_transitive │
                    │  (None)   │   │  flag set?        │
                    └──────────┘   └──────────────────┘
                                        │         │
                                   YES  │         │  NO
                                        ▼         ▼
                              ┌──────────┐   ┌──────────────┐
                              │  PATCH   │   │ force_version │
                              │  (!!)    │   │ flag set?     │
                              └──────────┘   └──────────────┘
                                                  │         │
                                             YES  │         │  NO
                                                  ▼         ▼
                                          ┌──────────┐   ┌──────────┐
                                          │  FORCE   │   │  PATCH   │
                                          │  (!)     │   │  (!!)    │
                                          └──────────┘   └──────────┘
                                                  │
                                                  ▼
                                       ┌────────────────────┐
                                       │   cargo fetch      │
                                       └────────────────────┘
                                                  │
                                          ┌──────┴──────┐
                                      SUCCESS         FAILURE
                                          │               │
                                          ▼               ▼
                                   [continue to    ┌────────────────────┐
                                    check/test]    │ Multi-version      │
                                                   │ conflict detected? │
                                                   └────────────────────┘
                                                          │         │
                                                     YES  │         │  NO
                                                          ▼         ▼
                                              ┌──────────────┐  [Report
                                              │ Already used │   failure]
                                              │ patch.crates-io?│
                                              └──────────────┘
                                                    │         │
                                               YES  │         │  NO
                                                    ▼         ▼
                                             ┌──────────┐  ┌──────────────┐
                                             │DEEP PATCH│  │ Apply patch  │
                                             │ (!!!)    │  │ and retry    │
                                             └──────────┘  └──────────────┘
                                                                  │
                                                                  ▼
                                                       ┌────────────────────┐
                                                       │   cargo fetch      │
                                                       │   (retry)          │
                                                       └────────────────────┘
                                                                  │
                                                          ┌──────┴──────┐
                                                      SUCCESS         FAILURE
                                                          │               │
                                                          ▼               ▼
                                                   [continue to    [DEEP PATCH
                                                    check/test]     or report
                                                                    failure]
```

## When Each Depth is Used

### `PatchDepth::None` (Baseline)

**Purpose**: Test what the dependent would naturally get from Cargo.

**When**:
- No `--test-versions` or `--force-versions` specified for this version
- First test per dependent (to establish baseline for comparison)

**Cargo.toml modification**: None

**Example output**: `- baseline`

### `PatchDepth::Force` (!)

**Purpose**: Force a specific published version by modifying the dependency spec.

**When**:
- `--force-versions X.Y.Z` specified
- Initial attempt before trying `[patch.crates-io]`

**Cargo.toml modification**:
```toml
# Before
[dependencies]
rgb = "^0.8"

# After
[dependencies]
rgb = "=0.7.0"
```

**Example output**: `✓ ≠0.7.0→!`

### `PatchDepth::Patch` (!!)

**Purpose**: Unify ALL versions of the crate across the dependency tree.

**When**:
- Force (!) failed with "multiple versions of crate X" error
- Automatic retry mechanism kicks in
- OR: `--patch-transitive` flag explicitly set

**Cargo.toml modification**:
```toml
[dependencies]
rgb = "^0.8"

[patch.crates-io]
rgb = { path = "/path/to/local/rgb" }
```

**How it works**: The `[patch.crates-io]` section tells Cargo to replace ALL instances of the crate with the local version, regardless of what version specifiers exist in the tree.

**Example output**: `✓ ≠0.8.91→!!`

### `PatchDepth::DeepPatch` (!!!)

**Purpose**: Identify and report when even `[patch.crates-io]` isn't enough.

**When**:
- Patch (!!) was applied but still got "multiple versions" error
- Usually happens with very strict version pins in transitive deps

**Cargo.toml modification**: Same as Patch (!!)

**Additional output**: Shows blocking crates that need their version specs adjusted:
```
  BLOCKING TRANSITIVE DEPS (need semver-compatible rgb specs):
    Recommend: Change restrictive specs (like =X.Y.Z) to ^0.8
    For forward compat: Use >=0.8 instead of exact version pins
```

**Example output**: `✗ ≠0.8.91→!!!`

## Multi-Version Conflict Detection

The auto-retry mechanism detects conflicts by looking for these error patterns in Cargo's output:

```rust
// From error_extract.rs
fn has_multiple_version_conflict(output: &str) -> bool {
    output.contains("multiple different versions of crate")
        || output.contains("two different versions of crate")
}
```

This happens when:
1. Dependent A requires `rgb ^0.8.50`
2. Dependent B (transitive) requires `rgb ^0.7`
3. Cargo cannot satisfy both constraints with a single version

## The Auto-Retry Flow

```
cargo fetch (with Force !)
           │
           ▼
    ┌──────────────┐      ┌─────────────────────────────────┐
    │   Success?   │─YES─►│ Continue to check/test          │
    └──────────────┘      └─────────────────────────────────┘
           │
           NO
           ▼
    ┌──────────────────────────────────┐
    │ Contains "multiple versions of   │─NO──► Report failure
    │ crate X" error?                  │
    └──────────────────────────────────┘
           │
          YES
           ▼
    ┌──────────────────────────────────┐
    │ Already tried [patch.crates-io]? │─YES─► DeepPatch (!!)
    └──────────────────────────────────┘       with advice
           │
           NO
           ▼
    ┌──────────────────────────────────┐
    │ Apply [patch.crates-io] to       │
    │ Cargo.toml                       │
    └──────────────────────────────────┘
           │
           ▼
    ┌──────────────────────────────────┐
    │ cargo fetch (retry with Patch !!)│
    └──────────────────────────────────┘
           │
           ▼
    ┌──────────────┐      ┌─────────────────────────────────┐
    │   Success?   │─YES─►│ Continue to check/test          │
    └──────────────┘      │ (with PatchDepth::Patch marker) │
           │              └─────────────────────────────────┘
           NO
           ▼
    ┌──────────────────────────────────┐
    │ DeepPatch (!!!) - Still failing  │
    │ Show blocking crates advice      │
    └──────────────────────────────────┘
```

## Backup/Restore Safety

All Cargo.toml modifications use a backup/restore pattern:

```rust
// From patching.rs
pub struct BackupGuard {
    path: PathBuf,
    restored: bool,
}

impl BackupGuard {
    pub fn new(path: &Path) -> std::io::Result<Self> {
        backup_file(path)?;
        Ok(Self { path: path.to_path_buf(), restored: false })
    }
}

impl Drop for BackupGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = restore_file(&self.path);
        }
    }
}
```

This ensures that:
1. Original Cargo.toml is backed up before any modification
2. Backup is restored even if the test panics (via Drop)
3. The staging directory is left in a clean state

## Files Involved

- `src/compile/types.rs`: `PatchDepth` enum definition
- `src/compile/patching.rs`: Cargo.toml manipulation functions
- `src/compile/retry.rs`: Multi-version conflict detection
- `src/compile/mod.rs`: Main `run_three_step_ict()` orchestration
- `src/error_extract.rs`: Error message parsing

## Output Markers in Reports

The patch depth is shown in the "Offered" column:

| Marker | Meaning | Example |
|--------|---------|---------|
| (none) | Natural resolution | `✓ =0.8.91` |
| `→!` | Forced version | `✓ ≠0.7.0→!` |
| `→!!` | Used patch.crates-io | `✓ ≠0.8.91→!!` |
| `→!!!` | Deep conflict | `✗ ≠0.8.91→!!!` |

The resolution symbol also indicates what happened:
- `=` - Exact match (Cargo chose what we offered)
- `↑` - Upgraded (Cargo chose newer compatible version)
- `≠` - Mismatch (forced or incompatible)
