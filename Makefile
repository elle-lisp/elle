.PHONY: all elle docs docgen smoke test test-git clean help \
       smoke-vm smoke-noffi smoke-jit smoke-wasm smoke-mlir smoke-diff doctest \
       elle-wasm elle-mlir elle-noffi plugins plugins-all mcp embedding \
       fmt fmt-check

.DEFAULT_GOAL := all

ifdef GITHUB_ACTIONS
  JOBS          ?= $(shell nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
  WASM_JOBS     ?= 2
  ELLE          ?= ./target/release/elle
  CARGO_PROFILE := --release
else
  JOBS          ?= 16
  WASM_JOBS     ?= 4
  ELLE          ?= ./target/debug/elle
  CARGO_PROFILE :=
endif
TIMEOUT ?= 30s
LISP_FILES := $(shell find stdlib.lisp prelude.lisp lib/ tests/ demos/ -name '*.lisp' 2>/dev/null)

all: elle docs  ## Build everything

# ── Build ───────────────────────────────────────────────────────────

elle:  ## Build the Elle binary
	cargo build $(CARGO_PROFILE) -p elle

MCP_PATCH := --config 'patch."https://github.com/elle-lisp/elle".elle-plugin.path="elle-plugin"'

plugins:  ## Build all portable plugins (from plugins submodule)
	$(MAKE) -C plugins portable

plugins-all:  ## Build all plugins including system-dep ones (vulkan, egui, etc.)
	$(MAKE) -C plugins all

mcp: elle  ## Build elle + MCP plugins (oxigraph, syn)
	$(MAKE) -C plugins mcp

# ── Docs ────────────────────────────────────────────────────────────

docs: docs/pipeline.svg  ## Generate documentation assets

docs/pipeline.svg: docs/pipeline.dot
	dot -Tsvg $< -o $@

docgen: elle  ## Generate documentation site (Rust docs + Elle site)
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
	$(ELLE) demos/docgen/generate.lisp

# ── Format ─────────────────────────────────────────────────────────

fmt: elle  ## Format all Elle source in-place
	@echo "=== elle fmt ==="
	@printf '%s\n' $(LISP_FILES) | parallel -j $(JOBS) '$(ELLE) fmt {}'

fmt-check: elle  ## Check Elle formatting (exit 1 on diff)
	@echo "=== elle fmt --check ==="
	@printf '%s\n' $(LISP_FILES) | parallel -j $(JOBS) '$(ELLE) fmt --check {}'

# ── Test ────────────────────────────────────────────────────────────

# Approximate runtimes (for guidance — vary by machine):
#   make smoke    ~3min docs + elle scripts, VM, JIT, WASM (parallel, debug build)
#   make test     ~3min smoke + rust unit tests (PROPTEST_CASES=4)
#   cargo test    ~60min full suite (unit + integration + property)
#
# Every Elle test target runs twice: first with JIT disabled (VM-only),
# then with default JIT. This catches bugs that only manifest in one mode.
# On failure the banner tells you which pass broke — capture it even if
# you only see the last few lines of output.

# Per-pass skip lists: tests that fail in one mode can still run in the other.
# jit-rejections    — requires JIT active (tests rejection tracking)
# gpu-eligible,mlir — test inline intrinsic compilation (bypassed by --checked-intrinsics)
ELLE_SKIP_VM  := -e jit-rejections.lisp -e gpu-eligible.lisp -e mlir.lisp
ELLE_SKIP_JIT := -e NOMATCH_PLACEHOLDER
ELLE_SKIP_MLIR := -e NOMATCH_PLACEHOLDER

# FFI skip list: tests requiring libffi (skipped when built --no-default-features)
ELLE_SKIP_FFI := -e ffi.lisp -e compress.lisp -e sqlite.lisp -e zmq.lisp -e git.lisp -e http.lisp

# WASM backend skip list: tests requiring features not yet in WASM backend
# (eval = dynamic compilation)
WASM_SKIP := -e eval.lisp -e eval-env.lisp

smoke-vm: elle
	@echo "=== elle scripts (VM, no JIT) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) --checked-intrinsics --jit=off --mlir=off {}' \
		|| { echo "FAILED: elle scripts VM-only pass (no JIT)"; exit 1; }

elle-noffi:           ## Build elle with no features (for smoke-noffi)
	@echo "=== build elle with no features ==="
	cargo build $(CARGO_PROFILE) -p elle --no-default-features -q

smoke-noffi: elle-noffi
	@echo "=== elle scripts (VM, no features) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | grep -v $(ELLE_SKIP_FFI) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=off {}' \
		|| { echo "FAILED: elle scripts VM-only pass (no features)"; exit 1; }

smoke-jit: elle
	@echo "=== elle scripts (eager JIT) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_JIT) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=eager {}' \
		|| { echo "FAILED: elle scripts JIT pass (eager)"; exit 1; }

elle-mlir:   ## Build elle with MLIR support (for smoke-mlir)
	@echo "=== build elle with MLIR ==="
	cargo build $(CARGO_PROFILE) -p elle --features mlir -q

smoke-mlir: elle-mlir
	@echo "=== elle scripts (eager MLIR) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_MLIR) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) --mlir=eager {}' \
		|| { echo "FAILED: elle scripts MLIR pass (eager)"; exit 1; }

elle-wasm:   ## Build elle with WASM support (for smoke-wasm)
	@echo "=== build elle with WASM ==="
	cargo build $(CARGO_PROFILE) -p elle --features wasm -q

smoke-wasm: elle-wasm
	@echo "=== elle scripts (WASM) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(WASM_SKIP) | \
		parallel -j $(WASM_JOBS) --halt now,fail=1 --tag \
			'timeout 300s $(ELLE) --wasm=full {}' \
		|| { echo "FAILED: elle scripts WASM pass (full)"; exit 1; }

doctest:   ## Test code examples in documentation (literate mode)
	@echo "=== doctest ==="
	@printf '%s\n' docs/*.md docs/impl/*.md docs/cookbook/*.md docs/signals/*.md docs/analysis/*.md | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: doctest"; exit 1; }

smoke-diff:    ## Cross-tier differential agreement tests (compile/run-on)
	@echo "=== differential tier-agreement tests ==="
	@printf '%s\n' tests/diff/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: differential tests"; exit 1; }

EMBED_TARGET_DIR = $(CURDIR)/target/$(if $(findstring --release,$(CARGO_PROFILE)),release,debug)

embedding: elle  ## Build + run embedding demos (Rust + C hosts)
	cargo build $(CARGO_PROFILE) -p elle-embed
	cargo run $(CARGO_PROFILE) -p elle-embed --bin host
	$(MAKE) -C demos/embedding chost TARGET_DIR=$(EMBED_TARGET_DIR)
	LD_LIBRARY_PATH=$(EMBED_TARGET_DIR) demos/embedding/chost

smoke: smoke-vm smoke-jit doctest smoke-diff embedding  ## Run docs, elle tests
	@echo "=== all smoke tests passed ==="

MLIR_PREFIX ?= $(HOME)/git/tmp/mlir-install
MLIR_ENV    := LLVM_SYS_220_PREFIX=$(MLIR_PREFIX) \
               MLIR_SYS_220_PREFIX=$(MLIR_PREFIX) \
               TABLEGEN_220_PREFIX=$(MLIR_PREFIX)

test: smoke  ## Rust unit tests + clippy + fmt + rustdoc after smoke
	cargo fmt --check
	$(MLIR_ENV) cargo clippy --workspace --all-targets --all-features -- -D warnings
	$(MLIR_ENV) RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
	$(MLIR_ENV) PROPTEST_CASES=4 cargo test --workspace --lib --all-features

# ── Clean ───────────────────────────────────────────────────────────

clean:  ## Remove build artifacts and generated docs
	cargo clean
	rm -f docs/pipeline.svg

# ── Help ────────────────────────────────────────────────────────────

help:  ## Show this help
	@grep -E '^[a-z].*:.*##' $(MAKEFILE_LIST) | \
		sed 's/:.*##/\t/' | \
		column -t -s '	'
