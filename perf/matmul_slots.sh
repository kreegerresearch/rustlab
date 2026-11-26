#!/usr/bin/env bash
# Sweep the SME slot cap for parmap matrix products (rustlab_llm request R1).
#
#   perf/matmul_slots.sh                       # caps 0 1 2 3 4 and the default
#   SLOTS="1 2 3" perf/matmul_slots.sh         # chosen caps
#   OVERFLOW=0 perf/matmul_slots.sh            # never move a product to NEON
#   RUSTLAB=target/release/rustlab perf/matmul_slots.sh
#
# Cap 0 is no cap (every product on SME, the 0.3.90 behaviour). Each line is
# PERF <key> <ms> <rate> GFLOP/s, prefixed by the cap. Without SME the cap
# does nothing.
set -euo pipefail
cd "$(dirname "$0")/.."
RUSTLAB=${RUSTLAB:-target/release/rustlab}
if [[ ! -x $RUSTLAB ]]; then
  cargo build --release -p rustlab-cli --bin rustlab
fi
SLOTS=${SLOTS:-"default 0 1 2 3 4"}
for s in $SLOTS; do
  if [[ $s == default ]]; then
    env -u RUSTLAB_SME_SLOTS ${OVERFLOW:+RUSTLAB_SME_OVERFLOW_MACS=$OVERFLOW} \
      "$RUSTLAB" run perf/matmul_slots.rlab | sed "s/^/slots=default /"
  else
    env RUSTLAB_SME_SLOTS="$s" ${OVERFLOW:+RUSTLAB_SME_OVERFLOW_MACS=$OVERFLOW} \
      "$RUSTLAB" run perf/matmul_slots.rlab | sed "s/^/slots=$s /"
  fi
done
