## GitHub Action

Use cargo-copter in your CI to catch breaking changes before publishing:

```yaml
# .github/workflows/copter.yml
name: Reverse Dependency Check

on:
  pull_request:
  push:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: imazen/cargo-copter@v1
        with:
          top-dependents: 5
```

### Action Inputs

| Input | Description | Default |
|-------|-------------|---------|
| `path` | Path to crate | `.` |
| `top-dependents` | Number of dependents to test | `5` |
| `dependents` | Space-separated list of specific dependents | |
| `test-versions` | Space-separated versions to test | |
| `force-versions` | Space-separated versions to force | |
| `only-check` | Skip tests, only run cargo check | `false` |
| `only-fetch` | Skip check and test | `false` |
| `error-lines` | Max error lines per failure | `10` |
| `fail-on-regression` | Fail if regressions detected | `true` |
| `version` | cargo-copter version to use | `latest` |
| `cache` | Cache staging directory | `true` |

### Action Outputs

| Output | Description |
|--------|-------------|
| `passed` | Number of passed dependents |
| `regressed` | Number of regressions |
| `broken` | Number already broken at baseline |
| `report-path` | Path to JSON report |

### Example: PR vs Main Branch

```yaml
jobs:
  # Fast check for PRs
  pr-check:
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: imazen/cargo-copter@v1
        with:
          top-dependents: 3
          only-check: true

  # Thorough check on main
  full-check:
    if: github.event_name == 'push'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: imazen/cargo-copter@v1
        with:
          top-dependents: 10
```

## Installation
