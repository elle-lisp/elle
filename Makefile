.PHONY: all elle dev plugins docs docgen examples smoke test plugin-tests test-git check-plugin-list clean help

ifdef GITHUB_ACTIONS
  JOBS    ?= 4
  ELLE    ?= ./target/release/elle
else
  JOBS    ?= 16
  ELLE    ?= ./target/debug/elle
  examples: dev
  plugin-tests: plugins
endif
TIMEOUT ?= 10s

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
#   make smoke    ~15s  examples + elle scripts (parallel, debug build)
#   make test     ~2min smoke + rust unit tests (PROPTEST_CASES=4)
#   cargo test    ~30min full suite (unit + integration + property)

examples:  ## Run all examples
	@printf '%s\n' examples/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}'

smoke: examples  ## Run examples, elle scripts, and docgen
	@printf '%s\n' tests/elle/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}'
	$(ELLE) demos/docgen/generate.lisp

plugin-tests:  ## Run plugin tests
	@printf '%s\n' tests/elle/plugins/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}'

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
