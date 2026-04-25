#!/usr/bin/env bash
# Regenerate notebook outputs from examples/notebooks/ sources.
#
# Sources live at examples/notebooks/*.md. All rendered output goes
# under examples/notebooks/site/ (gitignored) so source files are never
# mixed with rendered artifacts:
#
#   examples/notebooks/site/md/<name>.md     # GitHub-friendly markdown
#   examples/notebooks/site/md/plots/<name>/ # SVG plots referenced by the .md
#   examples/notebooks/site/html/<name>.html # interactive HTML w/ Plotly + KaTeX
#   examples/notebooks/site/html/index.html  # generated index for the HTML build
#
# Run from the repo root via `make notebooks`.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
src_dir="$repo_root/examples/notebooks"
site_dir="$src_dir/site"

# Release mode is mandatory: Rust debug builds run math-heavy code (sparse
# Gaussian elimination in spsolve, vector-calculus stencils, FFT in freqz)
# 50-150x slower. The laplacian notebook's Poisson solve goes from ~80
# seconds in debug to ~0.5 seconds in release.
cargo build -q --release -p rustlab-cli --bin rustlab
rustlab_bin="$repo_root/target/release/rustlab"

mkdir -p "$site_dir/md" "$site_dir/html"

# Markdown build (per-notebook .md plus shared plots/<stem>/ tree).
"$rustlab_bin" notebook render "$src_dir" \
    --format markdown \
    --output "$site_dir/md"

# Interactive HTML build (one self-contained .html per notebook, plus a
# generated index.html with prev/next navigation between notebooks).
"$rustlab_bin" notebook render "$src_dir" \
    --format html \
    --output "$site_dir/html" \
    --title "rustlab notebooks"
