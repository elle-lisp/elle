.PHONY: all elle dev plugins docs docgen smoke test plugin-tests test-git check-plugin-list clean help \
       smoke-vm smoke-jit smoke-wasm plugin-tests-vm plugin-tests-jit doctest

.DEFAULT_GOAL := all

ifdef GITHUB_ACTIONS
  JOBS    ?= 4
  ELLE    ?= ./target/release/elle
else
  JOBS    ?= 16
  ELLE    ?= ./target/debug/elle
  plugin-tests: plugins
endif
TIMEOUT ?= 30s

PLUGINS := \
    arrow \
    base64 \
    clap \
    compress \
    crypto \
    csv \
    egui \
    git \
    glob \
    hash \
    jiff \
    mqtt \
    msgpack \
    oxigraph \
    polars \
    protobuf \
    random \
    regex \
    selkie \
    semver \
    sqlite \
    syn \
    tls \
    toml \
    tree-sitter \
    uuid \
    watch \
    xml \
    yaml

all: elle plugins docs  ## Build everything

# ── Build ───────────────────────────────────────────────────────────

elle:  ## Build the Elle binary (release)
	cargo build --release -p elle

dev:  ## Build the Elle binary (debug, fast compile)
	cargo build -p elle

plugins:  ## Build all native plugins (.so)
	cargo build --release $(addprefix -p elle-,$(PLUGINS))

plugin-%:  ## Build a single plugin by bare name (e.g., make plugin-crypto)
	cargo build --release -p elle-$*

# ── Docs ────────────────────────────────────────────────────────────

docs: docs/pipeline.svg  ## Generate documentation assets

docs/pipeline.svg: docs/pipeline.dot
	dot -Tsvg $< -o $@

docgen: elle  ## Generate documentation site (Rust docs + Elle site)
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
	./target/release/elle demos/docgen/generate.lisp

# ── Test ────────────────────────────────────────────────────────────

# Approximate runtimes (for guidance — vary by machine):
#   make smoke    ~30s  examples + elle scripts, VM then JIT (parallel, debug build)
#   make test     ~3min smoke + rust unit tests (PROPTEST_CASES=4)
#   cargo test    ~30min full suite (unit + integration + property)
#
# Every Elle test target runs twice: first with JIT disabled (VM-only),
# then with default JIT. This catches bugs that only manifest in one mode.
# On failure the banner tells you which pass broke — capture it even if
# you only see the last few lines of output.

# Per-pass skip lists: tests that fail in one mode can still run in the other.
# jit-rejections    — requires JIT active (tests rejection tracking)
ELLE_SKIP_VM  := -e jit-rejections.lisp
ELLE_SKIP_JIT :=
# --jit=1 means threshold 0 (compile on first call)
ELLE_JIT_ARG := --jit=1

# WASM backend skip list: tests requiring features not yet in WASM backend
# (eval = dynamic compilation)
WASM_SKIP := -e eval.lisp

smoke-vm:
	@echo "=== elle scripts (VM, JIT disabled) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=0 {}' \
		|| { echo "FAILED: elle scripts VM-only pass (JIT was disabled)"; exit 1; }

smoke-jit:
	@echo "=== elle scripts (JIT enabled, $(ELLE_JIT_ARG)) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) $(ELLE_JIT_ARG) {}' \
		|| { echo "FAILED: elle scripts JIT pass (JIT was enabled, threshold=1)"; exit 1; }

smoke-wasm: elle
	@echo "=== elle scripts (WASM backend) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(WASM_SKIP) | \
		parallel -j 1 --halt now,fail=1 --tag \
			'timeout 300s ./target/release/elle --wasm=full {}' \
		|| { echo "FAILED: elle scripts WASM pass"; exit 1; }

doctest:  ## Test code examples in documentation (literate mode)
	@echo "=== doctest ==="
	@printf '%s\n' docs/*.md docs/impl/*.md docs/cookbook/*.md docs/signals/*.md docs/analysis/*.md | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: doctest"; exit 1; }

smoke: dev smoke-vm smoke-jit smoke-wasm doctest  ## Run examples + elle scripts (VM, JIT, WASM) + docgen + doctest
	cargo build --release -p elle -q
	./target/release/elle demos/docgen/generate.lisp

plugin-tests-vm:  ## Run plugin tests (VM, JIT disabled)
	@echo "=== plugin tests (VM, JIT disabled) ==="
	@printf '%s\n' tests/elle/plugins/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=0 {}' \
		|| { echo "FAILED: plugin tests VM-only pass (JIT was disabled)"; exit 1; }

plugin-tests-jit:  ## Run plugin tests (JIT enabled)
	@echo "=== plugin tests (JIT enabled) ==="
	@printf '%s\n' tests/elle/plugins/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: plugin tests JIT pass (JIT was enabled)"; exit 1; }

plugin-tests: plugin-tests-vm plugin-tests-jit  ## Run plugin tests (VM then JIT)

test-git:  ## Run git plugin integration tests (requires git, no network)
	cargo build -p elle-git
	$(ELLE) tests/git.lisp

check-plugin-list:  ## Assert every workspace plugin is in PLUGINS
	@ws=$$(sed -n 's/.*"plugins\/\([^"]*\)".*/\1/p' Cargo.toml | sort); \
	mk=$$(echo '$(PLUGINS)' | tr ' ' '\n' | sort); \
	if [ "$$ws" = "$$mk" ]; then \
		echo "✓ PLUGINS list matches Cargo.toml workspace members"; \
	else \
		echo "ERROR: Makefile PLUGINS and Cargo.toml workspace members differ"; \
		echo "  Cargo.toml:"; echo "$$ws" | sed 's/^/    /'; \
		echo "  Makefile:";   echo "$$mk" | sed 's/^/    /'; \
		exit 1; \
	fi

test: smoke  ## Rust unit tests + clippy + fmt + rustdoc after smoke
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
	PROPTEST_CASES=4 cargo test --workspace --lib

# ── Clean ───────────────────────────────────────────────────────────

clean:  ## Remove build artifacts and generated docs
	cargo clean
	rm -f docs/pipeline.svg

# ── Help ────────────────────────────────────────────────────────────

help:  ## Show this help
	@grep -E '^[a-z].*:.*##' $(MAKEFILE_LIST) | \
		sed 's/:.*##/\t/' | \
		column -t -s '	'
