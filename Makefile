CARGO       := cargo
INSTALL_DIR := $(HOME)/.local/bin
UNAME       := $(shell uname)

.PHONY: all build release test install perf octave-compare notebooks clean-notebooks clean help

all: help

build:
	$(CARGO) build --bin rustlab --features viewer
	$(CARGO) build -p rustlab-viewer
	$(CARGO) build -p rustlab-notebook --features mermaid

release:
	$(CARGO) build --release --bin rustlab --features viewer
	$(CARGO) build --release -p rustlab-viewer
	$(CARGO) build --release -p rustlab-notebook --features mermaid

test:
	$(CARGO) test --workspace --features viewer
	$(CARGO) test -p rustlab-notebook --features mermaid

install: release
	mkdir -p $(INSTALL_DIR)
	cp target/release/rustlab $(INSTALL_DIR)/rustlab
	cp target/release/rustlab-viewer $(INSTALL_DIR)/rustlab-viewer
	cp target/release/rustlab-notebook $(INSTALL_DIR)/rustlab-notebook
ifeq ($(UNAME), Darwin)
	codesign --sign - --force $(INSTALL_DIR)/rustlab
	codesign --sign - --force $(INSTALL_DIR)/rustlab-viewer
	codesign --sign - --force $(INSTALL_DIR)/rustlab-notebook
endif
	@echo "Installed to $(INSTALL_DIR) (override with INSTALL_DIR=...)"

perf:
	@bash perf/run_perf.sh

octave-compare:
	@bash tests/octave/run_compare.sh

# Regenerate rendered notebooks from sources at examples/notebooks/*.md.
# All output → gallery/. Markdown + plot SVGs are committed; HTML files
# (per-notebook .html and the generated index.html) are gitignored.
# Some notebooks use unseeded randn() so back-to-back renders differ in
# the generated plot SVGs — the README calls this out as a known
# limitation pending a seedable RNG.
notebooks:
	@bash dev/build-notebooks.sh

# Remove the gitignored HTML output from gallery/. Markdown and SVG
# plots (committed) are left alone — `make notebooks` will regenerate
# them, but we don't blow away tracked files from a `clean` target.
clean-notebooks:
	@rm -f gallery/*.html
	@echo "Removed gallery/*.html"

clean: clean-notebooks
	$(CARGO) clean

help:
	@echo ""
	@echo "Usage: make <target>"
	@echo ""
	@echo "  build     Debug build (all crates)"
	@echo "  release   Release build (all crates)"
	@echo "  test      Run all tests"
	@echo "  install   Release build + install to $(INSTALL_DIR)"
	@echo "  perf      Release build, run benchmarks, write perf/report.md"
	@echo "  octave-compare  Regenerate CSVs and compare rustlab vs Octave (requires octave)"
	@echo "  notebooks       Render examples/notebooks/*.md → gallery/ (md committed, html ignored)"
	@echo "  clean-notebooks Remove gallery/*.html (committed gallery/*.md is left alone)"
	@echo "  clean     Remove build artifacts (also runs clean-notebooks)"
	@echo ""
	@echo "Workflow:  make build → make test → make install"
	@echo ""
