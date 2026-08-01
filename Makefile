.PHONY: all elle docs docgen smoke test clean space help \
       smoke-elle smoke-vm smoke-noffi smoke-jit smoke-wasm smoke-mlir \
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

# oracle.lisp is the leak-measurement instrument: dozens of adaptive
# empirical-Bernstein probes, each looping blocks of heap ops until their
# interval converges. That is ~17s of CPU regardless of tier (the JIT does not
# accelerate it — the cost is region alloc/reclaim, not bytecode interpretation),
# which fits the corpus TIMEOUT on a fast box but stretches past 30s on CI's slow
# shared cores and gets SIGTERM'd. It (only) runs with a wider budget, pulled out
# of the tight parallel batch so every other file still fails fast on a hang.
# $(1) is the tier flags for the pass (e.g. --jit=off --mlir=off).
ORACLE_TIMEOUT ?= 120s
ORACLE_FILE    := tests/elle/oracle.lisp
define RUN_ORACLE
	@timeout $(ORACLE_TIMEOUT) $(ELLE) $(1) $(ORACLE_FILE) \
		|| { echo "FAILED: oracle.lisp ($(1))"; exit 1; }
endef

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
# The default-build elle scripts run through the agent-first runner
# (`smoke-elle`, see docs/testing.md): ONE `elle test` invocation covers the vm
# and jit policies and cross-tier divergence. The featured builds run the SAME
# corpus through the runner (smoke-mlir/smoke-wasm — the binary's extra tier
# joins the matrix and its divergence rows land in the session DB), plus one
# whole-file pass in the process-global mode the runner cannot vary per file
# (--mlir=eager / --wasm=full). smoke-vm/smoke-jit/smoke-noffi are direct-run
# debug targets for isolating a single backend.

# The agent-first runner: ONE process, the whole corpus, ONE SQLite session DB.
# `elle test` (docs/testing.md, docs/test-runner.md) compiles + runs every file
# and records each (form × tier) result; the gate is its exit code. A multi-form
# file runs under the :off JIT policy (recorded `vm`) AND :eager (recorded
# `jit`), while single-form files run on every tier with divergence — so one
# invocation is the whole vm/jit/differential gate. No per-pass skip list
# applies: a test gates itself in-file (gate!/:gated) and a backend the build
# lacks is dropped, not skip-listed. The session DB is the runner's default
# ($ELLE_CACHE/elle-tests.db) — every run, make-driven or not, accumulates in
# the one history that `--summary`/`--query` and the regression-archaeology
# queries read. Never point a run at a private DB. Concurrent runs share it
# safely: the connection waits on a busy database rather than raising
# (docs/test-runner.md § Concurrent runs wait).

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

# FFI skip list: tests requiring the `ffi` feature (skipped when built
# --no-default-features). These reference ffi/* primitives that are compiled out
# of a no-features build — some as a runtime "requires `ffi` feature" error
# (prim-ffi), others absent entirely so the file won't even compile
# (region-ffi-callback-arg-uaf's `ffi/callback`), which a runtime gate can't
# catch. prim-ffi is listed explicitly rather than left to the `ffi.lisp`
# substring accidentally matching `prim-ffi.lisp`.
ELLE_SKIP_FFI := -e ffi.lisp -e prim-ffi.lisp -e region-ffi-callback-arg-uaf.lisp \
                 -e compress.lisp -e sqlite.lisp -e zmq.lisp -e git.lisp -e http.lisp

# Skip list for the whole-file --wasm=full pass only (eval = dynamic
# compilation, not in the WASM backend). The runner needs no list: a form a
# tier cannot host is recorded :ineligible→skip per form.
WASM_SKIP := -e eval.lisp -e eval-env.lisp

