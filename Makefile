.PHONY: all elle docs docgen smoke test qa crosscheck clean space help \
       smoke-elle smoke-vm smoke-noffi smoke-jit smoke-nouring smoke-wasm smoke-mlir \
       doctest myplugin elle-wasm check-wasm elle-mlir elle-noffi plugins plugins-all mcp embedding \
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
# `find` is told to be quiet about a missing root, so a root that moves drops
# silently out of the format gate rather than failing it. The pin that every
# Elle source in the tree stays reachable from this list is
# tests/integration/paths.rs.
LISP_FILES := $(shell find src/ lib/ tests/ demos/ tools/ -name '*.lisp' 2>/dev/null)

# oracle.lisp is the leak-measurement instrument: a couple of hundred adaptive
# empirical-Bernstein probes, each looping blocks of heap ops until its interval
# converges. That is tens of seconds of CPU regardless of tier (the JIT does not
# accelerate it — the cost is region alloc/reclaim, not bytecode interpretation),
# which outgrows both the corpus TIMEOUT and the `elle test` per-form budget and
# gets killed there. It runs with a wider budget instead, pulled out of every
# batch so each other file still fails fast on a hang. The cost tracks the probe
# COUNT, so a pass that adds probes lengthens it — read the budget from a timed
# run, never from a number written here.
# $(1) is the tier flags for the pass (e.g. --jit=off --mlir=off).
ORACLE_TIMEOUT ?= 120s
ORACLE_FILE    := tests/elle/oracle.lisp
define RUN_ORACLE
	@timeout $(ORACLE_TIMEOUT) $(ELLE) $(1) $(ORACLE_FILE) \
		|| { echo "FAILED: oracle.lisp ($(1))"; exit 1; }
endef

# The io leak dashboard rides the same shape: pulled out of every batch, run
# under its own timed budget (io probes cost wall-clock; read the number from
# a timed run, as with ORACLE_TIMEOUT above).
PLUMB_TIMEOUT ?= 60s
PLUMB_FILE    := tests/elle/plumb.lisp
define RUN_PLUMB
	@timeout $(PLUMB_TIMEOUT) $(ELLE) $(1) $(PLUMB_FILE) \
		|| { echo "FAILED: plumb.lisp ($(1))"; exit 1; }
endef

# Two corpus files spend most of a per-file budget on work the case needs:
# h2-load-volume drives 500 requests over one h2 session, and
# region-jit-io-suspend-uaf reads 20000 lines to drive one function hot enough
# for the JIT to compile it. Cut either volume and the shape stops reaching the
# state it pins. Both fit TIMEOUT with room on an idle box, and both have been
# killed at it on CI, where the runner is shared and slower by an order of
# magnitude. A killed file is exit 124 — no output, no assertion message — so it
# reads as a flaky runner rather than as a budget that was never wide enough.
# They get a wider one, which is the bargain ORACLE_TIMEOUT already makes, in
# per-file form: an override for the files that need it, so every other file
# still fails fast on a hang. The pins are tests/integration/budget.rs.
#
# The wider budget is a BACKSTOP, not the deadline. h2-load-volume carries its
# own deadline and reports which request stalled and how long it waited; that
# message is the reason to run the file, and it only ever prints if the outer
# kill lands after it. Keep this above any in-file deadline, and read both from
# a timed run rather than from a number written here — remembering that a run
# `timeout` killed reports the cap, not what the file would have cost.
WIDE_TIMEOUT ?= 120s
WIDE_FILES   := -e h2-load-volume.lisp -e region-jit-io-suspend-uaf.lisp

# The budget for ONE corpus file: `parallel` substitutes the path into `{}` and
# the pass's shell picks a budget, once per file. Every pass that runs the corpus
# one file at a time spells `timeout $(FILE_TIMEOUT)` — a pass that spells
# `timeout $(TIMEOUT)` hands the narrow budget to the two files above.
#
# A `case` reads better here and does not survive the trip. `parallel` runs this
# under the platform's `/bin/sh`, which on macOS is bash 3.2, and that parser
# ends a `$(…)` at the first `)` inside it — the one closing a case pattern. The
# corpus then dies file by file on a syntax error rather than on anything it
# tests. `grep` needs no parentheses, and matches the shape the skip lists above
# already use. tests/integration/budget.rs runs the real thing under `sh`.
FILE_TIMEOUT = $$(echo {} | grep -q $(WIDE_FILES) && echo $(WIDE_TIMEOUT) || echo $(TIMEOUT))

