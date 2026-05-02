#!/usr/bin/env bash
# perf/run_perf.sh — build release binary, time every benchmark, write report.md
# Usage: ./perf/run_perf.sh  (or via  make perf)

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BINARY="$ROOT/target/release/rustlab"
REPORT="$ROOT/perf/report.md"
PERF_DIR="$ROOT/perf"

# ── helpers ───────────────────────────────────────────────────────────────────

ms() { python3 -c "import time; print(int(time.time()*1000))"; }

# Run one bench script; sets BENCH_OUT, BENCH_MS, BENCH_STATUS
run_bench() {
    local script="$1"
    local t0 t1
    t0=$(ms)
    if BENCH_OUT=$("$BINARY" run "$script" 2>&1); then
        BENCH_STATUS="PASS"
    else
        BENCH_STATUS="FAIL"
    fi
    t1=$(ms)
    BENCH_MS=$(( t1 - t0 ))
}

# ── build ─────────────────────────────────────────────────────────────────────

echo "Building release binary..."
cd "$ROOT"
cargo build --release -q
echo "  done."

# ── binary size ───────────────────────────────────────────────────────────────

BINARY_SIZE_H=$(ls -lh "$BINARY" | awk '{print $5}')

cp "$BINARY" /tmp/rustlab_perf_stripped
strip /tmp/rustlab_perf_stripped
STRIPPED_SIZE_H=$(ls -lh /tmp/rustlab_perf_stripped | awk '{print $5}')
rm /tmp/rustlab_perf_stripped

# ── discover and run all bench_*.rlab scripts ────────────────────────────────

echo "Running benchmarks..."

# Collect bench scripts sorted by name
BENCH_SCRIPTS=()
for f in "$PERF_DIR"/bench_*.rlab; do
    [ -f "$f" ] && BENCH_SCRIPTS+=("$f")
done

# Arrays to accumulate results (parallel arrays, bash 3 compatible)
BENCH_NAMES=()
BENCH_MS_ALL=()
BENCH_STATUS_ALL=()
BENCH_OUT_ALL=()

for script in "${BENCH_SCRIPTS[@]}"; do
    name=$(basename "$script" .rlab)
    echo "  $name..."
    run_bench "$script"
    BENCH_NAMES+=("$name")
    BENCH_MS_ALL+=("$BENCH_MS")
    BENCH_STATUS_ALL+=("$BENCH_STATUS")
    BENCH_OUT_ALL+=("$BENCH_OUT")
done

# ── dependency counts ─────────────────────────────────────────────────────────

DIRECT_DEPS=$(cargo tree --depth 1 -p rustlab-cli 2>/dev/null | grep -c "^[├└]" || echo "?")
TOTAL_DEPS=$(cargo tree -p rustlab-cli 2>/dev/null | wc -l | tr -d ' ')
DEP_TREE=$(cargo tree --depth 1 -p rustlab-cli 2>/dev/null || echo "n/a")

# ── write report ──────────────────────────────────────────────────────────────

TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
PLATFORM=$(uname -srm)
VERSION=$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
SIZE_TABLE=$(size "$BINARY" 2>/dev/null || echo "n/a")

