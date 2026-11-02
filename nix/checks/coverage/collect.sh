#!/usr/bin/env bash

set -euo pipefail

out=$1

mkdir -p "$out"
cargo nextest list --locked --workspace --all-features --message-format json > "$TMPDIR/coverage-tests.json"
test_count=$(jq -er '.["test-count"]' "$TMPDIR/coverage-tests.json")
if ((test_count == 0)); then
  echo "Coverage not measured: no Rust tests are available." | tee "$out/status.txt"
  exit 0
fi

cargo llvm-cov nextest \
  --locked \
  --workspace \
  --all-features \
  --no-report \
  --remap-path-prefix

cargo llvm-cov report --lcov --output-path "$out/coverage.lcov"
test -s "$out/coverage.lcov"
lcov --summary "$out/coverage.lcov" --fail-under-lines 100
lcov_cobertura "$out/coverage.lcov" --base-dir . --output "$out/cobertura.xml"
# Keep report generation independent of the build's wall-clock time.
sed -i 's/ timestamp="[0-9]*"/ timestamp="0"/' "$out/cobertura.xml"
