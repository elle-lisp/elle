.PHONY: all elle dev plugins docs docgen examples test smoke clean help

all: elle plugins docs  ## Build everything

# ── Build ───────────────────────────────────────────────────────────

elle:  ## Build the Elle binary (release)
	cargo build --release -p elle

dev:  ## Build the Elle binary (debug, fast compile)
	cargo build -p elle

plugins:  ## Build all native plugins (.so)
	@for p in glob regex sqlite crypto random selkie; do \
		cargo build --release -p elle-$$p; \
	done

# ── Docs ────────────────────────────────────────────────────────────

docs: docs/pipeline.svg  ## Generate documentation assets

docs/pipeline.svg: docs/pipeline.dot
	dot -Tsvg $< -o $@

docgen: elle  ## Generate documentation site (Rust docs + Elle site)
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
	./target/release/elle demos/docgen/generate.lisp

examples: dev  ## Run all examples
	@for f in examples/*.lisp; do \
		echo "  $$f"; \
		timeout 10s ./target/debug/elle "$$f" || exit 1; \
	done

# ── Test ────────────────────────────────────────────────────────────

# Approximate runtimes (for guidance — vary by machine):
#   make smoke    ~15s   examples + elle scripts (debug build)
#   make test     ~2min  smoke + rust unit tests (PROPTEST_CASES=8)
#   cargo test    ~30min full suite (unit + integration + property)

smoke: examples  ## Run examples and elle scripts using debug build (~15s)
	@for f in tests/elle/*.lisp; do \
		echo "  $$f"; \
		./target/debug/elle "$$f" || exit 1; \
	done
	./target/debug/elle demos/docgen/generate.lisp

test: smoke  ## Rust unit tests after smoke (PROPTEST_CASES=8, ~2min)
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