# One corpus pass with ONE PROCESS PER FILE, under a wall-clock TIMEOUT. Each
# file starts, runs as a whole program, and exits — the shape `elle test` never
# takes, and the only one that covers program teardown and process-global modes.
#
# $(1) the pass's `grep -v` skip patterns   $(3) the pass name, for the failure
# $(2) the elle flags for the pass          $(4) a short tag, for the joblog
# No argument may contain a comma: `$(call)` splits on them.
#
# The joblog is what makes a failure readable. `parallel` reports only a COUNT
# of failed jobs, and a file killed by `timeout` prints nothing at all — exit
# 124, no output, no name. Without the log the gate says "a file failed" and the
# reader has to bisect the corpus to learn which. Every non-zero row is named
# here, with its exit status and signal, before the gate fails.
define RUN_PER_FILE
	@mkdir -p target
	@rm -f target/smoke-$(4).joblog
	@printf '%s\n' tests/elle/*.lisp \
		| grep -v $(1) | grep -v $(ORACLE_FILE) | grep -v $(PLUMB_FILE) \
		| parallel -j $(JOBS) --tag --joblog target/smoke-$(4).joblog \
			'timeout $(FILE_TIMEOUT) $(ELLE) $(2) {}' \
		|| { \
			echo "--- files that failed the $(3) ---"; \
			awk 'NR > 1 && ($$7 != 0 || $$8 != 0) { \
				printf "  %s  exit=%s signal=%s  (%ss)\n", $$NF, $$7, $$8, $$4 }' \
				target/smoke-$(4).joblog; \
			echo "FAILED: elle tests $(3) — see target/smoke-$(4).joblog"; \
			exit 1; }
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
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
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
#   make smoke    docs + the elle test corpus (runner + per-file passes) + embedding
#   make qa       ~2min: the PR gate's QA job (rustfmt, workspace clippy, crosscheck, rustdoc)
#   make test     smoke + qa + rust unit/integration
#   cargo test    ~60min full suite (unit + integration + property)
#
# `make test` exists to predict the PR gate, so it runs what the gate runs. A
# target the workflow requires and `make test` skips is a failure a branch can
# only discover in CI.
#
# The default-build elle scripts get TWO corpus passes, because they are two
# different tests of the same files:
#
#   smoke-elle  ONE `elle test` invocation, whole corpus. It drives each file's
#               forms from inside one long-lived process under a per-form
#               budget, covering the vm and jit policies and cross-tier
#               divergence in that one run (see docs/testing.md).
#   smoke-vm    One process PER FILE, `--jit=off --mlir=off`, under a wall-clock
#   smoke-jit   TIMEOUT. Each file has to start, run as a whole program, and
#               EXIT. Program teardown, process-global config, and anything
#               wall-clock-sensitive are reachable only here.
#
# The featured builds run the SAME corpus through the runner
# (smoke-mlir/smoke-wasm — the binary's extra tier joins the matrix and its
# divergence rows land in the session DB), plus one whole-file pass in the
# process-global mode the runner cannot vary per file (--mlir=eager /
# --wasm=full). smoke-noffi is the same per-file shape for a build with no
# features.

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
# parked here with a tracked reason, plus the one file whose budget the runner
# cannot express.
#
# oracle.lisp is that file: it is a measurement instrument whose cost is tens of
# seconds of region alloc/reclaim on any tier, close enough to the runner's
# per-form budget that a batch running it beside 24 other files loses the race
# and records `timeout`. `RUN_CORPUS` runs it after the batch under
# `ORACLE_TIMEOUT` instead, which is what smoke-vm/jit/noffi already do — so the
# gate still covers it on both policies and every other file keeps failing fast
# on a hang.
#
# (Resolved: subprocess.lisp used to hang in a worker thread — children inherited
# the worker's all-blocked signal mask across fork/exec, so SIGTERM never landed
# and subprocess/wait wedged. Fixed by resetting the child's mask in pre_exec;
# see src/io/request.rs reset_child_signals + docs/posix-signals.md.)
ELLE_TEST_SKIP := $(ORACLE_FILE) $(PLUMB_FILE)

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
#
# wasm-tier-error-signal drives the TIERED backend through `compile/run-on
# :wasm`, which a whole-file `--wasm=full` compile cannot host — the forced tier
# needs the bytecode VM underneath it. Same shape as eval: the runner records it
# :ineligible per form, so the corpus pass still covers the file.
WASM_SKIP := -e eval.lisp -e eval-env.lisp -e wasm-tier-error-signal.lisp

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

# The files are dealt to the batches in hash-of-name order, not alphabetically.
# Sibling files share a name prefix and a subject, and a subject's files cost
# about the same, so alphabetical order gathers the whole corpus's heaviest
# files into one or two batches and leaves the rest nearly empty. Ordering by a
# hash of the path spreads each subject across the run, which flattens the peak
# every batch has to fit. The hash is a plain djb2 over the path, so the order
# is the same on every box and every run: a batch that fits today fits
# tomorrow, and a batch that does not can be reproduced. Both stages run under
# LC_ALL=C so the byte table and the sort do not follow the caller's locale.
DEAL_CORPUS := LC_ALL=C awk 'BEGIN { for (i = 0; i < 256; i++) ord[sprintf("%c", i)] = i } { h = 5381; for (i = 1; i <= length($$0); i++) h = (h * 33 + ord[substr($$0, i, 1)]) % 1000003; printf "%07d\t%s\n", h, $$0 }' | LC_ALL=C sort | cut -f2-

# ELLE_TEST_FLAGS threads extra runner flags into every corpus batch — empty
# by default. The macOS CI job sets `--trace=scrub` here: a released page is
# zeroed before the pool caches it, so a read through a stale pointer panics
# at the deref naming its site instead of surfacing minutes later as a
# wrong-typed value or a wedge (docs/impl/region/diagnostics.md).
ELLE_TEST_FLAGS ?=

define RUN_CORPUS
	@printf '%s\n' $(filter-out $(ELLE_TEST_SKIP),$(wildcard tests/elle/*.lisp)) \
		| $(DEAL_CORPUS) \
		| xargs -n $(CORPUS_BATCH) $(ELLE) test $(ELLE_TEST_FLAGS) \
		|| { echo "FAILED: elle test — a batch failed or was killed; query the session DB (docs/testing.md § Reading a run)"; exit 1; }
	$(call RUN_ORACLE,--jit=off)
	$(call RUN_ORACLE,--jit=eager)
	$(call RUN_PLUMB,--jit=off)
	$(call RUN_PLUMB,--jit=eager)
endef

smoke-elle: elle  ## Run the whole corpus through `elle test` (vm + jit + divergence)
	@echo "=== elle test (vm + jit policies, cross-tier divergence) ==="
	$(RUN_CORPUS)

smoke-vm: elle
	@echo "=== elle tests (VM, no JIT) ==="
	$(call RUN_PER_FILE,$(ELLE_SKIP_VM),--jit=off --mlir=off,VM-only pass (no JIT),vm)
	$(call RUN_ORACLE,--jit=off --mlir=off)
	$(call RUN_PLUMB,--jit=off --mlir=off)

elle-noffi:           ## Build elle with no features (for smoke-noffi)
	@echo "=== build elle with no features ==="
	cargo build $(CARGO_PROFILE) -p elle --no-default-features -q

smoke-noffi: elle-noffi
	@echo "=== elle tests (VM, no features) ==="
	$(call RUN_PER_FILE,$(ELLE_SKIP_VM) $(ELLE_SKIP_FFI),--jit=off,VM-only pass (no features),noffi)
	$(call RUN_ORACLE,--jit=off)
	$(call RUN_PLUMB,--jit=off)

smoke-jit: elle
	@echo "=== elle tests (eager JIT) ==="
	$(call RUN_PER_FILE,$(ELLE_SKIP_JIT),--jit=eager,JIT pass (eager),jit)
	$(call RUN_ORACLE,--jit=eager)
	$(call RUN_PLUMB,--jit=eager)

# The thread-pool I/O backend, on a Linux box. `create_platform_backend` picks
# the ring here and the pool on every other platform, so a Linux-only gate runs
# the whole corpus against the ring and none of it against the pool. This is the
# runtime half of the argument `crosscheck` makes below for the macOS `cfg`
# arms: that one compiles the code a Linux build never compiles, this one runs
# the code a Linux build never runs.
#
# Without it the pool's only sampler was the macOS job — once per PR, on the
# slowest runner, against failure modes that hang rather than fail. A hang
# spends the whole per-file budget, so it reads as a flaky timeout rather than
# as the defect it is, and two pool-only defects reached main that way.
#
# The per-file passes rather than the runner: whole-program teardown and
# anything wall-clock-sensitive are reachable only there (see the pass
# descriptions above), and that is where both defects surfaced.
#
# `tests/integration/elle_scripts.rs` § "I/O backend selection" pins a handful
# of corpus files under this flag one at a time, which is what this target
# generalises; those stay, because they also run under debug assertions.
smoke-nouring: elle  ## Corpus per-file passes on the thread-pool backend (what every non-Linux build runs)
	@echo "=== elle tests (thread-pool backend, VM) ==="
	$(call RUN_PER_FILE,$(ELLE_SKIP_VM),--no-uring --jit=off --mlir=off,thread-pool VM pass,nouring-vm)
	$(call RUN_ORACLE,--no-uring --jit=off --mlir=off)
	$(call RUN_PLUMB,--no-uring --jit=off --mlir=off)
	@echo "=== elle tests (thread-pool backend, eager JIT) ==="
	$(call RUN_PER_FILE,$(ELLE_SKIP_JIT),--no-uring --jit=eager,thread-pool JIT pass,nouring-jit)
	$(call RUN_ORACLE,--no-uring --jit=eager)
	$(call RUN_PLUMB,--no-uring --jit=eager)

elle-mlir:   ## Build elle with MLIR support (for smoke-mlir)
	@echo "=== build elle with MLIR ==="
	cargo build $(CARGO_PROFILE) -p elle --features mlir -q

smoke-mlir: elle-mlir  ## Corpus via elle test (+ mlir-cpu tier) + whole-file --mlir=eager pass
	@echo "=== elle test (mlir build: + mlir-cpu tier, cross-tier divergence) ==="
	$(RUN_CORPUS)
	@echo "=== elle tests (eager MLIR, whole-file) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ORACLE_FILE) | grep -v $(PLUMB_FILE) | \
		parallel -j $(JOBS) --tag \
			'timeout $(FILE_TIMEOUT) $(ELLE) --mlir=eager {}' \
		|| { echo "FAILED: elle tests MLIR pass (eager)"; exit 1; }
	$(call RUN_ORACLE,--mlir=eager)
	$(call RUN_PLUMB,--mlir=eager)

elle-wasm:   ## Build elle with WASM support (for check-wasm/smoke-wasm)
	@echo "=== build elle with WASM ==="
	cargo build $(CARGO_PROFILE) -p elle --features wasm -q

# The CI gate for the wasm backend while the tier carries no production
# workloads: the feature still compiles, and the full-module tier still boots —
# compiles a module to wasm, executes it, returns. The `[wasm]` marker is the
# proof the tier engaged: a binary built WITHOUT the feature accepts
# `--wasm=full` and silently runs the VM, which would green a build gate that
# gated nothing. Full corpus coverage on this tier is smoke-wasm.
check-wasm: elle-wasm  ## Build the WASM backend and boot one module through it
	@echo "=== wasm boot check ==="
	@out=$$(timeout 300s $(ELLE) --wasm=full tests/elle/arithmetic.lisp 2>&1); code=$$?; \
	printf '%s\n' "$$out"; \
	[ $$code -eq 0 ] || { echo "FAILED: wasm boot (exit $$code)"; exit 1; }; \
	printf '%s\n' "$$out" | grep -q '\[wasm\]' \
		|| { echo "FAILED: wasm boot ran without engaging the wasm tier"; exit 1; }

smoke-wasm: elle-wasm  ## Corpus via elle test (+ wasm tier) + whole-file --wasm=full pass
	@echo "=== elle test (wasm build: + wasm tier, cross-tier divergence) ==="
	$(RUN_CORPUS)
	@echo "=== elle tests (WASM, whole-file) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(WASM_SKIP) | \
		parallel -j $(WASM_JOBS) --tag \
			'timeout 300s $(ELLE) --wasm=full {}' \
		|| { echo "FAILED: elle tests WASM pass (full)"; exit 1; }

# A literate doc is one whole program, not one corpus form: the scheduler docs
# (processes.md, threads.md) run a dozen process systems in sequence, which is
# minutes of debug-profile CPU — the corpus TIMEOUT kills them mid-run on any
# debug binary. Same shape as ORACLE_TIMEOUT: a wider per-file budget, so every
# other file still fails fast on a hang. Read the budget from a timed run.
DOCTEST_TIMEOUT ?= 180s

# The plugin two literate documents load. docs/cookbook/plugins.md is the
# authoring guide and demos/myplugin is the crate it walks through, so the
# document's own test imports it as `plugin/myplugin`; docs/testing.md gates its
# example on the same import. Nothing else in the tree builds it — the
# `plugins/` submodule is a separate workspace, and it is not even checked out
# in CI — so without this every form below either import is dead: the guide
# fails its import, the gating example gates itself out, and `doctest` reports
# both as passing. tests/integration/doctest.rs pins the agreement.
myplugin:  ## Build the plugin the literate documents load
	cargo build $(CARGO_PROFILE) -p elle-myplugin -q

doctest: myplugin  ## Test code examples in documentation (literate mode)
	@echo "=== doctest ==="
	@printf '%s\n' docs/*.md docs/regions/*.md docs/impl/*.md docs/cookbook/*.md docs/signals/*.md docs/analysis/*.md | \
		parallel -j $(JOBS) --tag \
			'timeout $(DOCTEST_TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: doctest"; exit 1; }

EMBED_TARGET_DIR = $(CURDIR)/target/$(if $(findstring --release,$(CARGO_PROFILE)),release,debug)

embedding: elle  ## Build + run embedding demos (Rust + C hosts)
	cargo build $(CARGO_PROFILE) -p elle-embed
	cargo run $(CARGO_PROFILE) -p elle-embed --bin host
	$(MAKE) -C demos/embedding chost TARGET_DIR=$(EMBED_TARGET_DIR)
	LD_LIBRARY_PATH=$(EMBED_TARGET_DIR) demos/embedding/chost

# The two corpus passes are not the same test, so the gate runs both.
# `smoke-elle` drives every file's forms from inside one long-lived `elle test`
# process, under a per-form budget. `smoke-vm`/`smoke-jit` run each file as its
# own process that has to start, run as a whole program, and EXIT, under a
# wall-clock `TIMEOUT`. Program teardown, process-global config and anything
# wall-clock-sensitive are only reachable the second way, which is why the PR
# workflow's "VM+JIT Tests" job gates on those two targets. A `make smoke` that
# skipped them was weaker than the gate it exists to predict.
smoke: smoke-elle smoke-vm smoke-jit doctest embedding  ## Run the elle test corpus (runner + per-file VM and JIT passes) + docs + embedding
	@echo "=== all smoke tests passed ==="

MLIR_PREFIX ?= $(HOME)/git/tmp/mlir-install
MLIR_ENV    := LLVM_SYS_220_PREFIX=$(MLIR_PREFIX) \
               MLIR_SYS_220_PREFIX=$(MLIR_PREFIX) \
               TABLEGEN_220_PREFIX=$(MLIR_PREFIX)

# CI documents private items too, and most of this crate is private — without
# the flag rustdoc never resolves a link into a `pub(crate)` item, so a broken
# one reaches CI unseen. Keep the flag here and in .github/workflows in step.
qa: crosscheck  ## The PR gate's QA job, locally (~2min, no smoke): rustfmt, workspace clippy, rustdoc
	cargo fmt --check
	$(MLIR_ENV) cargo clippy --workspace --all-targets --all-features -- -D warnings
	$(MLIR_ENV) RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --document-private-items

test: smoke smoke-nouring qa  ## Rust unit + integration tests + QA (fmt/clippy/crosscheck/rustdoc) after smoke
	$(MLIR_ENV) cargo test --workspace --lib --all-features
	cargo test --test '*' -- --skip property

# Clippy over the macOS arm of every `cfg(target_os)`. A Linux-only gate sees
# only the io_uring side, so a binding the thread-pool backend never reads
# stays invisible until the Mac runner reports it. Clippy does not codegen or
# link, so this needs no macOS SDK — only the target's std. `ffi` and `zstd`
# build C for the host and cannot cross, hence `--no-default-features`; that
# also drops the variant balancing `HeapObject`, so allow that one lint (the
# default-features gates above still enforce it). CI's QA job runs the same
# command, so a missing target here only costs local feedback.
CROSS_TARGET := x86_64-apple-darwin

crosscheck:  ## Clippy the macOS cfg arms (cross-target, no SDK needed)
	@rustup target list --installed | grep -qx '$(CROSS_TARGET)' || { \
		echo "SKIPPED crosscheck: rustup target add $(CROSS_TARGET)"; exit 0; }; \
	cargo clippy --target $(CROSS_TARGET) --no-default-features -p elle \
		-- -D warnings -A clippy::large_enum_variant

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
