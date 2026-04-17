.PHONY: all elle dev docs docgen smoke test test-git clean help \
       smoke-vm smoke-jit smoke-wasm smoke-diff doctest

.DEFAULT_GOAL := all

ifdef GITHUB_ACTIONS
  JOBS          ?= 4
  ELLE          ?= ./target/release/elle
  CARGO_PROFILE := --release
else
  JOBS          ?= 16
  ELLE          ?= ./target/debug/elle
  CARGO_PROFILE :=
endif
TIMEOUT ?= 30s

all: elle docs  ## Build everything

# ── Build ───────────────────────────────────────────────────────────

elle:  ## Build the Elle binary (release)
	cargo build --release -p elle --features wasm

dev:  ## Build the Elle binary (debug, fast compile)
	cargo build -p elle --features wasm

MCP_PATCH := --config 'patch."https://github.com/elle-lisp/elle".elle-plugin.path="elle-plugin"'

mcp: elle  ## Build elle + MCP plugins (oxigraph, syn)
	cargo build --release --manifest-path plugins/Cargo.toml --target-dir target \
		-p elle-oxigraph -p elle-syn $(MCP_PATCH)

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
ELLE_SKIP_JIT := -e NOMATCH_PLACEHOLDER

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
	@echo "=== elle scripts (JIT enabled, threshold=1) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(ELLE_SKIP_JIT) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) --jit=1 {}' \
		|| { echo "FAILED: elle scripts JIT pass (JIT was enabled, threshold=1)"; exit 1; }

smoke-wasm: elle
	@echo "=== elle scripts (WASM backend) ==="
	@printf '%s\n' tests/elle/*.lisp | \
		grep -v $(WASM_SKIP) | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout 300s ./target/release/elle --wasm=full {}' \
		|| { echo "FAILED: elle scripts WASM pass"; exit 1; }

doctest:  ## Test code examples in documentation (literate mode)
	@echo "=== doctest ==="
	@printf '%s\n' docs/*.md docs/impl/*.md docs/cookbook/*.md docs/signals/*.md docs/analysis/*.md | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: doctest"; exit 1; }

smoke-diff:  ## Cross-tier differential agreement tests (compile/run-on)
	@echo "=== differential tier-agreement tests ==="
	@printf '%s\n' tests/diff/*.lisp | \
		parallel -j $(JOBS) --halt now,fail=1 --tag \
			'timeout $(TIMEOUT) $(ELLE) {}' \
		|| { echo "FAILED: differential tests"; exit 1; }

smoke: dev smoke-vm smoke-jit smoke-wasm smoke-diff doctest  ## Run examples + elle scripts (VM, JIT, WASM, differential) + docgen + doctest
	cargo build --release -p elle --features wasm -q
	./target/release/elle demos/docgen/generate.lisp

test-git:  ## Run git plugin integration tests (requires git, no network)
	cargo build $(CARGO_PROFILE) -p elle
	$(ELLE) tests/git.lisp

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
