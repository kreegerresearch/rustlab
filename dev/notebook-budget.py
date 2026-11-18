#!/usr/bin/env python3
"""Fail when a notebook's default render takes longer than its budget.

Renders every notebook in the repo with `rustlab-notebook render` (HTML,
default settings, a fresh output directory, no cache) and records wall time
and peak resident memory. Exits 1 if any notebook exceeds the budget or
exits nonzero.

Notebooks: every `.md` under examples/notebooks/, plus every `.md` under
docs/ and crates/rustlab-notebook/tests/fixtures/ that has a rustlab cell,
plus the root README.md.

Budget: `--budget` seconds per notebook (default 15). The target is a few
seconds; 10 s is the "slow" line, and the budget leaves headroom over it so
a slower CI runner does not flake.

Usage (from the repo root, after a release build):
    python3 dev/notebook-budget.py [--bin PATH] [--budget S] [--report FILE] [NOTEBOOK...]
or: make notebook-budget
"""

import argparse
import os
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def default_notebooks():
    found = sorted((REPO / "examples" / "notebooks").rglob("*.md"))
    for root in (REPO / "docs", REPO / "crates" / "rustlab-notebook" / "tests" / "fixtures"):
        for p in sorted(root.rglob("*.md")):
            if "```rustlab" in p.read_text(encoding="utf-8", errors="replace"):
                found.append(p)
    found.append(REPO / "README.md")
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", default=str(REPO / "target" / "release" / "rustlab-notebook"))
    ap.add_argument("--budget", type=float, default=15.0)
    ap.add_argument("--report", help="also write the table to this file (markdown)")
    ap.add_argument("notebooks", nargs="*", type=Path)
    args = ap.parse_args()

    if not os.access(args.bin, os.X_OK):
        sys.exit(f"error: {args.bin} not found (cargo build --release -p rustlab-notebook)")
    notebooks = [p.resolve() for p in args.notebooks] or default_notebooks()

    rows = []
    over = []
    failed = []
    with tempfile.TemporaryDirectory() as out_dir:
        for nb in notebooks:
            code, wall, peak_mib, err = run_one(args.bin, nb, out_dir)
            rel = nb.relative_to(REPO) if nb.is_relative_to(REPO) else nb
            status = "ok"
            if code != 0:
                status = f"exit {code}"
                failed.append((rel, err.strip().splitlines()[-1:] or [""]))
            elif wall > args.budget:
                status = "OVER BUDGET"
                over.append((rel, wall))
            rows.append((str(rel), wall, peak_mib, status))
            print(f"{wall:7.2f} s {peak_mib:7.0f} MiB  {status:11s} {rel}", flush=True)

    total = sum(r[1] for r in rows)
    table = ["| notebook | wall (s) | peak RSS (MiB) | status |", "|---|---:|---:|---|"]
    table += [f"| `{n}` | {w:.2f} | {m:.0f} | {s} |" for n, w, m, s in rows]
    table.append(f"| **total ({len(rows)})** | {total:.1f} | | budget {args.budget:g} s each |")
    if args.report:
        Path(args.report).write_text("\n".join(table) + "\n", encoding="utf-8")

    print(f"\n{len(rows)} notebooks, {total:.1f} s total, budget {args.budget:g} s each")
    for rel, wall in over:
        print(f"OVER BUDGET: {rel} took {wall:.2f} s (budget {args.budget:g} s)")
    for rel, last in failed:
        print(f"FAILED: {rel}: {last[0] if last else ''}")
    return 1 if over or failed else 0


def run_one(binary, nb, out_dir):
    """Wall seconds and peak RSS of one render, measured on the child alone."""
    out = Path(out_dir) / (nb.stem + ".html")
    env = dict(os.environ, CI="1")
    err_path = Path(out_dir) / "stderr.txt"
    t0 = time.monotonic()
    with open(err_path, "wb") as err_file:
        pid = os.fork()
        if pid == 0:
            os.chdir(REPO)
            devnull = os.open(os.devnull, os.O_WRONLY)
            os.dup2(devnull, 1)
            os.dup2(err_file.fileno(), 2)
            argv = [binary, "render", str(nb), "-o", str(out), "--format", "html"]
            try:
                os.execve(binary, argv, env)
            finally:
                os._exit(127)
        _, status, ru = os.wait4(pid, 0)
    wall = time.monotonic() - t0
    code = os.waitstatus_to_exitcode(status)
    peak_kib = ru.ru_maxrss if sys.platform != "darwin" else ru.ru_maxrss / 1024
    return code, wall, peak_kib / 1024, err_path.read_text(errors="replace")


if __name__ == "__main__":
    sys.exit(main())
