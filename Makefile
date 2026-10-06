.PHONY: all elle elle-rig docs docgen smoke test qa crosscheck clean space help \
       smoke-lang smoke-impl smoke-boot-image smoke-nojit smoke-pool smoke-mlir \
       smoke-noffi smoke-wasm \
       elle-nojit elle-pool elle-mlir elle-noffi elle-wasm check-wasm \
       doctest doctest-list myplugin plugins plugins-all \
       plugins-verify smoke-plugins mcp embedding semver-check \
       fmt fmt-check audit agents agents-check

.DEFAULT_GOAL := all

# The processors the box will schedule on.
#
# `nproc` first because it is the only one of the three that honours a CPU
# affinity mask: a run pinned to a subset of the box gets the subset, not the
# box. It is GNU coreutils, so it is absent on a stock macOS runner — and the
# `brew install coreutils` the macOS job runs installs it as `gnproc`, not on
# PATH under this name. `getconf` is the portable answer both platforms give.
# The literal is reached only if a box has neither.
CORES := $(shell nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)

ifdef GITHUB_ACTIONS
  # One runner process per processor. GitHub gives the Linux runners four and
  # the macOS runner three, and re-sizes them without notice, so this is read
  # from the runner rather than written down — a constant fits the runner it was
  # chosen on and over-subscribes the others. See docs/analysis/ci.md;
  # tests/integration/capacity.rs is the standing check.
  JOBS          ?= $(CORES)
  # Not $(CORES): the wasm passes are bounded by memory, not by processors. Each
  # `--wasm=full` run compiles a whole module before it runs anything.
  WASM_JOBS     ?= 2
  ELLE          ?= ./target/release/elle
  CARGO_PROFILE := --release
else
  # Deliberately a constant, and deliberately not $(CORES): a development box
  # shares its cores with everything else its owner is running, and its owner
  # can pass JOBS= when that is wrong.
  JOBS          ?= 16
  WASM_JOBS     ?= 4
  ELLE          ?= ./target/debug/elle
  CARGO_PROFILE :=
endif

# Where cargo leaves what this Makefile builds, for the profile it builds.
CARGO_OUT = target/$(if $(findstring --release,$(CARGO_PROFILE)),release,debug)

# The rig sits beside the `elle` it is built with (rig/overview.md), so it
# follows `ELLE` unless named.
ELLE_RIG ?= $(dir $(ELLE))elle-rig

# The WASM and MLIR builds, each with its rig: binaries of their own beside
# `elle`, which stays the default build (bins/overview.md).
ELLE_WASM     ?= $(CARGO_OUT)/elle-wasm
ELLE_RIG_WASM ?= $(CARGO_OUT)/elle-rig-wasm
ELLE_MLIR     ?= $(CARGO_OUT)/elle-mlir
ELLE_RIG_MLIR ?= $(CARGO_OUT)/elle-rig-mlir

# `find` is told to be quiet about a missing root, so a root that moves drops
# silently out of the format gate rather than failing it. The pin that every
# Elle source in the tree stays reachable from this list is
# tests/integration/paths.rs.
LISP_FILES := $(shell find src/ lib/ tests/ demos/ tools/ docs/ -name '*.lisp' 2>/dev/null)

all: elle docs  ## Build everything

# ── Build ───────────────────────────────────────────────────────────

elle:  ## Build the Elle binary
	cargo build $(CARGO_PROFILE) -p elle

elle-rig:  ## Build the rig: the Elle runtime a test configures (rig/overview.md)
	cargo build $(CARGO_PROFILE) -p elle-rig

MCP_PATCH := --config 'patch."https://github.com/elle-lisp/elle".elle-plugin.path="elle-plugin"'

plugins:  ## Build all portable plugins (from plugins submodule)
	$(MAKE) -C plugins portable

plugins-all:  ## Build every plugin in the submodule's workspace
	$(MAKE) -C plugins all

mcp: elle  ## Build elle + MCP plugins (oxigraph, syn)
	$(MAKE) -C plugins mcp

