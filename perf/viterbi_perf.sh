#!/usr/bin/env bash
# perf/viterbi_perf.sh — time the notebook's script-level Viterbi decoder.
#
# Rate 1/2, K = 7, generators 171 / 133 (octal), Eb/N0 = 3 dB, soft and
# hard decisions, 1e4 / 1e5 / 1e6 message bits (perf/viterbi_core.rlab).
# The decoder traces back over the whole terminated frame; it has no
# traceback-length parameter. Reports the decode time (tic/toc), the
# process wall time, and peak RSS, and checks each decoded checksum
# against the value recorded with rustlab 0.3.89, so a speedup that
# changes a single decoded bit fails.
#
# Usage: perf/viterbi_perf.sh [path/to/rustlab] [max_bits]
#   max_bits defaults to 1000000; pass 100000 to skip the 1e6 rows.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/release/rustlab}"
MAX="${2:-1000000}"
[ -x "$BIN" ] || { echo "no binary at $BIN (cargo build --release)" >&2; exit 2; }

# N HARD errors checksum, from rustlab 0.3.89 (before any optimization).
EXPECTED="
10000 0 6 265581
10000 1 317 461251
100000 0 27 913005
100000 1 2955 316097
1000000 0 446 628113
1000000 1 31594 219989
"

TMP=$(mktemp -d "${TMPDIR:-/tmp}/viterbi_perf.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
fail=0
printf "%-9s %-5s %10s %10s %12s %8s  %s\n" bits mode "decode s" "wall s" "peak RSS MB" errors check
while read -r n hard errs chk; do
    [ -z "$n" ] && continue
    [ "$n" -gt "$MAX" ] && continue
    printf 'N = %d;\nHARD = %d;\nrun %s/perf/viterbi_core.rlab\n' "$n" "$hard" "$ROOT" > "$TMP/run.rlab"
    # Peak RSS of the child through getrusage: KB on Linux, bytes on macOS.
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
    mode=soft; [ "$hard" = 1 ] && mode=hard
    printf "%-9s %-5s %10s %10s %12s %8s  %s\n" "$n" "$mode" "$dec" "$wall" "$mb" "$got_e" "$status"
done <<<"$EXPECTED"
exit $fail
