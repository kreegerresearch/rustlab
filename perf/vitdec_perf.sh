#!/usr/bin/env bash
# perf/vitdec_perf.sh — time the native `vitdec` builtin.
#
# Same inputs as perf/viterbi_perf.sh (rate 1/2, K = 7, generators 171 / 133,
# Eb/N0 = 3 dB, seed 42), decoded with `vitdec(..., "term", ...)`: whole-frame
# traceback (tblen 0 below means the frame length) and windows of 35 and 96.
# The whole-frame rows must give the script decoder's checksums from
# perf/viterbi_perf.sh. Reports decode time, wall time, and peak RSS.
#
# Usage: perf/vitdec_perf.sh [path/to/rustlab] [max_bits]

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/release/rustlab}"
MAX="${2:-1000000}"
[ -x "$BIN" ] || { echo "no binary at $BIN (cargo build --release)" >&2; exit 2; }

# N HARD TBLEN errors checksum
EXPECTED="
10000 0 0 6 265581
10000 0 35 6 265581
10000 0 96 6 265581
10000 1 0 317 461251
10000 1 35 356 580499
10000 1 96 317 461251
100000 0 0 27 913005
100000 0 35 38 74950
100000 0 96 27 913005
100000 1 0 2955 316097
100000 1 35 3309 238495
100000 1 96 2970 520301
1000000 0 0 446 628113
1000000 0 35 541 851953
1000000 0 96 446 628113
1000000 1 0 31594 219989
1000000 1 35 34158 587915
1000000 1 96 31598 104351
"

TMP=$(mktemp -d "${TMPDIR:-/tmp}/vitdec_perf.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
fail=0
printf "%-9s %-8s %-6s %10s %10s %12s %8s  %s\n" bits dectype tblen "decode s" "wall s" "peak RSS MB" errors check
while read -r n hard tb errs chk; do
    [ -z "$n" ] && continue
    [ "$n" -gt "$MAX" ] && continue
    printf 'N = %d;\nHARD = %d;\nTBLEN = %d;\nrun %s/perf/vitdec_core.rlab\n' "$n" "$hard" "$tb" "$ROOT" > "$TMP/run.rlab"
    out=$(python3 - "$BIN" "$TMP/run.rlab" <<'PY'
import resource, subprocess, sys, time
t0 = time.time()
p = subprocess.run([sys.argv[1], "run", sys.argv[2]], capture_output=True, text=True)
wall = time.time() - t0
rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
mb = rss / 1024 / 1024 if sys.platform == "darwin" else rss / 1024
line = [l for l in p.stdout.splitlines() if l.startswith("N=")]
print(f"{wall:.3f} {mb:.1f} {p.returncode} {line[0] if line else ''}")
PY
)
    read -r wall mb rc rest <<<"$out"
    dec=$(sed -n 's/.*decode=\([0-9.]*\) s.*/\1/p' <<<"$rest")
    got_e=$(sed -n 's/.*errors=\([0-9]*\).*/\1/p' <<<"$rest")
    got_c=$(sed -n 's/.*checksum=\([0-9]*\).*/\1/p' <<<"$rest")
    status=PASS
    if [ "$rc" != 0 ] || [ "$got_e" != "$errs" ] || [ "$got_c" != "$chk" ]; then
        status="FAIL (errors=$got_e checksum=$got_c, want $errs / $chk)"
        fail=1
    fi
    mode=unquant; [ "$hard" = 1 ] && mode=hard
    label=$tb; [ "$tb" = 0 ] && label=frame
    printf "%-9s %-8s %-6s %10s %10s %12s %8s  %s\n" "$n" "$mode" "$label" "$dec" "$wall" "$mb" "$got_e" "$status"
done <<<"$EXPECTED"
exit $fail
