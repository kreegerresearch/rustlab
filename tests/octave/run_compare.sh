#!/usr/bin/env bash
# Regenerate rustlab + Octave reference CSVs in tests/octave/ and run both
# comparison suites (compare.m, compare_full.m). Exits nonzero if any case
# exceeds its tolerance.
#
# Usage:  bash tests/octave/run_compare.sh
# Env:    RUSTLAB=/path/to/rustlab  OCTAVE=/path/to/octave

set -euo pipefail
cd "$(dirname "$0")"

# Pick rustlab: env override > repo release build > repo debug build > PATH.
REPO_ROOT=$(cd ../.. && pwd)
if [[ -z "${RUSTLAB:-}" ]]; then
    if [[ -x "$REPO_ROOT/target/release/rustlab" ]]; then
        RUSTLAB="$REPO_ROOT/target/release/rustlab"
    elif [[ -x "$REPO_ROOT/target/debug/rustlab" ]]; then
        RUSTLAB="$REPO_ROOT/target/debug/rustlab"
    else
        RUSTLAB="rustlab"
    fi
fi
OCTAVE=${OCTAVE:-octave}

if ! command -v "$OCTAVE" >/dev/null 2>&1; then
    echo "Error: '$OCTAVE' not found. Install with: brew install octave (macOS) or apt-get install octave (Linux)" >&2
    exit 1
fi
if ! command -v "$RUSTLAB" >/dev/null 2>&1 && [[ ! -x "$RUSTLAB" ]]; then
    echo "Error: '$RUSTLAB' not found. Build first with: make build (or make release)" >&2
    exit 1
fi

echo "==> rustlab : $RUSTLAB"
echo "==> octave  : $OCTAVE"

echo
echo "==> Generating rustlab CSV outputs..."
"$RUSTLAB" run rustlab_outputs.rlab
"$RUSTLAB" run rustlab_full.rlab

echo
echo "==> Generating Octave reference CSV outputs..."
"$OCTAVE" --no-gui --no-window-system --quiet reference.m
"$OCTAVE" --no-gui --no-window-system --quiet reference_full.m

LOG=$(mktemp -t rustlab-octave-XXXXXX)
trap 'rm -f "$LOG"' EXIT

echo
echo "==> Running DSP comparison suite (compare.m)..."
"$OCTAVE" --no-gui --no-window-system --quiet compare.m | tee -a "$LOG"

echo
echo "==> Running full comparison suite (compare_full.m)..."
"$OCTAVE" --no-gui --no-window-system --quiet compare_full.m | tee -a "$LOG"

echo
if grep -q "SOME TESTS FAILED" "$LOG"; then
    echo "OCTAVE COMPARISON: FAIL" >&2
    exit 1
fi
echo "OCTAVE COMPARISON: ALL SUITES PASSED"
