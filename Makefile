.PHONY: all elle docs docgen smoke test clean space help \
       smoke-elle smoke-vm smoke-noffi smoke-jit smoke-wasm smoke-mlir smoke-diff \
       doctest elle-wasm elle-mlir elle-noffi plugins plugins-all mcp embedding \
       fmt fmt-check

.DEFAULT_GOAL := all

ifdef GITHUB_ACTIONS
  JOBS          ?= 4
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
	@echo "=== elle fmt --check --no-epoch ==="
	@# --no-epoch: the gate checks FORMATTING only. Epoch migration is
	@# `elle rewrite`'s job (forward-compat, run explicitly) — not a gate,
	@# so bumping CURRENT_EPOCH must not flag every older-epoch file here.
	@printf '%s\n' $(LISP_FILES) | parallel -j $(JOBS) '$(ELLE) fmt --check --no-epoch {}'

# ── Test ────────────────────────────────────────────────────────────

# Approximate runtimes (for guidance — vary by machine):
#   make smoke    docs + the elle test corpus (one process) + embedding
#   make test     smoke + rust fmt/clippy/rustdoc/unit/integration
#   cargo test    ~60min full suite (unit + integration + property)
#
# The default-build elle scripts now run through the agent-first runner
# (`smoke-elle`, see docs/testing.md): ONE `elle test` invocation covers the vm
# and jit policies and cross-tier divergence. The tier-specific direct-run
# targets below (smoke-vm/smoke-jit/smoke-noffi/smoke-wasm/smoke-mlir) are kept
# for debugging a single backend and for the non-default feature builds.

# The agent-first runner: ONE process, the whole corpus, a SQLite session DB.
# `elle test` (docs/testing.md, docs/test-runner.md) compiles + runs every file
# and records each (form × tier) result; the gate is its exit code. It subsumes
# the default-build smoke split — a multi-form file runs under the :off JIT
# policy (recorded `vm`) AND :eager (recorded `jit`) — the old smoke-vm +
# smoke-jit — while single-form files run on every tier with divergence (the old
# smoke-diff). So no per-pass skip list applies: a test gates itself in-file
# (gate!/:gated) and a backend the build lacks is dropped, not skip-listed.
ELLE_TEST_DB ?= target/elle-tests.db

# Quarantine list for the gate — known HARNESS bugs (NOT test failures) get
# parked here with a tracked reason. Currently empty.
#
# (Resolved: subprocess.lisp used to hang in a worker thread — children inherited
# the worker's all-blocked signal mask across fork/exec, so SIGTERM never landed
# and subprocess/wait wedged. Fixed by resetting the child's mask in pre_exec;
# see src/io/request.rs reset_child_signals + docs/posix-signals.md.)
ELLE_TEST_SKIP :=

# Per-pass skip lists for the DIRECT-RUN tier targets only (smoke-vm/jit/noffi).
# jit-rejections    — requires JIT active (tests rejection tracking)
# gpu-eligible,mlir — require the MLIR tier active
ELLE_SKIP_VM  := -e jit-rejections.lisp -e gpu-eligible.lisp -e mlir.lisp
ELLE_SKIP_JIT := -e NOMATCH_PLACEHOLDER
ELLE_SKIP_MLIR := -e NOMATCH_PLACEHOLDER

# FFI skip list: tests requiring libffi (skipped when built --no-default-features)
ELLE_SKIP_FFI := -e ffi.lisp -e compress.lisp -e sqlite.lisp -e zmq.lisp -e git.lisp -e http.lisp

# WASM backend skip list: tests requiring features not yet in WASM backend
# (eval = dynamic compilation)
WASM_SKIP := -e eval.lisp -e eval-env.lisp

