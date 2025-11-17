# Console Output Test Fixtures

These fixtures capture the console output before refactoring the formatting code.
They serve as reference for ensuring the refactored code produces identical output.

## Fixtures

- **regression-scenario.txt**: Shows a failing test (ansi_colours fails with 0.8.91-alpha.3)
- **skipped-version-scenario.txt**: Shows a skipped version (0.8.51 doesn't match what cargo resolves)
- **all-passing-scenario.txt**: Shows all tests passing

## Normalization

Times are normalized to `X.Xs` and git hashes to `GITHASH` to make diffs easier.

## Usage

When refactoring console formatting, run the same commands and diff against these files
to ensure output remains identical (except for normalized values).