# The portable plugin set has ONE home: `PORTABLE` in plugins/Makefile. This
# reads that variable back out of the submodule's own make instead of repeating
# the list, so a plugin added there is demanded here with no second edit, and a
# plugin dropped there stops being demanded. The submodule carries no `print-%`
# rule, so one is supplied for the read.
#
# Recursively expanded (`=`, not `:=`): the read runs `make -C plugins`, and
# every invocation of this Makefile would pay for it under `:=` — including the
# ones on a tree where the submodule was never checked out.
#
# A package's cdylib is `lib<package with - as _>.so`, because no plugin crate
# overrides `[lib] name`. tests/integration/plugins.rs pins that mapping against
# the crates the submodule actually contains.
PORTABLE_PKGS = $(filter-out -p,$(shell $(MAKE) -s -C plugins --eval='print-portable:; @echo $$(PORTABLE)' print-portable))
PORTABLE_SO   = $(foreach p,$(PORTABLE_PKGS),target/release/lib$(subst -,_,$(p)).so)

# Every portable plugin produced its artifact.
#
# This is what makes the plugin corpus's self-gating harmless. Each
# plugins/tests/*.lisp imports its `.so` under `protect` and exits 0 when the
# import fails, so a plugin that did not build makes its own test report
# success. Asserting the build output separately means the tests never have to
# be the thing that detects a missing plugin. See docs/analysis/ci.md § "The
# plugins job".
#
# The empty list is a failure, not a vacuous pass: it is what a tree whose
# `plugins/` submodule was never checked out looks like.
#
# No prerequisite on `plugins`: the assertion is about what is on disk, and
# keeping it free of a build is what lets tests/integration/plugins.rs drive it
# both ways in a second.
plugins-verify:  ## Assert every portable plugin built its artifact
	@[ -n "$(PORTABLE_SO)" ] || { \
		echo "FAILED: no portable plugins named — run: git submodule update --init plugins"; \
		exit 1; }
	@missing=""; for so in $(PORTABLE_SO); do \
		[ -f "$$so" ] || missing="$$missing $$so"; \
	done; \
	[ -z "$$missing" ] || { \
		echo "FAILED: the plugins build produced no artifact for:$$missing"; \
		echo "  Each plugins/tests/*.lisp exits 0 when its import fails, so the"; \
		echo "  corpus would report success without them. Run: make plugins"; \
		exit 1; }
	@echo "=== plugins: $(words $(PORTABLE_SO)) portable artifacts present ==="

# ── Docs ────────────────────────────────────────────────────────────

docs: docs/pipeline.svg  ## Generate documentation assets

docs/pipeline.svg: docs/pipeline.dot
	dot -Tsvg $< -o $@

docgen: elle  ## Generate documentation site (Rust docs + Elle site)
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
	$(ELLE) demos/docgen/generate.lisp

audit:  ## What to audit next against DOCUMENTATION.md
	@./scripts/audit

agents:  ## Write each directory's AGENTS.md from its documents' call-outs
	@./scripts/agents

agents-check:  ## Fail when a committed AGENTS.md is stale
	@./scripts/agents --check

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

semver-check: elle  ## Verify every versioned library surface against its committed .surface
	$(ELLE) semver


# ── Test ────────────────────────────────────────────────────────────

# Approximate runtimes (for guidance — vary by machine):
#   make smoke    ~30min release: qa, both suites, doctests, embedding, the surface gate
#   make qa       ~2min: the PR gate's QA job (rustfmt, indexes, clippy, crosscheck, rustdoc)
#   make test     smoke + the Rust unit, integration and rig tests
#   cargo test    ~60min full suite (unit + integration + property)
#
# `make test` exists to predict the PR gate, so it runs what the gate runs. A
# target the workflow requires and `make test` skips is a failure a branch can
# only discover in CI.
#
# `smoke-plugins` is the one exception, and it is deliberate. It needs the
# `plugins/` submodule checked out, and the submodule is optional for building
# elle (INSTALL.md), so folding it in here would fail every tree that never
# initialized it. Run it by hand after `make plugins` when a change touches
# `elle_api!`; the `Plugin Tests` job runs it on every pull request.
#
# Two suites (docs/spec.md). The language suite, tests/lang, asserts what every
# correct Elle does, and every build runs it with no flag. The implementation
# suite, tests/impl, checks this implementation, and runs on the rig, which
# reads the mode each file's sidecar names (rig/overview.md). Each build is an
# implementation, and each runs the language suite: smoke-lang on the default
# build, smoke-nojit, smoke-pool, smoke-mlir and smoke-noffi on the others
# (docs/analysis/ci.md).

