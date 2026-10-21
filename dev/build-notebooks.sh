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
# sources. examples/notebooks/README.md is a notebook, so the markdown
# render writes gallery/README.md from it. The copy committed today is
# still the hand-written toolbox index; the next `make notebooks` replaces
# that index with the rendered source README.
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
# `mermaid` is default-on for the standalone bin in the Makefile, but this
# script used to build without it, so HTML diagrams stayed as source fences.
cargo build -q --release -p rustlab-notebook --bin rustlab-notebook --features mermaid
notebook_bin="$repo_root/target/release/rustlab-notebook"

mkdir -p "$gallery_dir"

# Markdown build (per-notebook .md plus shared plots/<stem>/ tree).
"$notebook_bin" render "$src_dir" \
    --format markdown \
    --output "$gallery_dir"

# Interactive HTML build (one .html per notebook, a shared gallery/assets/
# tree for Plotly + KaTeX, and a generated index.html with prev/next
# navigation). Lands alongside the .md files in gallery/; gitignore
# handles the visibility split. The HTML is not a single self-contained
# file: open it next to gallery/assets/.
"$notebook_bin" render "$src_dir" \
    --format html \
    --output "$gallery_dir" \
    --title "rustlab notebooks"
