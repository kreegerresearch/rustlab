#!/usr/bin/env bash
# Run every examples/**/*.rlab script under `rustlab run --plot none`.
#
# The three live audio monitors call figure_live and then loop on
# audio_read. `--plot none` is an explicit opt-out of the terminal UI,
# and figure_live returns an error in that mode (it is not a no-op: a
# no-op would fall into `while true` and hang when stdin is not a live
# source). They are skipped here with that reason. filter.rlab and
# passthrough.rlab stay in the run: audio_read on a closed stdin is a
# clean AudioEof and the process exits 0.
#
# Usage (from the repo root, after `cargo build --bin rustlab`):
#   bash dev/run-examples.sh
#   RUSTLAB=./target/release/rustlab bash dev/run-examples.sh
# or: make examples

set -u

repo_root=$(cd "$(dirname "$0")/.." && pwd)
bin="${RUSTLAB:-$repo_root/target/debug/rustlab}"

if [[ ! -x "$bin" ]]; then
    echo "error: rustlab binary not found at $bin (cargo build --bin rustlab, or set RUSTLAB=)" >&2
    exit 1
fi

cd "$repo_root"

pass=0
skip=0
fail=0
failed_list=()
log_dir=$(mktemp -d)

is_live_only() {
    case "$1" in
        examples/audio/spectrogram_monitor.rlab | \
            examples/audio/spectrum_monitor.rlab | \
            examples/audio/waterfall_monitor.rlab)
            return 0
            ;;
    esac
    return 1
}

while IFS= read -r rel; do
    if is_live_only "$rel"; then
        echo "SKIP $rel — live-only: figure_live is refused under --plot none (needs a TTY or rustlab-viewer)"
        skip=$((skip + 1))
        continue
    fi
    echo "RUN  $rel"
    out="$log_dir/out"
    err="$log_dir/err"
    if timeout 300 "$bin" run --plot none "$repo_root/$rel" </dev/null >"$out" 2>"$err"; then
        pass=$((pass + 1))
    else
        status=$?
        echo "FAIL $rel (exit $status)"
        if [[ -s "$err" ]]; then
            tail -n 40 "$err"
        fi
        if [[ -s "$out" ]]; then
            tail -n 20 "$out"
        fi
        fail=$((fail + 1))
        failed_list+=("$rel")
    fi
done < <(find examples -name '*.rlab' | sed "s|^$repo_root/||" | sort)

rm -rf "$log_dir"

echo
echo "examples: $pass passed, $skip skipped, $fail failed"
if ((fail > 0)); then
    printf '  %s\n' "${failed_list[@]}"
    exit 1
fi