# One corpus pass through the agent-first runner. Shared by smoke-elle and the
# featured-build targets: the runner probes the tiers the binary carries, so
# the same invocation gains the mlir-cpu / wasm tier — and its divergence
# rows — when $(ELLE) was built with that feature.
#
# The corpus runs in BATCHES of $(CORPUS_BATCH) files per `elle test` process,
# not one process over the whole corpus. The runner holds every file's compiled
# module and region heap for the process's lifetime, so a single all-files
# invocation grows without bound and is OOM-killed partway through — silently
# truncating coverage to whatever ran before the kill. Bounding files per process
# bounds peak memory; `xargs` runs every batch, so a batch that fails a test (exit
# 1–125) or is OOM-killed (a signal, which halts xargs) drives a non-zero exit and
# fails the gate loud. Divergence is a within-file, cross-tier property, so
# batching by file does not weaken it, and every batch appends to the one session
# DB that `--query`/`--summary` read (docs/testing.md § Reading a run).
CORPUS_BATCH ?= 25
define RUN_CORPUS
	@printf '%s\n' $(filter-out $(ELLE_TEST_SKIP),$(wildcard tests/elle/*.lisp)) \
		| xargs -n $(CORPUS_BATCH) $(ELLE) test \
		|| { echo "FAILED: elle test — a batch failed or was killed; query the session DB (docs/testing.md § Reading a run)"; exit 1; }
endef

smoke-elle: elle  ## Run the whole corpus through `elle test` (vm + jit + divergence)
	@echo "=== elle test (vm + jit policies, cross-tier divergence) ==="
	$(RUN_CORPUS)

smoke-vm: elle
	@echo "=== elle tests (VM, no JIT) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | grep -v $(ORACLE_FILE) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=off --mlir=off {}' \
		|| { echo "FAILED: elle tests VM-only pass (no JIT)"; exit 1; }
	$(call RUN_ORACLE,--jit=off --mlir=off)

elle-noffi:           ## Build elle with no features (for smoke-noffi)
	@echo "=== build elle with no features ==="
	cargo build $(CARGO_PROFILE) -p elle --no-default-features -q

smoke-noffi: elle-noffi
	@echo "=== elle tests (VM, no features) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | grep -v $(ELLE_SKIP_FFI) | grep -v $(ORACLE_FILE) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=off {}' \
		|| { echo "FAILED: elle tests VM-only pass (no features)"; exit 1; }
	$(call RUN_ORACLE,--jit=off)

smoke-jit: elle
	@echo "=== elle tests (eager JIT) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_JIT) | grep -v $(ORACLE_FILE) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=eager {}' \
		|| { echo "FAILED: elle tests JIT pass (eager)"; exit 1; }
	$(call RUN_ORACLE,--jit=eager)

elle-mlir:   ## Build elle with MLIR support (for smoke-mlir)
	@echo "=== build elle with MLIR ==="
	cargo build $(CARGO_PROFILE) -p elle --features mlir -q

smoke-mlir: elle-mlir  ## Corpus via elle test (+ mlir-cpu tier) + whole-file --mlir=eager pass
	@echo "=== elle test (mlir build: + mlir-cpu tier, cross-tier divergence) ==="
	$(RUN_CORPUS)
	@echo "=== elle tests (eager MLIR, whole-file) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ORACLE_FILE) | \
		parallel -j $(JOBS) --tag \
			'timeout $(TIMEOUT) $(ELLE) --mlir=eager {}' \
		|| { echo "FAILED: elle tests MLIR pass (eager)"; exit 1; }
	$(call RUN_ORACLE,--mlir=eager)

elle-wasm:   ## Build elle with WASM support (for smoke-wasm)
	@echo "=== build elle with WASM ==="
	cargo build $(CARGO_PROFILE) -p elle --features wasm -q

smoke-wasm: elle-wasm  ## Corpus via elle test (+ wasm tier) + whole-file --wasm=full pass
	@echo "=== elle test (wasm build: + wasm tier, cross-tier divergence) ==="
	$(RUN_CORPUS)
	@echo "=== elle tests (WASM, whole-file) ==="
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
