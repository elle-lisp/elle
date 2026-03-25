.PHONY: all elle dev plugins docs docgen examples smoke test plugin-tests test-git check-plugin-list clean help \
       examples-vm examples-jit smoke-vm smoke-jit plugin-tests-vm plugin-tests-jit

ifdef GITHUB_ACTIONS
  JOBS    ?= 4
  ELLE    ?= ./target/release/elle
  TIMEOUT ?= 60s
else
  JOBS    ?= 16
  ELLE    ?= ./target/debug/elle
  TIMEOUT ?= 30s
  examples: dev
  plugin-tests: plugins
endif

PLUGINS := \
    arrow \
    base64 \
    clap \
    compress \
    crypto \
    csv \
    git \
    glob \
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

examples-vm:
	@echo "=== examples (VM, JIT disabled) ==="
	@export ELLE_JIT=0 &&printf '%s\n' examples/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: examples VM-only pass (JIT was disabled)"; exit 1; }

examples-jit:
	@echo "=== examples (JIT enabled) ==="
	@printf '%s\n' examples/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: examples JIT pass (JIT was enabled)"; exit 1; }

examples: examples-vm examples-jit  ## Run all examples (VM then JIT)

smoke-vm: examples-vm
	@echo "=== elle scripts (VM, JIT disabled) ==="
	@export ELLE_JIT=0 &&printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_VM) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: elle scripts VM-only pass (JIT was disabled)"; exit 1; }

smoke-jit: examples-jit
	@echo "=== elle scripts (JIT enabled) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: elle scripts JIT pass (JIT was enabled)"; exit 1; }

smoke: smoke-vm smoke-jit  ## Run examples + elle scripts (VM then JIT) + docgen
	$(ELLE) demos/docgen/generate.lisp

plugin-tests-vm:  ## Run plugin tests (VM, JIT disabled)
	@echo "=== plugin tests (VM, JIT disabled) ==="
	@export ELLE_JIT=0 &&printf '%s\n' tests/elle/plugins/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
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

test: smoke  ## Rust unit tests + clippy + fmt after smoke
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
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
