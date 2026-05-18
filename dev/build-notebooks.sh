#!/usr/bin/env bash
# Regenerate notebook outputs from examples/notebooks/ sources.
#
# All rendered output lives at the top-level gallery/ directory:
#
#   gallery/<name>.md             # COMMITTED — GitHub-renderable Markdown
#   gallery/plots/<name>/*.svg    # COMMITTED — referenced by the .md
#   gallery/<name>.html           # gitignored — local interactive view
#   gallery/index.html            # gitignored — generated HTML index
#
# Sources stay at examples/notebooks/. Generated files never mix with
# sources. gallery/README.md is a hand-written index that GitHub displays
# when someone clicks the gallery dir; the renderer leaves it alone.
#
# Run from the repo root via `make notebooks`.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
src_dir="$repo_root/examples/notebooks"
gallery_dir="$repo_root/gallery"

# Release mode is mandatory: Rust debug builds run math-heavy code (sparse
# Gaussian elimination in spsolve, vector-calculus stencils, FFT in freqz)
# 50-150x slower. The laplacian notebook's Poisson solve goes from ~80
# seconds in debug to ~0.5 seconds in release.
#
# Notebook rendering lives in the standalone `rustlab-notebook` binary
# (per the keep-rustlab-small rule); we no longer build the main CLI here.
cargo build -q --release -p rustlab-notebook --bin rustlab-notebook
notebook_bin="$repo_root/target/release/rustlab-notebook"

mkdir -p "$gallery_dir"

# Markdown build (per-notebook .md plus shared plots/<stem>/ tree).
"$notebook_bin" render "$src_dir" \
    --format markdown \
    --output "$gallery_dir"

# Interactive HTML build (one self-contained .html per notebook plus a
# generated index.html with prev/next navigation). Lands alongside the
# .md files in gallery/; gitignore handles the visibility split.
"$notebook_bin" render "$src_dir" \
    --format html \
    --output "$gallery_dir" \
    --title "rustlab notebooks"
