.PHONY: all elle plugins docs docgen examples test smoke clean help

all: elle plugins docs  ## Build everything

# ── Build ───────────────────────────────────────────────────────────

elle:  ## Build the Elle binary (release)
	cargo build --release -p elle

plugins:  ## Build all native plugins (.so)
	@for p in regex sqlite crypto random selkie; do \
		cargo build --release -p elle-$$p; \
	done

# ── Docs ────────────────────────────────────────────────────────────

docs: docs/pipeline.svg  ## Generate documentation assets

docs/pipeline.svg: docs/pipeline.dot
	dot -Tsvg $< -o $@

docgen: elle  ## Generate documentation site (Rust docs + Elle site)
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
	./target/release/elle demos/docgen/generate.lisp

examples: elle  ## Run all examples
	@for f in examples/*.lisp; do \
		echo "  $$f"; \
		timeout 10s ./target/release/elle "$$f" || exit 1; \
	done

# ── Test ────────────────────────────────────────────────────────────

test:  ## Run all workspace tests
	cargo test --workspace

smoke:  ## Fast local smoke test (build + examples + elle scripts + unit tests)
	cargo build --release -p elle
	@for f in examples/*.lisp; do \
		timeout 10s ./target/release/elle "$$f" || exit 1; \
	done
	@for f in tests/elle/*.lisp; do \
		./target/release/elle "$$f" || exit 1; \
	done
	cargo test --workspace --lib

# ── Clean ───────────────────────────────────────────────────────────

clean:  ## Remove build artifacts and generated docs
	cargo clean
	rm -f docs/pipeline.svg

# ── Help ────────────────────────────────────────────────────────────

help:  ## Show this help
	@grep -E '^[a-z].*:.*##' $(MAKEFILE_LIST) | \
		sed 's/:.*##/\t/' | \
		column -t -s '	'
