#!/usr/bin/env bash
# Compute on a remote machine, plots in a rustlab-viewer window on this one.
#
#   ./examples/plot/remote_viewer.sh user@host                 # remote REPL, already connected
#   ./examples/plot/remote_viewer.sh user@host --print         # show the ssh command instead
#   ./examples/plot/remote_viewer.sh user@host \
#       --command "rustlab run /data/sim.rlab --plot viewer"  # run a script there instead
#
# Then, in the remote REPL:
#   >> A = randn(2000, 2000);      % computed on the remote box
#   >> plot(svd(A))                % drawn in the local viewer window
#
# Needs a rustlab built with the viewer feature on both ends (`make install`
# does that) and ssh access to the host. Full guide: docs/remote-viewer.md
set -euo pipefail

host="${1:?usage: $0 user@host [rustlab remote options]}"
shift

# Start the local viewer window unless one is already running.
if ! pgrep -x rustlab-viewer >/dev/null 2>&1; then
    rustlab-viewer &
    sleep 1
fi

exec rustlab remote "$host" "$@"
