#!/usr/bin/env bash
# Phase 6 planning prototype runner. Recreates the configurations whose
# numbers are recorded in dev/plans/fdtd3d_phase6_impl.md §4. Not CI.
#
#   bash dev/plans/fdtd3d_phase6_impl/run_proto.sh            # list configs
#   bash dev/plans/fdtd3d_phase6_impl/run_proto.sh lesson0    # one config
#   RUSTLAB=./target/release/rustlab bash ... ref_edge lesson1
#
# Each config sets the knobs documented at the top of patch_core.rlab, in
# this order: dxy nsub gpad air_xy air_z npml margin feed sigma nsteps
# ntff_on ffreqs dth dph excite Lmetal Wmetal. ffreqs is a named set below.

set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
bin="${RUSTLAB:-$here/../../../target/release/rustlab}"
S=0.011749584585529514   # 2*pi*2.4e9*4.4*eps0*0.02 (tan delta 0.02 at f0)

declare -A freqs=(
  [F14]="2.30e9:0.02e9:2.56e9"
  [F3]="[2.42e9, 2.44e9, 2.46e9]"
  [F4]="[2.34e9, 2.36e9, 2.38e9, 2.40e9]"
)

declare -A cfg=(
  [smoke]="2e-3 2 16e-3 10e-3 10e-3 8 2 edge 0 4000 1 F14 5 10 port 0 0"
  [edge24k]="1e-3 4 16e-3 10e-3 10e-3 8 2 edge 0 24000 1 F14 5 10 port 0 0"
  [inset30k]="1e-3 4 16e-3 10e-3 10e-3 8 2 inset 0 30000 1 F14 5 10 port 0 0"
  [lossy24k]="1e-3 4 16e-3 10e-3 10e-3 8 2 edge $S 24000 1 F14 5 10 port 0 0"
  [air5]="1e-3 4 16e-3 5e-3 5e-3 8 2 edge 0 12000 1 F3 3 3 port 0 0"
  [air10]="1e-3 4 16e-3 10e-3 10e-3 8 2 edge 0 12000 1 F3 3 3 port 0 0"
  [air20]="1e-3 4 16e-3 20e-3 20e-3 8 2 edge 0 12000 1 F3 3 3 port 0 0"
  [airz20]="1e-3 4 16e-3 10e-3 20e-3 8 2 edge 0 12000 1 F3 3 3 port 0 0"
  [airxy20]="1e-3 4 16e-3 20e-3 10e-3 8 2 edge 0 12000 1 F3 3 3 port 0 0"
  [ref_edge]="1e-3 4 16e-3 10e-3 20e-3 8 2 edge 0 12000 1 F4 3 3 port 0 0"
  [ref_lossy]="1e-3 4 16e-3 10e-3 20e-3 8 2 edge $S 12000 1 F4 3 3 port 0 0"
  [qring0]="1e-3 4 16e-3 10e-3 10e-3 8 2 edge 0 16000 0 F3 3 3 soft 0 0"
  [qring1]="1e-3 4 16e-3 10e-3 10e-3 8 2 edge $S 16000 0 F3 3 3 soft 0 0"
  [lesson0]="2e-3 2 16e-3 10e-3 20e-3 8 2 edge 0 8000 1 F4 3 3 port 0 0"
  [lesson1]="2e-3 2 16e-3 10e-3 20e-3 8 2 edge $S 8000 1 F4 3 3 port 0 0"
  [mid15]="1.5e-3 3 16e-3 10e-3 10e-3 8 2 edge 0 12000 1 F14 3 3 port 0 0"
  [conv2_w38]="2e-3 2 16e-3 10e-3 10e-3 8 2 edge 0 8000 0 F3 3 3 port 28e-3 38e-3"
  [conv2_w40]="2e-3 2 16e-3 10e-3 10e-3 8 2 edge 0 8000 0 F3 3 3 port 28e-3 40e-3"
  [conv05]="0.5e-3 8 16e-3 5e-3 5e-3 8 2 edge 0 24000 0 F3 3 3 port 28e-3 38e-3"
)

if [[ $# -eq 0 ]]; then
  printf '%s\n' "${!cfg[@]}" | sort
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for name in "$@"; do
  if [[ -z "${cfg[$name]:-}" ]]; then
    echo "unknown config: $name" >&2
    exit 1
  fi
  read -r dxy nsub gpad axy az npml margin feed sigma nsteps ntff fset dth dph excite lm wm \
    <<<"${cfg[$name]}"
  cat >"$tmp/$name.rlab" <<EOF
dxy = $dxy; nsub = $nsub; gpad = $gpad; air_xy = $axy; air_z = $az;
npml = $npml; margin = $margin; feed = "$feed"; sigma = $sigma; nsteps = $nsteps; ntff_on = $ntff;
ffreqs = ${freqs[$fset]}; dth = $dth; dph = $dph; excite = "$excite"; Lmetal = $lm; Wmetal = $wm;
run $here/patch_core.rlab
EOF
  echo "== $name"
  "$bin" run --plot none "$tmp/$name.rlab"
done