{
cat << HEADER
# Performance Report

| | |
|---|---|
| **Generated** | $TIMESTAMP |
| **Version** | $VERSION |
| **Platform** | $PLATFORM |
| **Binary** | \`target/release/rustlab\` |

---

## Binary Size

| Build | Size |
|---|---|
| Release (with debug symbols) | $BINARY_SIZE_H |
| After \`strip\` | $STRIPPED_SIZE_H |

**Section sizes:**
\`\`\`
$SIZE_TABLE
\`\`\`

---

## Benchmark Results

HEADER

# Emit one section per benchmark
for i in "${!BENCH_NAMES[@]}"; do
    name="${BENCH_NAMES[$i]}"
    ms_val="${BENCH_MS_ALL[$i]}"
    status="${BENCH_STATUS_ALL[$i]}"
    out="${BENCH_OUT_ALL[$i]}"

    # Human-readable title: bench_foo_bar → Foo bar
    title=$(echo "$name" | sed 's/^bench_//' | tr '_' ' ')

    cat << SECTION
### \`$name\` — $title

| | |
|---|---|
| **Wall time** | ${ms_val} ms |
| **Status** | $status |

\`\`\`
$out
\`\`\`

SECTION
done

cat << FOOTER
---

## Dependency Summary

| | |
|---|---|
| Direct dependencies (rustlab-cli) | $DIRECT_DEPS |
| Total transitive dependency lines | $TOTAL_DEPS |

\`\`\`
$DEP_TREE
\`\`\`

---

## Known Optimisation Opportunities

These were identified during the initial audit in \`perf/performance.md\`.
An AI agent reviewing this report should check whether each item is still
open, estimate its impact given the numbers above, and prioritise them.

| ID | Change | Expected saving | Status |
|----|--------|----------------|--------|
| OPT-1 | \`zip\`: \`default-features = false, features = ["deflate"]\` | ~300–500 KB | **open** |
| OPT-2 | \`ndarray\`: remove \`features = ["rayon"]\` (unused) | ~150–250 KB | **open** |
| OPT-3 | \`[profile.release]\` — \`lto = "thin"\`, \`codegen-units = 1\`, \`strip = "symbols"\` | ~400–800 KB + runtime | **open** |

---

<!-- AI_ANALYSIS

You are an AI assistant reviewing a rustlab performance report.
Produce a section headed "## AI Analysis" and append it to this file.

Work through these tasks in order:

1. BINARY SIZE
   - Current unstripped size: $BINARY_SIZE_H  stripped: $STRIPPED_SIZE_H
   - Are OPT-1, OPT-2, OPT-3 still open?  Apply whichever are still pending
     and re-measure, or estimate the new size if you cannot rebuild.
   - Flag any new dependencies added since the last report that have not
     been audited for unnecessary default features.

2. BENCHMARK TIMINGS
   Thresholds for concern (flag if exceeded):
   - bench_upfirdn total:        > 500 ms
   - bench_fft total:            > 100 ms
   - bench_linalg total:         > 200 ms
   - bench_convolve total:       > 800 ms
   - bench_filter_design total:  > 600 ms
   - bench_builtins total:       > 300 ms
   - bench_interpreter total:    > 2000 ms
   For each benchmark that exceeds its threshold, suggest a specific next
   profiling step (e.g. samply, flamegraph, criterion micro-benchmark).

3. DEPENDENCY GROWTH
   Total transitive dependency lines: $TOTAL_DEPS
   If this is > 400 lines, identify the largest contributor among direct
   deps and suggest whether its feature flags can be trimmed further.

4. ALGORITHMIC SUGGESTIONS
   Review the bench script outputs above.  If any workload output looks
   wrong (e.g. unexpected lengths, NaN values) flag it.  Otherwise suggest
   one concrete algorithmic improvement for the slowest benchmark.
   For bench_interpreter specifically: if the scalar loop time is more than
   10x the equivalent C loop time (estimate ~1ms for 10K additions), flag
   interpreter dispatch overhead and suggest a JIT or bytecode compilation path.

5. SUMMARY TABLE
   End with a markdown table:
   | Recommendation | Priority | Effort | Expected gain |
   listing all findings from steps 1-4, highest priority first.

-->
FOOTER
} > "$REPORT"

# ── console summary ───────────────────────────────────────────────────────────

echo ""
echo "Report written to perf/report.md"
echo ""
echo "  Binary (unstripped): $BINARY_SIZE_H"
echo "  Binary (stripped):   $STRIPPED_SIZE_H"
echo ""
echo "  Benchmark results:"
for i in "${!BENCH_NAMES[@]}"; do
    printf "  %-36s %d ms  [%s]\n" "${BENCH_NAMES[$i]}:" "${BENCH_MS_ALL[$i]}" "${BENCH_STATUS_ALL[$i]}"
done
echo ""
echo "Run the AI analysis:"
echo "  claude --print 'Read perf/report.md and follow the AI_ANALYSIS instructions.'"