smoke-elle: elle  ## Run the whole corpus through `elle test` (vm + jit + divergence)
	@echo "=== elle test (vm + jit policies, cross-tier divergence) ==="
	@$(ELLE) test \
		$(filter-out $(ELLE_TEST_SKIP),$(wildcard tests/elle/*.lisp) $(wildcard tests/diff/*.lisp)) \
		--db $(ELLE_TEST_DB) \
		|| { $(ELLE) test --summary --db $(ELLE_TEST_DB); echo "FAILED: elle test — inspect the session DB $(ELLE_TEST_DB) (see docs/testing.md § Reading a run)"; exit 1; }

smoke-vm: elle
	@echo "=== elle tests (VM, no JIT) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=off --mlir=off {}' \
		|| { echo "FAILED: elle tests VM-only pass (no JIT)"; exit 1; }

elle-noffi:           ## Build elle with no features (for smoke-noffi)
	@echo "=== build elle with no features ==="
	cargo build $(CARGO_PROFILE) -p elle --no-default-features -q

smoke-noffi: elle-noffi
	@echo "=== elle tests (VM, no features) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | grep -v $(ELLE_SKIP_FFI) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=off {}' \
		|| { echo "FAILED: elle tests VM-only pass (no features)"; exit 1; }

smoke-jit: elle
	@echo "=== elle tests (eager JIT) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_JIT) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=eager {}' \
		|| { echo "FAILED: elle tests JIT pass (eager)"; exit 1; }

elle-mlir:   ## Build elle with MLIR support (for smoke-mlir)
	@echo "=== build elle with MLIR ==="
	cargo build $(CARGO_PROFILE) -p elle --features mlir -q

smoke-mlir: elle-mlir
	@echo "=== elle tests (eager MLIR) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_MLIR) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --mlir=eager {}' \
		|| { echo "FAILED: elle tests MLIR pass (eager)"; exit 1; }

elle-wasm:   ## Build elle with WASM support (for smoke-wasm)
	@echo "=== build elle with WASM ==="
	cargo build $(CARGO_PROFILE) -p elle --features wasm -q

smoke-wasm: elle-wasm
	@echo "=== elle tests (WASM) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(WASM_SKIP) | \
		parallel -j $(WASM_JOBS) --tag \
			'timeout 300s $(ELLE) --wasm=full {}' \
		|| { echo "FAILED: elle tests WASM pass (full)"; exit 1; }

doctest:   ## Test code examples in documentation (literate mode)
	@echo "=== doctest ==="
	@printf '%s\n' docs/*.md docs/regions/*.md docs/impl/*.md docs/cookbook/*.md docs/signals/*.md docs/analysis/*.md | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: doctest"; exit 1; }

smoke-diff:    ## Cross-tier differential agreement tests (compile/run-on)
	@echo "=== differential tier-agreement tests ==="
	@printf '%s\n' tests/diff/*.lisp | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: differential tests"; exit 1; }

EMBED_TARGET_DIR = $(CURDIR)/target/$(if $(findstring --release,$(CARGO_PROFILE)),release,debug)

embedding: elle  ## Build + run embedding demos (Rust + C hosts)
	cargo build $(CARGO_PROFILE) -p elle-embed
	cargo run $(CARGO_PROFILE) -p elle-embed --bin host
	$(MAKE) -C demos/embedding chost TARGET_DIR=$(EMBED_TARGET_DIR)
	LD_LIBRARY_PATH=$(EMBED_TARGET_DIR) demos/embedding/chost

smoke: smoke-elle doctest embedding  ## Run the elle test corpus + docs + embedding
	@echo "=== all smoke tests passed ==="

MLIR_PREFIX ?= $(HOME)/git/tmp/mlir-install
MLIR_ENV    := LLVM_SYS_220_PREFIX=$(MLIR_PREFIX) \
               MLIR_SYS_220_PREFIX=$(MLIR_PREFIX) \
               TABLEGEN_220_PREFIX=$(MLIR_PREFIX)

test: smoke  ## Rust unit + integration tests + clippy + fmt + rustdoc after smoke
	cargo fmt --check
	$(MLIR_ENV) cargo clippy --workspace --all-targets --all-features -- -D warnings
	$(MLIR_ENV) RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
	$(MLIR_ENV) cargo test --workspace --lib --all-features
	cargo test --test '*' -- --skip property

# ── Clean ───────────────────────────────────────────────────────────

clean:  ## Remove build artifacts and generated docs
	cargo clean
	rm -f docs/pipeline.svg

space:  ## Reclaim disk: drop cargo intermediates, keep built executables
	rm -rf target/{debug,release}/{deps,build,incremental,.fingerprint,examples}

# ── Help ────────────────────────────────────────────────────────────

help:  ## Show this help
	@grep -E '^[a-z].*:.*##' $(MAKEFILE_LIST) | \
		sed 's/:.*##/\t/' | \
		column -t -s '	'