LANG_FILES := $(sort $(wildcard tests/lang/*.lisp))
IMPL_FILES := $(sort $(wildcard tests/impl/*.lisp))

# The producers, the files a ledger's `(producer "…")` header names, run
# in-process on the rig in a pass of their own, and the isolated passes run the
# rest (docs/test-runner.md). Read off the ledgers, so the ledger directory
# stays the one list. The runner's own producer, `elle test`, is no file.
PRODUCER_FILES      := $(sort $(shell sed -n 's/^(producer "\(.*\.lisp\)")$$/\1/p' tests/ledger/*.lisp))
ISOLATED_IMPL_FILES := $(filter-out $(PRODUCER_FILES),$(IMPL_FILES))

# The charge pass reads what each file's second run in-process costs the
# runner's heap (docs/test-gauges.md). It runs the language suite and the
# implementation files with no sidecar, since a sidecar names a mode an
# in-process run cannot give. It leaves out the producers, which have their
# own pass, and config.lisp, which asserts that a program cannot change the JIT
# policy the in-process runner sets.
CHARGE_SKIP       := tests/impl/config.lisp
SIDECAR_FREE_IMPL := $(foreach f,$(IMPL_FILES),$(if $(wildcard $(f:.lisp=.toml)),,$(f)))
CHARGE_FILES      := $(filter-out $(PRODUCER_FILES) $(CHARGE_SKIP),$(LANG_FILES) $(SIDECAR_FREE_IMPL))

# The runner's own acceptance tests drive `elle test` themselves and read the
# store the pass records into, so they ride the implementation suite's first
# pass.
RUNNER_ACCEPTANCE := tests/runner/tiers.lisp tests/runner/acceptance.lisp

# The rig profiles (rig/overview.md). The eager profile runs both suites with
# every function compiled on its first call. `IMPL_PROFILES` names more profiles
# for the language suite: the macOS job sets it to tests/impl/profiles/scrub.toml.
EAGER_PROFILE     := tests/impl/profiles/jit-eager.toml
WASM_FULL_PROFILE := tests/impl/profiles/wasm-full.toml
IMPL_PROFILES     ?=

# The build with no features cannot compile a file that calls an `ffi/`
# primitive: some fail at run time with "requires `ffi` feature", and others
# name a primitive the build does not have, so the file never compiles and no
# gate inside it can run. The patterns are `grep -e` substrings of a path.
ELLE_SKIP_FFI := -e ffi.lisp -e prim-ffi.lisp -e compress.lisp -e sqlite.lisp \
                 -e zmq.lisp -e git.lisp -e http.lisp \
                 -e git-write.lisp -e semver-check.lisp
NOFFI_FILES = $(shell printf '%s\n' $(LANG_FILES) | grep -v $(ELLE_SKIP_FFI))

# The no-features binary, beside the runner's build. That build cannot host the
# runner: the runner's store reaches SQLite and zstd through FFI. So the pass
# runs `elle test` on the default build and each child on this binary.
ELLE_NOFFI ?= $(CARGO_OUT)/elle-noffi

# The files a whole-module `--wasm=full` compile cannot host. `eval` needs
# dynamic compilation, which the WASM backend does not have. The two tiered
# backend files force closures onto the tier with `compile/run-on :wasm`, which
# needs the bytecode VM underneath; the wasm rig's sidecar pass runs them.
WASM_SKIP := -e eval.lisp -e eval-env.lisp -e wasm-tier-error-signal.lisp \
             -e wasm-tier.lisp
WASM_FULL_FILES = $(shell printf '%s\n' $(LANG_FILES) $(IMPL_FILES) | grep -v $(WASM_SKIP))

# Some suite files spend most of the runner's default budget on work the case
# needs: the h2 families drive hundreds of requests or streams over one
# session, region-jit-io-suspend-uaf reads 20000 lines to drive one function
# hot, and the two dashboards loop a shape under a heap gauge until each
# interval converges. They fit the default on an idle box and have been killed
# at it on CI, where the runner is shared and slower. A killed file records
# `timeout` and nothing else, so it reads as a flaky runner rather than as a
# budget that was never wide enough.
#
# They get a wider budget, named here and nowhere else, so every other file
# still fails fast on a hang. The wider budget is a BACKSTOP, not the deadline:
# each h2 file carries its own `deadline` and reports which request stalled,
# and that report prints only if the runner's kill lands after it. So
# WIDE_TIMEOUT_MS stays above the largest in-file deadline, which is 120 s.
# Read both from a timed run rather than from a number written here.
#
# An h2 entry is a family prefix, not a file name: a deadline is a property of
# the family, so a new sibling arrives with the budget its neighbours have.
# One runner process takes a whole batch of files, so no shell sees a path in
# time to choose; the policy travels to the runner as flags instead.
# tests/integration/runner_budget.rs pins the list against the suites and the
# runner.
WIDE_TIMEOUT_MS ?= 150000
WIDE_FAMILIES   := h2-bidi- h2-load- h2-stream- h2-timeout- \
                   region-jit-io-suspend-uaf.lisp oracle.lisp plumb.lisp
WIDE_FLAGS      := --wide-timeout $(WIDE_TIMEOUT_MS) \
                   $(patsubst %,--wide %,$(WIDE_FAMILIES))

# Files per `elle test` process. macOS and AArch64 get a smaller batch than
# everything else; docs/analysis/ci.md
# owns the argument. HOST_OS and HOST_ARCH are overridable so that
# tests/integration/capacity.rs can present a platform the suite is not running
# on. The AArch64 runner says `Linux` to `uname -s`, so it is told apart by
# `uname -m`: `aarch64` on Linux, `arm64` on a Mac.
HOST_OS   ?= $(shell uname -s)
HOST_ARCH ?= $(shell uname -m)
ifneq ($(filter Darwin,$(HOST_OS))$(filter aarch64 arm64,$(HOST_ARCH)),)
  CORPUS_BATCH ?= 10
else
  CORPUS_BATCH ?= 25
endif

# The files are dealt to the batches in hash-of-name order, not alphabetically.
# Sibling files share a name prefix and a subject, and a subject's files cost
# about the same, so alphabetical order gathers the heaviest files into one or
# two batches and leaves the rest nearly empty. Ordering by a hash of the path
# spreads each subject across the run, which flattens the peak every batch has
# to fit. The hash is a plain djb2 over the path, so the order is the same on
# every box and every run: a batch that fits today fits tomorrow, and a batch
# that does not can be reproduced. Both stages run under LC_ALL=C so the byte
# table and the sort do not follow the caller's locale.
DEAL_CORPUS := LC_ALL=C awk 'BEGIN { for (i = 0; i < 256; i++) ord[sprintf("%c", i)] = i } { h = 5381; for (i = 1; i <= length($$0); i++) h = (h * 33 + ord[substr($$0, i, 1)]) % 1000003; printf "%07d\t%s\n", h, $$0 }' | LC_ALL=C sort | cut -f2-

# One suite pass: the files, dealt into batches, `$(JOBS)` runner processes side
# by side. A language pass runs its files inside the runner
# (docs/test-runner.md). A pass whose runner flags carry `--isolate FLAGS` runs
# each file as its own child — the runner itself, as `RUNNER FLAGS PATH`, or
# `PROGRAM FLAGS PATH` under `--host` — so it starts, runs as a whole program
# and exits, and a fault kills one child rather than the run. Every verdict
# lands in the session DB, the runner's default in the state directory
# (docs/testing.md). Concurrent runners share it: a connection waits on a busy
# database rather than raising.
#
# `xargs` runs every batch, and a batch that fails a file (exit 1–125) or dies
# on a signal drives a non-zero exit, so the gate fails loud. Every recipe
# reaches the runner through this and nowhere else:
# tests/integration/run_artifacts.rs reads the targets that call it to know
# which CI jobs record runs. The pass is one line, because
# tests/common/passes.rs reads its files and its flags off one line of
# `make --dry-run`.
#
# The runner is the build the target runs its suites on: `elle`, or a variant's
# own binary where its target sets `SUITE_ELLE`. A target-specific `ELLE` would
# lose to an `ELLE=` on the command line, which is what every CI job passes. A
# pass that names a rig as its third argument runs as that rig's `test`
# instead, so the run has the rig's build and its readings are judged
# (docs/ratchet.md). `SUITE_ELLE` reads the argument because it expands inside
# the `$(call)`.
#
# $(1) the files   $(2) the runner's flags, such as `--isolate 'FLAGS'`
# $(3) the rig the pass runs on, or nothing for the target's own build. No
# argument may contain a comma: `$(call)` splits on them.
SUITE_ELLE = $(or $(3),$(ELLE))

define RUN_SUITE
	@printf '%s\n' $(1) | $(DEAL_CORPUS) | xargs -P $(JOBS) -n $(CORPUS_BATCH) $(SUITE_ELLE) test $(2) $(WIDE_FLAGS) || { echo "FAILED: elle test — a batch failed or was killed; query the session DB (docs/testing.md)"; exit 1; }
endef

# The language suite under one rig profile. The blank line before `endef` ends
# each expansion's line, so a `foreach` over profiles makes one pass each.
define RUN_LANG_PROFILE
	@echo "=== the language suite, under $(1) ==="
	$(call RUN_SUITE,$(LANG_FILES),--isolate '--profile $(1)',$(ELLE_RIG))

endef

smoke-lang: elle  ## The language suite on this build
	@echo "=== the language suite ==="
	$(call RUN_SUITE,$(LANG_FILES),)

smoke-impl: elle elle-rig  ## The implementation suite on the rig, the producers, each file's charge, then both suites under each rig profile
	@echo "=== the implementation suite, on the rig ==="
	$(call RUN_SUITE,$(ISOLATED_IMPL_FILES) $(RUNNER_ACCEPTANCE),--isolate '',$(ELLE_RIG))
	@echo "=== both suites, every function compiled on its first call ==="
	$(call RUN_SUITE,$(LANG_FILES) $(ISOLATED_IMPL_FILES),--isolate '--profile $(EAGER_PROFILE)',$(ELLE_RIG))
	@echo "=== the producers, in-process on the rig ==="
	$(call RUN_SUITE,$(PRODUCER_FILES),,$(ELLE_RIG))
	@echo "=== each file's charge on the runner's heap, in-process on the rig ==="
	$(call RUN_SUITE,$(CHARGE_FILES),--charge,$(ELLE_RIG))
	$(foreach profile,$(IMPL_PROFILES),$(call RUN_LANG_PROFILE,$(profile)))

# The language suite booted from an image instead of from core.lisp,
# prelude.lisp and stdlib.lisp — dump-boot's gate (docs/impl/image/boot.md).
# `--boot-image=` is off by default and stays off while a hydrated stdlib
# reaches neither the JIT tier nor cross-unit inlining, so nothing else in the
# tree boots from one.
#
# The directory lives under target/ rather than the temp root: an image is
# megabytes, a store prunes the one an earlier digest left, and `make clean`
# takes the directory with the rest of the build output. It starts empty, so
# the first start below always pays the store every child then reads.
#
# The hydration proof is the `[trace:boot] image-hydrate` mark, and it is the
# whole difference between this target and `smoke-lang`. A binary that ignored
# `--boot-image=` would accept it and boot from source, and an image every
# start refuses is replaced and refused again — either way the suite passes and
# the gate reports a boot that never happened. Same argument as `check-wasm`'s
# `[wasm]` marker.
BOOT_IMAGE_DIR ?= target/boot-image

smoke-boot-image: elle  ## The language suite booted from an image (dump-boot's gate)
	@echo "=== boot image: store one, then hydrate it ==="
	@rm -rf "$(BOOT_IMAGE_DIR)"
	@$(ELLE) --boot-image=$(BOOT_IMAGE_DIR) -e '(+ 1 2)' >/dev/null
	@out=$$($(ELLE) --boot-image=$(BOOT_IMAGE_DIR) --trace=boot -e '(+ 1 2)' 2>&1 >/dev/null); \
	printf '%s\n' "$$out" | grep 'image-hydrate' \
		|| { printf '%s\n' "$$out"; \
		     echo "FAILED: the second start did not hydrate the stored image, so the suite would boot from source"; \
		     exit 1; }
	@echo "=== the language suite, each file booted from the image in $(BOOT_IMAGE_DIR) ==="
	$(call RUN_SUITE,$(LANG_FILES),--isolate '--boot-image=$(BOOT_IMAGE_DIR)')

# Each variant below is a build of its own, and runs the language suite as the
# default build does: every file, no flag. A build is its features
# (docs/config.md): the interpreter alone, the thread-pool I/O backend, the
# MLIR tier, or no features at all.

elle-nojit:  ## Build elle with no JIT tier (for smoke-nojit)
	@echo "=== build elle with no JIT tier ==="
	cargo build $(CARGO_PROFILE) -p elle --no-default-features --features ffi,uring -q

smoke-nojit: elle-nojit  ## The language suite on the interpreter alone
	@echo "=== the language suite, no JIT tier ==="
	$(call RUN_SUITE,$(LANG_FILES),)

# The thread-pool I/O backend, on a Linux box. The default build takes the ring
# on Linux and the pool on every other platform, so a Linux-only gate runs no
# suite against the pool. This is the runtime half of the argument `crosscheck`
# makes below for the macOS `cfg` arms: that one compiles the code a Linux build
# never compiles, this one runs the code a Linux build never runs. A pool-only
# defect hangs rather than fails, so on the macOS runner alone it reads as a
# flaky timeout rather than as the defect it is.
#
# The rig of the same build runs the implementation suite too: some of its files
# count the pool's worker threads, which exist on no other build.
elle-pool:  ## Build elle and its rig without io_uring (for smoke-pool)
	@echo "=== build elle and its rig without io_uring ==="
	cargo build $(CARGO_PROFILE) -p elle -p elle-rig --no-default-features --features jit,ffi -q

smoke-pool: elle-pool  ## Both suites on the thread-pool I/O backend (what every non-Linux build runs)
	@echo "=== the language suite, thread-pool I/O ==="
	$(call RUN_SUITE,$(LANG_FILES),)
	@echo "=== the implementation suite, on the thread-pool build's rig ==="
	$(call RUN_SUITE,$(ISOLATED_IMPL_FILES),--isolate '',$(ELLE_RIG))
	@echo "=== the producers, in-process on the thread-pool build's rig ==="
	$(call RUN_SUITE,$(PRODUCER_FILES),,$(ELLE_RIG))

elle-mlir:  ## Build elle-mlir and elle-rig-mlir, the MLIR build (for smoke-mlir)
	@echo "=== build elle and its rig with MLIR ==="
	cargo build $(CARGO_PROFILE) --manifest-path bins/mlir/Cargo.toml --target-dir target -q

# The MLIR build's rig is the one rig that carries the MLIR tier, so the
# implementation suite's MLIR files run there.
smoke-mlir: SUITE_ELLE = $(or $(3),$(ELLE_MLIR))
smoke-mlir: elle-mlir  ## Both suites on the MLIR build
	@echo "=== the language suite, MLIR build ==="
	$(call RUN_SUITE,$(LANG_FILES),)
	@echo "=== the implementation suite, on the MLIR build's rig ==="
	$(call RUN_SUITE,$(ISOLATED_IMPL_FILES),--isolate '',$(ELLE_RIG_MLIR))
	@echo "=== the producers, in-process on the MLIR build's rig ==="
	$(call RUN_SUITE,$(PRODUCER_FILES),,$(ELLE_RIG_MLIR))

# The no-features binary is copied beside the build, and the default build then
# rebuilt in its place, so the runner is always a build that has FFI.
elle-noffi:  ## Build elle with no features beside the default build (for smoke-noffi)
	@echo "=== build elle with no features ==="
	cargo build $(CARGO_PROFILE) -p elle --no-default-features -q
	cp $(CARGO_OUT)/elle $(ELLE_NOFFI)
	cargo build $(CARGO_PROFILE) -p elle -q

smoke-noffi: elle-noffi  ## The language suite on a build with no features, less the FFI files
	@echo "=== the language suite, no features ==="
	$(call RUN_SUITE,$(NOFFI_FILES),--host $(ELLE_NOFFI) --isolate '')

elle-wasm:  ## Build elle-wasm and elle-rig-wasm, the WASM build (for check-wasm/smoke-wasm)
	@echo "=== build elle and its rig with WASM ==="
	cargo build $(CARGO_PROFILE) --manifest-path bins/wasm/Cargo.toml --target-dir target -q

# The CI gate for the wasm backend while the tier carries no production
# workloads: the feature still compiles, and the full-module tier still boots —
# compiles a module to wasm, executes it, returns. The `[wasm]` marker is the
# proof the tier engaged: a binary that ran the VM instead would green a build
# gate that gated nothing. Suite coverage on this tier is smoke-wasm.
check-wasm: elle-wasm  ## Build the WASM backend and boot one module through it
	@echo "=== wasm boot check ==="
	@out=$$(timeout 300s $(ELLE_WASM) --wasm=full tests/lang/arithmetic.lisp 2>&1); code=$$?; \
	printf '%s\n' "$$out"; \
	[ $$code -eq 0 ] || { echo "FAILED: wasm boot (exit $$code)"; exit 1; }; \
	printf '%s\n' "$$out" | grep -q '\[wasm\]' \
		|| { echo "FAILED: wasm boot ran without engaging the wasm tier"; exit 1; }

# Both suites on the wasm build (docs/impl/wasm.md). The language suite runs
# inside the runner of `elle-wasm`, as on every build. The implementation suite
# runs on the build's rig under each file's sidecar, where the tiered backend's
# files run. Both suites then run on the rig with each file compiled whole to
# one module.
smoke-wasm: JOBS = $(WASM_JOBS)
smoke-wasm: SUITE_ELLE = $(or $(3),$(ELLE_WASM))
smoke-wasm: elle-wasm  ## Both suites on the WASM build
	@echo "=== the language suite, WASM build ==="
	$(call RUN_SUITE,$(LANG_FILES),)
	@echo "=== the implementation suite, on the WASM build's rig ==="
	$(call RUN_SUITE,$(IMPL_FILES),--host $(ELLE_RIG_WASM) --isolate '')
	@echo "=== both suites, each file compiled whole to one module ==="
	$(call RUN_SUITE,$(WASM_FULL_FILES),--host $(ELLE_RIG_WASM) --isolate '--profile $(WASM_FULL_PROFILE)')

# A literate doc is one whole program: the scheduler docs (processes.md,
# threads.md) run a dozen process systems in sequence, which is minutes of
# debug-profile CPU. So a document gets a budget of its own, wider than a suite
# file's, and every other file still fails fast on a hang. Read the budget from
# a timed run.
DOCTEST_TIMEOUT ?= 180s

# The plugin two literate documents load. docs/cookbook/plugins.md is the
# authoring guide and demos/myplugin is the crate it walks through, so the
# document's own test imports it as `plugin/myplugin`; docs/testing.md gates its
# example on the same import. Nothing else in the tree builds it — the
# `plugins/` submodule is a separate workspace, checked out by one CI job that
# runs no doctest — so without this every form below either import is dead: the guide
# fails its import, the gating example gates itself out, and `doctest` reports
# both as passing. tests/integration/doctest.rs pins the agreement.
myplugin:  ## Build the plugin the literate documents load
	cargo build $(CARGO_PROFILE) -p elle-myplugin -q


# The documents `doctest` runs: the three root guides, and every document under
# lib/ and docs/ at any depth (docs/README.md). `find` walks the directories so a
# subdirectory added later is covered without an edit here, and `make` expands
# the list itself so tests/integration/doctest_scope.rs reads what runs.
DOCTEST_DOCS := README.md QUICKSTART.md INSTALL.md $(shell find lib docs -name '*.md' | sort)

doctest: myplugin  ## Test code examples in documentation (literate mode)
	@echo "=== doctest ==="
	@printf '%s\n' $(DOCTEST_DOCS) | \
		parallel -j $(JOBS) --tag \
			'timeout $(DOCTEST_TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: doctest"; exit 1; }

doctest-list:  ## List the documents doctest runs
	@printf '%s\n' $(DOCTEST_DOCS)

# A plugin test drives a whole library through one long program — the oxigraph
# file loads an RDF store, the tree-sitter file parses a grammar — so it takes a
# budget of its own. Same shape as DOCTEST_TIMEOUT: a wider per-file budget, so
# every file still fails fast on a hang. Read the budget from a timed run, never
# from a number written here.
PLUGIN_TIMEOUT ?= 120s

# The plugin tests: one process per file, against the release binary and the
# artifacts `plugins-verify` just asserted. Each file's `import-file` path is
# relative to the repository root, so the run has to start here rather than in
# the submodule.
#
# `plugins-verify` is a prerequisite rather than a step of the CI job, so that a
# `make smoke-plugins` on a tree where `make plugins` was never run fails loud
# instead of reporting a corpus that imported nothing.
smoke-plugins: elle plugins-verify  ## Run the plugin corpus (needs `make plugins` first)
	@echo "=== plugin tests ==="
	@printf '%s\n' plugins/tests/*.lisp | \
		parallel -j $(JOBS) --tag \
			'timeout $(PLUGIN_TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: plugin tests"; exit 1; }

EMBED_TARGET_DIR = $(CURDIR)/$(CARGO_OUT)

embedding: elle  ## Build + run embedding demos (Rust + C hosts)
	cargo build $(CARGO_PROFILE) -p elle-embed
	cargo run $(CARGO_PROFILE) -p elle-embed --bin host
	$(MAKE) -C demos/embedding chost TARGET_DIR=$(EMBED_TARGET_DIR)
	LD_LIBRARY_PATH=$(EMBED_TARGET_DIR) demos/embedding/chost


# What a contributor runs before a push and what the merge queue runs: `qa`,
# then both suites on this build, the documents, the embedding demo and the
# surface gate. `qa` takes about two minutes and the passes about thirty, so a
# formatting or clippy failure stops the gate before the suites start.
#
# The passes run in a sub-make that starts only once `qa` has finished, whatever
# `-j` says; as prerequisites beside `qa`, `make -j` would start them together.
# A platform's Smoke job runs each pass as a step of its own, and no `qa`
# (docs/analysis/ci.md).
SMOKE_PASSES := smoke-lang smoke-impl doctest embedding semver-check

smoke: qa  ## qa, then both suites, the doctests, the embedding demo and the surface gate
	@$(MAKE) --no-print-directory $(SMOKE_PASSES)
	@echo "=== all smoke tests passed ==="

# CI documents private items too, and most of this crate is private — without
# the flag rustdoc never resolves a link into a `pub(crate)` item, so a broken
# one reaches CI unseen. Keep the flag here and in .github/workflows in step.
#
# `--doc` is separate from `cargo doc` because rendering an example is not
# compiling one: `cargo doc` will happily render a call whose signature moved
# under it. Nothing else builds doctests — `test` passes `--lib` and
# `--test '*'`, both of which exclude them.
#
# `--all-features` builds the MLIR tier, which finds LLVM 22 through
# `MLIR_SYS_220_PREFIX` in the environment (docs/impl/mlir.md).
qa: audit agents-check crosscheck  ## The PR gate's QA job, locally (~2min, no smoke): rustfmt, indexes, clippy, rustdoc, doctests
	cargo fmt --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --document-private-items
	cargo test --workspace --doc


test: smoke  ## smoke (qa first), then the Rust unit, integration and rig tests
	cargo test --workspace --lib --all-features
	cargo test --test '*' -- --skip property
	cargo test -p elle-rig

# Compile the arms a Linux gate never reaches. There are two of them, and the
# workflow checks both — so this target checks both, or a branch discovers the
# second one in CI.
#
# macOS is the io_uring blind spot: a binding the thread-pool backend never
# reads stays invisible until the Mac runner reports it, so this arm runs
# clippy. Android is the `not(target_os = "linux")` blind spot: Rust spells it
# `"android"`, so it takes every else-arm written for a desktop unix, and its
# libc answers for a different set of calls. It runs `cargo check`, which is
# what the workflow's Android job runs — this target predicts the gate rather
# than raising it.
#
# Neither step codegens or links, so neither needs an SDK or an NDK — only the
# target's std. `ffi` and `zstd` build C for the host and cannot cross, hence
# `--no-default-features`; that also drops the variant balancing `HeapObject`,
# so allow that one lint (the default-features gates above still enforce it).
# A missing target costs local feedback and nothing else.
CROSS_TARGET := x86_64-apple-darwin
ANDROID_TARGET := aarch64-linux-android

crosscheck:  ## Compile the macOS and Android cfg arms (no SDK or NDK needed)
	@if rustup target list --installed | grep -qx '$(CROSS_TARGET)'; then \
		cargo clippy --target $(CROSS_TARGET) --no-default-features -p elle \
			-- -D warnings -A clippy::large_enum_variant || exit 1; \
	else echo "SKIPPED macOS: rustup target add $(CROSS_TARGET)"; fi
	@if rustup target list --installed | grep -qx '$(ANDROID_TARGET)'; then \
		cargo check --target $(ANDROID_TARGET) --no-default-features -p elle || exit 1; \
	else echo "SKIPPED Android: rustup target add $(ANDROID_TARGET)"; fi

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

# One variable's value, as make expands it: `make print-JOBS`. Several of these
# are conditional on the environment or computed by `$(shell …)`, so reading
# the assignment is not reading the value. tests/integration/capacity.rs checks
# the job count through this rule.
print-%:
	@echo '$($*)'
