<!-- devlog-instructions

# Devlog generation

This file is both the output and the instructions for generating it.
An agent reads these instructions, then appends/updates entries below.

## How to invoke

    update DEVLOG.md

## Commit enumeration

    git log --format='%h %ai %s' --reverse

Each line is one squash-merged PR. The PR number is in the subject
line (e.g. `(#740)`). The hash links to GitHub.

## Per-commit process

For each commit HASH:

1. Read the commit message: `git log -1 --format='%B' HASH`
2. Read the diffstat: `git show HASH --stat`
3. Read the full diff: `git show HASH`
   - For very large diffs (>5000 lines), focus on new files, renamed
     files, and files with the most changes. Skip generated/lock files.
4. Write a devlog entry using the format below.

The commit message is a starting point, not the answer. The diff is
ground truth. When they disagree, trust the diff.

## Entry format

    ---

    ## [#PR](https://github.com/elle-lisp/elle/pull/PR) — narrative headline
    [`HASH`](https://github.com/elle-lisp/elle/commit/HASH) · DATE · `tag1` `tag2`

    Body text. Focus on:
    - What changed and WHY (architecture decisions, not file lists)
    - What was broken/missing before and what's different now
    - Key design choices visible in the diff

    For large PRs, use **bold section headers** to break up phases
    or subsystems. Keep each section to 2-4 sentences.

    For trivial PRs (submodule bumps, one-liners), a single sentence.

## Tags

Use backtick-wrapped lowercase tags from this vocabulary:

    vm        bytecode interpreter, dispatch, call
    jit       Cranelift JIT compiler
    wasm      Wasmtime WASM backend
    mlir      MLIR/LLVM backend
    gpu       GPU compute, Vulkan, SPIR-V
    compiler  HIR, LIR, bytecode emission
    memory    allocator, arenas, GC, rotation
    signals   signal system, inference, enforcement
    runtime   fibers, scheduler, async, concurrency
    stdlib    stdlib.lisp, prelude.lisp
    reader    reader, syntax, surface languages
    macros    macro system, hygiene
    ffi       foreign function interface
    plugins   plugin system, individual plugins
    mcp       MCP server, tools, RDF
    cli       command-line interface, config
    build     Makefile, Cargo, build system
    ci        GitHub Actions, workflows
    docs      documentation, AGENTS.md
    errors    error types, diagnostics
    deps      dependency upgrades
    tools     tools/, codegen, utilities
    demos     demos/, examples
    http      HTTP client/server library
    gui       GTK4, SDL3, egui
    performance  optimization, benchmarks

## Ordering

Most-recent-first (reverse chronological).

## Incremental updates

1. Find the most recent entry's commit hash
2. `git log --format='%h %s' HASH..HEAD` to find new commits
3. Generate entries for only the new commits
4. Insert after this comment block, before existing entries

end-devlog-instructions -->

# Elle Development Log

Per-PR entries capturing significant development work, generated from
git history by reading actual diffs. Most recent first.

## [#737](https://github.com/elle-lisp/elle/pull/737) — MLIR/SPIR-V float support, differential testing harness, epoch 8 immutable-by-default bindings
[`19fd778a`](https://github.com/elle-lisp/elle/commit/19fd778a) · 2026-04-18 · `compiler` `lir` `jit` `epoch`

Massive multi-front commit spanning execution backends and language semantics. Adds a `compile/run-on` primitive that force-dispatches closures on any of four tiers (:bytecode, :jit, :wasm, :mlir-cpu), enabling a differential testing harness (`tests/diff/`) that runs the same closure on all eligible tiers and asserts agreement. MLIR-CPU and SPIR-V backends gain full float support (typed arithmetic, mixed int/float promotion, float comparisons, bool returns, captures as extra parameters, within-block type reassignment). The JIT gains a tail-call trampoline, and squelch enforcement is added to the compile/run-on dispatch path. Introduces `integer`/`float` as numeric-only coercion (string parsing split to `parse-int`/`parse-float`), a `LirInstr::Convert` for type conversions that makes them GPU-eligible, and immutable constant propagation in the lowerer via `ValueConst`. Epoch 8 introduces immutable-by-default bindings with `@` prefix for opt-in mutability, gated on source epoch so older files remain compatible. 363 files changed.

---

## [#746](https://github.com/elle-lisp/elle/pull/746) — Add DEVLOG.md and CHANGELOG.md from git history
[`f13573ea`](https://github.com/elle-lisp/elle/commit/f13573ea) · 2026-04-18 · `docs`

Generates two project history documents from the full commit log. DEVLOG.md contains per-PR entries for all 368 squash-merged PRs with narrative summaries written from the actual diffs. CHANGELOG.md provides an agent-optimized summary grouped by narrative arc (memory model, execution backends, signal system, compiler pipeline, etc.). Both files are self-documenting with embedded generation instructions and are linked from README.md and QUICKSTART.md.

---

## [#745](https://github.com/elle-lisp/elle/pull/745) — Fix TLS read silently dropping final plaintext segment on TCP close
[`fcc70ef2`](https://github.com/elle-lisp/elle/commit/fcc70ef2) · 2026-04-18 · `tls` `bugfix`

When a TLS peer sends `close_notify` in the same record as application data, `port/read` returned nil before draining the plaintext buffer, silently dropping the last segment. The fix adds one final `read-plaintext-fn` call on EOF to flush any remaining buffered plaintext before signaling end-of-stream. A one-file, six-line change in `lib/tls.lisp`.

---

## [#742](https://github.com/elle-lisp/elle/pull/742) — Epoch 7: flat let bindings (Clojure-style alternating name/value)
[`4b8a2f1d`](https://github.com/elle-lisp/elle/commit/4b8a2f1d) · 2026-04-17 · `language` `epoch` `compiler`

Switches `let`, `let*`, `letrec`, `if-let`, `when-let`, and `when-ok` from nested-pair bindings `[[a 1] [b 2]]` to flat alternating name/value pairs `[a 1 b 2]`, matching Clojure style. Destructuring patterns (which start with `[`) are still recognized unambiguously. Epoch 6 files are migrated transparently at compile time via a new `FlattenBindings` epoch transform. The change touches 314 files: stride-2 iteration in the HIR binding analyzer, flat `Array` generation in syntax-case expansion, all 18 prelude macros rewritten, and mechanical migration of ~280 `.lisp` files across stdlib, lib, tests, demos, and tools. Backward compatibility is verified by `tests/elle/epoch6.lisp`.

---

## [#744](https://github.com/elle-lisp/elle/pull/744) — Plugins submodule bump (second pass)
[`107ea501`](https://github.com/elle-lisp/elle/commit/107ea501) · 2026-04-17 · `submodule`

Submodule pointer update only -- one file changed. Follows #743 with the same theme: wrapping unsafe `Api::arg()` calls in the plugins repo, tracking the `main` branch, and Makefile alignment.

---

## [#743](https://github.com/elle-lisp/elle/pull/743) — Plugins submodule bump: unsafe arg(), branch main, Makefile
[`3eade74c`](https://github.com/elle-lisp/elle/commit/3eade74c) · 2026-04-17 · `submodule`

First of two submodule pointer bumps. Picks up safety wrappers around raw `Api::arg()` calls in the plugins repo and switches the submodule to track the `main` branch.

---

## [#741](https://github.com/elle-lisp/elle/pull/741) — Fix plugin dispatch in JIT/WASM backends; add make mcp target
[`c11d86bc`](https://github.com/elle-lisp/elle/commit/c11d86bc) · 2026-04-17 · `bugfix` `jit` `wasm` `plugins`

JIT and WASM backends were calling plugin primitives directly through the sentinel function pointer instead of dispatching through `call_plugin()`, causing a panic whenever a plugin primitive was invoked from JIT-compiled code. The fix adds `PLUGIN_SENTINEL` checks to `elle_jit_call`, `elle_jit_tail_call`, and the WASM `call_primitive` host function, matching the existing check in the VM interpreter. A new `make mcp` target builds elle plus the oxigraph and syn plugins in a single cargo invocation, and `.gitmodules` now tracks the `main` branch for both the plugins and mcp submodules.

---

## [#740](https://github.com/elle-lisp/elle/pull/740) — Stable plugin ABI; plugins and MCP server split into separate repos
[`231664be`](https://github.com/elle-lisp/elle/commit/231664be) · 2026-04-17 · `abi` `plugins` `architecture`

This is the largest structural change in the batch: all 24 plugins move to a separate repo (`elle-lisp/plugins`) as a git submodule, and the MCP server moves to `elle-lisp/mcp`.

**Plugin ABI.** A new `elle-plugin` crate defines a zero-dependency stable ABI surface: `ElleValue`, `ElleApiLoader`, `Api`, `EllePrimDef`, and a `define_plugin!` macro. A companion `plugin_api.rs` in the main crate provides the `extern "C"` implementations (constructors, accessors, struct/array iteration, keywords, async `poll_fd`). The old plugin loader is replaced wholesale -- no backward compatibility shim. All 24 plugins are migrated to the new crate dependency.

**Repo topology.** Plugin CI lives in the plugins repo; the elle repo only builds and tests elle itself. The `.claude/` directory is removed from tracking and gitignored. Tools like `mcp-server.lisp`, `test-mcp.lisp`, and various demo/graph tools that depended on plugins move to the mcp submodule. The dead keyword override mechanism (`set_keyword_fns`) is removed. Net: -36k lines deleted, +2.8k added (mostly `elle-plugin/src/lib.rs` and `plugin_api.rs`).

---

## [#739](https://github.com/elle-lisp/elle/pull/739) — Add eval MCP tool: monadic bind over persistent Elle image
[`e5d60e90`](https://github.com/elle-lisp/elle/commit/e5d60e90) · 2026-04-17 · `mcp` `eval` `architecture`

Adds the `eval` tool to the MCP server, implementing a monadic-bind interface over a persistent Elle image. An agent submits an Elle lambda plus input handles (UUIDs from prior evals); the server applies the lambda to the resolved values and returns a new handle naming the result. Large values stay in the image -- agents probe them by submitting further lambdas. stdout/stderr are captured via `parameterize` to temp files. Errors produce handles too (`ok:false`, `kind:error`) so agents can inspect failures through further eval calls.

The second commit in this squash hardens the MCP server against 10 of 12 planned operational risks: bounded input lines (reject >10MB), `port/flush` after every response, watcher restarts with backoff, SPARQL query timeouts, atomic store operations, and `populate-primitives` running synchronously for faster startup. It also restructures `lib/rdf.lisp` into `lib/rdf/elle.lisp` and `lib/rdf/rust.lisp`, and fixes the import resolver's fast path (`.exists()` changed to `.is_file()` so directories adjacent to `.lisp` files no longer shadow the file). 30 integration tests in `test-eval.lisp`.

---

## [#738](https://github.com/elle-lisp/elle/pull/738) — aws-codegen: mutable vector instead of map+append for param lists
[`b061c535`](https://github.com/elle-lisp/elle/commit/b061c535) · 2026-04-17 · `codegen` `style`

Replaces the `(map camel->kebab positional)` + `(append ... (list "&keys" "opts"))` pattern in three code-generation functions with a mutable vector that accumulates parameters via `push`. Same output, avoids intermediate list allocation and the `append` call.

---

## [#736](https://github.com/elle-lisp/elle/pull/736) — Upgrade cranelift 0.116 to 0.130
[`4856207a`](https://github.com/elle-lisp/elle/commit/4856207a) · 2026-04-16 · `jit` `dependencies`

Wasmtime 43 already pulled in cranelift 0.130.1 transitively, so the workspace carried two copies. Bumping the direct deps deduplicates to one. Two API adjustments in the JIT: `declare_var(Variable, Type)` becomes `declare_var(Type) -> Variable` (sequential auto-allocation matches the existing index scheme), and `jump`/`brif` block args switch from `&[Value]` to `&[BlockArg]` with values wrapped via `BlockArg::Value`.

---

## [#735](https://github.com/elle-lisp/elle/pull/735) — http: chunked transfer, HTTPS/TLS, query params, redirects, compression, SSE
[`1bb0de83`](https://github.com/elle-lisp/elle/commit/1bb0de83) · 2026-04-16 · `stdlib` `http` `networking`

Major expansion of `lib/http.lisp` (+1.9k lines). The module is now parameterized: `((import "std/http") :tls plug :compress plug)`. A transport abstraction routes reads/writes through closure structs, sharing wire-format helpers across TCP, TLS, and file ports.

New capabilities: chunked transfer-encoding (read and write), HTTPS via the TLS module, query parameters (struct or pre-encoded string, merged with URL query), redirect following (301/302/303 rewrite to GET, 307/308 preserve method and body), and optional gzip/zlib/deflate/zstd compression. Server-Sent Events get both `sse-get` (coroutine-based with spec-compliant auto-reconnect, Last-Event-ID, and retry) and `sse-post` (for non-idempotent requests like LLM chat streams, no auto-reconnect). The prelude's `each` macro now iterates coroutines via `coro/resume` until nil yield, with a test. 452 lines of new HTTP tests.

---

## [#734](https://github.com/elle-lisp/elle/pull/734) — Runtime configuration, tracing, MCP test orchestration
[`fa14f2d7`](https://github.com/elle-lisp/elle/commit/fa14f2d7) · 2026-04-15 · `runtime` `config` `tracing` `mcp`

Introduces `RuntimeConfig` on the VM, replacing static debug flags with a mutable, per-VM configuration accessible from Elle via `(vm/config)` and `(vm/config-set :key value)`. A new `--trace=call,signal,fiber,jit,...` CLI flag gates trace output through an `etrace!` macro. JIT and WASM policies get named modes (`--jit=off/eager/adaptive`, `--wasm=off/full/lazy`). The `--eval` flag is fixed (expressions were being opened as files). Duplicate error output on stderr is eliminated.

Six new MCP tools for test orchestration: `test_run` (run tests and record results as RDF triples keyed by sha+mode), `test_status` (structured summary with pass/fail counts), `test_history` (results across recent commits via SPARQL), `test_gate` (check if HEAD has passing tests on a clean worktree), `push_ready` (git push gated on test_gate), and `push_wip` (unconditional push). Server version bumps to 0.6.0 (20 tools).

---

## [#732](https://github.com/elle-lisp/elle/pull/732) — Python surface syntax reader
[`137e07c4`](https://github.com/elle-lisp/elle/commit/137e07c4) · 2026-04-14 · `reader` `python` `syntax`

An indentation-aware lexer (INDENT/DEDENT tokens, bracket-depth tracking) and Pratt-precedence parser that translate Python source into the same Syntax trees as s-expressions. Files with `.py` extension dispatch through the Python reader automatically. Supports `def`, `lambda`, `if/elif/else`, `while`, `for-in`, `try/except`, `assert`, `raise`, f-strings with interpolation, list/dict literals (mutable), destructuring, `*args` rest/spread, type annotations (skipped), implicit string concatenation, triple-quoted strings, r-strings, and all Python operators mapped to Elle primitives. Python-style function scoping uses `assign` inside loops/ifs so mutations reach the enclosing function scope. 35 unit tests plus `demos/syntax.py`. Also adds `--dump-ast` flag and restores `demos/syntax.lua`.

---

## [#731](https://github.com/elle-lisp/elle/pull/731) — JavaScript surface syntax reader
[`8972ffe6`](https://github.com/elle-lisp/elle/commit/8972ffe6) · 2026-04-14 · `reader` `javascript` `syntax`

Lexer and Pratt-precedence parser for JavaScript, producing the same Syntax trees as s-expressions. `.js` files dispatch automatically. Supports `const`/`let`, arrow functions, function declarations, `if`/`else`, `while`, `for-of`, C-style `for`, `do-while`, `try`/`catch`, ternary, template literals with interpolation, destructuring, rest/spread, compound assignment, increment/decrement, object and array literals (mutable), shorthand properties, and all JS operators mapped to Elle primitives. 42 unit tests plus `demos/syntax.js`. Together with #732, Elle can now accept programs written in JavaScript, Python, Lua, or s-expressions.

---

## [#730](https://github.com/elle-lisp/elle/pull/730) — Resource measurement library, debug/ namespace, memory model phases 0-2
[`e78e5fd2`](https://github.com/elle-lisp/elle/commit/e78e5fd2) · 2026-04-16 · `memory` `runtime` `stdlib`

A sprawling PR (113 files, +4.9k/-1k lines) that spans several connected themes:

**Resource measurement.** New primitives `debug/intern-count`, `debug/symbol-count`, `debug/keyword-count` for deterministic resource accounting. Arena primitives renamed to `debug/arena-*` (old `arena/*` kept as aliases). Fixed `arena/peak` and `arena/bytes` returning 0 under `ev/run` -- when the shared allocator is active, allocations route to the parent heap but the counters only tracked the local pool. `lib/resource.lisp` provides `snapshot`, `measure`, `report`, `suite` with peak calibration that subtracts measurement overhead. 14 test scenarios including a `tco-alloc-10000` canary exposing that swap pool rotation doesn't apply to the shared allocator path.

**SharedAllocator rotation.** Adds `rotation_mark()`, `rotate()`, and `SharedSwapPool` mirroring FiberHeap's protocol. Per-parameter independence analysis proves when no cross-generation reference chains exist, enabling rotation for self-tail-calls. The `tco-alloc-10000` canary drops from 20,002 allocs to 2.

**Memory model redesign (phases 0-2).** Phase 0 replaces `BTreeMap` backing of immutable structs with a sorted `Vec<(TableKey, Value)>` for cache-friendly binary-search lookups. Phase 1 adds a bump arena allocator. Phase 2 introduces inline slices. Also adds `--dump` and `--flip` CLI integration tests, an outbox test suite, and sorted-struct tests.

---

## [#729](https://github.com/elle-lisp/elle/pull/729) — Rewrite microgpt demo: idiomatic modules, Gaussian init, fused ops
[`6162c868`](https://github.com/elle-lisp/elle/commit/6162c868) · 2026-04-14 · `demo` `performance`

Full rewrite of the microgpt demo to idiomatic Elle. Closure-as-module pattern for all files, plugin access via struct, Gaussian weight init via `rng:normal`, functions decomposed to stay under the JIT function size limit. Imperative accumulation replaced with `map` where possible. New fused `vdot` (dot product) and `vsum` operations in autograd reduce node count per matrix-vector multiply from ~512 to 16. Training speed: 26.9s to 9.9s for 100 steps (190ms/step to 69ms/step, 2.7x faster).

---

## [#728](https://github.com/elle-lisp/elle/pull/728) — Image, SVG, and color libraries; macro hygiene fix
[`a925f833`](https://github.com/elle-lisp/elle/commit/a925f833) · 2026-04-13 · `plugins` `stdlib` `macros`

Three new libraries: a Rust `image` plugin (35 primitives for I/O, transforms, drawing, compositing, analysis via the `image` + `imageproc` crates), a Rust `svg` plugin (4 primitives for SVG rasterization via resvg), and two Elle libraries -- `lib/svg.lisp` for declarative SVG construction and XML emission as struct trees, and `lib/color.lisp` for color science (sRGB/HSL/Lab/Oklch conversions, mixing, gradients, CIEDE2000 distance).

Also fixes macro hygiene via a sets-of-scopes approach (Flatt 2016): quasiquote wraps template symbols as `SyntaxLiteral` to preserve definition-site scopes, the prelude gets `ScopeId(0)` while user files get fresh file scopes, and top-level `def` uses syntax node scopes instead of an empty slice. Adds `math/fmod` primitive, struct iteration in `each`, and counter-factual hygiene tests.

---

## [#727](https://github.com/elle-lisp/elle/pull/727) — Vulkan compute, SPIR-V emitter, MLIR backend, GPU eligibility analysis
[`ef2e6a0a`](https://github.com/elle-lisp/elle/commit/ef2e6a0a) · 2026-04-16 · `gpu` `spirv` `mlir` `signals`

The largest single PR in this batch (101 files, +7.9k/-0.8k lines), building out the GPU compute pipeline from scratch and wiring in an MLIR backend.

**Vulkan compute** (`plugins/vulkan/`). Three-phase async dispatch: submit (non-blocking), wait (fiber suspends on fence fd via `VK_KHR_external_fence_fd` + `IoOp::PollFd`), collect (readback). Zero thread pool threads consumed. Buffer pooling, command pool recycling, persistent GPU buffers, and integer data type support follow in later commits within the same PR.

**SPIR-V emitter** (`lib/spirv.lisp`). Runtime SPIR-V bytecode generation from Elle code -- no GLSL, no glslc, no shaderc. Supports f32 storage buffers, arithmetic, comparisons, select, loops, local variables, structured control flow, integer bitwise ops, and bitcast. Output validated by spirv-val.

**MLIR backend** (`src/mlir/`). Adds melior 0.27 (MLIR C API bindings) behind `--features mlir`. LIR lowers to MLIR arith/func/cf dialects, converts to LLVM dialect, JIT-compiles via ExecutionEngine, and executes natively. Benchmark: MLIR 7.7ms vs Cranelift 2.4ms compile time, with context creation (4ms) dominating the MLIR cold path.

**Signal analysis fixes.** `compute_inferred_signal` was discarding `SIG_ERROR`, `SIG_FFI`, etc. when a function body didn't suspend. Non-suspension bits are now preserved in all paths. `SignalBits` widens from u32 to u64 throughout bytecode encoding, JIT, and WASM ABI.

**GPU eligibility.** `is_gpu_candidate` (ClosureTemplate) and `is_gpu_eligible` (LirFunction) check GPU compilation candidacy. `fn/gpu-eligible?` exposed as a primitive.

---

## [#726](https://github.com/elle-lisp/elle/pull/726) — Structured errors throughout the compilation pipeline
[`de9ab21e`](https://github.com/elle-lisp/elle/commit/de9ab21e) · 2026-04-13 · `errors` `compiler` `diagnostics`

Migrates the compilation pipeline from string-encoded errors to structured `LError` with typed `ErrorKind` variants. The analyzer now accumulates recoverable errors (undefined variables, unterminated forms) instead of stopping at the first one, and surfaces them through `compile/analyze` diagnostics and `elle lint`. Undefined variables get "did you mean?" suggestions via Levenshtein distance. Compile errors show source snippets with `^` carets. The `--json` flag produces structured JSON to stderr. `HirKind::Error` serves as a poison node for error accumulation. Signal mismatches remain fatal (not accumulated) since they represent explicit constraint violations.

---

## [#725](https://github.com/elle-lisp/elle/pull/725) — MCP server: async startup via yield-based population fiber
[`185a1e60`](https://github.com/elle-lisp/elle/commit/185a1e60) · 2026-04-12 · `mcp` `performance`

The MCP server was blocking for >60s before responding to `initialize` because `populate-primitives` and `populate-rust` ran synchronously. Population now runs in a `fiber/new` coroutine with `|:yield|` mask, yielding between files so the main loop can interleave requests. Targeted globs (`src/`, `plugins/`, `tests/`, `benches/`, `patches/`) skip the deep `target/` tree. A test verifies `initialize` responds within 10 seconds.

---

## [#724](https://github.com/elle-lisp/elle/pull/724) — IRC module; structured errors and style refresh across all libs
[`dd18042b`](https://github.com/elle-lisp/elle/commit/dd18042b) · 2026-04-12 · `stdlib` `irc` `errors` `style`

New `lib/irc.lisp`: IRCv3 client with coroutine-based read stream, CAP negotiation, SASL PLAIN, message tags, and auto-PONG. Connection struct exposes `:messages` (coroutine), `:send` (function), `:close` (function).

Every error across all 11 Elle source files (9 libs + stdlib + prelude) is converted to a structured error carrying `:error` (module category), `:reason` (specific condition keyword), and promoted data fields. The `:message` string contains no information not already in a struct field. Style modernization throughout: section headers `# ====` become `## --`, hyphenated string predicates become slash-prefixed in http, `cond` dispatch becomes `case` for constant dispatch in dns, `cond` becomes `match` with extracted handlers in irc.

---

## [#723](https://github.com/elle-lisp/elle/pull/723) — Capability enforcement, emit special form, defmacro &opt
[`08a4da87`](https://github.com/elle-lisp/elle/commit/08a4da87) · 2026-04-12 · `capabilities` `compiler` `macros`

Three substantial features in one PR (78 files, +1.3k/-0.4k lines):

**Capability enforcement.** `fiber/new :deny` withholds capabilities; `fiber/caps` introspects them. `NativeFn` stores `&'static PrimitiveDef` for signal metadata at call sites. A denial check in `call_inner` blocks primitives whose declared signals overlap the withheld set; the denial payload includes the primitive name, args, and denied keyword set. Withheld capabilities propagate transitively at fiber resume.

**emit special form.** `(emit :keyword val)` extracts signal bits at compile time; dynamic `(emit var val)` falls through to a primitive. All internal IR nodes (`HirKind::Yield`, `Terminator::Yield`, `Instruction::Yield`) are replaced with `Emit` variants carrying signal bits. `yield` becomes a prelude macro: `(defmacro yield (&opt v) \`(emit :yield ,v))`. The VM's `handle_emit` distinguishes error (propagate) from suspension (frame).

**defmacro &opt.** `MacroDef` gains `optional_params`; the parameter parser recognizes `&opt`. Arity checking supports `Range(required, required+optional)`.

Also fixes a JIT spill slot bug: the old check (`may_suspend`) missed functions that emit `:error` via the Emit terminator; now checks `yield_points` and `call_sites` directly. JIT `elle_jit_yield` takes a `signal_bits` parameter instead of hardcoding `SIG_YIELD`.

---

## [#721](https://github.com/elle-lisp/elle/pull/721) — Fix MCP server startup + plugin ABI bug; add test-mcp integration test
[`60f79205`](https://github.com/elle-lisp/elle/commit/60f79205) · 2026-04-11 · `bugfix` `mcp` `plugins` `testing`

The MCP server crashed at startup because it imported the deleted `glob` plugin instead of `lib/glob.lisp`. While adding an integration test, a deeper bug surfaced: `cargo build -p elle` and `cargo build -p elle-oxigraph` as separate invocations produce different compilations of the elle crate (different transitive wasmtime feature sets produce different `HeapObject` layouts). Plugin `Value::native_fn(f)` values then deref as garbage in the main binary. Release builds happened to work by layout coincidence.

Fix: always build elle alongside its plugins in a single cargo invocation. The rule is documented in CONTRIBUTING.md with the exact symptom. New `test-mcp` target runs 13 assertions against a freshly nuked temporary store: initialize, tools/list, ping, SPARQL queries, load_rdf, reset, and unknown method. Also documents that `docs/*.md` files are literate Elle programs runnable by the reader.

---

## [#720](https://github.com/elle-lisp/elle/pull/720) — Sendable channels, JIT SuspendingCall fix, worker-thread JIT, threaded mandelbrot
[`523e796a`](https://github.com/elle-lisp/elle/commit/523e796a) · 2026-04-10 · `concurrency` `jit` `channels`

Channels (`chan/sender`, `chan/receiver`) can now cross `sys/spawn` boundaries via new `SendValue` variants that clone the crossbeam endpoint. The JIT's `SuspendingCall` in silent functions now skips yield checks -- these calls can't actually yield, and emitting yield-through-call code without `call_sites` metadata caused crashes.

The critical fix is LIR transfer across `sys/spawn` for worker-thread JIT. `SendableClosure` now carries `lir_function`, and a new `LirConst::ClosureRef(idx)` mechanism handles closure-valued `ValueConst` instructions (which arise whenever user code references a stdlib function). On the receiving side, `patch_lir_closure_refs` rewrites each `ClosureRef` back to `ValueConst` with the reconstructed closure. Without this, the serializer dropped `lir_function` on any closure whose LIR touched stdlib, forcing mandelbrot workers into the interpreter.

The mandelbrot demo uses a thread pool (NCPU workers, sendable channels) with cardioid/period-2 bulb early-exit. Workers now JIT-compile `compute-row` thanks to the ClosureRef fix.

---

## [#719](https://github.com/elle-lisp/elle/pull/719) — Disambiguate LBox (user boxes) from CaptureCell (compiler captures)
[`5e32ecaf`](https://github.com/elle-lisp/elle/commit/5e32ecaf) · 2026-04-09 · `compiler` `refactoring`

Splits `HeapObject::LBox { is_local: bool }` into two distinct variants: `HeapObject::LBox` for user-facing boxes (`box`/`unbox`/`rebox`) and `HeapObject::CaptureCell` for compiler-created cells for mutable captures (auto-unwrapped by `LoadUpvalue`). Every compiler-internal capture operation is renamed across bytecode, LIR, HIR, VM, JIT, and WASM (e.g. `MakeLBox` becomes `MakeCapture`, `lbox_params_mask` becomes `capture_params_mask`). 76 files touched, purely mechanical renaming with no behavioral changes. Primitives file renamed from `cell.rs` to `box.rs`.

---

## [#718](https://github.com/elle-lisp/elle/pull/718) — Docs: design philosophy, agent reasoning, and cross-linking
[`625907b1`](https://github.com/elle-lisp/elle/commit/625907b1) · 2026-04-09 · `docs`

Adds `docs/philosophy.md` (why polymorphic-by-default is the right choice, the semantic gap as a visibility issue), `docs/analysis/agent-reasoning.md` (workflow guide for AI agents using portrait + MCP + compile-safe refactoring), and `docs/analysis/README.md` tying together three layers of code understanding. Expands `docs/mcp.md` with full RDF schema, real SPARQL examples, and trace tool documentation. Reframes `docs/modules.md` tradeoffs as architectural constraints. Documents graph staleness and verification protocol. All documents cross-linked so the story flows from design rationale through implementation mechanics to tool exposure and agent usage.

---

## [#717](https://github.com/elle-lisp/elle/pull/717) — Remove stale CI jobs from main workflow
[`df9e7f5d`](https://github.com/elle-lisp/elle/commit/df9e7f5d) · 2026-04-07 · `ci`

The main workflow's `build-plugins`, `tests`, and `check-plugin-list` jobs referenced removed plugins (base64, clap, compress, git, glob, semver, sqlite, uuid), causing build failures on every merge to main. The merge queue already runs all validation, so the main workflow only needs post-merge tasks. Deletes 82 lines of YAML.

---

## [#716](https://github.com/elle-lisp/elle/pull/716) — Declarative GTK4 bindings via FFI
[`a875a509`](https://github.com/elle-lisp/elle/commit/a875a509) · 2026-04-07 · `ffi` `gui` `stdlib`

New `lib/gtk4.lisp` with sub-modules `bind`, `widgets`, `webview` -- pure Elle, no Rust plugin. Calls GTK4/GLib/GObject/WebKit6 directly through `ffi/native`, `ffi/defbind`, and `ffi/callback`. Widget tree syntax: `[:v-box {:spacing 8} [:button {:id :ok} "OK"]]`. Supports 30 widget types (display, input, layout) plus WebKit webview with JS-to-Elle IPC via user content manager. The mandelbrot demo is rewritten to use `std/gtk4/bind`. Also simplifies the `ffi/defbind` macro (removes redundant gensyms) and adds or-pattern documentation to `docs/match.md`.

---

## [#715](https://github.com/elle-lisp/elle/pull/715) — Move 9 plugins from Rust to native Elle
[`7167796f`](https://github.com/elle-lisp/elle/commit/7167796f) · 2026-04-07 · `plugins` `stdlib` `ffi` `architecture`

The largest plugin migration in the project's history. Nine Rust cdylib plugins -- base64, clap, compress, git, glob, semver, sqlite, uuid, and watch -- are removed and reimplemented as pure Elle modules. The Rust layers were entirely marshalling with no algorithms or safety beyond what the C libraries or Elle itself provide.

**FFI replacements:** `lib/sqlite.lisp` calls libsqlite3.so directly, `lib/compress.lisp` calls libz.so and libzstd.so, `lib/git.lisp` calls libgit2.so (30 functions covering repo lifecycle, commits, staging, branches, tags, remotes, config). **Pure Elle replacements:** `lib/base64.lisp` (bitwise encode/decode), `lib/semver.lisp` (string parsing), `lib/uuid.lisp` (v4 via /dev/urandom, v5 when hash plugin provided), `lib/glob.lisp` (pattern matcher + recursive file/ls), `lib/cli.lisp` (renamed from clap). Also fixes `ptr/from-int` to accept negative values (sentinel pointers like `SQLITE_TRANSIENT`), `bytes/@bytes` to be idempotent on bytes input, and normalizes all import paths from `"lib/X"` to `"std/X"`.

Net: -6,505 lines of Rust, +1,718 lines of Elle. Also adds `docs/coming-from.md` (orientation guide for 8 source languages), `docs/mcp.md`, and restores `drop`/`range` as Rust primitives after discovering the WASM backend hangs on recursive Elle implementations.

---

## [#714](https://github.com/elle-lisp/elle/pull/714) — SDL3 bindings, JIT polymorphic+SuspendingCall, env var cleanup
[`400941cf`](https://github.com/elle-lisp/elle/commit/400941cf) · 2026-04-07 · `ffi` `jit` `sdl`

A multi-chapter PR that builds SDL3 bindings, fixes several JIT bugs discovered in the process, and cleans up environment variable configuration.

**SDL3** (`lib/sdl3.lisp`). Pure FFI via `ffi/defbind`, no Rust plugin. Three tiers: P0 (init, window, renderer, events, timing), P1 (textures, images, TTF, geometry), P2 (audio, input, clipboard). Event marshalling from 128-byte `SDL_Event` union at verified offsets. Pre-allocated rect/event buffers for zero-allocation hot paths. Conway demo rewritten to use the new bindings (~115 FPS for an 80x60 grid).

**JIT fixes.** Polymorphic functions (calling params as functions) now JIT via `elle_jit_call` runtime dispatch. Spill metadata gains `num_params` -- the old metadata only tracked `num_locals`, causing the env/stack split to be wrong on resume. `jit_rotation_base` save/restore across nested JIT calls fixes a SIGSEGV in mutual recursion (the inner function's stale base mark caused the outer's live cons cells to be swept). Letrec signal seeding fixed: forward-referenced siblings defaulted to `Signal::yields()`, causing spurious SuspendingCall instructions.

**Overflow fixes.** LoadUpvalue/StoreUpvalue indices widen from u8 to u16 (module closures with >255 bindings like std/sdl at 368 hit wrong slots). Call/TailCall arg count and MakeClosure capture count also widen to u16. All `ELLE_*` env var references removed from source and docs; all runtime knobs are CLI flags parsed into Config.

---

## [#713](https://github.com/elle-lisp/elle/pull/713) — WASM performance: per-closure compilation, LirModule, disk caching
[`0d11d37d`](https://github.com/elle-lisp/elle/commit/0d11d37d) · 2026-04-05 · `wasm` `performance` `compiler`

A deep rework of the WASM backend for compilation speed, touching 37 files.

**LirModule.** Replaces nested `Box<LirFunction>` inside `MakeClosure` with `ClosureId` references into a flat module-level closure list. This structural change enables independent per-closure compilation.

**Per-closure compilation.** Each closure in `LirModule.closures` is compiled as a standalone WASM Module, cached on disk by WASM bytes hash. The full module contains only stubs (unreachable) for pre-compiled closures, reducing it from ~2MB to ~34KB. At runtime, `rt_call` dispatches to pre-compiled Modules. All instruction types are supported in standalone mode: `MakeClosure` via `rt_make_closure`, `TailCall` via `rt_prepare_tail_call`, and `Yield` via CPS transform.

**Caching.** Emit closure WASM functions before entry for stable constant pool indices, enabling ~75% cross-program cache hits. Atomic cache writes via tempfile + persist. Default cache directory: `ELLE_CACHE` > `TMPDIR/elle-cache` > `TMP/elle-cache`.

**Results.** `functional.lisp`: 57s to 2.5s (single-pass regalloc), then 2.5s to 0.35s (expression chunking). `smoke-wasm`: from ~8min to ~12s with release build and -j16. Also fixes an O(N^2) bytecode emission bug (clone+collect per iteration) that added ~200ms to startup, and removes `module_closures` from `ClosureTemplate` fixing an 8x memory regression (570MB to 82MB for stdlib init).

---

## [#712](https://github.com/elle-lisp/elle/pull/712) — CLI args, single-pass regalloc, --json, --eval
[`5be772b3`](https://github.com/elle-lisp/elle/commit/5be772b3) · 2026-04-02 · `cli` `wasm` `config`

Replaces all `ELLE_*` environment variables with CLI arguments parsed once at startup into a global `Config` struct. Adds `--jit=N`, `--wasm=N/full`, `--cache=PATH`, `--json` (structured JSON on stderr), `--stats`, `--eval/-e EXPR`, and various debug flags. `--help` no longer prints the primitive catalog (use REPL `(help)` instead).

The key performance change: switches Wasmtime to a single-pass register allocator. The backtracking allocator is superlinear on large functions (stdlib entry exceeds 2M WASM instructions). `functional.lisp` drops from 57s to 2.5s (23x). Fixes a regalloc slot reuse bug: registers whose last use is at instruction N cannot have their slot freed before that instruction's def allocation (the instruction reads operands before writing results), corrected to free only at strictly earlier indices.

---

## [#711](https://github.com/elle-lisp/elle/pull/711) — Process module: readiness protocol, restart limits, subprocess helper
[`ab385425`](https://github.com/elle-lisp/elle/commit/ab385425) · 2026-04-01 · `feature` `stdlib` `docs`

Adds four capabilities to the supervisor: a startup readiness protocol (child specs with `:ready true` block the supervisor until the child calls `supervisor-notify-ready`, preventing races where child B depends on child A), restart intensity limits (sliding-window `:max-restarts N` within `:max-ticks M`), structured logging via a `:logger` callback receiving lifecycle events, and a `make-subprocess-child` helper for managing OS subprocesses under a supervisor. `docs/processes.md` is completely rewritten as a literate Elle document with runnable examples covering all process modules. 376 lines of new supervisor tests cover readiness ordering, crash-before-ready deadlock prevention, intensity limits, one-for-all simultaneous crashes, and transient vs permanent restart behavior.

---

## [#710](https://github.com/elle-lisp/elle/pull/710) — Fix stdin reads starved when ev/spawn fiber is active
[`777afa1a`](https://github.com/elle-lisp/elle/commit/777afa1a) · 2026-04-01 · `bugfix` `io`

When a long-lived `ev/spawn` fiber (e.g. a file watcher) had pending io_uring ops alongside a stdin read, the scheduler's `wait()` blocked indefinitely in `wait_uring()`. Stdin completions arrive via a crossbeam channel (`StdinThread`), not io_uring, so they were never drained until `wait_uring` returned -- which never happened if the only uring op was a watcher waiting for file changes. The fix polls uring non-blocking when stdin ops are pending and then selects across the stdin receiver (and network pool receiver if applicable), mirroring the existing pattern for network pool ops. This bug prevented the MCP server from processing more than one request when its file watcher fiber was active.

---

## [#709](https://github.com/elle-lisp/elle/pull/709) — Cycle detection for mutable containers; SyncBackend removal
[`8aed49b4`](https://github.com/elle-lisp/elle/commit/8aed49b4) · 2026-04-01 · `bugfix` `refactor` `runtime`

Display, Debug, PartialEq, Hash, and Ord now detect cycles in mutable containers (`@[]`, `@{}`, `@||`, LBox) instead of recursing to stack overflow, using thread-local visited sets with RAII guards and Floyd's tortoise-and-hare for cons-cell cdr chains. Beyond the headline feature, this PR also removes the 1,980-line `SyncBackend` and `sync-scheduler` entirely -- all production code uses the async scheduler, and the sync backend was a parallel maintenance burden that duplicated the async path's I/O logic. The pipeline gains `with_compilation_cache(|vm, expander, meta| ...)` replacing raw `*mut VM` pointer returns, and a duplicate thread-local `SYMBOL_TABLE` in `primitives/list` is consolidated with `context::resolve_symbol_name()`.

---

## [#708](https://github.com/elle-lisp/elle/pull/708) — Code quality: math.rs dedup, plugin init macro, spawn extraction
[`863f923d`](https://github.com/elle-lisp/elle/commit/863f923d) · 2026-04-01 · `refactor`

Three targeted deduplication passes. `math.rs` goes from 948 to 449 lines via three helpers (`unary_float`, `unary_to_int`, `require_number`) replacing 20+ identical primitives. `vm/arithmetic.rs` shrinks from 350 to 261 lines via `int_binop!` and `generic_binop!` macros. `lsp/run.rs` drops from 538 to 447 lines with `extract_position()` and `extract_uri()` helpers. The `elle_plugin_init!` macro eliminates boilerplate across 29 plugins (-447 lines of copy-paste, +143 lines for the macro and helper). Process spawn logic is deduplicated between sync and async backends by extracting `SpawnRequest::build_command()` and `spawn_to_struct()`.

---

## [#707](https://github.com/elle-lisp/elle/pull/707) — REPL rework: form-by-form evaluation with def persistence
[`85f1b71b`](https://github.com/elle-lisp/elle/commit/85f1b71b) · 2026-04-01 · `feature` `repl` `compiler`

Replaces the REPL's compilation model to fix three defects: multi-line accumulation (broken by case mismatch in error detection), def persistence (each `def` now registers its value, signal, and arity in the compilation cache via `register_repl_binding`), and per-form results (input is parsed via `read_syntax_all` then each form compiled and executed independently). The infrastructure work is substantial: `SyntaxReader` now threads byte offsets from the lexer through to spans, giving every syntax node accurate source byte ranges so the REPL can slice per-form source text. `repl.rs` is rewritten as a proper REPL engine with shared eval logic between readline and fallback paths. 207 lines of integration tests cover multi-form input, def persistence, error recovery, and exit codes.

---

## [#706](https://github.com/elle-lisp/elle/pull/706) — Two-pool rotation for tail-call memory reclamation
[`2b20fc1a`](https://github.com/elle-lisp/elle/commit/2b20fc1a) · 2026-04-02 · `memory` `compiler` `escape-analysis`

The foundational memory reclamation PR (57 files, +2.3k/-0.4k lines), introducing compile-time escape analysis and runtime pool rotation for tail-call loops.

**Escape analysis.** Three relaxations for tail-call scopes: (A1) tail calls are safe if both callee and all arg expressions are safe, (A2) suspension check is bypassed for tail calls since they replace the frame, (A3) non-primitive callee check is skipped for tail calls. `pending_region_exits` counter ensures correct `RegionExit` emission before `TailCall`.

**Rotation-safety analysis.** Compile-time determination of whether a function's tail-call loop is safe for pool rotation. A function is rotation-unsafe when its body stores heap values into external data structures (push, put, fiber/resume, assign to outer bindings, or calls to non-primitive functions). Interprocedural fixpoint analysis precomputes rotation safety across all function definitions in the compilation unit (start optimistic, flip to unsafe, converge monotonically).

**Call-scoped reclamation.** A two-mark protocol (`RegionEnter` before arg evaluation, `RegionEnter` as barrier before Call, `RegionExitCall` that releases only the range between the two marks) frees arg temporaries while preserving the callee's allocations. `return_params` bitmask analysis tracks which parameter indices may flow to the return position, ensuring call-scoped reclamation fires only when heap-allocating args are not in return positions.

**Runtime.** Pool rotation in both VM trampolines (`execute_bytecode` and `execute_bytecode_saving_stack`). The trampoline checks `prev_rotation_safe`: only rotate when the function that just completed its iteration was proven safe. Epoch 7 swaps `(fiber/new closure mask)` to `(fiber/new mask closure)`. Config is properly wired into the VM (previously `config::init` was never called, so all `--flags` were ignored and env vars were the actual mechanism). Also adds `CONTRIBUTING.md` with the green-main invariant.

---

## [#705](https://github.com/elle-lisp/elle/pull/705) — Bytes literals and documentation pass
[`cba74f24`](https://github.com/elle-lisp/elle/commit/cba74f24) · 2026-04-01 · `feature` `reader` `docs`

Adds `b[1 2 3]` bytes literal syntax (immutable) and `@b[1 2 3]` (mutable), desugaring to `(bytes ...)` / `(@bytes ...)` calls. The lexer, syntax reader, HIR analyzer, epoch transform, and display all handle the new `SyntaxKind::Bytes`/`BytesMut` variants. A `ffi/struct` bug is fixed (was rejecting immutable arrays). Documentation across 35 files gets dead-link fixes, stale-claim removal, fence-type corrections (`janet` to `lisp`), and conversion of text blocks to runnable lisp where possible. AGENTS.md files are trimmed by 230 lines, replacing inline documentation with pointers.

---

## [#704](https://github.com/elle-lisp/elle/pull/704) — Encapsulate SignalBits for u32-to-u64 migration
[`cc54a6e0`](https://github.com/elle-lisp/elle/commit/cc54a6e0) · 2026-04-01 · `refactor` `signals`

Makes `SignalBits`' inner field private and adds named methods (`from_bit`, `union`, `intersection`, `subtract`, `intersects`, `has_bit`, `is_empty`, `trailing_zeros`, `from_i64`, `raw`) so the integer type is never leaked outside the impl block. After this change, switching from u32 to u64 requires editing only the struct definition and its inherent methods. Touches 45 files to replace all raw `.0` field access and `u32` squelch masks with `SignalBits` throughout closures, LIR, HIR analysis, VM execution, and JIT compilation.

---

## [#703](https://github.com/elle-lisp/elle/pull/703) — Memory model documentation and SlabPool extraction
[`f2f664bc`](https://github.com/elle-lisp/elle/commit/f2f664bc) · 2026-04-01 · `refactor` `docs` `memory`

Replaces stale memory model descriptions (references to `Rc<RefCell>`, bumpalo, `ActiveAlloc::Bump`) with documentation of the actual architecture: slab allocator with free-list reuse, three-tier allocation dispatch, and ownership topology driven by signal inference. On the code side, `SlabPool` is extracted as a shared core holding the slab, allocs, dtors, and alloc_count. `SharedAllocator` shrinks from ~140 to ~40 lines by wrapping `SlabPool` plus scope marks. `FiberHeap` composes `SlabPool` with routing, custom allocators, stats, and limits. Demo code is updated for current stdlib conventions.

---

## [#702](https://github.com/elle-lisp/elle/pull/702) — Compile-time source splicing with include/include-file
[`b674d095`](https://github.com/elle-lisp/elle/commit/b674d095) · 2026-03-31 · `feature` `compiler`

Adds `include-file` (resolves relative to current file) and `include` (uses search-path resolution) for compile-time source splicing. Parsed forms from the included file are spliced into the including file before macro expansion, enabling macro definitions to be shared across files. Circular includes are detected at compile time. Five files changed, 146 lines added.

---

## [#701](https://github.com/elle-lisp/elle/pull/701) — Docgen rework: markdown rendering and auto-generated API reference
[`5cee76c8`](https://github.com/elle-lisp/elle/commit/5cee76c8) · 2026-03-31 · `tooling` `docs`

The docgen demo gains a markdown-to-HTML parser (`lib/markdown.lisp`) and auto-generates an API reference page listing all primitives, prelude macros, stdlib functions, library modules, and plugins -- each with signal badges, source links to GitHub, and signal profile summaries. Section navigation is derived from the `docs/` directory structure. Content fixes across 12 documentation files address stale status claims, missing io_uring documentation, and incorrect module/import descriptions.

---

## [#700](https://github.com/elle-lisp/elle/pull/700) — egui plugin, literate mode, import prefixes, docs overhaul
[`ac5f8609`](https://github.com/elle-lisp/elle/commit/ac5f8609) · 2026-03-31 · `feature` `plugin` `docs` `reader`

Three major changes in one PR, plus a massive docs restructuring.

**egui plugin**: A fiber-friendly immediate-mode GUI via egui + winit + glow running single-threaded on the Elle thread (no Arc/Mutex). Display fd watching through a new `IoOp::PollFd` backed by `IORING_OP_POLL_ADD` (thread-pool fallback: `libc::poll`). The library provides 20 widget types with fiber-aware event waiting.

**Literate mode**: `.md` files are now directly executable -- `strip_markdown` in the reader extracts fenced lisp blocks while preserving line numbers for error reporting. `elle docs/language.md` just works. A `make doctest` target runs all docs.

**Import prefixes**: `std/X` resolves to `lib/X.lisp`, `plugin/X` resolves to the appropriate shared library with profile-aware fallback (debug prefers debug, release prefers release).

The docs directory is comprehensively reorganized: `language.md` is rewritten from scratch (the old version had fabricated function names and non-existent syntax), `signals.md` is split into 7 focused files, the cookbook is broken into per-topic files, and all examples are deleted from the repo (their content now lives in docs). Roughly 12,500 lines removed and 10,200 added across 137 files.

---

## [#699](https://github.com/elle-lisp/elle/pull/699) — Numeric tower: mixed int/float, overflow, IEEE 754, macOS CI, event-driven watch
[`067227d1`](https://github.com/elle-lisp/elle/commit/067227d1) · 2026-03-30 · `bugfix` `feature` `portability`

A sweeping fix to the numeric tower. VM comparison handlers (`Lt`, `Gt`, `Le`, `Ge`) used `as_float()` which fails on integers; switched to `as_number()` matching the JIT. Value ordering is unified so mixed int/float collections sort correctly instead of grouping all ints before floats. Arithmetic gains checked overflow (signals a catchable error instead of panicking), whole floats display as `3.0` not `3`, float division by zero returns `Inf` per IEEE 754, and `+inf`/`-inf`/`nan` constants are added. Hash is canonicalized so `(= 1 1.0)` implies equal hashes.

Separately, the polling-based watch plugin is replaced with built-in inotify-on-io_uring primitives (`watch`, `watch-add`, `watch-remove`, `watch-next`, `watch-close`) -- zero polling, zero threads. A kqueue backend provides macOS support. FFI and plugin gates are broadened from linux-only to unix, io_uring is gated to Linux only, and several thread-pool backend fixes restore macOS CI.

---

## [#698](https://github.com/elle-lisp/elle/pull/698) — JIT fixes: callable collections, sync primitives, import rework
[`8f02e5a3`](https://github.com/elle-lisp/elle/commit/8f02e5a3) · 2026-03-29 · `bugfix` `jit` `feature`

Fixes JIT callable-collection dispatch: the JIT call paths handled native functions, parameters, and closures but not callable collections (structs, arrays, sets, strings, bytes), so `(cell 0)` syntax failed after JIT tiering. The fix adds `call_collection` dispatch to both JIT call and tail-call paths. Also introduces `lib/sync.lisp`, a full cooperative synchronization toolkit (lock, semaphore, condvar, rwlock, barrier, latch, once, queue, monitor) built on new `ev/futex-wait`/`ev/futex-wake` scheduler ops. The MCP server gains auto-detection and rebuild of corrupt oxigraph stores. Import resolution is reworked with `std/X` and `plugin/X` prefix conventions. Quasiquote gains bracket-syntax support. The `when-ok` macro is added.

---

## [#697](https://github.com/elle-lisp/elle/pull/697) — WASM backend
[`2bdabf9b`](https://github.com/elle-lisp/elle/commit/2bdabf9b) · 2026-04-01 · `feature` `backend` `wasm`

A complete LIR-to-WASM backend built in phases over 15 sub-commits, totaling ~50,000 lines (most of which is a vendored `wasmparser` patch). The backend compiles Elle's LIR to WebAssembly via `wasm-encoder` and executes it through Wasmtime. It handles: constants, arithmetic, comparisons, control flow (if/else with merge, nested if, loops), let* bindings, primitive calls via `rt_call` host function dispatch, data operations (cons/car/cdr, arrays, structs, destructuring), closure capture via linear memory and funcref tables, tail calls via a trampoline loop, fiber yield/resume via `rt_yield`/`rt_resume` host calls with lazy recompilation of resumed closures, and pattern matching including nested destructuring. The register allocator maps LIR virtual registers to WASM locals with spill/fill. The pipeline gains `compile_to_lir()` and the CLI gains `--tier=wasm`. Tests: 616-line smoke suite and 112-line stdlib suite. The HIR analyzer learns `file_letrec` support for top-level mutual recursion.

---

## [#696](https://github.com/elle-lisp/elle/pull/696) — Living model: MCP server with cross-language tracing
[`357dc319`](https://github.com/elle-lisp/elle/commit/357dc319) · 2026-03-28 · `feature` `mcp` `tooling`

The largest single PR in this batch at 6,500+ lines added. Exposes Elle's compilation pipeline as a queryable semantic model through an MCP server with 14 JSON-RPC tools. The core is `src/primitives/compile.rs` (2,470 lines) implementing `compile/analyze`, `compile/signal`, `compile/captures`, `compile/callers`, `compile/callees`, `compile/call-graph`, `compile/diagnostics`, `compile/symbols`, `compile/bindings`, `compile/rename`, `compile/extract`, and `compile/parallelize`. Supporting libraries: `lib/portrait.lisp` for semantic portraits with phase classification and failure-mode analysis, `lib/rdf.lisp` for N-Triples generation, and a new `watch` plugin for filesystem monitoring via `notify`. The MCP server's `trace` tool follows calls from Elle through primitives into Rust source with file:line resolution, producing RDF IRIs for SPARQL queryability. The `syn` plugin gains span-location support for line numbers.

---

## [#695](https://github.com/elle-lisp/elle/pull/695) — GenServer, Actor, Task, Supervisor, and EventManager
[`0db8222d`](https://github.com/elle-lisp/elle/commit/0db8222d) · 2026-03-27 · `feature` `stdlib`

OTP-style abstractions layered on the process scheduler: GenServer with call/cast/stop/reply callbacks, Actor as a simple state wrapper, Task for one-shot async work as a monitored process, Supervisor with one-for-one/one-for-all/rest-for-one strategies and dynamic child management, and EventManager for pub/sub with handler modules. All pure Elle in `lib/process.lisp`. The "Agent" name is deliberately avoided to prevent AI confusion. 619 lines of tests covering all abstractions.

---

## [#694](https://github.com/elle-lisp/elle/pull/694) — Negative indexing and sequence accessor widening
[`03b31634`](https://github.com/elle-lisp/elle/commit/03b31634) · 2026-03-27 · `feature` `primitives`

Negative indices now resolve as `len + index` across `get`, `put`, callable form, `slice`, `insert`, and `remove`, with a centralized `resolve_index` helper. `first`, `second`, `last`, `rest`, and `butlast` are widened to all sequence types including `@string` and `bytes`/`@bytes`. A semantic change: `first`/`second`/`last` now error on empty or too-short sequences instead of returning nil, eliminating the ambiguity of `(last [1 2 nil])`. Callable collections (structs, arrays, strings) gain JIT call-dispatch support. 168 lines of negative-index tests and 121 lines of callable tests.

---

## [#693](https://github.com/elle-lisp/elle/pull/693) — Public release prep: new primitives, stdlib, style, docs
[`f92da1fd`](https://github.com/elle-lisp/elle/commit/f92da1fd) · 2026-03-27 · `release` `primitives` `stdlib` `docs`

A broad surface-area expansion for public release. Adds 15 new primitives (`not=`, `hash`, `deep-freeze`, `immutable?`, `nonempty?`, `nan?`, `string/repeat`, `->array`, `->list`, trig/hyperbolic/log functions, etc.) and 10 stdlib functions (`from-pairs`, `sum`, `product`, `get-in`, `put-in`, `update-in`, etc.). `map` and `filter` now preserve container types (array in, array out). Cross-mutability equality (`= [1 2] @[1 2]`) is enabled for all collection pairs. The REPL accepts multiple forms per input. Short-read truncation is fixed for `port/read` on files and pipes -- both sync and async backends now loop until the requested byte count is reached. All examples are restyled with `──` section headers and `defn` throughout. Zlib compress/decompress is added to the compress plugin, and `append` gains cross-mutability support.

---

## [#692](https://github.com/elle-lisp/elle/pull/692) — Fix port/read-all returning nil on empty files
[`e137bf6d`](https://github.com/elle-lisp/elle/commit/e137bf6d) · 2026-03-26 · `bugfix`

`port/read-all` returned `nil` instead of an empty bytes value when reading a zero-length file. The completion handler in `completion.rs` now distinguishes "no data accumulated + EOF" from "no completion at all." Also cleans up some unnecessary `unsafe` blocks in FFI and memory primitives.

---

## [#691](https://github.com/elle-lisp/elle/pull/691) — Move aarch64 CI to PR workflow
[`575de0d7`](https://github.com/elle-lisp/elle/commit/575de0d7) · 2026-03-26 · `ci`

Moves the aarch64 CI job from the main (post-merge) workflow to the PR workflow so architecture regressions are caught before merge rather than after.

---

## [#689](https://github.com/elle-lisp/elle/pull/689) — Hash plugin with streaming API
[`0e9abdbe`](https://github.com/elle-lisp/elle/commit/0e9abdbe) · 2026-03-26 · `feature` `plugin`

A new `hash` plugin providing universal hashing (SHA-256, SHA-512, BLAKE3, MD5, etc.) with both one-shot and streaming digest APIs. Includes `lib/hash.lisp` convenience wrappers and 159 lines of tests. CI workflows updated to build the plugin across all three pipelines.

---

## [#688](https://github.com/elle-lisp/elle/pull/688) — Fix scope-allocation use-after-free and JIT yield LBox reconstruction
[`47a032df`](https://github.com/elle-lisp/elle/commit/47a032df) · 2026-03-25 · `bugfix` `jit` `escape-analysis`

Escape analysis allowed scope-allocating let forms whose body called non-intrinsic functions that internally created heap objects escaping to external mutable structures (e.g. `histogram-record` storing `@{}` via `put`). `RegionExit` freed those objects while still referenced -- a use-after-free. The fix flags any call to a non-intrinsic/non-immediate-primitive function in the outward-set walk, since callees may create escaping heap objects even with safe arguments. Separately, JIT yield environment reconstruction is fixed: the JIT auto-unwraps LBox cells in registers, so spilled values for mutable-captured locals were raw and needed re-wrapping via `lbox_params_mask`/`lbox_locals_mask` from the closure template.

---

## [#687](https://github.com/elle-lisp/elle/pull/687) — Add aarch64 CI
[`61fda341`](https://github.com/elle-lisp/elle/commit/61fda341) · 2026-03-26 · `ci`

Adds an aarch64 build-and-test job to the CI main workflow, and generalizes x86-specific JIT comments (register names, calling conventions) to be architecture-neutral.

---

## [#684](https://github.com/elle-lisp/elle/pull/684) — Fix block-push heap corruption, JIT io-request leak, dual VM/JIT CI
[`4ecad46b`](https://github.com/elle-lisp/elle/commit/4ecad46b) · 2026-03-25 · `bugfix` `ci` `runtime`

Three major runtime fixes. First, the scheduler now crashes on unjoined fiber errors instead of silently swallowing them -- `fiber/abort` sets child status to `:error` even when the parent mask catches the signal, and `ev/scope` is rewritten to detect child errors immediately via `ev/as-completed`. Second, `FiberHeap` gains a separate `shared_alloc_count` field to correctly track allocations routed through the shared allocator, fixing `arena/count` and checkpoint/reset under mark/release scoping. Third, the Makefile adds dual VM/JIT CI targets to catch tier-specific regressions. Numerous test fixes cascade from the unjoined-fiber-crash change: closed-port assertions, nested `ev/run` removal, and `all?` logic correction in stdlib.

---

## [#681](https://github.com/elle-lisp/elle/pull/681) — MQTT plugin, ZMQ FFI library, and FFI type improvements
[`5e3663d1`](https://github.com/elle-lisp/elle/commit/5e3663d1) · 2026-03-24 · `feature` `plugin` `ffi`

An MQTT plugin using a Rust codec (`mqttbytes`) with a state-machine pattern where all TCP I/O stays in Elle, and a pure-FFI ZMQ library binding system `libzmq` for REQ/REP, PUB/SUB, PUSH/PULL, DEALER/ROUTER, and PAIR sockets. FFI marshalling is improved: `ffi/write` accepts immutable arrays and bytes, `ffi/read` returns bytes directly for u8/i8 arrays, and the `(default name value)` macro is added to prelude for `&named` parameter defaults. Tests and examples for both protocols.

---

## [#677](https://github.com/elle-lisp/elle/pull/677) — QUICKSTART and fibers documentation expansion
[`5921fabb`](https://github.com/elle-lisp/elle/commit/5921fabb) · 2026-03-24 · `docs`

Expands QUICKSTART.md with deeper language semantics and adds hyperlinks and spoiler blocks to the docgen site. The docgen generator itself gets fixes for `format-links` paren balance and table-cell link rendering.

---

## [#676](https://github.com/elle-lisp/elle/pull/676) — OpenTelemetry metrics library and port/read edge-case fixes
[`9224f6d6`](https://github.com/elle-lisp/elle/commit/9224f6d6) · 2026-03-24 · `feature` `stdlib` `bugfix`

`lib/telemetry.lisp` is a pure-Elle OTLP/HTTP JSON metrics exporter supporting counters, gauges, and histograms with pre-aggregation at record time, snapshot-and-clear export, and retry with exponential backoff. A background fiber flushes at a configurable interval. On the runtime side, `port/read` on streams now handles partial-read edge cases more carefully, and the DNS import is removed from `lib/http.lisp` since `tcp/connect` handles resolution natively via the Resolve I/O op.

---

## [#675](https://github.com/elle-lisp/elle/pull/675) — Conway's Game of Life and Mandelbrot Explorer demos
[`2b133df1`](https://github.com/elle-lisp/elle/commit/2b133df1) · 2026-03-24 · `demos` `ffi`

Two interactive graphics demos: a Conway's Game of Life using SDL2 with click-to-draw and seed patterns, and a Mandelbrot fractal explorer using GTK4+Cairo with scroll-zoom and pan. Both exercise the FFI callback system for GUI event loops and demonstrate real-world use of struct marshalling, plugin loading (`elle-random`), and pixel-buffer rendering.

---

## [#674](https://github.com/elle-lisp/elle/pull/674) — Process scheduler with I/O integration and structured concurrency
[`64587af8`](https://github.com/elle-lisp/elle/commit/64587af8) · 2026-03-24 · `feature` `concurrency` `stdlib`

Introduces `lib/process.lisp`, an Erlang-inspired process scheduler with fuel-based preemption, mailboxes, links, monitors, named processes, timers, and selective receive. The key design decision is I/O integration: process fibers catch `SIG_IO`, `SIG_EXEC`, and `SIG_WAIT`, so when a process does I/O the scheduler parks it and continues scheduling others rather than blocking the world. Structured concurrency (`ev/spawn`, `ev/join`, `ev/select`, `ev/abort`) works inside processes via sub-fiber tracking. The stdlib exposes `*io-backend*` as a parameter so the process scheduler shares the async backend with the main scheduler. 26 tests and a rewritten example exercise all features.

---

## [#673](https://github.com/elle-lisp/elle/pull/673) — Elle-native AWS client, SigV4 signing, and uring read-loop fixes
[`9ed7b880`](https://github.com/elle-lisp/elle/commit/9ed7b880) · 2026-03-23 · `feature` `stdlib` `jit` `io`

A pure-Elle AWS client with SigV4 signing over TLS, backed by a deterministic code generator that reads AWS Smithy models and emits service modules for restXml, restJson1, awsJson, and awsQuery protocols. The old demo-level sigv4 implementation is removed in favor of the production library. Alongside the AWS work, `squelch` is tightened to exactly two arguments (closure, signals), `integer` gains optional radix parsing (2--36), and `contains?` is extended to strings (absorbing the `string-contains?` alias). The most consequential runtime fix: `ReadAll` on io_uring now resubmits until EOF, fixing truncated reads in `port/read-all` and `system` subprocess output, and `wait_uring` loops correctly when resubmissions produce no new completions. A GLIBC_TUNABLES auto-re-exec is added for C++ plugins that exhaust static TLS.

---

## [#672](https://github.com/elle-lisp/elle/pull/672) — Fix JIT LBox wrapping for mutable-captured parameters
[`359fcff5`](https://github.com/elle-lisp/elle/commit/359fcff5) · 2026-03-23 · `bugfix` `jit`

The JIT loaded fixed parameters as raw values without consulting lbox_params_mask. When a parameter was both mutated and captured by a nested closure, the missing LBox wrapper caused panics in StoreUpvalue ("Cannot mutate non-lbox closure environment variables"). Three changes: compiler.rs wraps fixed params in LBox at function entry when the mask bit is set, translate.rs LoadCapture auto-unwraps via load_lbox, and StoreCapture writes through store_lbox instead of the env_ptr path. Also fixes wait_uring returning empty completions when ReadAll resubmission consumed all CQEs.

---

## [#671](https://github.com/elle-lisp/elle/pull/671) — Redis client, ev/run error propagation, uring short-read fix
[`40c7d6ee`](https://github.com/elle-lisp/elle/commit/40c7d6ee) · 2026-03-23 · `feature` `runtime` `bugfix`

A large PR delivering the Redis client library and fixing several runtime issues discovered while building it.

**Redis client (lib/redis.lisp, 966 lines).** Full RESP2 protocol over async TCP: string/key/hash/list/set/sorted-set commands, transactions (MULTI/EXEC/DISCARD/WATCH with a redis:atomic CAS helper), Lua scripting (EVAL/EVALSHA/SCRIPT), AUTH, scan cursors (SCAN/HSCAN/SSCAN/ZSCAN with drain helper), expiry variants, pub/sub, pipelining, and a connection manager with reconnection. Internal RESP self-tests run without a Redis server.

**Runtime fixes.** FiberStatus::Error is now set before the caught/uncaught branch in handle_sig_switch, so fiber/status correctly returns :error for caught errors. ev/run checks fiber/status instead of fiber/bits for error propagation. Uring short-read resubmission is guarded to stream sockets only (fixes port/read-all regression on regular files). Uring ReadLine resubmits when no newline is found in a partial read. The threadpool read loop accumulates until full size or EOF. Completion handler prepends buffered bytes from prior over-reads. I/O error messages now use std::io::Error::from_raw_os_error for human-readable errno messages instead of raw "errno 111".

**Stdlib additions.** Fiber status predicates (fiber/new?, fiber/alive?, fiber/paused?, fiber/dead?, fiber/error? with short aliases). fiber alias for fiber/new. type-of becomes the canonical form (type is the alias).

**CI.** Redis is installed in all CI workflows for the smoke test suite.

---

## [#670](https://github.com/elle-lisp/elle/pull/670) — Fix cargo-audit advisories
[`268d05d3`](https://github.com/elle-lisp/elle/commit/268d05d3) · 2026-03-23 · `maintenance`

Upgrades rustls-webpki (CRL matching advisory), replaces serde_yml with serde_yaml_ng in elle-yaml (unsound/unmaintained libyml), replaces rustls-pemfile with rustls-pki-types built-in PEM parsing in elle-tls (unmaintained), and removes the iai-callgrind dev-dependency and benchmarks (unmaintained bincode + proc-macro-error). Four advisories resolved.

---

## [#669](https://github.com/elle-lisp/elle/pull/669) — Slab-only allocation + JIT RegionEnter/RegionExit
[`db0dec69`](https://github.com/elle-lisp/elle/commit/db0dec69) · 2026-03-22 · `architecture` `allocator`

Replaces per-scope bump allocators with slab-only allocation for all FiberHeap operations. Scope marks now record slab position; RegionExit deallocates slots back to the free list via release(). This enables slot reuse in long-running fibers instead of accumulating bump memory indefinitely. The per-resume SharedAllocator leak from #664 is fixed by adding get_or_create_shared_allocator() to reuse existing allocators across resumes. SharedAllocator gains push_mark/pop_mark_and_release so child fibers can reclaim shared allocations mid-execution. JIT RegionEnter/RegionExit, which were previously no-ops, are now implemented via elle_jit_region_enter/exit runtime helpers. A large cleanup across 63 files, removing ActiveAlloc, scope_bumps, active_allocator, save/restore_active_allocator, and bump_depth.

---

## [#668](https://github.com/elle-lisp/elle/pull/668) — Redis AGENTS.md docs and bytes->string mutability fix
[`ad4933d5`](https://github.com/elle-lisp/elle/commit/ad4933d5) · 2026-03-22 · `docs` `bugfix`

Adds agent documentation for lib/redis.lisp. Fixes `(string @bytes)` to return an immutable string instead of a mutable @string.

---

## [#667](https://github.com/elle-lisp/elle/pull/667) — Fix JIT yield-through-call SuspendedFrame reconstruction
[`06a67f00`](https://github.com/elle-lisp/elle/commit/06a67f00) · 2026-03-22 · `bugfix` `jit`

Two bugs caused silent fiber death when JIT-compiled code called functions that yield (e.g., TCP I/O through Redis). CallSiteMeta.num_spilled double-counted locals, causing out-of-bounds reads past the spill buffer. SuspendedFrame.stack was missing locals that the interpreter's LoadLocal expects at stack[frame_base + idx]. The stack now contains [locals..., operands...] matching the interpreter's layout.

---

## [#666](https://github.com/elle-lisp/elle/pull/666) — Add hyperlinks and spoilers to docgen site
[`a9201b13`](https://github.com/elle-lisp/elle/commit/a9201b13) · 2026-03-22 · `docs`

Fixes the format-links function in the docgen generator (it was disabled due to a compiler bug; reimplemented with let* instead of def). Adds details/spoiler content blocks with CSS styling. Every doc and example file now links to its GitHub URL. Code excerpts are wrapped in spoilers so the file links stay prominent.

---

## [#665](https://github.com/elle-lisp/elle/pull/665) — Overhaul docgen site
[`06b732b6`](https://github.com/elle-lisp/elle/commit/06b732b6) · 2026-03-21 · `docs`

The generated documentation site gets new Documentation and Examples pages that index all docs/ and examples/ files. All pages are updated for the post-epoch-3 API: define becomes def, lambda becomes fn, display becomes print, let uses parens not brackets, integers are 64-bit. The dead stdlib-reference.json (previously generated from runtime metadata) is removed.

---

## [#664](https://github.com/elle-lisp/elle/pull/664) — Async-first runtime unification
[`9f868d82`](https://github.com/elle-lisp/elle/commit/9f868d82) · 2026-03-22 · `architecture` `runtime`

The largest PR in this batch at 62 files and ~2600 lines changed. This is the architectural shift to running all user code under the async scheduler by default.

**Core change.** execute_scheduled now wraps user code in a synthetic bytecode sequence that calls ev/run(thunk), so the async scheduler is always present. The normal Call instruction path handles closure env setup and SIG_SWITCH trampolining.

**Structured concurrency.** A new SIG_WAIT signal (bit 14) enables a suite of concurrency primitives: ev/join (wait for fibers, propagate errors), ev/select (wait for first of N), ev/race (first wins, abort rest), ev/timeout (deadline), ev/scope (structured nursery that aborts siblings on error), ev/map and ev/map-limited (parallel map with optional concurrency bounds), ev/as-completed (lazy completion iterator). The scheduler gains internal state for waiters, select-sets, and completion tracking.

**JIT fixes.** fiber/resume and coro/resume signal annotations gain SIG_YIELD so the JIT compiles callers with yield-through-call checks. MakeClosure in the JIT passes symbol_names through to nested emitters (was creating closures with empty symbol tables). JIT side-exit for yielding functions is disabled pending frame reconstruction fixes.

**Error propagation.** do_fiber_abort with FiberResume inner frames now sets the error signal before resuming outer bytecode frames. The async scheduler protects io/submit against failure and fiber/aborts requesting fibers. ev/run checks fiber/status for error propagation.

**Allocation fix.** with_child_fiber now provisions a SharedAllocator for all children regardless of signal mask, fixing a use-after-free where non-yielding children's heaps were torn down while closures on other heaps still referenced their bytecode.

**Epoch 4.** stream/read, stream/read-line, stream/read-all, stream/write, stream/flush are renamed to port/ namespace since they operate on ports, not abstract streams.

---

## [#663](https://github.com/elle-lisp/elle/pull/663) — Add Lua surface syntax reader
[`3b6a4413`](https://github.com/elle-lisp/elle/commit/3b6a4413) · 2026-03-21 · `feature` `reader`

A complete Lua surface syntax that parses .lua files into the same Syntax trees the s-expression reader produces. Everything downstream (expander, analyzer, lowerer, emitter, VM) is unchanged. The implementation is a tokenizer (lua_lexer.rs, 726 lines) and a recursive-descent Pratt expression parser (lua_parser.rs, 1329 lines). Covers local/function/if/while/for/repeat-until/break/return, table constructors, field/index access and assignment, method syntax (obj:method), multiple assignment, varargs, Lua string escapes, leveled long strings, and a backtick escape hatch for inline s-expressions. A Lua compatibility prelude (175 lines) provides math, string, table, pairs/ipairs, pcall, etc.

---

## [#662](https://github.com/elle-lisp/elle/pull/662) — Fiber trampoline switch v2
[`82b23d13`](https://github.com/elle-lisp/elle/commit/82b23d13) · 2026-03-21 · `runtime` `jit`

Continues the trampoline work from #655 with three key fixes. (1) handle_fiber_resume_signal now saves the caller's continuation frame, which was unreachable from call_inner's frame-saving code for native function calls like coro/resume. (2) JIT exec_result_to_jit_value handles SIG_SWITCH as a suspending signal instead of silently dropping it and returning nil. (3) The JIT yield helpers now reconstruct the interpreter env correctly: the spill buffer is split using num_locals from call-site metadata, with locals appended to closure.env so LoadUpvalue indexing works on resume. This fixes the stream/zip crash with JIT-compiled map.

---

## [#661](https://github.com/elle-lisp/elle/pull/661) — Fix JIT signal checks: exact equality vs. bitwise containment
[`9607cfc8`](https://github.com/elle-lisp/elle/commit/9607cfc8) · 2026-03-21 · `bugfix` `jit`

JIT signal checks used Rust `matches!` with pattern OR, which tests exact equality. I/O primitives return compound signals like `SIG_YIELD | SIG_IO`, which don't exactly equal `SIG_YIELD`. The checks missed these compounds, causing YIELD_SENTINEL to leak into registers and crash when dereferenced as a Value. Six call sites fixed across three JIT runtime functions to use bitwise containment checks instead.

---

## [#660](https://github.com/elle-lisp/elle/pull/660) — Add arrow and polars plugins
[`9c9e26b2`](https://github.com/elle-lisp/elle/commit/9c9e26b2) · 2026-03-21 · `plugin`

Two columnar data plugins. elle-arrow (745 lines) wraps Apache Arrow with 12 primitives for RecordBatch construction, schema inspection, column extraction, zero-copy slicing, and IPC/Parquet serialization. elle-polars (1391 lines) wraps Polars DataFrames with 30 primitives covering eager and lazy APIs for construction, CSV/Parquet/JSON I/O, select/filter/sort/group-by/join, and summary statistics. A 4497-line addition, most of it in Cargo.lock.

---

## [#659](https://github.com/elle-lisp/elle/pull/659) — Elle MCP server
[`1d536099`](https://github.com/elle-lisp/elle/commit/1d536099) · 2026-03-21 · `feature` `tooling`

The MCP (Model Context Protocol) server for Elle, enabling LLM agents to query the codebase. Built entirely in Elle using the oxigraph plugin as a persistent RDF store.

**Tools.** elle-graph.lisp extracts RDF triples from Elle source (defs, functions, macros, imports). rust-graph.lisp does the same for Rust source via the syn plugin. load-all.lisp orchestrates extraction and loading. The MCP server itself handles JSON-RPC over stdin/stdout.

**IO fixes.** Three io_uring bugs prevented the server from running as a subprocess: (1) Read/Write used pread/pwrite which returns EINVAL on pipes -- fixed by setting offset to u64::MAX to use read/write. (2) Flush on stdout/stderr submitted fsync which returns EINVAL on pipes -- added to the no-op list. (3) wait() blocked forever when all pending ops were stdin reads via StdinThread -- fixed to detect and block on the receiver directly.

scripts/ is renamed to tools/.

---

## [#658](https://github.com/elle-lisp/elle/pull/658) — Fix use-after-free in batch JIT compilation
[`168d92aa`](https://github.com/elle-lisp/elle/commit/168d92aa) · 2026-03-21 · `bugfix` `jit`

Batch compilation (compile_batch) discarded the closure_constants returned by translate_function, allowing Rc<ClosureTemplate> references to be freed while JIT code still held raw pointers. The freed memory (0xdeadcafedeadcafe sentinel) would be accessed during subsequent calls, crashing in plugin primitives. The fix collects closure_constants during the batch loop and passes them into JitCode::new_shared so they live as long as the JIT code.

---

## [#657](https://github.com/elle-lisp/elle/pull/657) — Fix match decision tree duplicating arm bodies for or-patterns
[`bd28d08d`](https://github.com/elle-lisp/elle/commit/bd28d08d) · 2026-03-21 · `bugfix` `compiler`

The match lowerer emitted the same arm body multiple times when an or-pattern had several alternatives (e.g., `(or :array :@array ...)`). Each copy shared binding slots via binding_to_slot but only the first copy emitted MakeLBox cell initialization. When a later alternative matched at runtime, the shared slot contained stale data, causing "Expected cell, got closure" panics. The fix tracks which arms have been lowered and jumps to the existing code on subsequent encounters. A tight 50-line fix.

---

## [#656](https://github.com/elle-lisp/elle/pull/656) — Consolidate sync output primitives, add stderr support
[`aca337d1`](https://github.com/elle-lisp/elle/commit/aca337d1) · 2026-03-20 · `refactor` `api` `breaking`

Replaces Rust-side print!/println! primitives (prim_display, prim_print, prim_write, prim_newline) with Elle stdlib functions that write through `*stdout*`/`*stderr*` ports. This means output now respects parameterize rebinding -- you can redirect stdout by rebinding the parameter. The new API is print, println, eprint, eprintln. Two new epoch migrations handle the renames: epoch 2 (print->println, newline->println, remove write), epoch 3 (display->print). All .lisp files are rewritten to epoch 3 via `elle rewrite`. A 129-file diff, mostly mechanical.

---

## [#655](https://github.com/elle-lisp/elle/pull/655) — Fiber trampoline v1 (SIG_SWITCH infrastructure)
[`55c94cea`](https://github.com/elle-lisp/elle/commit/55c94cea) · 2026-03-20 · `runtime` `wip`

Adds the iterative trampoline infrastructure for fiber/resume. Instead of recursing into do_fiber_resume (which could overflow the Rust stack with deeply nested fiber chains), resume_suspended's FiberResume arm now returns a new SIG_SWITCH signal (bit 13), and a trampoline loop in do_fiber_resume handles the re-dispatch. The PR is explicitly WIP: handle_fiber_resume_signal still calls do_fiber_resume directly because of a frame-chain interaction with TailCall that needs fixing. Also ships a height.md design document analyzing the signal propagation height limit problem.

---

## [#654](https://github.com/elle-lisp/elle/pull/654) — Expand regex plugin, fix CI plugin coverage
[`b1c5517b`](https://github.com/elle-lisp/elle/commit/b1c5517b) · 2026-03-20 · `plugin` `ci`

The regex plugin gains replace, replace-all, split, and captures-all primitives. CI is restructured: the examples and plugin-tests jobs merge into one to avoid building elle twice per workflow. A new check-plugin-list CI target catches divergence between the Makefile PLUGINS list and Cargo.toml workspace members. A plugin cookbook recipe is added to docs.

---

## [#653](https://github.com/elle-lisp/elle/pull/653) — Add elle-tree-sitter plugin
[`4442aed7`](https://github.com/elle-lisp/elle/commit/4442aed7) · 2026-03-20 · `plugin`

Query-first API wrapping tree-sitter with 16 primitives: parsing (C, Rust language grammars), tree navigation, and S-expression pattern matching via ts/query, ts/matches, and ts/captures. Nodes use a safe path-from-root representation to avoid holding raw pointers into tree-sitter's arena.

---

## [#652](https://github.com/elle-lisp/elle/pull/652) — Remove assertions.lisp, consolidate to built-in assert
[`d6b6658b`](https://github.com/elle-lisp/elle/commit/d6b6658b) · 2026-03-20 · `refactor` `testing`

A sweeping mechanical refactoring. The assertions.lisp module that 95 test/example files imported for assert-eq, assert-true, assert-false, etc. is eliminated -- these were trivial wrappers around the built-in `(assert)` primitive that added import boilerplate to every file. The epoch migration system is extended with a Replace rule type that structurally rewrites call forms using positional placeholders. `elle rewrite` applied 9 Replace rules to transform ~5000 assertion calls across 95 files. The `(elle N)` epoch tag is renamed to `(elle/epoch N)` to preserve the `elle` namespace. Net result: -2152 lines. The 134-file diff is almost entirely mechanical.

---

## [#650](https://github.com/elle-lisp/elle/pull/650) — Add elle-jiff plugin for date/time support
[`1c7085a2`](https://github.com/elle-lisp/elle/commit/1c7085a2) · 2026-03-20 · `plugin`

The largest plugin so far at 3726 lines. Wraps all 7 jiff types (timestamp, date, time, datetime, zoned, span, signed-duration) as External values with 104 primitives covering construction, parsing, formatting, accessors, arithmetic, comparison, calendar helpers, timezone ops, epoch conversions, and series generation. Deliberately kept as a plugin rather than core integration to avoid coupling the VM to a 0.x crate. The bare names (no jiff/ prefix) are chosen so user code won't need to change if these types later move in-core.

---

## [#649](https://github.com/elle-lisp/elle/pull/649) — Add IoOp::Task for background thread closures
[`24cd033b`](https://github.com/elle-lisp/elle/commit/24cd033b) · 2026-03-20 · `feature` `runtime`

A new IoOp variant that lets primitives (including plugins) submit arbitrary closures to the thread pool and yield the fiber until completion. The closure runs on a background thread, returns `(i32, Vec<u8>)`, and the fiber resumes with the result. This is the mechanism that enables plugins to do blocking work (database queries, compression, etc.) without blocking the scheduler. The async backend routes to io_uring's network pool or the main pool; the sync backend calls inline.

---

## [#648](https://github.com/elle-lisp/elle/pull/648) — Epoch-based migration system
[`608ea558`](https://github.com/elle-lisp/elle/commit/608ea558) · 2026-03-19 · `architecture` `tooling`

Introduces a versioning mechanism for breaking language changes. Source files can declare `(elle N)` to target a specific epoch. The compiler transparently rewrites old-epoch syntax before macro expansion. `elle rewrite` updates source files on disk. Migration rules are data: Rename and Remove variants, with chained-rename collapsing so epoch 0 -> epoch 5 doesn't require intermediate steps. This is the infrastructure that will power the assertion consolidation and print/display renames in subsequent PRs.

---

## [#647](https://github.com/elle-lisp/elle/pull/647) — Add sys/argv primitive
[`be8d1aa5`](https://github.com/elle-lisp/elle/commit/be8d1aa5) · 2026-03-19 · `feature` `api`

sys/argv returns the full argv vector including the script name as element 0, complementing sys/args which returns only the user arguments. Useful for tools that need to know their own invocation path.

---

## [#646](https://github.com/elle-lisp/elle/pull/646) — Add elle-protobuf plugin
[`e4f3c3a3`](https://github.com/elle-lisp/elle/commit/e4f3c3a3) · 2026-03-19 · `plugin`

Schema-driven protobuf encoding/decoding without code generation. You define message schemas as Elle data structures and the plugin handles wire-format serialization. Supports nested messages, repeated fields, enums, oneof, maps, and well-known types. The schema module (290 lines) and converter (1087 lines) are the bulk of the implementation.

---

## [#645](https://github.com/elle-lisp/elle/pull/645) — elle-tls: async TLS plugin via rustls UnbufferedConnection
[`27d92ae0`](https://github.com/elle-lisp/elle/commit/27d92ae0) · 2026-03-20 · `plugin` `networking`

A full async TLS implementation. The Rust plugin (1419 lines) implements the rustls UnbufferedConnection state machine as primitives: tls/client-state, tls/process, tls/write-plaintext, tls/get-outgoing, tls/get-plaintext, tls/handshake-complete?. The Elle stdlib layer (lib/tls.lisp, 330 lines) builds tls/connect, tls/accept, tls/read, tls/write, tls/close, tls/lines, tls/chunks on top, with async handshake loops.

Also adds a `sys/resolve` primitive routing getaddrinfo through the thread pool, and fixes io_uring's wait() to drain network_pool completions when the ring has no in-flight ops (preventing hangs on pool-only operations). The HTTP library is updated to use tls/connect for HTTPS.

---

## [#644](https://github.com/elle-lisp/elle/pull/644) — Update QUICKSTART with int size, port/seek/tell, async I/O
[`a5ed4af7`](https://github.com/elle-lisp/elle/commit/a5ed4af7) · 2026-03-19 · `docs`

Documents the post-#640 world: integers are full 64-bit, port/seek and port/tell are available, and the async I/O section reflects current reality.

---

## [#643](https://github.com/elle-lisp/elle/pull/643) — Add elle-clap plugin for CLI argument parsing
[`3d504c67`](https://github.com/elle-lisp/elle/commit/3d504c67) · 2026-03-19 · `plugin`

Wraps clap v4 for declarative CLI argument parsing from Elle. You describe arguments as Elle data structures (name, type, required, default, help text) and get back a parsed struct. Supports subcommands, positional args, flags, and value validation.

---

## [#642](https://github.com/elle-lisp/elle/pull/642) — Replace generic :error keywords with meaningful error types
[`22ec3d1e`](https://github.com/elle-lisp/elle/commit/22ec3d1e) · 2026-03-19 · `refactor` `errors`

A systematic pass across the entire codebase replacing bare `:error` keywords with specific error types like `:type-error`, `:arity-error`, `:io-error`, `:index-error`, etc. The Rust-side error_val_extra helper is added for structured error fields (e.g., including `:path` in file errors). An ERROR_INVENTORY.csv catalogs all 70 error types. Covers 40 files across primitives, VM, and value layers. Also fixes string Debug escaping and struct Display quoting.

---

## [#641](https://github.com/elle-lisp/elle/pull/641) — CI: per-plugin build matrix
[`91a72fc9`](https://github.com/elle-lisp/elle/commit/91a72fc9) · 2026-03-19 · `ci`

Converts the single-job plugin build into a matrix strategy where each plugin builds independently. This improves CI parallelism and makes per-plugin failures easier to isolate.

---

## [#640](https://github.com/elle-lisp/elle/pull/640) — Migrate from 8-byte NaN-boxed to 16-byte tagged-union Value
[`ce5d0807`](https://github.com/elle-lisp/elle/commit/ce5d0807) · 2026-03-19 · `architecture` `value`

The foundational value representation change. `Value(u64)` becomes `Value { tag: u64, payload: u64 }`. This doubles the size of every value but eliminates three classes of bugs: integers are now full i64 (no more 48-bit silent truncation in FFI), heap pointers are full 64-bit, keyword hashes are full 64-bit. HeapObject::Float is eliminated since all floats fit in the immediate payload. The JIT calling convention changes to pass (tag, payload) pairs through Cranelift I64 variables, with a new JitValue `#[repr(C)]` struct for runtime helpers. A 47-file, 5775-line diff touching every layer from value representation through JIT codegen to property tests. Also removes the Binding heap variant (completing #630's work) now that the tag space is explicit.

---

## [#639](https://github.com/elle-lisp/elle/pull/639) — Add elle-syn plugin for Rust syntax parsing
[`18c5942a`](https://github.com/elle-lisp/elle/commit/18c5942a) · 2026-03-18 · `plugin`

Wraps the syn crate to parse Rust source code into Elle data structures. At 905 lines of Rust, this is a full AST bridge covering functions, structs, enums, traits, impls, and expressions. The primary use case is tooling -- the MCP server uses this to extract a knowledge graph from Rust source.

---

## [#638](https://github.com/elle-lisp/elle/pull/638) — Add elle-git plugin backed by git2
[`77780902`](https://github.com/elle-lisp/elle/commit/77780902) · 2026-03-19 · `plugin`

Wraps libgit2 for repository operations: open, init, clone, status, staging, commits, branches, tags, remotes, diffs, and config. The code is well-factored across seven source modules. At 2660 lines added, this is one of the larger plugins.

---

## [#637](https://github.com/elle-lisp/elle/pull/637) — Fix (doc name) for stdlib functions
[`98261552`](https://github.com/elle-lisp/elle/commit/98261552) · 2026-03-18 · `bugfix` `introspection`

The analyzer was rewriting `(doc stdlib-fn)` to `(doc "stdlib-fn")`, which only searched the native primitive docs table. Stdlib functions are closures stored in primitive_values, not native primitives. The fix changes the rewrite condition to pass closures through to prim_doc, which already knows how to extract closure docstrings.

---

## [#636](https://github.com/elle-lisp/elle/pull/636) — sys/args returns a list instead of an array
[`f8ebcb6a`](https://github.com/elle-lisp/elle/commit/f8ebcb6a) · 2026-03-18 · `bugfix` `api`

sys/args was returning an array, but the rest of the language idiom for argument lists uses lists. Changed for consistency.

---

## [#635](https://github.com/elle-lisp/elle/pull/635) — Add base64, compress, csv, toml, yaml, semver plugins
[`cbc8719c`](https://github.com/elle-lisp/elle/commit/cbc8719c) · 2026-03-18 · `plugin`

Six plugins in one PR, all following the same pattern: thin Rust wrappers around established crates, each with AGENTS.md documentation and Elle test suites. base64 handles encode/decode with URL-safe variants. compress wraps flate2 for gzip/deflate/zlib. csv does read/write with configurable delimiters and headers. toml and yaml handle round-trip serialization. semver provides parse/compare/increment operations.

---

## [#634](https://github.com/elle-lisp/elle/pull/634) — Add elle-msgpack plugin
[`a0683f81`](https://github.com/elle-lisp/elle/commit/a0683f81) · 2026-03-19 · `plugin`

Two-tier msgpack API: interop mode does direct encode/decode for FFI integration, tagged mode preserves Elle types through serialization round-trips. 1015 lines of Rust, 234 lines of tests.

---

## [#633](https://github.com/elle-lisp/elle/pull/633) — Wire DNS client into HTTP library
[`bd9087e6`](https://github.com/elle-lisp/elle/commit/bd9087e6) · 2026-03-18 · `feature` `networking`

Despite reusing #631's commit message, this is actually a small follow-up: it integrates the DNS library into lib/http.lisp so HTTP requests can resolve hostnames via the pure-Elle DNS client instead of relying on external tools.

---

## [#632](https://github.com/elle-lisp/elle/pull/632) — Add elle-uuid, elle-xml plugins; expand elle-random
[`d80a48e6`](https://github.com/elle-lisp/elle/commit/d80a48e6) · 2026-03-18 · `plugin`

Three plugins in one PR. elle-uuid wraps uuid crate for v4/v5 generation, parsing, and introspection. elle-xml provides SAX-style streaming and DOM-style parse/emit. elle-random is migrated to rand 0.9 and gains distribution samplers (normal, uniform, exponential, etc.) and CSPRNG support. Each plugin has comprehensive Elle test suites.

---

## [#631](https://github.com/elle-lisp/elle/pull/631) — Pure Elle DNS client (RFC 1035)
[`34b15bdd`](https://github.com/elle-lisp/elle/commit/34b15bdd) · 2026-03-18 · `feature` `networking`

A complete DNS client written entirely in Elle: wire-format codec, query building, response parsing with CNAME following, nameserver discovery from /etc/resolv.conf, retry logic. All I/O goes through udp/send-to and udp/recv-from for scheduler compatibility. At 620 lines of Elle, this is a significant demonstration of the language's capability for low-level protocol work. Wire-format helpers are tested with synthetic packets (no network required).

---

## [#630](https://github.com/elle-lisp/elle/pull/630) — Replace NaN-boxed Binding with arena-indexed compile-time type
[`f981c73f`](https://github.com/elle-lisp/elle/commit/f981c73f) · 2026-03-18 · `refactor` `compiler`

Binding was a NaN-boxed value wrapping an Rc<BindingInner>, which leaked via Rc::into_raw and was never freed. This PR introduces BindingArena (a Vec<BindingInner>) owned by the compilation pipeline. Binding becomes a 4-byte Copy type (u32 index into the arena) instead of an 8-byte heap-allocated value. The analyzer holds `&mut BindingArena`; the lowerer holds `&BindingArena` -- the phase boundary is now enforced by the type system. Touches 37 files across the HIR, LIR, pipeline, and LSP layers.

---

## [#629](https://github.com/elle-lisp/elle/pull/629) — Add as->, some->, some->> threading macros
[`3a464549`](https://github.com/elle-lisp/elle/commit/3a464549) · 2026-03-18 · `feature` `prelude`

Pure prelude macro additions, no Rust changes required. `as->` threads a value to an explicit user-named binding. `some->` and `some->>` add nil short-circuiting to thread-first and thread-last. Closes #124.

---

## [#628](https://github.com/elle-lisp/elle/pull/628) — README: escape set literal pipes in markdown tables
[`764dcbd9`](https://github.com/elle-lisp/elle/commit/764dcbd9) · 2026-03-18 · `docs`

One-liner: escape `|` characters in set literal syntax inside markdown tables so they don't break table rendering.

---

## [#627](https://github.com/elle-lisp/elle/pull/627) — Generalized sort with compare primitive
[`2869a1b0`](https://github.com/elle-lisp/elle/commit/2869a1b0) · 2026-03-18 · `feature` `stdlib`

Sort previously only accepted numbers. It now works on any comparable value type via Value::Ord. A new `compare` primitive returns -1/0/1 for any two comparable values. A stdlib `sort-with` function accepts a custom comparator.

---

## [#626](https://github.com/elle-lisp/elle/pull/626) — number->string radix argument and seq->hex
[`c5bba2ad`](https://github.com/elle-lisp/elle/commit/c5bba2ad) · 2026-03-18 · `feature` `stdlib`

number->string gains an optional radix argument (2-36). New seq->hex primitive converts byte sequences to hex strings. Both are useful for binary protocol work and debugging.

---

## [#625](https://github.com/elle-lisp/elle/pull/625) — Add port/seek and port/tell primitives
[`78768090`](https://github.com/elle-lisp/elle/commit/78768090) · 2026-03-18 · `feature` `io`

Adds Seek and Tell IoOp variants with full implementation in both sync and async backends. port/seek accepts `:start`, `:current`, `:end` whence keywords. The async backend implementation is substantial at ~180 lines, handling the uring submission path.

---

## [#624](https://github.com/elle-lisp/elle/pull/624) — Extend concat to bytes, sets, and structs
[`c810f45f`](https://github.com/elle-lisp/elle/commit/c810f45f) · 2026-03-18 · `feature` `stdlib`

The concat primitive previously only worked on lists and arrays. It now supports bytes, @bytes, set, @set, struct, and @struct, with a mutability-matching requirement on both arguments. Includes a large test suite exercising all type combinations.

---

## [#623](https://github.com/elle-lisp/elle/pull/623) — Add file/stat and file/lstat primitives
[`fac75930`](https://github.com/elle-lisp/elle/commit/fac75930) · 2026-03-18 · `feature` `io`

Returns an 18-field immutable struct with full filesystem metadata: size, timestamps, type flags, permissions, uid/gid, inode, etc. file/stat follows symlinks; file/lstat does not.

---

## [#622](https://github.com/elle-lisp/elle/pull/622) — Pointer arithmetic primitives
[`c0d1931b`](https://github.com/elle-lisp/elle/commit/c0d1931b) · 2026-03-18 · `feature` `ffi`

Adds ptr/add, ptr/diff, ptr/to-int, ptr/from-int for FFI pointer navigation. These are deliberately separate from numeric `+` and `-` to preserve the closed return-type contract of arithmetic operators. Also makes `ptr?` the canonical predicate (pointer? becomes an alias) and changes type-of to return `:ptr` for pointers.

---

## [#621](https://github.com/elle-lisp/elle/pull/621) — Fix JIT side-exit panic when yielding primitive called from hot function
[`b2bafdf4`](https://github.com/elle-lisp/elle/commit/b2bafdf4) · 2026-03-18 · `bugfix` `jit`

Three bugs in the JIT yield path combined to panic at call.rs:221 when a JIT-compiled function called stream/write (or any SIG_IO|SIG_YIELD primitive) on the 10th+ invocation. (1) `jit_handle_primitive_signal` never stored the signal on fiber.signal for SIG_YIELD, so the interpreter resume path unwrapped None. (2) `run_jit` hardcoded SIG_YIELD, discarding compound signals like SIG_YIELD|SIG_IO. (3) `elle_jit_yield_through_call` used expect() on fiber.suspended, which is None for primitives. All three fixed; regression tests added.

---

## [#619](https://github.com/elle-lisp/elle/pull/619) — Restore QUICKSTART.md
[`701117dd`](https://github.com/elle-lisp/elle/commit/701117dd) · 2026-03-18 · `docs`

Restores the QUICKSTART guide with ~850 lines of content covering the full language surface. Previously reduced to a stub during some prior refactoring.

---

## [#618](https://github.com/elle-lisp/elle/pull/618) — vm/list-primitives returns symbols; vm/primitive-meta accepts symbols
[`90c7239d`](https://github.com/elle-lisp/elle/commit/90c7239d) · 2026-03-18 · `bugfix` `introspection`

vm/list-primitives was returning strings; vm/primitive-meta expected strings. Both now use symbols, consistent with how primitives are referenced everywhere else in the language.

---

## [#617](https://github.com/elle-lisp/elle/pull/617) — CI: parallelize and cache plugin builds
[`76a7e7a9`](https://github.com/elle-lisp/elle/commit/76a7e7a9) · 2026-03-18 · `ci`

Splits plugin build+test into a parallel CI job across all three workflows (PR, main, merge queue). Plugins are now built via a single cargo invocation instead of sequential per-plugin builds.

---

## [#616](https://github.com/elle-lisp/elle/pull/616) — Rename :native-function to :native-fn, add native-fn? predicate
[`bf97db80`](https://github.com/elle-lisp/elle/commit/bf97db80) · 2026-03-18 · `api` `cleanup`

Shortens the type tag from `:native-function` to `:native-fn` for consistency with the fn convention used elsewhere. Adds a `native-fn?` predicate. Removes the ambiguous `function?` predicate that matched both closures and native functions.

---

## [#615](https://github.com/elle-lisp/elle/pull/615) — @string length and put are now grapheme-indexed
[`ca75eb83`](https://github.com/elle-lisp/elle/commit/ca75eb83) · 2026-03-17 · `bugfix` `unicode`

Mutable strings (@string) were byte-indexed for length, put, push, and pop while immutable strings were grapheme-indexed. This inconsistency meant the same logical operation could give different results depending on mutability. All @string operations now use grapheme indexing, matching immutable string behavior.

---

## [#614](https://github.com/elle-lisp/elle/pull/614) — sys/env string keys, meta/origin, json/parse :keys :keyword, eliminate --
[`cf26ecbe`](https://github.com/elle-lisp/elle/commit/cf26ecbe) · 2026-03-17 · `api` `stdlib`

A grab-bag of small API improvements. sys/env gains string key support and optional single-variable lookup. json/parse gains a `:keys :keyword` option to produce keyword-keyed structs instead of string-keyed ones. A new meta/origin primitive returns the source location of a closure. The `--` separator is eliminated from the CLI; arguments after the source file become sys/args directly. Defer and protect docstrings are clarified.

---

## [#613](https://github.com/elle-lisp/elle/pull/613) — Fix cond/match register corruption in variadic calls
[`bc3c2c26`](https://github.com/elle-lisp/elle/commit/bc3c2c26) · 2026-03-17 · `bugfix` `lir`

The LIR emitter sorted basic blocks by label number before emitting bytecode. Because lower_cond and lower_match allocate their merge/done block first, that block got a lower label number than the arm blocks, so it was emitted before its predecessors had saved their stack state. This zeroed out argument registers at the merge point. The fix is to iterate blocks in append order instead of sorted order, matching the lowerer's natural predecessor-before-merge invariant. Seventeen regression tests added across Rust and Elle.

---

## [#609](https://github.com/elle-lisp/elle/pull/609) — Rename process/* to subprocess/*, fix sys/args and sys/env
[`58838890`](https://github.com/elle-lisp/elle/commit/58838890) · 2026-03-17 · `api` `breaking`

All process/* primitives move to the subprocess/ namespace, which better reflects their role (these spawn child processes, not manage the current one). The args sequence type is generalized to accept list, array, or @array. A new sys/env primitive exposes the process environment as a keyword struct. The `--` separator for sys/args is fixed so shebang invocations actually work. The old test file is replaced wholesale with expanded coverage.

---

## [#608](https://github.com/elle-lisp/elle/pull/608) -- Async port/open via io_uring openat
[`b188a891`](https://github.com/elle-lisp/elle/commit/b188a891) · 2026-03-17 · `io`

Makes `port/open` async by submitting file opens through io_uring's `openat` operation with optional timeout support (linked `IORING_OP_LINK_TIMEOUT`). The sync backend gets a thread-pool fallback. Also clarifies the `defer` docstring to distinguish cleanup arguments from body arguments. 834 lines of new I/O backend code.

---

## [#607](https://github.com/elle-lisp/elle/pull/607) — Oxigraph RDF quad store + SPARQL plugin
[`05a068a8`](https://github.com/elle-lisp/elle/commit/05a068a8) · 2026-03-17 · `plugin` `architecture`

A large PR that delivers the oxigraph plugin and, in the process, redesigns keyword representation. The plugin itself wraps oxigraph for RDF quad CRUD and SPARQL query/update, but the interesting engineering is in the infrastructure it forced.

**Keyword hash rewrite.** Keywords were interned string pointers, which broke across DSO boundaries (each cdylib gets its own static table). The fix replaces the payload with a 47-bit FNV-1a hash, making equality a single u64 comparison. Keyword ordering becomes hash-based instead of pointer-based.

**Plugin context routing.** Because Rust statics aren't shared across DSOs, keyword intern/lookup is routed through function pointers in PluginContext. Plugins call `ctx.init_keywords()` at startup to install the host's functions, ensuring keyword identity works across the host-plugin boundary.

**CI.** Plugin builds and tests now run in the merge queue workflow.

---

## [#599](https://github.com/elle-lisp/elle/pull/599) -- Structural correctness improvements
[`6899eaff`](https://github.com/elle-lisp/elle/commit/6899eaff) · 2026-03-16 · `fix` `jit` `compiler`

Four targeted fixes. Implements `Hash`+`Eq` for `PatternLiteral` and `Constructor` using `f64::to_bits()`, replacing a `format!("{:?}", c)` hack in decision tree column selection. Ungates JIT yield side-exit tests so CI catches regressions. Replaces a panic in `jit_handle_primitive_signal` with graceful suspension for composed signals (e.g., `SIG_YIELD|SIG_IO`) -- all I/O primitives return composed signals, so the old exact-match dispatch crashed on any JIT-compiled I/O call. Adds fiber swap protocol documentation with Mermaid and Graphviz diagrams.

---

## [#598](https://github.com/elle-lisp/elle/pull/598) -- Hammer time: global cleanup and simplification
[`820276ef`](https://github.com/elle-lisp/elle/commit/820276ef) · 2026-03-16 · `refactor`

An 8-phase cleanup pass across 58 files (-3010, +2627 lines). Removes dead code and consolidates the error module (phase 1). Renames `fiber_heap` to `fiberheap` (phase 2). Splits the emitter into `emit/mod.rs` + `emit/stack.rs` (phase 3). Extracts `lir/lower/access.rs` from pattern matching, deletes `tree.rs` (phase 3). Extracts `vm/env.rs` from `vm/call.rs` (phase 5). Splits `jit/compiler` into `compiler.rs` + `calls.rs` and `jit/dispatch` into `dispatch.rs` + `vtable.rs` (phase 6). Extracts `io/pending.rs` from `io/aio.rs` (phase 7). Extracts `primitives/formatspec.rs` from `format.rs` (phase 8). Splits the lexer into `lexer.rs` + `numbers.rs` (phase 3).

---

## [#594](https://github.com/elle-lisp/elle/pull/594) -- Numeric literal formats
[`afcbca88`](https://github.com/elle-lisp/elle/commit/afcbca88) · 2026-03-16 · `reader`

Adds hex (`0xff`), octal (`0o77`), binary (`0b1010`), underscore separators (`1_000_000`), and scientific notation (`1.5e10`, `2.3e-4`) to the reader's number literal parsing. 451 lines of new lexer code with 81 lines of Elle tests.

---

## [#593](https://github.com/elle-lisp/elle/pull/593) -- Fiber fuel system: preemption via instruction budget
[`b8470ac3`](https://github.com/elle-lisp/elle/commit/b8470ac3) · 2026-03-16 · `vm` `scheduling`

Adds cooperative preemption to fibers via an instruction budget ("fuel"). `SIG_FUEL` (bit 12) is a new signal. `fiber/set-fuel` sets the budget; the VM decrements at backward jumps and call instructions, suspending the fiber with `SIG_FUEL` when exhausted. `fiber/fuel` reads remaining fuel, `fiber/clear-fuel` removes the budget. Includes a round-robin scheduler example demonstrating fair CPU sharing across fibers.

---

## [#592](https://github.com/elle-lisp/elle/pull/592) -- JIT: 20 new instructions + IsTable -> IsStructMut rename
[`4e999734`](https://github.com/elle-lisp/elle/commit/4e999734) · 2026-03-16 · `jit`

Implements JIT translation for 20 previously-unsupported instructions across several categories: type predicates (`IsArray`, `IsArrayMut`, `IsStruct`, `IsStructMut`, `IsSet`, `IsSetMut`), destructuring (`CarOrNil`, `CdrOrNil`, `CarDestructure`, `CdrDestructure`, `ArrayMutRefOrNil`, `ArrayMutRefDestructure`, `ArrayMutSliceFrom`, `ArrayMutLen`, `ArrayMutPush`, `ArrayMutExtend`, `PushParamFrame`), struct access (`TableGetOrNil`, `TableGetDestructure`, `StructRest`, `CheckSignalBound`), calls (`CallArrayMut`, `TailCallArrayMut`), and closures (`MakeClosure` -- inner lambdas now JIT-compile). Renames `IsTable`/`TableGet*` to `IsStructMut`/`StructGet*`. Also fixes io_uring `EINTR` handling in `submit_with_args` -- signals like `SIGCHLD` during wait now retry instead of returning zero completions.

---

## [#591](https://github.com/elle-lisp/elle/pull/591) -- Contracts: validator compilation and enforcement
[`e60578a1`](https://github.com/elle-lisp/elle/commit/e60578a1) · 2026-03-16 · `lib`

Adds a pure-Elle contract library (`lib/contract.lisp`, 453 lines) with `compile-validator`, `validate`, and combinators for composable runtime validation. Includes `explain` for human-readable error messages and `contract` for function wrapping. 290 lines of tests.

---

## [#590](https://github.com/elle-lisp/elle/pull/590) -- Squelch redesign: runtime closure transform
[`a0f43c6f`](https://github.com/elle-lisp/elle/commit/a0f43c6f) · 2026-03-16 · `signals` `refactor`

Redesigns squelch from a compile-time special form to a runtime closure transform. Adds a `squelch_mask` field to `Closure` and an `effective_signal()` method. `(squelch f :yield)` returns a new closure with the mask set; enforcement happens at the call boundary in `call_inner`. Removes the `squelch` special form, `CheckSignalForbidden` instruction, and `BoundKind` enum (now single-variant dead complexity). Fixes squelch enforcement lost on tail-call invocation (#588).

---

## [#589](https://github.com/elle-lisp/elle/pull/589) -- JIT rejection diagnostics
[`967925ce`](https://github.com/elle-lisp/elle/commit/967925ce) · 2026-03-16 · `jit` `diagnostics`

Two new diagnostic mechanisms for understanding JIT compilation decisions. `(jit/rejections)` returns a list of `{:name :reason :calls}` structs for every rejected closure, sorted by call count. `ELLE_JIT_STATS=1` prints a summary to stderr on exit. Rejections are deduplicated by bytecode pointer. Expected reasons are `UnsupportedInstruction` and `Polymorphic`; all others remain panics.

---

## [#587](https://github.com/elle-lisp/elle/pull/587) -- Macro system infrastructure: syntax-case, begin-for-syntax
[`87d8233c`](https://github.com/elle-lisp/elle/commit/87d8233c) · 2026-03-16 · `macros` `syntax`

Three additions to the macro system. **Syntax predicates** (9 new primitives): `syntax-pair?`, `syntax-list?`, `syntax-symbol?`, `syntax-keyword?`, `syntax-nil?`, `syntax->list`, `syntax-first`, `syntax-rest`, `syntax-e` -- enabling macros to inspect and destructure syntax objects without leaving the syntax domain. **begin-for-syntax**: evaluates `(def <symbol> <expr>)` forms at compile time, storing results in `compile_time_env` on the `Expander` (reset on clone to prevent leakage). **syntax-case**: expander-recognized pattern matching over syntax objects with wildcards, pattern variables, literal values, list patterns, and `when` guards. Generates let/if chains using syntax predicates at expansion time -- no `eval_syntax` calls.

---

## [#580](https://github.com/elle-lisp/elle/pull/580) -- Add squelch: blacklist signal constraint form
[`de8ebe0f`](https://github.com/elle-lisp/elle/commit/de8ebe0f) · 2026-03-15 · `signals` `compiler`

`silence` is whitelist: `(silence f)` forbids all signals, `(silence f :error)` allows only `:error`. `squelch` is blacklist: `(squelch f :yield)` forbids only `:yield`, everything else passes through. This makes the constraint system open-world, matching the emission system -- user-defined signals not listed in `squelch` pass through without rejection, enabling composition across module boundaries. New `CheckSignalForbidden` bytecode instruction. `BoundKind` enum distinguishes silence (whitelist) from squelch (blacklist) bounds. Strips keyword arguments from `silence` (keywords now mean "use squelch instead").

---

## [#579](https://github.com/elle-lisp/elle/pull/579) -- Rewrite nqueens demo
[`7131873a`](https://github.com/elle-lisp/elle/commit/7131873a) · 2026-03-15 · `demo`

Rewrites the nqueens demo using `defn` and cons-list accumulator instead of mutable arrays, reducing from 75 to 43 lines while being more idiomatic.

---

## [#577](https://github.com/elle-lisp/elle/pull/577) -- SIG_EXEC + subprocess management via io_uring
[`95207b76`](https://github.com/elle-lisp/elle/commit/95207b76) · 2026-03-15 · `io` `primitives`

Adds `SIG_EXEC` capability bit and subprocess management: `process/exec` (spawn via io_uring or thread pool), `process/wait` (io_uring `IORING_OP_WAITID` or fallback `waitpid`), `process/kill` (accepts signal keywords like `:sigterm`), `process/pid`, and `process/system` stdlib wrapper. `PortKind::Pipe` for subprocess stdio. `ev/spawn` mask extended with `SIG_EXEC`. 163 lines of Elle tests for subprocess lifecycle.

---

## [#576](https://github.com/elle-lisp/elle/pull/576) -- Replace root bump allocator with slab free-list
[`2ade68eb`](https://github.com/elle-lisp/elle/commit/2ade68eb) · 2026-03-15 · `allocator` `perf`

Replaces the root bump allocator with `RootSlab`, a chunk-based slab allocator that supports reclamation without GC -- essential for long-running processes where a bump allocator would exhaust memory. Adds `ActiveAlloc` enum to distinguish allocation strategies. Unifies `arena/stats`, `arena/fiber-stats`, and `arena/scope-stats` into a single introspection call. Deletes the `SharedAllocator` bump path.

---

## [#574](https://github.com/elle-lisp/elle/pull/574) -- Traits: per-value dispatch tables
[`0d5313c9`](https://github.com/elle-lisp/elle/commit/0d5313c9) · 2026-03-14 · `runtime` `value`

Adds a traits field to heap values -- a dispatch table (struct) attached to individual values. `with-traits` attaches a trait struct, `traits` reads it back. The formatter uses trait dispatch for custom display. Trait attachment produces independent copies (no mutable sharing). Full stack: heap type modification, `SendValue` serialization for cross-thread transfer, primitives, and 375 lines of Elle tests.

---

## [#573](https://github.com/elle-lisp/elle/pull/573) -- I/O Phase 6: stream combinators + Signal::Inert -> Silent
[`e9266673`](https://github.com/elle-lisp/elle/commit/e9266673) · 2026-03-14 · `io` `refactor`

Adds stream sink combinators (`for-each`, `fold`, `collect`, `into-array`), transform combinators (`map`, `filter`, `take`, `drop`, `concat`, `zip`, `pipe`), and port-to-stream converters (`port/lines`, `port/chunks`, `port/writer`) with lazy I/O semantics. Renames `Signal::Inert` to `Signal::Silent` throughout the codebase. Fixes `SignalBits::covers` for mask checks so `SIG_IO` propagates through coroutines correctly.

---

## [#572](https://github.com/elle-lisp/elle/pull/572) -- Root fiber uses FiberHeap instead of HEAP_ARENA
[`341abbd5`](https://github.com/elle-lisp/elle/commit/341abbd5) · 2026-03-14 · `allocator` `vm`

Gives the root fiber a persistent `FiberHeap` via a leaked `Box` in a `ROOT_HEAP` thread-local, then deletes `HEAP_ARENA`, `HeapArena`, and all the root-fiber special-case branches from arena primitives and signal handlers. Moves `ALLOC_ERROR` from a thread-local into `FiberHeap::alloc_error`. `RegionEnter`/`RegionExit` are now effective on the root fiber. Fixes an escape analysis defect where `walk_for_outward_set` failed to treat calls receiving non-immediate scope-local values as potential outward escapes.

---

## [#571](https://github.com/elle-lisp/elle/pull/571) -- Strict &keys destructuring + struct rest pattern
[`b1a10639`](https://github.com/elle-lisp/elle/commit/b1a10639) · 2026-03-14 · `compiler` `destructuring`

Makes `&keys` destructuring strict (missing keys signal an error) and migrates callers that need optional keyword args to `&named`. Adds a rest field to Struct/Table HIR patterns with `&` syntax (e.g., `{:a a & rest}`) and a new `StructRest` instruction that collects all keys NOT in an exclude set into a new immutable struct. Wired through binding lowering, pattern matching, and decision tree access paths.

---

## [#570](https://github.com/elle-lisp/elle/pull/570) -- CI: consolidate QA jobs
[`4403b314`](https://github.com/elle-lisp/elle/commit/4403b314) · 2026-03-14 · `ci`

Merges fmt, clippy, and doc checks into a single QA job on PRs.

---

## [#569](https://github.com/elle-lisp/elle/pull/569) -- Strict destructuring in binding forms
[`01b08834`](https://github.com/elle-lisp/elle/commit/01b08834) · 2026-03-14 · `compiler` `semantics`

Binding forms (`def`, `var`, `let`, `let*`, `letrec`, `fn` body destructuring) now signal errors on destructuring mismatches instead of silently producing nil. Renames `CarOrNil` to `CarDestructure` (strict), re-adds `CarOrNil` as a separate instruction for parameter contexts where nil-on-miss is correct (`&opt` params, `&keys` keyword args). Adds `TableGetDestructure` for strict struct key access. The `strict: bool` flag on `HirKind::Destructure` tracks binding vs parameter context through the pipeline.

---

## [#568](https://github.com/elle-lisp/elle/pull/568) -- Remove benchmark CI jobs
[`a6204cf0`](https://github.com/elle-lisp/elle/commit/a6204cf0) · 2026-03-14 · `ci`

Removes benchmark jobs from PR and main workflows -- they were being killed by GitHub Actions and reddening the CI badges.

---

## [#567](https://github.com/elle-lisp/elle/pull/567) -- Cache compiled macro transformer closures
[`7fb6682c`](https://github.com/elle-lisp/elle/commit/7fb6682c) · 2026-03-14 · `perf` `syntax`

Caches the compiled macro transformer closure on `MacroDef` so it is compiled once and reused across all invocations of the same macro. Adds a `call_closure` VM helper for direct cached invocation. Adds a `macro_expansion` benchmark group for regression tracking.

---

## [#566](https://github.com/elle-lisp/elle/pull/566) -- Fix ffi/signature with immutable arrays
[`797ab0b5`](https://github.com/elle-lisp/elle/commit/797ab0b5) · 2026-03-13 · `fix`

One-line fix: `prim_ffi_signature` only checked for mutable arrays (`@[...]`) and lists. Adds an `as_array()` branch so immutable array literals (`[...]`) are accepted. Three regression tests.

---

## [#565](https://github.com/elle-lisp/elle/pull/565) -- Pure Elle HTTP/1.1 client and server
[`9ee24ba4`](https://github.com/elle-lisp/elle/commit/9ee24ba4) · 2026-03-14 · `lib` `io` `http`

A 93-file PR that builds a complete HTTP/1.1 stack in pure Elle on top of the async I/O system. `lib/http.lisp` provides URL parsing, header parsing/serialization, request/response wire format, an HTTP client (`http-request`, `http-get`, `http-post`), and an HTTP server (`read-request`, `write-response`, `http-serve`). Also adds `port/path` and `string/size-of` primitives, `string/uppercase`/`lowercase`, optional end for `slice`, `pairs` iterator, `inc`/`dec`, and fiber aliases. Fixes `LoadLocal` corruption and defer+I/O in async fibers by adding `SuspendedFrame::FiberResume` to correctly restore caller frame locals after fiber I/O resumes in nested calls.

---

## [#564](https://github.com/elle-lisp/elle/pull/564) -- Rename effects -> signals
[`22dc9551`](https://github.com/elle-lisp/elle/commit/22dc9551) · 2026-03-12 · `refactor`

The grand rename: 186 files touched. `Effect` type becomes `Signal`, the `effects` module becomes `signals`, `inert?` becomes `silent?`. All related types, functions, identifiers, docs, plugins, examples, and tests updated. Purges `throw`/`raise`/`except` terminology from docs. Strikes stale instruction references. Rewrites the README introduction with fibers+signals examples.

---

## [#561](https://github.com/elle-lisp/elle/pull/561) -- Generalize closure sending across threads
[`95060a2a`](https://github.com/elle-lisp/elle/commit/95060a2a) · 2026-03-11 · `concurrency` `vm`

Replaces manual closure serialization in `spawn_closure_impl` with a general `SendBundle` that can transfer closures (including recursive and mutually recursive ones) across threads. Adds `SendableClosure` and a closure intern table to `SendBundle::from_value`, with an `LBox` fixup pass in `into_value` for mutual recursion. Reduces `concurrency.rs` from ~280 lines of manual serialization to a simple `SendBundle::from_value` call.

---

## [#559](https://github.com/elle-lisp/elle/pull/559) -- Module system documentation
[`6523933b`](https://github.com/elle-lisp/elle/commit/6523933b) · 2026-03-11 · `docs`

Adds comprehensive module system documentation covering the closure-as-module convention, parametric modules, four import styles, and trade-offs (no caching, runtime circular detection, no path resolution, no cross-file static analysis). Includes a test that pins letrec isolation: importing without binding the result does not make the file's definitions visible.

---

## [#553](https://github.com/elle-lisp/elle/pull/553) -- Type system cleanup: @-predicates, Cell -> LBox, blob/buffer purge
[`420a1cb1`](https://github.com/elle-lisp/elle/commit/420a1cb1) · 2026-03-10 · `refactor` `types`

Removes `@array?`, `@string?`, `@bytes?`, `@struct?`, `@set?` predicates (the `mutable?` predicate covers the use case). Renames `Cell` to `LBox` throughout Rust internals. Fixes display: `@bytes` uses `#@bytes[]`, symbols print as `'sym`. Purges legacy blob/buffer conversion functions. Merges `buffer.rs` into `string.rs`. Adds `mutable?` and `box?` predicates. Fixes stale terminology in comments across ~50 files (Buffer to @string, Tuple to array, Blob to @bytes, Cell to box, pure to inert).

---

## [#552](https://github.com/elle-lisp/elle/pull/552) -- Complete the effects (signals) implementation
[`b51790ef`](https://github.com/elle-lisp/elle/commit/b51790ef) · 2026-03-11 · `signals` `compiler` `vm`

Implements the `effect` and `restrict` forms for compile-time signal bounds on closure parameters. The `restrict` preamble on lambda parameters emits `CheckEffectBound` instructions that verify at runtime that a passed closure's inferred signals fit within the declared bound. Adds a signal registry with `resolve_signal_bits` handling arrays, lists, and sets of keywords (OR-ing all bits together). Fixes #558: VM signal dispatch treated `SIG_IO` as subordinate to `SIG_YIELD` instead of checking bits independently with `contains()`. Also switches to debug builds for smoke/examples/elle-scripts (faster iteration).

---

## [#551](https://github.com/elle-lisp/elle/pull/551) -- README restructure + remove string/char-at + fix 5 issues
[`8a996bca`](https://github.com/elle-lisp/elle/commit/8a996bca) · 2026-03-10 · `stdlib` `docs` `fix`

Removes `string/char-at` (use `get` instead). Implements five issues: #546 (defn validation for missing name), #545 (take/drop polymorphic on all sequences), #544 (integer overflow detection), #543 (sort polymorphic on all sequences), #542 (identical? uses pointer identity). Makes `contains?` polymorphic across all collections, with `has?` and `has-key?` as aliases. Replaces "character" terminology with "grapheme cluster" throughout. Simplifies the `each` macro by removing gensyms (macros are hygienic) and consolidating indexed-type branches.

---

## [#550](https://github.com/elle-lisp/elle/pull/550) -- CI: consolidate jobs
[`903c7cf2`](https://github.com/elle-lisp/elle/commit/903c7cf2) · 2026-03-10 · `ci`

Combines integration and property tests into a single job, moves plugin tests into the elle-tests job, runs elle-tests and combined tests in parallel. Moves dependency audit from merge-queue to main branch. Eliminates 5 separate jobs.

---

## [#547](https://github.com/elle-lisp/elle/pull/547) -- Global refactoring: type renames, visibility, file splits
[`6e3bc5ef`](https://github.com/elle-lisp/elle/commit/6e3bc5ef) · 2026-03-10 · `refactor`

A 291-file, 15k-line refactoring. Narrows `pub` to `pub(crate)` throughout, removing dead code exposed by visibility narrowing. Splits 15 oversized files into submodules (io/aio, io/backend, jit/dispatch, jit/translate, ffi/marshal, primitives/debug, primitives/ffi, primitives/table, primitives/fibers, primitives/net, primitives/list, vm/call, lir/emit, lir/lower/decision, value/fiber_heap, reader/syntax, hir/analyze/binding, value/heap). Renames primitives: `has-key?` becomes `has?`, `make-parameter` becomes `parameter`, `module/import` becomes `import`, `pkg/version` becomes `elle/version`, `chan/new` becomes `chan`. Removes redundant `concat` primitive (variadic `string` covers it).

---

## [#538](https://github.com/elle-lisp/elle/pull/538) -- CI: memory benchmarks on main merge
[`c9b9dbb8`](https://github.com/elle-lisp/elle/commit/c9b9dbb8) · 2026-03-09 · `ci`

Adds a memory benchmark job to the main branch CI workflow.

---

## [#536](https://github.com/elle-lisp/elle/pull/536) -- Switch to mimalloc
[`2ea824da`](https://github.com/elle-lisp/elle/commit/2ea824da) · 2026-03-09 · `perf` `build` `ci`

Switches the global allocator to mimalloc. Beyond the allocator change, this PR is a kitchen-sink cleanup: slims AGENTS.md, extracts oddities to `docs/oddities.md`, moves test-modules to `tests/modules/`, merges fn-graph/fn-flow tests, sets `PROPTEST_CASES=1` in merge queue, and reorganizes CI job dependencies extensively.

---

## [#532](https://github.com/elle-lisp/elle/pull/532) -- CI: rename job for badge
[`86d6faa9`](https://github.com/elle-lisp/elle/commit/86d6faa9) · 2026-03-08 · `ci`

Renames the main branch CI job to "CI" so the status badge resolves correctly.

---

## [#531](https://github.com/elle-lisp/elle/pull/531) -- CI: move Rust tests to PR, keep merge queue fast
[`260866ea`](https://github.com/elle-lisp/elle/commit/260866ea) · 2026-03-08 · `ci`

Trims the merge queue workflow to skip Rust tests (covered in PR checks), keeping merge queue wall-clock time short.

---

## [#528](https://github.com/elle-lisp/elle/pull/528) -- Migrate integration tests + CI restructure
[`a6ccd7fe`](https://github.com/elle-lisp/elle/commit/a6ccd7fe) · 2026-03-08 · `testing` `ci`

Migrates 14 more Rust integration test files (~1,240 lines) to Elle. Splits the monolithic `ci.yml` into four separate workflows: `pr.yml`, `merge-queue.yml`, `main.yml`, `weekly.yml`. Cleans up leftover test-510*.lisp scratch files from the variadic JIT work.

---

## [#527](https://github.com/elle-lisp/elle/pull/527) -- Allocation observability, limits, and reclamation
[`4cbff9cb`](https://github.com/elle-lisp/elle/commit/4cbff9cb) · 2026-03-08 · `vm` `allocator`

Adds `arena/set-object-limit`, `arena/object-limit`, `arena/bytes` primitives. Implements per-scope bump allocators so `RegionExit` reclaims bump memory (not just scope marks). Expands the escape analysis whitelist. Adds `arena/peak`, `arena/reset-peak`, `arena/fiber-stats` for heap introspection. Documents VM re-entrancy semantics.

---

## [#526](https://github.com/elle-lisp/elle/pull/526) -- I/O Phase 5: Network I/O
[`892549a4`](https://github.com/elle-lisp/elle/commit/892549a4) · 2026-03-08 · `io` `networking`

Adds TCP, UDP, and Unix socket support across both sync and async backends. Network port types (`PortKind::TcpListener`, `TcpStream`, `UdpSocket`, `UnixListener`, `UnixStream`), `ConnectAddr`, and `IoRequest` timeout fields. A `kwarg.rs` helper handles keyword argument extraction for network primitives (`tcp/listen`, `tcp/connect`, `udp/open`, `unix/listen`, `unix/connect`, `tcp/accept`). Port options (`port/set-options`). The async backend implements io_uring network ops with linked timeouts. 3,870 lines of new code across 20 files.

---

## [#524](https://github.com/elle-lisp/elle/pull/524) -- MicroGPT: fix vocab extraction
[`d40820f8`](https://github.com/elle-lisp/elle/commit/d40820f8) · 2026-03-08 · `demo`

Fixes a vocab extraction bug in the microgpt demo and adds a Python reference implementation for alignment verification.

---

## [#523](https://github.com/elle-lisp/elle/pull/523) -- Set types (immutable and mutable)
[`096d952a`](https://github.com/elle-lisp/elle/commit/096d952a) · 2026-03-08 · `types` `compiler` `vm`

Adds `LSet` and `LSetMut` heap types with `|1 2 3|` and `@|1 2 3|` literal syntax. Renames the `set` special form to `assign` to free the name. Adds `|` as a delimiter token, migrating or-patterns from the old `|` symbol to `SyntaxKind::Pipe`. Full stack implementation: HIR desugaring, pattern matching with `IsSet`/`IsSetMut` bytecode instructions, set algebra primitives (`contains?`, `add`, `del`, `union`, `intersection`, `difference`, `set->list`), and `each` macro support. Values are auto-frozen before insertion.

---

## [#521](https://github.com/elle-lisp/elle/pull/521) -- Remove Pop, Move, Dup from LirInstr
[`67f500fd`](https://github.com/elle-lisp/elle/commit/67f500fd) · 2026-03-08 · `compiler` `lir`

Eliminates three stack-manipulation instructions from the LIR by switching to a store-to-slot-then-reload pattern. `if`/`and`/`or`/`match`/`destructuring` store results to named slots; `discard()` uses a scratch slot with auto-pop semantics. The lowerer now emits only semantic instructions; the emitter derives all stack operations. Removes JIT special-case handling for the deleted variants.

---

## [#520](https://github.com/elle-lisp/elle/pull/520) -- JIT support for variadic functions
[`a95178ae`](https://github.com/elle-lisp/elle/commit/a95178ae) · 2026-03-08 · `jit`

Initially excluded variadic functions from JIT compilation because the entry block only loaded fixed params, leaving the rest-parameter slot as NIL instead of EMPTY_LIST. The fix emits a Cranelift cons-building loop in the JIT entry block that collects rest arguments into a proper cons chain driven by `nargs`. Also fixes `LoadCapture`/`StoreCapture` index partitioning to use `num_params` instead of `fixed_params()`. `VarargKind::Struct` and `VarargKind::StrictStruct` still fall back to the interpreter (they require fiber access for error reporting).

---

## [#519](https://github.com/elle-lisp/elle/pull/519) -- Implement Ord, Hash, and Eq on Value
[`9efe5a2a`](https://github.com/elle-lisp/elle/commit/9efe5a2a) · 2026-03-07 · `value` `runtime`

Implements `Eq`, `Hash`, `Ord`, and `PartialOrd` on `Value` in Rust, using `f64::to_bits()` for float comparison. Fixes `Buffer` `PartialEq`. Adds ordering tests in both Elle and proptest.

---

## [#517](https://github.com/elle-lisp/elle/pull/517) -- Rename Effect::none() -> Effect::inert()
[`798e2143`](https://github.com/elle-lisp/elle/commit/798e2143) · 2026-03-07 · `refactor`

Collapses all Rust-side aliases for the zero effect into `Effect::inert()`. "Inert" means the function runs to completion without interrupting the fiber chain -- unlike "none" (too generic) or "pure" (misleading, since inert functions can still mutate state). Removes `Effect::pure()`, `Effect::PURE`, `Effect::is_pure()`. Also bundles the initial microgpt demo (which gets its own entry below).

---

## [#516](https://github.com/elle-lisp/elle/pull/516) -- MicroGPT demo
[`5873eb23`](https://github.com/elle-lisp/elle/commit/5873eb23) · 2026-03-07 · `demo`

A minimal GPT implementation with scalar autograd, ported from Common Lisp. Split across `autograd.lisp`, `helpers.lisp`, `model.lisp`, and `microgpt.lisp`. Uses the random plugin instead of adding a `math/random` primitive. Also extends the `slice` primitive to work on all sequence types (array, tuple, list, string, buffer).

---

## [#513](https://github.com/elle-lisp/elle/pull/513) -- Replace 'raises'/'raise' with 'signals'/'signal'
[`1c5d39d8`](https://github.com/elle-lisp/elle/commit/1c5d39d8) · 2026-03-07 · `refactor`

Completes the terminology cleanup in the `effects` module itself. Collapses `Effect` from ~80 lines of constructor/predicate methods down to a minimal API now that the `io()`, `io_errors()`, and other compound constructors are gone. Updates `stream.rs` to use `Effect::errors()`.

---

## [#512](https://github.com/elle-lisp/elle/pull/512) -- Types section in README
[`e9744396`](https://github.com/elle-lisp/elle/commit/e9744396) · 2026-03-07 · `docs`

Adds 175 lines documenting Elle's complete type system in the README.

---

## [#511](https://github.com/elle-lisp/elle/pull/511) -- I/O Phase 4: async scheduler and io_uring backend
[`be059556`](https://github.com/elle-lisp/elle/commit/be059556) · 2026-03-07 · `io` `vm`

Adds `AsyncBackend` with an io_uring submission/completion queue and thread-pool fallback for operations that io_uring cannot handle (e.g., stdin). Introduces `BufferPool` for pinned kernel I/O buffers, `io/submit`, `io/reap`, `io/wait` primitives, and `:async` backend mode. The `make-async-scheduler` and `ev/run` stdlib functions provide the async scheduling loop. Extends the shared allocator gate to include I/O fibers.

---

## [#508](https://github.com/elle-lisp/elle/pull/508) -- Migrate ~870 integration tests from Rust to Elle
[`35502c23`](https://github.com/elle-lisp/elle/commit/35502c23) · 2026-03-07 · `testing`

The largest test migration batch. Moves all pure `eval_source()` Rust tests into Elle scripts. Creates 18 new Elle test files (advanced, arena, brackets, buffer, bytes, concurrency, environment, ffi, fn-flow, fn-graph x3, glob, jit-yield, lexical-scope, new-pipeline, pipeline, primitives, table-keys). Deletes `PROPERTY_TEST_ANALYSIS.md`. Keeps only Rust tests that require bytecode inspection, Rust type APIs, proptest, or `process::Command`.

---

## [#506](https://github.com/elle-lisp/elle/pull/506) -- Effect fixpoint convergence limit
[`a18e1b3c`](https://github.com/elle-lisp/elle/commit/a18e1b3c) · 2026-03-07 · `fix` `compiler`

Adds a panic if the effect (signal) fixpoint loop does not converge after 10 iterations. Also completes the `raises`/`raise` to `signals`/`signal` terminology migration across all doc comments, docstrings, test names, and prose.

---

## [#505](https://github.com/elle-lisp/elle/pull/505) -- I/O Phase 3: synchronous I/O, scheduler foundation, SIG_IO
[`b1320f9e`](https://github.com/elle-lisp/elle/commit/b1320f9e) · 2026-03-06 · `io` `vm`

Adds `SIG_IO` as a signal constant (bit 9) with `Effect::io()` and `may_io()` predicates. Introduces `IoRequest` wrapping `IoOp` (ReadLine, Read, ReadAll, Write, Flush) as an `ExternalObject`. Implements `SyncBackend` with per-fd buffered I/O via libc read/write. Adds `sync-scheduler` as a trampoline that resumes fibers and dispatches `SIG_IO` requests, `*scheduler*` as a dynamic parameter, and `ev/spawn` for running fibers through the current scheduler. Top-level execution now routes through `*scheduler*` when available via `VM::execute_scheduled()`.

---

## [#504](https://github.com/elle-lisp/elle/pull/504) -- File-as-letrec compilation model
[`1813ac34`](https://github.com/elle-lisp/elle/commit/1813ac34) · 2026-03-09 · `compiler` `modules`

Eliminates globals for module files by compiling each file as a single `letrec` body. File-level `defn` forms become local bindings, not global definitions. Fixes `needs_cell()` to skip cell allocation for immutable captured locals (only mutable captures get cells). Adds `is_prebound` flag to `Binding` to distinguish let-bound vs begin/letrec-bound. New pipeline entry points: `compile_file()`, `analyze_file()`, `eval_file()`. Converts module files to return closures that export their public API. Fixes fiber stack corruption during `eval` macro expansion and `import-file` by saving/restoring the fiber stack.

---

## [#497](https://github.com/elle-lisp/elle/pull/497) -- Variadic string and string/format
[`887fa488`](https://github.com/elle-lisp/elle/commit/887fa488) · 2026-03-06 · `primitives`

Makes the `string` primitive variadic for ergonomic string building (`(string "hello " name "!")`) and adds `string/format` with positional, named, and format-spec support. The format implementation (714 lines) handles width, alignment, fill, and precision specifiers.

---

## [#496](https://github.com/elle-lisp/elle/pull/496) -- Rename Effect::raises -> Effect::errors
[`6df14008`](https://github.com/elle-lisp/elle/commit/6df14008) · 2026-03-06 · `refactor`

Mechanical rename across the codebase: `raises()` to `errors()`, `may_raise()` to `may_error()`, and all compound variants. Elle has `(error)`, not `(raise)` -- the old name imported misleading associations from stack-unwinding languages. Also updates two JIT tests that asserted rejection of yielding functions, since PR #465 made them accepted via side-exit.

---

## [#492](https://github.com/elle-lisp/elle/pull/492) -- Housekeeping: stale files, BLAS/LAPACK demo, plugin purge
[`02d3149f`](https://github.com/elle-lisp/elle/commit/02d3149f) · 2026-03-06 · `chore` `refactor`

A 200-file cleanup. Removes stale files and dead CI config. Adds a BLAS/LAPACK FFI demo and a pure-Elle heat diffusion matrix demo. Deletes four graph visualization plugins (sugiyama, fdg, dagre, mermaid). Moves `elle-doc` to `demos/docgen`. Restructures `docs/` with AGENTS.md coverage throughout. Consolidates examples.

---

## [#491](https://github.com/elle-lisp/elle/pull/491) -- Channel primitives
[`c3a28f59`](https://github.com/elle-lisp/elle/commit/c3a28f59) · 2026-03-06 · `primitives` `concurrency`

Adds `chan/*` primitives wrapping `crossbeam-channel` for inter-fiber messaging. 492 lines of Rust implementing bounded/unbounded channels, send/recv (blocking and non-blocking), close, and predicates. Tests written in Elle from the start.

---

## [#490](https://github.com/elle-lisp/elle/pull/490) -- Kill ScopeStack
[`305e4279`](https://github.com/elle-lisp/elle/commit/305e4279) · 2026-03-05 · `vm` `perf` `refactor`

Removes the `ScopeStack` from the VM hot path. `LoadGlobal` goes directly to `vm.globals` (no hash lookup), `StoreGlobal` collapses to unconditional store (no 4-way dispatch). The scope stack was always empty at runtime -- no compiler emits scope instructions -- so this is pure dead code removal. Deletes ~817 lines across `scope/`, scope handlers, and their unit tests. `PushScope`/`PopScope`/`DefineLocal` dispatch arms now panic (dead instructions retained for `repr(u8)` stability).

---

## [#489](https://github.com/elle-lisp/elle/pull/489) -- I/O Phase 2: Ports
[`515402cb`](https://github.com/elle-lisp/elle/commit/515402cb) · 2026-03-06 · `io` `primitives`

Introduces the `Port` type wrapping `OwnedFd` with kind-aware display, direction, and encoding. Adds port primitives (`port/open`, `port/open-bytes`, `port/close`, `port/stdin`, `port/stdout`, `port/stderr`, `port?`, `port/open?`) in a new `src/primitives/ports.rs`. Defines `*stdin*`, `*stdout*`, `*stderr*` as Racket-style parameters in `stdlib.lisp`. 110 lines of Elle behavioral tests.

---

## [#488](https://github.com/elle-lisp/elle/pull/488) -- Escape analysis tier 6: while/block/break-aware scope allocation
[`2bcce3e3`](https://github.com/elle-lisp/elle/commit/2bcce3e3) · 2026-03-05 · `compiler` `escape-analysis`

Teaches escape analysis to handle `while` (always returns nil, safe), `block` (checks both normal exit and all `break` values), and `break` (safe in result position -- jumps away, never produces a value locally). Relaxes `can_scope_allocate_block` and `can_scope_allocate_let` to check whether break values are safe immediates instead of rejecting all blocks/lets with breaks. Fixes a bug where `region_depth_at_entry` was recorded after `RegionEnter` instead of before, causing breaks to leak region marks.

---

## [#486](https://github.com/elle-lisp/elle/pull/486) -- Colon syntax as struct/env access desugaring
[`cb144bb9`](https://github.com/elle-lisp/elle/commit/cb144bb9) · 2026-03-05 · `syntax` `compiler`

Moves qualified symbol desugaring (e.g., `obj:field`) from the expander to the analyzer, where it has access to binding information. Deletes the expander-level `qualified.rs` and its tests, replaces with analyzer-level `forms.rs` handling. 101 lines of new integration tests.

---

## [#485](https://github.com/elle-lisp/elle/pull/485) -- (environment) primitive
[`1171d510`](https://github.com/elle-lisp/elle/commit/1171d510) · 2026-03-05 · `primitives` `vm`

Adds `defined_globals` tracking to the VM for O(defined) environment enumeration, and an `(environment)` primitive that returns all defined globals as a struct via `SIG_QUERY`.

---

## [#483](https://github.com/elle-lisp/elle/pull/483) -- Remove .opencode from tracking
[`f44f7c9d`](https://github.com/elle-lisp/elle/commit/f44f7c9d) · 2026-03-05 · `chore`

One-liner: remove `.opencode/agents` from git tracking.

---

## [#482](https://github.com/elle-lisp/elle/pull/482) -- Racket-style parameters for dynamic bindings
[`9e4d2fe7`](https://github.com/elle-lisp/elle/commit/9e4d2fe7) · 2026-03-05 · `vm` `semantics`

Implements Racket-style parameters for fiber-scoped dynamic bindings. New heap type `Parameter` with unique id and default value. Parameters are callable: `(param)` reads the current value from the fiber's `param_frames`. The `parameterize` special form gets HIR, bytecode (`PushParamFrame`/`PopParamFrame`), and VM dispatch. Child fibers inherit the parent's flattened parameter frames on first resume. Body is deliberately NOT in tail position since `PopParamFrame` must execute after.

---

## [#473](https://github.com/elle-lisp/elle/pull/473) -- Source locations in runtime errors
[`21e1a25d`](https://github.com/elle-lisp/elle/commit/21e1a25d) · 2026-03-05 · `errors` `vm`

A multi-phase overhaul of error reporting. Error values migrate from tuples `[:kind "message"]` to structs `{:error :kind :message "message"}`. The `location_map` is threaded through the dispatch loop, `instr_ip` is tracked and resolved to source location on error, and stack traces with source context are added to error messages. Also fixes `frame_base` in `CallFrame` -- closures always execute with a fresh stack, so the old code that captured the caller's stack length was stale and caused addressing errors in nested calls with control flow forms.

---

## [#472](https://github.com/elle-lisp/elle/pull/472) -- Source-to-source rewrite tool and subcommand dispatch
[`6d8e3890`](https://github.com/elle-lisp/elle/commit/6d8e3890) · 2026-03-05 · `tooling`

Replaces `--lint`/`--lsp` CLI flags with `lint`/`lsp`/`rewrite` subcommands. Adds a source-to-source rewrite engine (`src/rewrite/`) with a rule-based architecture: rules match syntax patterns, an engine applies them in order with fixpoint iteration, and a runner coordinates file I/O. The lexer gains span-tracking for token locations needed by the rewrite tool.

---

## [#465](https://github.com/elle-lisp/elle/pull/465) -- JIT yield side-exit
[`aebf966e`](https://github.com/elle-lisp/elle/commit/aebf966e) · 2026-03-06 · `jit`

Enables JIT compilation of yielding closures by generating side-exit code at yield points. When a JIT-compiled function yields, live registers are spilled to a stack slot and `YIELD_SENTINEL` is returned; post-call yield checks detect when a callee yields and propagate the sentinel up the JIT call chain. Resume always goes through the interpreter (no re-entry to JIT after yield). The emitter now collects `YieldPointInfo` and `CallSiteInfo` metadata for the JIT to use during code generation. Fixes local variable preservation across yield/resume by spilling locals into the `SuspendedFrame` in the interpreter's expected layout.

---

## [#464](https://github.com/elle-lisp/elle/pull/464) -- Skip LocalCell allocations for non-captured let bindings
[`b5447e03`](https://github.com/elle-lisp/elle/commit/b5447e03) · 2026-03-05 · `vm` `perf`

Non-captured let bindings were unnecessarily wrapped in `LocalCell` heap allocations. The emitter now threads a `cell_locals_mask` from `LirFunction` through `Closure` to the VM, redirecting non-cell locals to stack slots (`StoreLocal`/`LoadLocal` instead of `StoreUpvalue`/`LoadUpvalue`). All four env-building sites updated: VM call handler, FFI callback, JIT fallback, and thread spawn. Fixes an off-by-one in `allocate_slot()` for variadic functions that used `arity.fixed_params()` (0 for variadics) instead of `num_params` (1, accounting for the rest parameter slot).

---

## [#462](https://github.com/elle-lisp/elle/pull/462) -- Migrate 60 coroutine tests to Elle
[`f8356368`](https://github.com/elle-lisp/elle/commit/f8356368) · 2026-03-05 · `testing`

Pure test migration: 509 lines of Elle coroutine tests replace their Rust equivalents.

---

## [#460](https://github.com/elle-lisp/elle/pull/460) -- Migrate fiber tests + tail call optimization for blocks
[`9b1df537`](https://github.com/elle-lisp/elle/commit/9b1df537) · 2026-03-04 · `testing` `compiler`

Migrates 32 fiber integration tests to Elle scripts and also delivers the tail call optimization for block bodies and break values (fixes #333), extending `tailcall.rs` to recognize tail positions inside block forms.

---

## [#459](https://github.com/elle-lisp/elle/pull/459) -- Enable tail call optimization for block bodies (empty merge)
[`b33e2bfb`](https://github.com/elle-lisp/elle/commit/b33e2bfb) · 2026-03-05 · `compiler`

Empty squash-merge commit; the actual changes landed in PR #460.

---

## [#458](https://github.com/elle-lisp/elle/pull/458) -- Symbol keys in destructuring + letrec destructuring
[`32117a0f`](https://github.com/elle-lisp/elle/commit/32117a0f) · 2026-03-04 · `compiler` `destructuring`

Two features. First, quoted symbols can now be used as keys in struct/table destructuring patterns alongside keywords, via a new `PatternKey` enum at the HIR level. Second, `letrec` now accepts full destructuring patterns (cons, list, array, struct/table), not just simple symbols. A two-pass approach pre-binds all leaf names (enabling mutual recursion through destructured bindings), then wraps the body with `Destructure` nodes.

---

## [#457](https://github.com/elle-lisp/elle/pull/457) -- Comparison operators for strings and keywords
[`be9a6784`](https://github.com/elle-lisp/elle/commit/be9a6784) · 2026-03-04 · `primitives` `jit`

Extends `<`, `>`, `<=`, `>=` to support lexicographic comparison of strings and keywords, in both the VM primitives and JIT runtime helpers. 128 integration tests and 48 property tests added.

---

## [#456](https://github.com/elle-lisp/elle/pull/456) -- Reuse VM across proptest cases
[`855202aa`](https://github.com/elle-lisp/elle/commit/855202aa) · 2026-03-04 · `testing` `perf`

Adds a shared VM helper to `tests/common/mod.rs` that reuses a single bootstrapped VM instance across all proptest cases in a file, eliminating the per-case bootstrap cost.

---

## [#455](https://github.com/elle-lisp/elle/pull/455) -- Migrate property tests to Elle
[`821b4901`](https://github.com/elle-lisp/elle/commit/821b4901) · 2026-03-04 · `testing`

Migrates 8 categories of input-invariant property tests (arithmetic laws, determinism, eval properties, type conversions, sequences, macros, strings, bug regressions) from Rust proptest to Elle scripts that run once. The Rust property test files are trimmed to keep only tests that genuinely depend on random input generation. Net: -2153 lines Rust, +1411 lines Elle.

---

## [#452](https://github.com/elle-lisp/elle/pull/452) -- Match overhaul: decision trees, or-patterns, exhaustiveness
[`902c67d6`](https://github.com/elle-lisp/elle/commit/902c67d6) · 2026-03-04 · `compiler` `match`

A comprehensive rewrite of the match system. Fixes cross-block register tracking for match/cond/block expressions used as call arguments (results were lost across basic block boundaries -- now stored to local slots). Adds dotted rest patterns `(a b . c)` in match. Introduces or-patterns with `(p1 | p2 | p3)` syntax, requiring all alternatives to bind the same variable set. Promotes non-exhaustive match from a lint warning to a compile-time error. Refactors `analyze_pattern` to a callback-based design so one copy of compound pattern logic serves both normal binding and or-pattern binding reuse.

---

## [#451](https://github.com/elle-lisp/elle/pull/451) -- Functional programming primitives
[`9cac68e3`](https://github.com/elle-lisp/elle/commit/9cac68e3) · 2026-03-04 · `stdlib`

Adds `sort` and `range` as Rust primitives, then builds a substantial functional programming library in Elle: `identity`, `compose`/`comp`, `partial`, `complement`, `constantly`, `juxt`, `all?`, `any?`, `find`, `find-index`, `count`, `nth`, `zip`, `flatten`, `take-while`, `drop-while`, `distinct`, `frequencies`, `mapcat`, `group-by`, `map-indexed`, `partition`, `interpose`, `min-key`, `max-key`, `memoize`, `sort-by`, plus `freeze`/`thaw` primitives for value immutability control. 304 lines of Elle tests cover the new API.

---

## [#450](https://github.com/elle-lisp/elle/pull/450) -- Decompose pipeline.rs into submodules
[`2ab2b8ba`](https://github.com/elle-lisp/elle/commit/2ab2b8ba) · 2026-03-05 · `refactor`

Extracts the monolithic `src/pipeline.rs` (508 lines) into `src/pipeline/` with 7 focused submodules: `mod.rs`, `cache.rs` (thread-local compilation cache), `scan.rs` (pre-scanning with unified `prescan_forms`), `fixpoint.rs` (shared fixpoint loop), `compile.rs`, `analyze.rs`, `eval.rs`. Also adds custom allocator infrastructure (`with-allocator`) for per-fiber heap routing.

---

## [#446](https://github.com/elle-lisp/elle/pull/446) -- Fix letrec binding lost after fiber yield/resume
[`d95e0365`](https://github.com/elle-lisp/elle/commit/d95e0365) · 2026-03-04 · `fix` `vm`

When a fiber body tail-calls into another function and that function yields, the VM was saving the outer closure's bytecode but the tail-called function's IP in the `SuspendedFrame`, causing corruption on resume. Fix: introduce `ExecResult` struct that returns the active bytecode/constants/env at exit from `execute_bytecode_*` functions, so suspended frames always capture the correct execution context.

---

## [#445](https://github.com/elle-lisp/elle/pull/445) -- Type predicates: function?, primitive?, zero?
[`9fb6f26e`](https://github.com/elle-lisp/elle/commit/9fb6f26e) · 2026-03-04 · `primitives`

Adds three new type predicate primitives with unit tests. The commit also bundles some test migration work from the same PR branch.

---

## [#444](https://github.com/elle-lisp/elle/pull/444) -- Test migration batch 2
[`8ee02a31`](https://github.com/elle-lisp/elle/commit/8ee02a31) · 2026-03-03 · `testing`

Migrates ~450 tests (prelude, eval, core, destructuring, splice, blocks) from Rust to Elle scripts. Net reduction: ~2900 lines of Rust replaced by ~1200 lines of Elle. Also restructures CI so Elle scripts run in the examples job, adds `make smoke`, and tiers proptest cases (8 for PRs, 16 for merge queue, 128 for weekly).

---

## [#443](https://github.com/elle-lisp/elle/pull/443) -- Glob plugin
[`5bb85b6b`](https://github.com/elle-lisp/elle/commit/5bb85b6b) · 2026-03-03 · `plugin`

Adds a native Rust plugin wrapping file glob pattern matching, with 199 lines of integration tests.

---

## [#442](https://github.com/elle-lisp/elle/pull/442) -- Sequence operation consistency
[`47b0a644`](https://github.com/elle-lisp/elle/commit/47b0a644) · 2026-03-03 · `fix` `stdlib`

Closes six issues at once. `take`/`drop` now error on negative counts instead of unsigned wraparound. Bitwise operations accept floats by truncating to integer (NaN/Infinity rejected). `first`, `rest`, `reverse` become polymorphic over all sequence types. `CdrOrNil` returns `EMPTY_LIST` (truthy) instead of `NIL` (falsy) for exhausted rest patterns. The splice operator `;` now accepts lists.

---

## [#441](https://github.com/elle-lisp/elle/pull/441) -- Org migration: disruptek -> elle-lisp
[`332f0159`](https://github.com/elle-lisp/elle/commit/332f0159) · 2026-03-03 · `ci` `docs`

Update all `disruptek/elle` references to `elle-lisp/elle` across documentation, CI workflows, and GitHub Pages URLs.

---

## [#440](https://github.com/elle-lisp/elle/pull/440) -- Testing strategy: tiered tests and Elle test scripts
[`32876624`](https://github.com/elle-lisp/elle/commit/32876624) · 2026-03-03 · `testing` `infrastructure`

Lays the foundation for migrating the test suite from Rust to Elle. Adds `docs/testing.md` with a decision tree for test placement and a tier structure. Introduces a `tests/elle/` directory with a Rust harness that discovers and runs `.lisp` test scripts, and a `proptest_cases()` helper respecting `PROPTEST_CASES` env var (replacing ~100 hardcoded call sites). The first migrated test (`booleans.lisp`) replaces `booleans.rs`, establishing the pattern: eval-based Rust tests become Elle scripts that run in seconds instead of minutes.

---

## [#439](https://github.com/elle-lisp/elle/pull/439) -- Enable merge queue
[`6e904152`](https://github.com/elle-lisp/elle/commit/6e904152) · 2026-03-03 · `ci`

Add `merge_group` trigger to CI so GitHub's merge queue can batch-validate PRs (batch size 5), eliminating the constant rebasing required by strict status checks.

---

## [#438](https://github.com/elle-lisp/elle/pull/438) -- Compilation pipeline diagram and root Makefile
[`02d3ccf5`](https://github.com/elle-lisp/elle/commit/02d3ccf5) · 2026-03-03 · `docs` `build`

Adds a Graphviz pipeline diagram (dot/SVG) showing all compilation passes including the fixpoint loop, macro expansion re-entry, and JIT branch. New README sections document the module system and native plugin architecture. A root `Makefile` provides `make`, `make plugins`, and `make docs` targets.

---

## [#437](https://github.com/elle-lisp/elle/pull/437) -- Numeric-aware equality
[`4bddb378`](https://github.com/elle-lisp/elle/commit/4bddb378) · 2026-03-03 · `semantics` `jit`

`=` now compares numbers across types: `(= 1 1.0)` is true. Previously it was bitwise identity, so int and float never matched. All three execution paths updated: the primitive fallback, the VM bytecode intrinsic, and the JIT runtime helpers. `identical?` added for strict bitwise identity (the old `=` behavior). `eq?` removed -- it was confusingly named for identity semantics when the language has no pointer-equality concept exposed elsewhere.

---

## [#435](https://github.com/elle-lisp/elle/pull/435) -- Chained comparisons + escape analysis tiers 1-6
[`3e047fd8`](https://github.com/elle-lisp/elle/commit/3e047fd8) · 2026-03-04 · `compiler` `escape-analysis` `semantics`

A large squashed PR that delivers two features. **Chained comparisons** (issue #114): `(< a b c)` desugars to `(and (< a b) (< b c))` with short-circuit, implemented via a `chain_cmp` helper in the primitives layer. **Escape analysis tiers 1-5**: a whitelist of 48 immediate-returning primitives (tier 1), unary minus (tier 2), variable-in-result-position with scope awareness (tier 3), nested let/letrec/block recursion (tier 4), and match arm analysis (tier 5). Adds `arena/scope-stats` and renames all arena primitives to the `arena/*` namespace. The `ELLE_SCOPE_STATS` env var enables compile-time scope allocation statistics.

---

## [#422](https://github.com/elle-lisp/elle/pull/422) — Grab bag: os/* to sys/*, boolean?/integer?/float?, sys/args
[`c002e881`](https://github.com/elle-lisp/elle/commit/c002e881) · 2026-03-03 · `api` `primitives`

Five small changes bundled together: `current-thread-id` returns an integer instead of a stringified Debug format; `boolean?` becomes canonical with `bool?` as alias, `coroutine?` becomes canonical with `coro?` as alias; stale compilation-cache docs get notes; `sys/args` primitive returns CLI args as a tuple; and `os/*` primitives are renamed to `sys/*` (os/spawn to sys/spawn, etc.) with old names kept as aliases. Also adds `integer?` and `float?` predicates.

---

## [#421](https://github.com/elle-lisp/elle/pull/421) -- Escape analysis infrastructure
[`706b5770`](https://github.com/elle-lisp/elle/commit/706b5770) · 2026-03-05 · `compiler` `escape-analysis`

Extends the escape analysis pass with additional primitive whitelist entries (48 total), fixes `fn/raises?` phantom alias to `fn/errors?`, and migrates remaining coroutine/comparison/matching property tests from Rust to Elle scripts.

---

## [#420](https://github.com/elle-lisp/elle/pull/420) — Make error/cancel args optional, rename fn/raises? to fn/errors?
[`2581781a`](https://github.com/elle-lisp/elle/commit/2581781a) · 2026-03-03 · `api`

`(error)` and `(fiber/cancel f)` now work without arguments. `fn/raises?` renamed to `fn/errors?` for consistency with the error/signal model.

---

## [#417](https://github.com/elle-lisp/elle/pull/417) — Rename fn/graph to fn/cfg, add Mermaid output, cfgviz demo
[`97ad7d61`](https://github.com/elle-lisp/elle/commit/97ad7d61) · 2026-03-03 · `introspection` `tooling`

Renames `fn/graph` to `fn/cfg` with `:dot` and `:mermaid` format keywords. Adds a new `src/lir/display.rs` with compact human-readable formatting for all LIR instructions (e.g., "r2 <- r0 + r1" instead of verbose Debug output). `fn/flow` gains `:display`, `:term-display`, `:term-kind`, `:spans`, `:annotated`, and `:term-span` fields. Mermaid output uses diamond/stadium/hexagon shapes and color classes by terminator kind. A `demos/cfgviz/` demo renders CFGs to SVG via graphviz, with pre-rendered SVGs for factorial, fizzbuzz, identity, make-adder, and a 6-way eval-expr match.

---

## [#416](https://github.com/elle-lisp/elle/pull/416) — Scope escape analysis: enable RegionEnter/RegionExit for safe patterns
[`c451cb0b`](https://github.com/elle-lisp/elle/commit/c451cb0b) · 2026-03-02 · `allocator` `compiler`

Replaces the always-false escape analysis stubs with real 5-condition analysis: (1) no binding captured by closures, (2) body cannot suspend (no yield), (3) body result is provably a NaN-boxed immediate, (4) no outward mutation (set to non-let binding), (5) no break in body. Scopes meeting all conditions emit RegionEnter/RegionExit, enabling scope-based destructor invocation. 50 escape analysis tests covering positive, negative, correctness, and regression cases.

---

## [#414](https://github.com/elle-lisp/elle/pull/414) — Per-fiber heaps with zero-copy sharing
[`a79565e6`](https://github.com/elle-lisp/elle/commit/a79565e6) · 2026-03-02 · `allocator` `vm`

Implements per-fiber heap ownership in 6 packages. Package 1: `FiberHeap` struct with dormant routing and fiber-transition swaps. Package 2: bump allocator (bumpalo) replacing `Vec<Box<HeapObject>>`, with destructor tracking via exhaustive `needs_drop` match. Package 3: `RegionEnter`/`RegionExit` scope bytecodes threaded through the full pipeline as no-ops. Package 4: `active_allocator` raw pointer plumbing for scope-bump routing, saved/restored on call/return and fiber yield/resume. Package 5: scope allocation wired to RegionEnter/RegionExit with conservative escape analysis stubs (all return false). Package 6: shared allocators for zero-copy fiber exchange -- parent-owned `SharedAllocator` with raw pointer propagation, effect-gated to skip non-yielding fibers. 870-line `fiber_heap.rs`, 178-line `shared_alloc.rs`.

---

## [#413](https://github.com/elle-lisp/elle/pull/413) — Break-with-value tests for while and each loops
[`5f43c269`](https://github.com/elle-lisp/elle/commit/5f43c269) · 2026-03-02 · `testing`

Adds 8 integration tests confirming that break-with-value already works correctly in while, each, and forever loops. No code changes needed.

---

## [#407](https://github.com/elle-lisp/elle/pull/407) — Cleanup pass: file renames, merges, stdlib consolidation
[`9cb82044`](https://github.com/elle-lisp/elle/commit/9cb82044) · 2026-03-02 · `refactor`

Extracts pipeline.rs inline tests to tests/integration/pipeline.rs (1,345 lines). Merges compiler/bytecode_debug.rs into compiler/bytecode.rs. Renames files to single-word lowercase convention: `file_io.rs` to `fileio.rs`, `module_loading.rs` to `modules.rs`, `type_check.rs` to `types.rs`, `syntax_parser.rs` to `syntax.rs`. Consolidates stdlib definitions into `stdlib.lisp`. Merges `debugging.rs` and `graph_def.rs` into `debug.rs`. Adds arena introspection via SIG_QUERY for heap memory tracking.

---

## [#406](https://github.com/elle-lisp/elle/pull/406) — Examples rewrite
[`bcc75e10`](https://github.com/elle-lisp/elle/commit/bcc75e10) · 2026-03-02 · `examples`

Rewrites the entire examples directory. String primitives now operate on Unicode grapheme clusters instead of codepoints (`(length "wave-emoji")` returns 1). `string/index` is renamed to `string/find` with substring search and optional offset. 17 old scattered example files are replaced with 12 cohesive ones: basics, functions, control, collections, errors, concurrency, coroutines, destructuring, meta, introspection, io, ffi, and processes. Adds an `error` prelude macro. Net: 3,349 lines added, 9,338 removed.

---

## [#405](https://github.com/elle-lisp/elle/pull/405) — Thread-local heap arena with mark/release for macro expansion
[`68100218`](https://github.com/elle-lisp/elle/commit/68100218) · 2026-03-01 · `memory`

Value is Copy (u64), and heap objects allocated via `Rc::into_raw` were never freed (the Rc refcount was never decremented). Each macro expansion leaked ~10.9 MB of temporary cons cells, syntax objects, bindings, and closures. Replaces `Rc::into_raw` in `alloc()` with a thread-local arena (`Vec<Box<HeapObject>>`) with `mark`/`release` scoping via `ArenaGuard` RAII in `expand_macro_call_inner`. `alloc_permanent()` retains the old path for NativeFn values. Known unsoundnesses are documented; the full design is in `docs/heap-arena-plan.md`.

---

## [#404](https://github.com/elle-lisp/elle/pull/404) — Fix from_value() dropping keys when converting structs and tables
[`7210510f`](https://github.com/elle-lisp/elle/commit/7210510f) · 2026-03-01 · `bugfix`

`Syntax::from_value()` used `flat_map(|(_, v)| ...)` which discarded the TableKey from each BTreeMap entry. Struct and table values converted back to Syntax lost all their keys. Adds `table_key_to_syntax()` helper and interleaves keys with values.

---

## [#402](https://github.com/elle-lisp/elle/pull/402) — CI: limit test threads to 2
[`e9e88213`](https://github.com/elle-lisp/elle/commit/e9e88213) · 2026-03-01 · `ci`

Three CI commits in quick succession trying to solve OOM on runners: first single-threaded, then reverted, then settled on 2 threads.

---

## [#401](https://github.com/elle-lisp/elle/pull/401) — Revert --test-threads=1
[`9d209e2f`](https://github.com/elle-lisp/elle/commit/9d209e2f) · 2026-03-01 · `ci`

---

## [#400](https://github.com/elle-lisp/elle/pull/400) — CI: --test-threads=1
[`0d3622b3`](https://github.com/elle-lisp/elle/commit/0d3622b3) · 2026-03-01 · `ci`

---

## [#399](https://github.com/elle-lisp/elle/pull/399) — Remove code coverage job from CI
[`dc9e37ff`](https://github.com/elle-lisp/elle/commit/dc9e37ff) · 2026-02-28 · `ci`

Removes the 23-line code coverage job.

---

## [#398](https://github.com/elle-lisp/elle/pull/398) — Add &opt, &keys, &named parameter markers
[`6a17a3ce`](https://github.com/elle-lisp/elle/commit/6a17a3ce) · 2026-03-01 · `language`

Introduces three new parameter markers: `&opt` for optional parameters with nil defaults (using Arity::Range), `&keys` for keyword arguments collected into an immutable struct, and `&named` for strict named parameters with unknown-key validation. Threads `VarargKind`, `num_required`, `num_params` through the full pipeline (HIR to LIR to Closure to VM). Extracts `Arity::for_lambda()` to replace 7 identical if/else chains. Duplicate keyword arguments now produce a runtime error.

---

## [#397](https://github.com/elle-lisp/elle/pull/397) — Accept bracket syntax in special forms; add case, if-let, when-let, forever
[`18a910d5`](https://github.com/elle-lisp/elle/commit/18a910d5) · 2026-03-01 · `language` `macros`

Allows `[...]` alongside `(...)` in structural positions: fn/lambda params, let/letrec bindings, cond clauses, match arms, and defmacro params. Mutable `@[...]` is still rejected. Adds four prelude macros: `case` (flat-pair equality dispatch), `if-let` (conditional binding), `when-let`, and `forever` (infinite loop via `(while true ...)`). Allows multiple body forms in `while` without `begin` wrapping.

---

## [#396](https://github.com/elle-lisp/elle/pull/396) — Add selkie plugin for SVG rendering
[`9048a057`](https://github.com/elle-lisp/elle/commit/9048a057) · 2026-02-28 · `plugins`

A new plugin wrapping selkie-rs for SVG document generation and manipulation.

---

## [#394](https://github.com/elle-lisp/elle/pull/394) — Allow fibers, closures, and externals as table keys
[`1b65a341`](https://github.com/elle-lisp/elle/commit/1b65a341) · 2026-02-28 · `value`

Consolidates value-to-table-key conversion into `TableKey::from_value()` and adds an `Identity(Value)` variant using reference equality for fibers, closures, and external objects.

---

## [#393](https://github.com/elle-lisp/elle/pull/393) — fn/flow, fn/graph, fn/save-graph: expose LIR CFGs from Elle
[`6f225cf3`](https://github.com/elle-lisp/elle/commit/6f225cf3) · 2026-03-01 · `introspection` `primitives`

Adds `fn/flow` for runtime LIR CFG introspection (returns block structs with instructions, terminators, and edges), `fn/graph` for DOT/Mermaid rendering, and `fn/save-graph` for file output. Also moves `set_symbol_table()` before `init_stdlib()` at all call sites so prelude macros using gensym work during stdlib initialization. Adds compilation cache design documents.

---

## [#392](https://github.com/elle-lisp/elle/pull/392) — Erlang-style process model example
[`f122a8a5`](https://github.com/elle-lisp/elle/commit/f122a8a5) · 2026-02-28 · `examples`

A 415-line example demonstrating an Erlang-style process model built entirely on top of the fiber scheduler protocol: mailboxes, selective receive, process linking, and supervisors.

---

## [#387](https://github.com/elle-lisp/elle/pull/387) — Replace path primitives with camino-based path/* API
[`6b78a392`](https://github.com/elle-lisp/elle/commit/6b78a392) · 2026-02-28 · `primitives` `refactor`

Replaces the old `file/*` path primitives with a `path/*` API backed by camino, path-clean, and pathdiff. Adds `src/path.rs` with property tests and migrates compiler internals from `std::path` to `crate::path`. Also adds `src/repl.rs` (extracted from main) and removes the dead `ExceptionInFinally` error variant.

---

## [#386](https://github.com/elle-lisp/elle/pull/386) — Docstrings: extract, thread, and query
[`30f206b1`](https://github.com/elle-lisp/elle/commit/30f206b1) · 2026-02-28 · `language`

Adds a `doc` field to Lambda, LirFunction, and Closure. The HIR analyzer extracts a leading string literal from lambda bodies as a docstring. `(doc name)` checks the closure's doc field before falling back to builtin docs. The LSP hover and completion providers use the new docstring field.

---

## [#383](https://github.com/elle-lisp/elle/pull/383) — Mermaid, sqlite, crypto plugins; updated plugin init protocol
[`c4fa3dfd`](https://github.com/elle-lisp/elle/commit/c4fa3dfd) · 2026-02-28 · `plugins`

Changes the plugin init protocol so `PluginInitFn` returns a `Value`, allowing plugins to return API structs from `import-file`. Adds mermaid (SVG diagram generation), sqlite (rusqlite with bundled SQLite), crypto (moved from core, adds all SHA-2 variants + HMAC), random (fastrand), sugiyama, dagre, and fdg graph layout plugins. Removes sha2/hmac dependencies from the core crate.

---

## [#381](https://github.com/elle-lisp/elle/pull/381) — JIT: skip unnecessary LocalCell allocations for non-captured let bindings
[`5c3fcd86`](https://github.com/elle-lisp/elle/commit/5c3fcd86) · 2026-02-28 · `jit` `performance`

Adds `cell_locals_mask` to `LirFunction` so the JIT can distinguish variables needing cell wrapping (captured or mutated) from those that can be stored directly as Cranelift variables. 3.2x speedup on N-Queens N=12 (4.4s to 1.38s), 30x reduction in kernel time. Adds memory allocation benchmarks using stats_alloc.

---

## [#379](https://github.com/elle-lisp/elle/pull/379) — JIT: inline integer fast paths and direct self-calls
[`9f86e18a`](https://github.com/elle-lisp/elle/commit/9f86e18a) · 2026-02-28 · `jit` `performance`

Adds a `src/jit/fastpath.rs` module with inlined integer fast paths for all binary and comparison operations and fully-inlined unary Not. Also adds direct self-calls for solo-compiled functions, bypassing the dispatch trampoline. 362 new JIT tests.

---

## [#378](https://github.com/elle-lisp/elle/pull/378) — Plugin system: dynamically-loaded Rust libraries
[`ff905ca3`](https://github.com/elle-lisp/elle/commit/ff905ca3) · 2026-02-28 · `plugins`

Adds a plugin mechanism where Rust cdylib crates register primitives into the Elle VM at runtime via `dlopen` + `elle_plugin_init`. Plugins work directly with `Value` (no C FFI marshalling). Adds `HeapObject::External` for opaque plugin-provided Rust objects. `import-file` is extended to handle `.so` files. Ships the first plugin: `plugins/regex/` wrapping the regex crate with 5 primitives.

---

## [#377](https://github.com/elle-lisp/elle/pull/377) — Add cookbook, data flow, failure triage, pipeline docs, test guide
[`c1243898`](https://github.com/elle-lisp/elle/commit/c1243898) · 2026-02-28 · `docs`

Adds docs/cookbook.md (647 lines), docs/pipeline.md (201 lines), and tests/AGENTS.md (335 lines with failure triage and blast radius documentation). Updates AGENTS.md files across 12 directories.

---

## [#376](https://github.com/elle-lisp/elle/pull/376) — Remove aws-sigv4-demo subdirectory
[`95727c4f`](https://github.com/elle-lisp/elle/commit/95727c4f) · 2026-02-28 · `cleanup`

Removes 2,889 lines of planning/implementation documents that were accidentally committed alongside the SigV4 demo.

---

## [#374](https://github.com/elle-lisp/elle/pull/374) — Remove dead module system stubs, add import function
[`2ab09033`](https://github.com/elle-lisp/elle/commit/2ab09033) · 2026-02-28 · `cleanup`

Removes the unused module system infrastructure that was never connected to runtime: HirKind::Module/Import/ModuleRef, the analyzer arms, LIR lowering arms, VM modules HashMap, init_*_module functions. Adds a working `import` function (eval + read-all + slurp + begin wrapper) and fixes coverage of the help/doc system.

---

## [#373](https://github.com/elle-lisp/elle/pull/373) — Bytes/blob types, crypto primitives, uri-encode, SigV4 demo
[`1b180a2c`](https://github.com/elle-lisp/elle/commit/1b180a2c) · 2026-02-28 · `value` `primitives`

Adds Bytes (immutable) and Blob (mutable) heap types with constructors, predicates, conversions, polymorphic get/length, hex encoding, and 20+ primitives. Adds crypto primitives (SHA-256, HMAC-SHA256 with RFC 4231 test vectors) and `uri-encode`. Includes an AWS SigV4 signing demo. Also implements NaN-box tag reassignment for single-comparison truthiness and short string optimization (SSO) for strings of 6 bytes or fewer. Migrates `each` to a prelude macro and makes `map` polymorphic.

---

## [#371](https://github.com/elle-lisp/elle/pull/371) — Splice special form
[`b9d96d07`](https://github.com/elle-lisp/elle/commit/b9d96d07) · 2026-02-27 · `language` `reader`

Two prerequisite changes: the comment character moves from `;` to `#`, and unquote-splicing changes from `,@` to `,;`. Then Janet-style compile-time splice is added: `(splice expr)` or `;expr` spreads array/tuple elements into function call arguments and data constructors. The implementation threads through the full pipeline: `CallArg { expr, spliced }` in HIR, `ArrayExtend`/`ArrayPush`/`CallArray`/`TailCallArray` instructions in LIR and bytecode, plus VM handlers for array building and array-based calls. 22 splice tests.

---

## [#370](https://github.com/elle-lisp/elle/pull/370) — Collection literal semantics, polymorphic primitives, buffer type
[`f4695dc8`](https://github.com/elle-lisp/elle/commit/f4695dc8) · 2026-02-27 · `language` `value`

A sweeping overhaul of collection semantics. `[...]` now produces immutable Tuples, `@[...]` mutable Arrays, `{...}` immutable Structs, `@{...}` mutable Tables. Each gets its own SyntaxKind, HirPattern, bytecode instructions (IsTuple, IsStruct, IsTable), type predicates (array?, tuple?, table?, struct?), and match pattern discrimination. `set!` is renamed to `set` (dropping the `!` suffix). `get` and `put` become polymorphic across all collection types. `push`/`pop`/`insert`/`remove`/`append`/`concat` are added as polymorphic operations. A new `buffer` type with `@"..."` literal syntax is introduced. 113 files changed, ~5,300 lines added.

---

## [#369](https://github.com/elle-lisp/elle/pull/369) — Strip debug info from release binary, add profiling profile
[`3e5d6e11`](https://github.com/elle-lisp/elle/commit/3e5d6e11) · 2026-02-26 · `build`

One-liner Cargo.toml change: strips debug info from release builds and adds a profiling profile.

---

## [#367](https://github.com/elle-lisp/elle/pull/367) — Fix: build binary before coverage
[`b85eb2e3`](https://github.com/elle-lisp/elle/commit/b85eb2e3) · 2026-02-26 · `ci`

Adds a `cargo build` step before coverage so integration tests find the binary.

---

## [#366](https://github.com/elle-lisp/elle/pull/366) — Merge elle-lint and elle-lsp into main binary
[`68fb199c`](https://github.com/elle-lisp/elle/commit/68fb199c) · 2026-02-26 · `refactor`

Absorbs the elle-lint and elle-lsp crates into the main binary as `src/lint/` and `src/lsp/` modules, dispatched via `--lint` and `--lsp` CLI switches. Deletes the old crate directories. The Cargo workspace shrinks to a single member. 1800+ tests.

---

## [#364](https://github.com/elle-lisp/elle/pull/364) — Optimize CI: skip redundant jobs, benchmark reporting, llvm-cov
[`8a744fe3`](https://github.com/elle-lisp/elle/commit/8a744fe3) · 2026-02-26 · `ci`

Restructures CI to skip redundant push jobs when a PR is open, adds benchmark reporting with criterion, switches code coverage from tarpaulin to llvm-cov, and adds a concurrency group to cancel stale CI runs.

---

## [#362](https://github.com/elle-lisp/elle/pull/362) — Tuple destructuring, types doc, lowercase docs
[`3593897c`](https://github.com/elle-lisp/elle/commit/3593897c) · 2026-02-26 · `bugfix` `docs`

Fixes tuple destructuring and adds a 437-line types.md document. Renames all docs/ files from UPPER_CASE.md to lowercase-with-hyphens.md.

---

## [#360](https://github.com/elle-lisp/elle/pull/360) — Migrate boolean literals from #t/#f to true/false
[`5bc8231f`](https://github.com/elle-lisp/elle/commit/5bc8231f) · 2026-02-26 · `language`

`true` and `false` are now recognized as keyword literals in the lexer alongside nil, producing `Token::Bool`. Display output changes from `#t`/`#f` to `true`/`false`. Legacy `#t`/`#f` still parse and normalize to the new canonical form. A legacy roundtrip property test verifies backward compatibility. Touches 72 files across Rust source, examples, tests, and docs.

---

## [#356](https://github.com/elle-lisp/elle/pull/356) — Improve "must be a list" errors with bracket hints
[`6ec5dd99`](https://github.com/elle-lisp/elle/commit/6ec5dd99) · 2026-02-25 · `ux`

Error messages for misplaced brackets now hint at the correct syntax (e.g., suggesting `[a b]` for binding positions instead of `(a b)`).

---

## [#355](https://github.com/elle-lisp/elle/pull/355) — Multi-function JIT compilation for mutually recursive call groups
[`1d498dc7`](https://github.com/elle-lisp/elle/commit/1d498dc7) · 2026-02-26 · `jit`

When a hot function calls other global functions, discovers the compilation group by scanning LIR for LoadGlobal + Call/TailCall patterns and resolving globals at runtime. Compiles all group members into a single Cranelift module with direct call instructions between them, eliminating `elle_jit_call` dispatch overhead for intra-group calls. Bounded by MAX_GROUP_SIZE=16 and MAX_DISCOVERY_DEPTH=4 to prevent unbounded BFS.

---

## [#354](https://github.com/elle-lisp/elle/pull/354) — Support break in while loops
[`215bd704`](https://github.com/elle-lisp/elle/commit/215bd704) · 2026-02-25 · `language`

Wraps while loops in an implicit named block (`:while`) so that `break` can target them. Unnamed `break` targets the innermost block, which is now the while loop's implicit block.

---

## [#353](https://github.com/elle-lisp/elle/pull/353) — Pass resume value as argument to fiber closure on first resume
[`5b387102`](https://github.com/elle-lisp/elle/commit/5b387102) · 2026-02-25 · `bugfix` `fibers`

`do_fiber_first_resume` was calling `build_closure_env` with an empty args slice, so fiber closures with parameters had an empty environment and `LoadUpvalue` panicked. Now the resume value from `fiber/resume` is passed as the closure's argument when the closure expects parameters.

---

## [#352](https://github.com/elle-lisp/elle/pull/352) — Serialize keyword keys and values in JSON
[`ffbb7f24`](https://github.com/elle-lisp/elle/commit/ffbb7f24) · 2026-02-25 · `bugfix` `json`

Keywords are now properly serialized in JSON output, both as keys (stripped of the colon prefix) and as values (quoted with colon).

---

## [#351](https://github.com/elle-lisp/elle/pull/351) — Managed FFI pointers prevent use-after-free and double-free
[`1740b52b`](https://github.com/elle-lisp/elle/commit/1740b52b) · 2026-02-25 · `ffi` `safety`

`ffi/malloc` now returns a heap-allocated `ManagedPointer` that tracks its freed state. `ffi/free` invalidates the pointer before calling libc free; double-free is caught and signals an error. `ffi/read`, `ffi/write`, `ffi/string` check pointer liveness before dereferencing. Raw CPointers from `ffi/call`, `ffi/lookup` etc. remain unmanaged (C-owned).

---

## [#350](https://github.com/elle-lisp/elle/pull/350) — string/slice returns nil for OOB indices
[`edfe02b1`](https://github.com/elle-lisp/elle/commit/edfe02b1) · 2026-02-25 · `bugfix`

Changes `string/slice` to return nil instead of signaling an error for out-of-bounds indices. Type/arity errors still signal. 4 property tests.

---

## [#345](https://github.com/elle-lisp/elle/pull/345) — Allow bare library names in dlopen
[`5c74f80d`](https://github.com/elle-lisp/elle/commit/5c74f80d) · 2026-02-25 · `bugfix` `ffi`

The `Path::exists()` pre-check was rejecting bare library names like "libm.so.6" that dlopen would resolve via the dynamic linker search path. Fixed to only check existence for paths containing '/'.

---

## [#344](https://github.com/elle-lisp/elle/pull/344) — Add FFI example
[`ea86390d`](https://github.com/elle-lisp/elle/commit/ea86390d) · 2026-02-25 · `examples`

A 55-line example exercising all layers of the C interop.

---

## [#343](https://github.com/elle-lisp/elle/pull/343) — Rebuild FFI on libffi
[`ccf2b040`](https://github.com/elle-lisp/elle/commit/ccf2b040) · 2026-02-25 · `ffi`

A complete FFI rebuild: guts the old implementation and replaces it with libffi-based calling. Adds `Value::pointer()` NaN-boxed variant for raw C pointers, a type descriptor system (TypeDesc, Signature, StructDesc), a marshaller, FFISignature heap type, 10 primitive functions (ffi/native, ffi/call, ffi/lookup, ffi/malloc, ffi/free, ffi/read, ffi/write, ffi/string, ffi/struct, ffi/array), variadic function support, and callback trampolines for passing Elle closures as C function pointers (demonstrated with qsort). The old 2,900-line FFI (split across handlers, marshal, safety, callbacks, and 6 test files) is replaced with a cleaner 3,100-line implementation. Adds ffi/defbind prelude macro.

---

## [#337](https://github.com/elle-lisp/elle/pull/337) — Add eval/read, split oversized files, expand test coverage
[`6ba6d9c8`](https://github.com/elle-lisp/elle/commit/6ba6d9c8) · 2026-02-25 · `language` `refactor` `testing`

Adds `eval` special form (HirKind::Eval, Instruction::Eval, VM handler with cached Expander) and `read`/`read-all` primitives for runtime string-to-datum parsing. Splits 5 more oversized files (string to convert, file_io to path, registration to docs, binding to destructure+lambda). Deduplicates `analyze_define`/`analyze_const` into a shared helper. Migrates all 22 test files to a shared `eval_source` helper. Adds ~290 new tests including property tests for eval, arithmetic, strings, effects, NaN-boxing, reader, and determinism.

---

## [#335](https://github.com/elle-lisp/elle/pull/335) — JIT-aware tail call resolution for mutual recursion
[`660f2689`](https://github.com/elle-lisp/elle/commit/660f2689) · 2026-02-24 · `jit` `performance`

When `elle_jit_tail_call` handles a non-self tail call, it now checks `vm.jit_cache` for the target closure before falling back to the interpreter trampoline. N-queens N=12 wall time drops from 12.6s to 11.1s (-12%), branch misses drop 27%.

---

## [#328](https://github.com/elle-lisp/elle/pull/328) — Prelude macro migration, named blocks, property tests
[`0d76d5bc`](https://github.com/elle-lisp/elle/commit/0d76d5bc) · 2026-02-24 · `language` `macros` `stdlib`

A large feature PR in several waves. Migrates `defn`, `let*`, `->`, `->>` from Rust desugaring to prelude macros (~140 lines of Rust replaced by ~25 lines of Elle). Adds named blocks with `break`, array pattern matching in `match` (IsArray/ArrayLen instructions), variadic macro parameters (`& rest`), prelude.lisp loaded automatically by the Expander, standard macros (when, unless, try/catch, protect, defer, with), `yield*` macro for bidirectional sub-coroutine delegation, a `vm/` prefix for introspection primitives, keyword table keys, version 1.0.0, `IsTable` bytecode instruction, unified doc system, and table destructuring property tests. Deletes the `trash/cranelift/` directory (15 abandoned JIT prototype files). 40 new property tests for macro equivalence, threading, block/break semantics, scope isolation, and hygiene.

---

## [#327](https://github.com/elle-lisp/elle/pull/327) — Array primitives and nqueens-array benchmark
[`68685d40`](https://github.com/elle-lisp/elle/commit/68685d40) · 2026-02-24 · `primitives`

Adds `array/push!`, `array/pop!`, `array/new` primitives. The array-based nqueens variant is 1.5x slower than the list version due to RefCell/dispatch overhead.

---

## [#326](https://github.com/elle-lisp/elle/pull/326) — Compiler-level destructuring with wildcard and rest patterns
[`f151270c`](https://github.com/elle-lisp/elle/commit/f151270c) · 2026-02-24 · `language` `compiler`

Replaces the `(def (f x) body)` shorthand with `(defn f (x) body)` to free up list-in-binding-position for destructuring. Adds compiler-level destructuring for `def`, `var`, `let`, `let*`, and `fn` parameters, supporting nested list/array/struct patterns, `_` wildcard, and `& rest` patterns. The `Arity` type becomes an enum (`Fixed(n) | AtLeast(n)`) and the VM's `build_closure_env` collects extra args into a list for the rest slot. 52 destructuring tests and 15 variadic function tests.

---

## [#325](https://github.com/elle-lisp/elle/pull/325) — Compile-time arity checking and declarative primitive registration
[`e4961ad1`](https://github.com/elle-lisp/elle/commit/e4961ad1) · 2026-02-24 · `compiler` `primitives`

Reworks primitive registration from ~1400 lines of imperative `register_fn` calls to declarative `pub const PRIMITIVES: &[PrimitiveDef]` tables (~50 lines of registration code). Each PrimitiveDef carries name, func, effect, arity, doc, params, category, example, and aliases. Threads `PrimitiveMeta` through the pipeline to the analyzer for compile-time arity checking: known arity mismatches produce hard compile errors. Adds bitwise primitives (bit/and, bit/or, etc.), REPL tools (doc, pp, describe), and slash-namespaced primitives (math/, string/, file/) with old names as aliases.

---

## [#324](https://github.com/elle-lisp/elle/pull/324) — Replace BindingId + HashMap with NaN-boxed Binding type
[`d995781d`](https://github.com/elle-lisp/elle/commit/d995781d) · 2026-02-23 · `compiler` `refactor`

Replaces the `BindingId(u32)` plus side-channel `HashMap<BindingId, BindingInfo>` system with a `Binding` newtype wrapping a NaN-boxed Value pointing to `HeapObject::Binding(RefCell<BindingInner>)`. Binding is Copy (8 bytes) and identity is bit-pattern equality. This unifies `HirKind::Define` and `HirKind::LocalDefine` into a single `Define`, removes `AnalysisContext`, `AnalysisResult.bindings`, `Lowerer.bindings`, and the entire `src/binding/` module (VarRef, ResolvedVar, Scope, ScopeStack).

---

## [#323](https://github.com/elle-lisp/elle/pull/323) — Rename vector to array
[`5414b996`](https://github.com/elle-lisp/elle/commit/5414b996) · 2026-02-23 · `api` `refactor`

Renames the mutable indexed collection type from "vector" to "array" to align with Janet. All user-facing primitives change: `vector` to `array`, `vector-ref` to `array-ref`, `vector-set!` to `array-set!`. 74 files changed across Rust source, Elle examples, tests, and docs.

---

## [#322](https://github.com/elle-lisp/elle/pull/322) — Rename binding forms: define to var, const to def
[`f76a8815`](https://github.com/elle-lisp/elle/commit/f76a8815) · 2026-02-23 · `language` `refactor`

Aligns with Janet/Clojure conventions where `def` is the immutable default and `var` is the explicit mutable form. Touches 92 files: all Rust form recognition, all .lisp files, all embedded Elle in tests and stdlib definitions, LSP completion/rename, and all documentation. 1845 tests pass. Also adds design docs for destructuring and scoping.

---

## [#321](https://github.com/elle-lisp/elle/pull/321) — Replace hash-based keywords with interned strings
[`43935f04`](https://github.com/elle-lisp/elle/commit/43935f04) · 2026-02-23 · `value` `refactor`

Keywords previously used a 48-bit FNV-1a hash as their NaN-box payload with a thread-local registry for display. Now they use interned string pointers (same mechanism as `Value::string()` but tagged with TAG_KEYWORD), making keywords self-describing and eliminating the keyword registry entirely. Net reduction of ~170 lines.

---

## [#319](https://github.com/elle-lisp/elle/pull/319) — Add const form for immutable bindings
[`dea7af1b`](https://github.com/elle-lisp/elle/commit/dea7af1b) · 2026-02-23 · `language`

`(const name value)` creates an immutable binding; `(set! name ...)` on a const is a compile-time error. For literal values (int, float, string, bool, nil, keyword), references emit `LoadConst` instead of `LoadGlobal` for zero runtime cost. Cross-form enforcement works: a const declared in form 1 cannot be set! in form 3. No new HirKind variants; immutability is a `BindingInfo` flag.

---

## [#318](https://github.com/elle-lisp/elle/pull/318) — Add (halt) primitive for graceful VM termination
[`df2d747f`](https://github.com/elle-lisp/elle/commit/df2d747f) · 2026-02-23 · `vm` `primitives`

Adds `SIG_HALT` (bit 8) and `(halt [value])` for terminating VM execution and returning a value to the host without killing the process. Maskable by fiber signal masks for sandboxing untrusted code, non-resumable (halted fiber is Dead), and non-suspending (JIT-compatible). At translation boundaries SIG_HALT maps to `Ok(value)`.

---

## [#317](https://github.com/elle-lisp/elle/pull/317) — Sets-of-scopes hygiene and syntax objects
[`309bce58`](https://github.com/elle-lisp/elle/commit/309bce58) · 2026-02-23 · `macros` `hygiene`

Implements sets-of-scopes macro hygiene. Macro arguments are wrapped as syntax objects carrying scope sets, and binding resolution filters by scope subset. `HeapObject::Syntax` variant, `SyntaxKind::SyntaxLiteral` for wrapping runtime values, `ScopedBinding` with scope-aware bind/lookup, and a fix for gensym (now returns symbols instead of strings, closing #306). Adds `datum->syntax` and `syntax->datum` primitives as hygiene escape hatches for anaphoric macros. 14 integration tests for hygiene, 11 unit tests for scope primitives.

---

## [#316](https://github.com/elle-lisp/elle/pull/316) — Remove dead code, fix stray tests, fix fiber errors at root level
[`33b5f7d1`](https://github.com/elle-lisp/elle/commit/33b5f7d1) · 2026-02-23 · `cleanup` `bugfix`

Removes dead `prim_is_coro`, `prim_div`, and the native-only `higher_order.rs` (superseded by Elle definitions of map/filter/fold). Moves 5 stray tests into a `cfg(test)` module. Fixes fiber/propagate, fiber/resume, and fiber/cancel at root level to produce clear errors instead of "Unexpected yield outside coroutine context".

---

## [#308](https://github.com/elle-lisp/elle/pull/308) — type-of returns :list for all list-like values
[`ea29676d`](https://github.com/elle-lisp/elle/commit/ea29676d) · 2026-02-23 · `bugfix`

`HeapObject::Cons::type_name()` returned "cons" while empty list returned "list". Now all list-like values consistently return `:list` from `type-of`, aligning with the `list?` predicate.

---

## [#307](https://github.com/elle-lisp/elle/pull/307) — Replace template-based macro expansion with VM evaluation
[`5a4b51fa`](https://github.com/elle-lisp/elle/commit/5a4b51fa) · 2026-02-23 · `macros` `compiler`

Macro bodies are now compiled and executed in the real VM during expansion via `pipeline::eval_syntax()`, replacing the template substitution system entirely. `substitute()` and `eval_quasiquote_to_syntax()` are deleted. All macros must use quasiquote to return code templates. The Expander now takes `&mut SymbolTable` and `&mut VM`. A recursion guard (MAX_MACRO_EXPANSION_DEPTH = 200) prevents runaway expansion. Pipeline function names drop the `_new` suffix. Also adds JANET-COMPILER.md (692 lines) and a merge strategy document.

---

## [#300](https://github.com/elle-lisp/elle/pull/300) — Show symbol name in undefined variable errors
[`21afa292`](https://github.com/elle-lisp/elle/commit/21afa292) · 2026-02-23 · `bugfix` `ux`

Undefined variable errors showed "symbol #208" instead of the actual name. Adds `resolve_symbol_name()` that looks up names via the thread-local SymbolTable, with a fallback to the numeric form.

---

## [#299](https://github.com/elle-lisp/elle/pull/299) — Caught SIG_ERROR leaves fiber Suspended, not Error
[`2973e263`](https://github.com/elle-lisp/elle/commit/2973e263) · 2026-02-23 · `bugfix` `fibers`

`with_child_fiber` was unconditionally setting `FiberStatus::Error` for SIG_ERROR before the caller checked the mask. When the mask caught the error, the fiber was already terminal and unresumable. Now the fiber gets provisional status and callers finalize: caught SIG_ERROR becomes Suspended (resumable), uncaught becomes Error (terminal). This intentionally diverges from Janet, where all non-yield signals are terminal; Elle treats terminality as a handler decision.

---

## [#298](https://github.com/elle-lisp/elle/pull/298) — Remove dead macro infrastructure
[`5d687ccf`](https://github.com/elle-lisp/elle/commit/5d687ccf) · 2026-02-23 · `cleanup`

Removes the parallel MacroDef type, gensym_id(), SymbolTable.macros field/methods, and the stub primitives/macros.rs (which always returned false/passthrough). The real macro implementations in the Expander are untouched; runtime `prim_gensym` is kept. Adds MACROS.md and MACROS_IMPL.md design documents.

---

## [#297](https://github.com/elle-lisp/elle/pull/297) — 8.9x fib(30) speedup
[`58ad5d85`](https://github.com/elle-lisp/elle/commit/58ad5d85) · 2026-02-23 · `performance` `jit`

Profile-driven optimization taking fib(30) from 285ms to 32ms, making Elle faster than Python (67ms) and Janet (64ms), competitive with Lua (50ms). JIT-to-JIT calls keep recursive calls in native code with zero-copy arg/env passing. Operator specialization compiles `+`, `-`, `<`, `*`, `/`, `=`, `>`, `<=`, `>=` to BinOp/Compare LIR instructions instead of LoadGlobal + Call. Eliminates `.to_vec()` copies in the JIT call path, reuses env Vec allocation via `env_cache` on the VM, switches to FxHashMap for pointer-keyed JIT cache, and fixes `UnaryOp::Neg` (was computing `src-0` instead of `0-src`). Also adds clock/time primitives and SIG_QUERY for VM introspection.

---

## [#295](https://github.com/elle-lisp/elle/pull/295) — Rename coroutine? to coro?
[`6a659b13`](https://github.com/elle-lisp/elle/commit/6a659b13) · 2026-02-22 · `api`

Renames the `coroutine?` predicate to `coro?` across primitives, docs, examples, and tests.

---

## [#293](https://github.com/elle-lisp/elle/pull/293) — Fiber/signal system (Steps 1-8)
[`a4e911bc`](https://github.com/elle-lisp/elle/commit/a4e911bc) · 2026-02-22 · `fibers` `vm`

The largest PR in this batch: ~11,000 lines added, ~10,000 removed, touching 140 files. Implements the fiber/signal system in 8 steps: (1) Fiber struct with stack, frames, status, signal mask, parent/child chain; (2) move execution state from VM to Fiber (stack, call frames, exception handlers, pending yield); (3) rewrite exception handling as fiber signal dispatch with try/catch/finally; (4) fiber/new, fiber/resume, fiber/status, fiber/value, fiber/signal, fiber/propagate, fiber/cancel primitives; (5) signal masking and handler dispatch; (6) coroutine compatibility layer mapping old APIs to fibers; (7) migrate JIT for fiber-based call frames; (8) delete the old Coroutine type and exception handler infrastructure. The old coroutines become syntactic sugar over fibers.

---

## [#292](https://github.com/elle-lisp/elle/pull/292) — FIBERS.md and EFFECTS.md: reviewed and hardened
[`d4d96cd4`](https://github.com/elle-lisp/elle/commit/d4d96cd4) · 2026-02-21 · `docs` `design`

Incorporates review feedback on the fiber/effect design documents. Structural fixes include: Frame holds `Rc<Closure>` + ip + base (not copies), parent pointer uses `Weak` to prevent Rc cycles, VM owns the current fiber directly (no `Rc<RefCell>` on the hot path), `Effect.propagates` becomes a u32 bitmask, signal mask lives on the child fiber, and signal bits are partitioned (0-2 user, 3-7 VM ops, 8-15 reserved, 16-31 user). Adds 876-line FIBERS.md covering the full fiber lifecycle.

---

## [#291](https://github.com/elle-lisp/elle/pull/291) — Debugging toolkit, effect unification, and design docs
[`34876e48`](https://github.com/elle-lisp/elle/commit/34876e48) · 2026-02-21 · `primitives` `effects` `docs`

Replaces the POSIX `clock_gettime`/libc debugging approach with Rust-native `std::time` backed by first-class `Instant` and `Duration` heap types. All debugging primitives are consolidated into `src/primitives/debugging.rs`. The `Effect` type is restructured from an enum to a struct with `yield_behavior` and `may_raise` fields, and every primitive now declares its full effect at registration time (eliminating the separate side-table in `effects/primitives.rs`). Adds `disbit`/`disjit` introspection primitives and two new design documents: EFFECTS.md (654 lines) and JANET.md (578 lines analyzing Janet's fiber architecture).

---

## [#290](https://github.com/elle-lisp/elle/pull/290) — Update README
[`3fdc7470`](https://github.com/elle-lisp/elle/commit/3fdc7470) · 2026-02-19 · `docs`

Adds 32 lines to README.md.

---

## [#289](https://github.com/elle-lisp/elle/pull/289) — JIT phase 4 overflow: optimizations and infrastructure
[`fb1e08da`](https://github.com/elle-lisp/elle/commit/fb1e08da) · 2026-02-19 · `jit` `vm`

Changes that did not land in the phase 4 PR: Vec-based globals with UNDEFINED sentinel replacing HashMap, SmallVec for handler stacks, pre-allocated closure environments, elimination of inline jumps from LIR (proper basic blocks for all control flow), locally-defined variable support in JIT, cross-form effect tracking with fixpoint iteration, Value::UNDEFINED sentinel, and a `LoadCapture` fix for auto-unwrapping `LocalCell` from the captures region so nested closures work under JIT.

---

## [#288](https://github.com/elle-lisp/elle/pull/288) — JIT phase 4: remove feature gate, TCO, exceptions
[`74074f90`](https://github.com/elle-lisp/elle/commit/74074f90) · 2026-02-19 · `jit`

Cranelift becomes a required dependency; all `#[cfg(feature = "jit")]` gates are removed. Self-recursive tail calls become native loops via jump to loop header, distinguished from non-self calls by a closure identity check (5th calling convention parameter). Cross-form effect tracking is added so a pure function defined in form 1 can be JIT-compiled when called in form 3. Recursive function effect inference seeds `effect_env` with Pure before analyzing. Exception propagation through JIT code works via `elle_jit_has_exception` checks after every Call instruction.

---

## [#287](https://github.com/elle-lisp/elle/pull/287) — JIT phase 3: full LirInstr coverage
[`bf3b9d30`](https://github.com/elle-lisp/elle/commit/bf3b9d30) · 2026-02-19 · `jit`

Replaces the globals parameter in the JIT calling convention with a VM pointer, enabling runtime helpers to access VM state. Adds support for function calls (Call, TailCall), data structures (Cons, Car, Cdr, MakeVector, IsPair), cells (MakeCell, LoadCell, StoreCell, StoreCapture), and globals (LoadGlobal, StoreGlobal). 12 new `extern "C"` dispatch helpers. Splits compiler.rs into compiler.rs + translate.rs and runtime.rs into runtime.rs + dispatch.rs.

---

## [#286](https://github.com/elle-lisp/elle/pull/286) — JIT phases 1-2: Cranelift scaffold and VM integration
[`54bbceb2`](https://github.com/elle-lisp/elle/commit/54bbceb2) · 2026-02-19 · `jit` `compiler`

Builds a new JIT compiler from scratch under `src/jit/`. Phase 1 creates the `LirFunction`-to-Cranelift-IR-to-native pipeline with 21 `extern "C"` runtime helpers for arithmetic, bitwise, comparison, cons, and type checks. Feature-gated behind `--features jit`. Phase 2 adds hot function detection (threshold of 10 calls for `Effect::Pure` closures), a `jit_cache` on the VM keyed by bytecode pointer, and graceful fallback to the interpreter on JIT failure. 52 new tests.

---

## [#285](https://github.com/elle-lisp/elle/pull/285) — Hammer time: fix bytecode instructions, split 5 critical files
[`fb5fd86a`](https://github.com/elle-lisp/elle/commit/fb5fd86a) · 2026-02-19 · `refactor` `cleanup`

A refactoring pass splitting five files that had grown past the ~1000-line convention: `hir/analyze.rs` into 5 files, `lir/lower.rs` into 6, `syntax/expand.rs` into 7, `value/repr.rs` into 5, and `vm/mod.rs` into 4. Also wires the new bitwise and remainder instructions to existing VM arithmetic handlers, renames a Common Lisp demo file to `.lisp.cl`, and regenerates stale Cargo.lock files. All 1,672 tests pass.

---

## [#284](https://github.com/elle-lisp/elle/pull/284) — Soundness fixes: default unknown callees to Yields, StoreCapture stack mismatch, define shorthand, list display
[`4ad4de21`](https://github.com/elle-lisp/elle/commit/4ad4de21) · 2026-02-18 · `bugfix` `effects`

Four fixes in one PR. Unknown local and global callee effects now conservatively default to Yields instead of Pure for soundness. The `StoreCapture` stack mismatch causing corruption on subsequent operations is fixed by emitting `Pop` after `StoreCapture` for let bindings. The `(define (f x) body)` shorthand is added to the expander. List display no longer shows `. ()` because the formatter now checks `is_empty_list()` as the list terminator. Effect tracking is extended with `Polymorphic(BTreeSet<usize>)` for multi-parameter cases. The nqueens demo now runs correctly (92 solutions for N=8).

---

## [#283](https://github.com/elle-lisp/elle/pull/283) — Interprocedural effect tracking and enforcement
[`2922bd12`](https://github.com/elle-lisp/elle/commit/2922bd12) · 2026-02-18 · `effects` `compiler`

Implements compile-time interprocedural effect tracking. The analyzer now maintains an `effect_env` (BindingId to Effect) for local bindings and a `primitive_effects` table for builtins. Call-site resolution looks up callee effects, propagates through nested calls, and resolves `Polymorphic(i)` effects by examining the argument at position i (enabling map/filter/fold to inherit the effect of their callback argument). The long-standing gap where `(f)` was marked Pure even when `f` was bound to a yielding lambda is closed. 22 integration tests cover the new analysis.

---

## [#282](https://github.com/elle-lisp/elle/pull/282) — Phase D: fix correctness bugs, clean docs, remove dead deps
[`b79c15b6`](https://github.com/elle-lisp/elle/commit/b79c15b6) · 2026-02-18 · `bugfix` `cleanup`

Fixes silent correctness bugs in the emitter: `BinOp::Rem` was emitting `Instruction::Div`, all bitwise operations were emitting `Instruction::Add`, and `UnaryOp::BitNot` was emitting logical `Not`. Adds proper `Rem`, `BitAnd`, `BitOr`, `BitXor`, `BitNot`, `Shl`, `Shr` to the `Instruction` enum with VM handlers and unit tests. Deletes 1,200+ lines of stale documentation and removes unused target-lexicon and bindgen dependencies.

---

## [#281](https://github.com/elle-lisp/elle/pull/281) — Phase C: restore macros, modules, yield-from
[`314a5bc7`](https://github.com/elle-lisp/elle/commit/314a5bc7) · 2026-02-18 · `macros` `compiler`

Restores features that were broken by the pipeline migration. Quasiquote macro templates now produce actual Syntax trees via `eval_quasiquote_to_syntax`. `macro?` and `expand-macro` are handled at expansion time in the Expander. Module-qualified names (`string:upcase`) are recognized by the lexer and resolved to flat primitive names. Yield-from coroutine delegation is implemented via a `delegate` field on the Coroutine struct. All 8 previously-ignored tests now pass; zero ignored tests remain.

---

## [#280](https://github.com/elle-lisp/elle/pull/280) — Phase B: delete JIT, migrate value types, implement LocationMap
[`8a75323b`](https://github.com/elle-lisp/elle/commit/8a75323b) · 2026-02-18 · `refactor` `cleanup`

A massive cleanup removing roughly 17,500 lines in four sub-phases. B.1 deletes the entire Cranelift JIT compiler and old Expr-based compilation pipeline (14 Cranelift files, old compiler, optimizer, pattern matcher, macro expander, effect inference, JIT primitives and benchmarks, plus the cranelift dependencies). B.2 migrates `value_old` into proper `value/` submodules (types, closure, coroutine, ffi) and eliminates the old-to-new value bridge. B.3 implements `LocationMap` for source location tracking: `SpannedInstr` wrappers in LIR, span propagation through the lowerer, and `LocationMap` in `Bytecode` used by `capture_stack_trace`. B.4 verifies thread transfer with 18 integration tests.

---

## [#279](https://github.com/elle-lisp/elle/pull/279) — Yield as a proper LIR terminator
[`eb7c3bc5`](https://github.com/elle-lisp/elle/commit/eb7c3bc5) · 2026-02-18 · `lir` `compiler`

Yield was an inline LIR instruction inside a single basic block, which misrepresented the control flow (it suspends and resumes in a new block). This PR makes it a `Terminator::Yield { value, resume_label }` that splits the block, with a `LoadResumeValue` instruction as the first instruction in the resume block. Stack simulation state is carried across the yield boundary via `yield_stack_state`. The bytecode output is functionally identical; only the LIR representation changes to enable future analysis and JIT compilation.

---

## [#278](https://github.com/elle-lisp/elle/pull/278) — Harden continuations: handler-case, O(1) append, edge cases
[`b5d842e8`](https://github.com/elle-lisp/elle/commit/b5d842e8) · 2026-02-18 · `vm` `continuations`

Fixes the interaction between handler-case and yield: exception handler state is now saved in `ContinuationFrame` and restored on resume, so yielding inside a handler-case body no longer loses the handler. Frame ordering changes from outermost-first to innermost-first, turning the O(n) `prepend_frame` into O(1) `append_frame`. Adds 10 new tests covering handler-case + yield, tail-position yields, and deep call chains.

---

## [#277](https://github.com/elle-lisp/elle/pull/277) — Delete the CPS interpreter
[`f7598cf8`](https://github.com/elle-lisp/elle/commit/f7598cf8) · 2026-02-18 · `cleanup` `vm`

Removes the entire CPS tree-walking interpreter: 13 files under `src/compiler/cps/`, roughly 4,400 lines. With first-class continuations in the bytecode VM, there is no longer a need for CpsExpr, CpsTransformer, CpsInterpreter, Trampoline, Action, or the CPS JIT bridge. The Coroutine struct drops its saved CPS state fields, and resume logic simplifies to a single bytecode path with no fallbacks.

---

## [#276](https://github.com/elle-lisp/elle/pull/276) — First-class continuations for yield across call boundaries
[`c1fc48dd`](https://github.com/elle-lisp/elle/commit/c1fc48dd) · 2026-02-18 · `vm` `continuations`

Introduces `Value::Continuation` to capture the full frame chain when a coroutine yields deep inside nested calls. A new `ContinuationFrame` type stores the return IP, base pointer, and saved stack slice. The VM's `Call` handler prepends the caller's frame to the chain, and `resume_continuation` replays the chain on resume. The previously-ignored `test_calling_yielding_function_propagates_effect` now passes.

---

## [#275](https://github.com/elle-lisp/elle/pull/275) — CPS rework Phase 0: property tests and effect threading
[`9b2ee292`](https://github.com/elle-lisp/elle/commit/9b2ee292) · 2026-02-18 · `coroutines` `testing`

Lays groundwork for unifying the coroutine execution model. Adds property-based coroutine tests (sequential yields, resume values, conditionals, loops, interleaving) and fixes effect propagation: `LirFunction` gains an `effect` field so closures that yield are no longer falsely marked pure. Tests requiring yield-across-call-boundaries are marked `#[ignore]` pending Phase 1.

---

## [#274](https://github.com/elle-lisp/elle/pull/274) — Update AGENTS.md for post-migration state
[`b6087d81`](https://github.com/elle-lisp/elle/commit/b6087d81) · 2026-02-18 · `docs`

Adds or refreshes AGENTS.md files for elle-lint, elle-lsp, compiler, effects, hir, lint, and symbols directories to reflect the HIR pipeline migration.

---

## [#273](https://github.com/elle-lisp/elle/pull/273) — Migrate elle-lint and elle-lsp to the new HIR pipeline
[`7b6d0213`](https://github.com/elle-lisp/elle/commit/7b6d0213) · 2026-02-18 · `refactor` `lint` `lsp`

The linter and language server were still consuming the old Value-to-Expr pipeline. This PR rewrites both to go through Syntax-to-HIR: a new `HirLinter` in `src/hir/lint.rs`, a new `extract_symbols_from_hir` in `src/hir/symbols.rs`, and `analyze_new`/`analyze_all_new` pipeline entry points that stop before bytecode generation. Diagnostic and SymbolIndex types move to pipeline-agnostic locations under `src/lint/` and `src/symbols/`. The old context, handler, and protocol files in the elle-lint and elle-lsp crates are deleted, along with the tokio dependency from elle-lsp. Old compiler re-exports are kept as shims.

---

## [#272](https://github.com/elle-lisp/elle/pull/272) — New pipeline: TCO, let semantics, property tests, old pipeline removal
[`ec3b8c91`](https://github.com/elle-lisp/elle/commit/ec3b8c91) · 2026-02-18 · `compiler` `testing` `refactor`

The final commit in this origin batch completes the new pipeline and removes the old one.

**Tail call optimization**: A new `src/hir/tailcall.rs` (462 lines) implements a tail-call marking pass over HIR. The new pipeline now handles 50,000+ recursion depth for self-recursion, accumulator patterns, and mutual recursion.

**Let semantics fix**: `let` gets proper parallel binding semantics (Scheme standard) -- all init expressions are evaluated in the outer scope before any bindings take effect. `let*` retains sequential semantics. Also fixes let/letrec inside closures corrupting the caller's stack: bindings were using `StoreLocal`/`LoadLocal` (stack-based) instead of `StoreCapture`/`LoadCapture` (environment-based).

**Old pipeline removal**: Deletes `src/resident_compiler/` (4 files), and removes 30+ old integration test files that tested against the old compilation path (catchable_exceptions, closure_capture_optimization, deep_tail_recursion, handler_case, jit_integration, let_semantics, macros, etc.). Replaces them with `pipeline_property.rs` (603 lines) and `pipeline_point.rs` (516 lines) targeting the new pipeline. Effects module moves from `src/compiler/effects/` to `src/effects/`. `ScopeType` moves to `vm/scope/`. Net: +4,152 / -12,549 lines.

---

## [#271](https://github.com/elle-lisp/elle/pull/271) — AGENTS.md documentation for LLM agents
[`8a94c0b3`](https://github.com/elle-lisp/elle/commit/8a94c0b3) · 2026-02-16 · `docs`

Establishes a dual-audience documentation pattern across the codebase: `AGENTS.md` files provide terse technical reference for LLM agents (module boundaries, invariants, dependencies), `README.md` files provide human-oriented documentation with rationale. Coverage: root project, binding, error, compiler, hir, lir, vm, reader, syntax, primitives, ffi, formatter, resident_compiler. 25 files, 2,130 lines.

---

## [#269](https://github.com/elle-lisp/elle/pull/269) — NaN-boxing Value type migration
[`083df9d9`](https://github.com/elle-lisp/elle/commit/083df9d9) · 2026-02-17 · `value` `architecture`

The single largest PR in this batch: 197 files changed, +25,487 / -10,214 lines. Migrates `Value` from a 24-byte Rust enum to an 8-byte NaN-boxed representation using IEEE 754 quiet NaN tagging. Value is now `Copy` (no more `.clone()` everywhere). Integers are 48-bit signed (plus/minus 140 trillion range). Strings are interned for O(1) equality. Nil and empty-list become distinct values; only nil and `#f` are falsy (0, empty string, empty list, empty vector are truthy). Lists terminate with `'()`, not nil.

The new `src/value/` module is split into `repr.rs` (1127 lines -- the NaN-boxing core), `heap.rs` (heap-allocated objects), `intern.rs` (string interning), `closure.rs`, `condition.rs`, `display.rs`, `send.rs` (deep-copy for thread boundaries). The old `value.rs` is preserved as `value_old/mod.rs`.

Alongside the value migration, this PR completes a comprehensive exception system rework (documented in `docs/EXCEPT.md`): `Condition` gets mandatory messages, named constructors (`type_error`, `division_by_zero`, etc.), and a hierarchy table. `NativeFn` changes from `LResult<Value>` to `Result<Value, Condition>`, and all ~183 primitives are migrated. VM instruction handlers are updated so all data errors go through `current_exception` (the catchable channel), reserving `Err(String)` exclusively for VM bugs (stack underflow, etc.). The CPS interpreter and trampoline are updated accordingly.

The migration required fixing three coroutine bugs (exception channel routing, stack isolation in `execute_bytecode_coroutine`, and Cell/LocalCell distinction in NaN-boxed values), and a concurrency fix (bytecode now carries its own symbol table via `symbol_names` so `spawn` can remap globals on the spawned thread). All integration tests are migrated to the new pipeline. The old `trash/` directory of dead Cranelift code is finally deleted.

---

## [#268](https://github.com/elle-lisp/elle/pull/268) — New compilation pipeline (Syntax -> HIR -> LIR -> Bytecode)
[`4157b508`](https://github.com/elle-lisp/elle/commit/4157b508) · 2026-02-16 · `compiler` `architecture`

The architectural centerpiece of early Elle: a completely new compilation pipeline that will eventually replace the direct Value-to-bytecode path.

**Syntax layer** (`src/syntax/`): `Syntax` and `SyntaxKind` types for pre-analysis AST, `ScopeId` for hygienic macro expansion, `Span` for source location tracking. The `Expander` handles macro definition, template substitution, and threading macros (`->`, `->>`).

**HIR** (`src/hir/`): The analyzed intermediate representation with `BindingId` for compile-time binding resolution, `CaptureInfo` for closure analysis, `Effect` tracking on every expression, and `HirPattern` for pattern matching. The `Analyzer` converts `Syntax` to `HIR`, resolving all variable references to binding IDs and computing captures.

**LIR** (`src/lir/`): Low-level intermediate representation with register-like slots. The `Lowerer` converts HIR to LIR instructions (266 lines of type definitions), handling lambda lowering, closure environment layout, cell boxing for mutable captures, and the `LoadCaptureRaw` instruction for preserving cells.

**Emitter** (`src/lir/emit.rs`, 789 lines): Converts LIR to the existing bytecode format. The `pipeline.rs` module orchestrates the full chain.

The PR includes a brutal debugging log: 20+ incremental fix commits addressing stack pollution in Begin/Block, closure factory independence (closures from the same factory sharing state), mutable parameter cell boxing, `set!` not returning its value, `define` inside `fold` lambdas using stack-based locals instead of environment-based ones, and deeply nested lambda local variable capture. Ships with 57 property-based tests (proptest) and 20/20 examples passing. The old pipeline is not removed -- both coexist.

---

## [#267](https://github.com/elle-lisp/elle/pull/267) — Unified error system (LError)
[`52ebf846`](https://github.com/elle-lisp/elle/commit/52ebf846) · 2026-02-15 · `errors` `architecture`

Replaces pervasive `Result<T, String>` with `Result<T, LError>` across the entire codebase. `LError` is a struct with `ErrorKind` enum, source location, and deferred trace capture via `TraceSource`. The `StackFrame` struct carries function names and source locations. `capture_stack_trace()` on the VM collects call frames; `wrap_error()` appends formatted stack traces at 16 error points.

The migration touches ~100+ primitive functions via updated `NativeFn` and `VmAwareFn` type aliases, all `Value::as_*` methods, VM handlers, compiler errors, and FFI wrappers. Also introduces `reader/syntax_parser.rs` (706 lines) as part of the new compilation pipeline, and fully modularizes the FFI primitives (callbacks, calling, enums, handlers, library, memory, types, unions -- each in its own file under `src/ffi/primitives/`). 49 files changed, +4,534 / -641 lines.

---

## [#265](https://github.com/elle-lisp/elle/pull/265) — Examples overhaul: 47 to 23 files, 1000+ assertions
[`b1eb76bb`](https://github.com/elle-lisp/elle/commit/b1eb76bb) · 2026-02-14 · `examples` `testing` `language`

Consolidates 47 examples down to 23 focused, assertion-heavy executable specifications. Exception handling goes from 5 files to 2, recursion integrates into closures, types unify (atoms + type-checking + mutable-storage), control flow unifies (loops, conditionals, pattern matching). All examples import from a centralized `assertions.lisp`, demonstrating the module system.

Beyond the examples, this PR introduces new language features: `(forever)` infinite loop form, `(each)` loop form (renamed from `for`), `box?` predicate (renamed from `cell?`). Fixes `import-file` to properly export definitions by using the caller's symbol table and implementing multi-form reading. Adds 2000+ lines of documentation (BUILTINS.md) and ~4000 lines of new integration tests across 12 test modules. Net: +17,421 / -1,634 lines.

---

## [#264](https://github.com/elle-lisp/elle/pull/264) — Proper lexical scope with compile-time capture resolution
[`950c45a8`](https://github.com/elle-lisp/elle/commit/950c45a8) · 2026-02-14 · `closures` `architecture`

A major refactoring that replaces Elle's scope system with proper lexical scope resolved entirely at compile time. The new `src/binding/` module introduces `VarRef` (an enum with `Local`, `LetBound`, `Upvalue`, `Global` variants), `Scope` and `ScopeStack` for compile-time binding tracking, and `CaptureInfo` that records both the symbol and the source `VarRef` for each capture.

The AST changes are significant: `Expr::Var(sym, depth, index)` becomes `Expr::Var(VarRef)`, `Expr::GlobalVar` is absorbed into `VarRef::Global`, `Expr::Set` uses `target: VarRef`, and `Expr::Lambda` gains `num_captures` and `num_locals`. The converter's `capture_resolution.rs` becomes a no-op (analysis happens at parse time), and `analysis.rs` is heavily simplified.

The path to green was rocky -- the PR log shows 12 incremental fix commits addressing nested capture bugs, mutable capture support, sorted-locals index mismatch, double adjustment of nested lambda body indices, and dead capture elimination not checking `set!` targets. 26 unused Cranelift files from #152 are moved to `trash/`. All 2,246 tests pass when done. Ships with 538 lines of lexical scope integration tests.

---

## [#263](https://github.com/elle-lisp/elle/pull/263) — Handle quoted symbols in yield correctly
[`97647f3c`](https://github.com/elle-lisp/elle/commit/97647f3c) · 2026-02-13 · `bugfix` `coroutines`

Root cause: `handle_load_upvalue` was looking up symbol values as global variables instead of treating them as literal values. Fixed by simplifying the match: `LocalCell` gets auto-unwrapped, everything else (including `Symbol`) is pushed as-is. Also adds `symbol->string` primitive, `list?` predicate, `eq?` alias for `=`, and improved symbol display (shows name instead of `Symbol(id)`). Rewrites `examples/coroutines.lisp` with 12 comprehensive test sections now that issues #258-260 are all resolved.

---

## [#262](https://github.com/elle-lisp/elle/pull/262) — Fix coroutine state stuck as Running on CPS error
[`8878be68`](https://github.com/elle-lisp/elle/commit/8878be68) · 2026-02-13 · `bugfix` `coroutines`

When `interpreter.eval()` failed during CPS execution, the `?` operator returned early without transitioning the coroutine from `Running` to `Error`, causing subsequent resume attempts to report "Coroutine is already running." Fixed by catching the error, popping the coroutine from the VM stack, and setting state to `Error` before returning. Seven regression tests.

---

## [#261](https://github.com/elle-lisp/elle/pull/261) — Fix captured variables after yield/resume
[`9ea7e968`](https://github.com/elle-lisp/elle/commit/9ea7e968) · 2026-02-13 · `bugfix` `coroutines`

The CPS interpreter required `depth=0` to find variables, but in CPS execution the environment is a flat array where depth is a bytecode compiler artifact. Fixed by removing the depth check in both `eval()` and `eval_pure_expr()` for `CpsExpr::Var` and `Expr::Var`.

---

## [#257](https://github.com/elle-lisp/elle/pull/257) — Unified error handling with full exception integration
[`86a5152d`](https://github.com/elle-lisp/elle/commit/86a5152d) · 2026-02-13 · `errors` `vm`

Makes all runtime errors into catchable exceptions. The `Condition` struct gains a `location` field and a `format_runtime_error()` function that resolves `SymbolId` to symbol names for readable error messages. Undefined-variable errors become exception ID 5, arity errors become exception ID 6 -- both catchable via `handler-case`. The VM gets `current_source_loc` tracking for attaching location info to exceptions. Temporarily disables several coroutine example tests pending bug fixes for issues #258-260.

---

## [#256](https://github.com/elle-lisp/elle/pull/256) — Stateless CPS interpreter with index-based locals
[`e47057d2`](https://github.com/elle-lisp/elle/commit/e47057d2) · 2026-02-13 · `coroutines` `refactor`

Replaces symbol-based variable lookup in the CPS interpreter with index-based locals. `CpsTransformer` now maintains a `local_indices` HashMap mapping symbols to integer indices; `CpsExpr::Let` and `CpsExpr::For` drop their `var` field in favor of indices. Lambdas track `num_locals`. This fixes four coroutine bugs (let bindings across yield, nested yielding calls, lambdas inside coroutines, recursive yielding) by switching from mutable `Environment` objects to flat `RefCell<Vec<Value>>` arrays with proper continuation-passing protocol: parent passes `(return_cont, return_env)` to child, child calls `return_cont` when complete.

---

## [#249](https://github.com/elle-lisp/elle/pull/249) — Complete coroutine implementation
[`ae58672f`](https://github.com/elle-lisp/elle/commit/ae58672f) · 2026-02-12 · `coroutines` `vm`

A large PR that takes coroutines from "API exists" to "everything works." Six incremental fixes land in a single merge:

1. **Yield instruction and VM infrastructure**: `CoroutineContext`/`CoroutineCallFrame` structs, `VmResult` enum (`Done`/`Yielded`), coroutine stack on the VM, `resume_from_context`. Changes `Value::Coroutine` to `Rc<RefCell<Coroutine>>` for interior mutability.

2. **Free variable analysis for yield**: `analyze_free_vars()` was not handling `Expr::Yield`, causing closures containing yield to have empty environments.

3. **Local variable allocation**: Coroutine execution bypasses the `Call` handler that normally allocates cells for locally-defined variables. Fixed by pre-allocating cells at coroutine creation and resume.

4. **Yield propagation through calls**: When a function called from within a coroutine yields, the yield must propagate up. Fixed by checking `in_coroutine()` in the `Call` handler and using `execute_bytecode_coroutine()`.

5. **CPS interpreter** (870 lines): Full CPS-based execution engine for coroutine bodies that handles yield/resume with proper continuation capture.

Ships with a 682-line `examples/coroutines.lisp` and 872 lines of integration tests covering 33 scenarios.

---

## [#248](https://github.com/elle-lisp/elle/pull/248) — Skip REPL print for non-terminal output
[`769d3bf9`](https://github.com/elle-lisp/elle/commit/769d3bf9) · 2026-02-12 · `repl`

Four-line fix: suppresses the REPL's print phase when stdout is not a terminal, so piped output contains only explicit `println` results.

---

## [#247](https://github.com/elle-lisp/elle/pull/247) — Add (exit) builtin
[`dce369b0`](https://github.com/elle-lisp/elle/commit/dce369b0) · 2026-02-12 · `language`

`(exit)` and `(exit code)` for process termination. 70 lines in `process.rs`.

---

## [#244](https://github.com/elle-lisp/elle/pull/244) — Coroutine API (Phase 6)
[`47b16fd8`](https://github.com/elle-lisp/elle/commit/47b16fd8) · 2026-02-12 · `coroutines` `language`

User-facing coroutine primitives: `make-coroutine`, `coroutine-resume` (with optional value), `coroutine-status`, `yield`, `yield-from` (delegation to sub-coroutine), `coroutine->iterator`, `coroutine-next`. Introduces `VmAwareFn` -- a variant of `NativeFn` that receives a mutable reference to the VM, needed because `coroutine-resume` must execute bytecode on the caller's VM. Also adds `coroutine?` and `coroutine-done?` predicates.

---

## [#243](https://github.com/elle-lisp/elle/pull/243) — JIT integration for CPS coroutines (Phase 5)
[`fb529cbd`](https://github.com/elle-lisp/elle/commit/fb529cbd) · 2026-02-12 · `jit` `coroutines`

Routes pure functions to existing native compilation and CPS functions to a new `CpsJitCompiler`. Defines `JitAction` (48-byte C-compatible struct) and `ActionTag` for CPS return values. Adds runtime helpers: `jit_call_cps_function` (trampoline for CPS execution), `jit_resume_coroutine`, `jit_is_suspended_coroutine`. A `ContinuationPool` provides thread-local pooling for `Done` continuations to reduce allocation overhead.

---

## [#242](https://github.com/elle-lisp/elle/pull/242) — Selective CPS transform (Phase 4)
[`fae08512`](https://github.com/elle-lisp/elle/commit/fae08512) · 2026-02-12 · `coroutines`

The key design decision: only transform functions that need it. Yield expressions become `CpsExpr::Yield`, calls to yielding functions use `CpsExpr::CpsCall` (continuation passed for resumption), but calls to pure functions use `CpsExpr::PureCall` (no CPS overhead). Let/begin/if/while/for with yield in subexpressions get appropriate transformations. Higher-order functions are specialized at call sites based on argument effects. Pure code runs natively; only yielding code paths pay the transformation cost.

---

## [#241](https://github.com/elle-lisp/elle/pull/241) — CPS infrastructure for colorless coroutines (Phase 3)
[`eac7d44f`](https://github.com/elle-lisp/elle/commit/eac7d44f) · 2026-02-12 · `coroutines` `architecture`

Builds the runtime machinery for coroutines under `src/compiler/cps/`: `Continuation` type with `Done`, `Sequence`, `IfBranch`, `LetBinding`, `CallReturn`, `Apply` variants; `Action` enum (`Return`, `Yield`, `Call`, `TailCall`, `Done`, `Error`); a trampoline executor with configurable step limits; and a continuation arena for batch allocation. Adds `Expr::Yield` to the AST, `yield` keyword parsing, and `Yield` bytecode instruction. Also adds `Coroutine` value type with state tracking and `make-coroutine`/`coroutine-status`/`coroutine-done`/`coroutine-value` primitives.

---

## [#240](https://github.com/elle-lisp/elle/pull/240) — Effect system for colorless coroutines (Phase 2)
[`09df972d`](https://github.com/elle-lisp/elle/commit/09df972d) · 2026-02-12 · `coroutines` `architecture`

Introduces the `Effect` enum (`Pure`, `Yields`, `Polymorphic(n)`) and an effect inference engine. Every closure and expression gets an effect annotation: the compiler tracks which functions may yield so it can later apply CPS transformation only where needed. Higher-order functions like `map`, `filter`, `fold` get `Polymorphic(n)` effects meaning they inherit the effect of their nth argument. Ships as `src/compiler/effects/` (inference.rs, primitives.rs, mod.rs) with ~840 lines.

---

## [#239](https://github.com/elle-lisp/elle/pull/239) — Native tail calls for self-recursive JIT functions
[`badc1462`](https://github.com/elle-lisp/elle/commit/badc1462) · 2026-02-12 · `jit` `performance`

Self-recursive tail calls in JIT-compiled functions now compile to jump instructions (constant stack space) instead of call instructions. The implementation uses entry/body block separation: the entry block loads arguments and jumps to the body block, which has block parameters. Self-recursive tail calls jump back to the body block with new arguments. Also adds `return_call_indirect` for tail calls to primitives and `jit_tail_call_closure`/`jit_load_global` runtime helpers for tail calls to other closures, enabling mutual recursion between globally-defined JIT functions.

---

## [#238](https://github.com/elle-lisp/elle/pull/238) — Primitives as JIT targets + proper let/let* semantics
[`497d5321`](https://github.com/elle-lisp/elle/commit/497d5321) · 2026-02-12 · `jit` `language`

Two significant changes bundled together. First, a `PrimitiveRegistry` (244 lines) that maps primitive names to Cranelift-callable function pointers, allowing JIT-compiled code to call primitives like `+`, `<`, `empty?` directly. Second, `let` and `let*` get proper Lisp semantics: `let` now produces `Expr::Let` directly (not lambda+call), binding expressions are evaluated in the outer scope, and `let*` produces nested `Expr::Let` forms where each binding sees previous ones. Adds `ScopeType` enum to distinguish lambda scopes from let scopes, fixing closure capture of let-bound variables. 17 new let semantics tests.

---

## [#235](https://github.com/elle-lisp/elle/pull/235) — Wire up jit-compile to actually compile closures
[`0921e8ab`](https://github.com/elle-lisp/elle/commit/0921e8ab) · 2026-02-11 · `bugfix` `jit`

The `jit-compile` primitive was a stub that returned the original closure uncompiled. This commit adds thread-local JIT context and symbol table storage, wires `prim_jit_compile` to invoke Cranelift, fixes comparison operators (i8 boolean results extended to i64), fixes if-expression block parameter setup, and handles `GlobalVar` in addition to `Literal(Symbol)` for operator lookup. The JIT now successfully compiles simple numeric functions. `jit-stats` returns real compilation statistics.

---

## [#233](https://github.com/elle-lisp/elle/pull/233) — Peephole optimization: length-zero to empty?
[`b7b3d94b`](https://github.com/elle-lisp/elle/commit/b7b3d94b) · 2026-02-11 · `optimization`

Adds `src/compiler/optimize.rs` (287 lines) that transforms `(= (length x) 0)` to `(empty? x)` in the AST, applied recursively through conditionals, function bodies, let bindings, loops, and boolean expressions. Only zero comparisons are optimized. ~10% improvement on recursive list-heavy code.

---

## [#232](https://github.com/elle-lisp/elle/pull/232) — Table/Struct API naming consistency
[`47a44284`](https://github.com/elle-lisp/elle/commit/47a44284) · 2026-02-11 · `language` `api`

Adds polymorphic `get`, `keys`, `values`, `has-key?` for both tables and structs. Adds mutation markers `put!`/`del!` as aliases. Adds Scheme-style conversion functions: `string->int`, `string->float`, `any->string`. 50 new tests, full backward compatibility.

---

## [#231](https://github.com/elle-lisp/elle/pull/231) — Universal length primitive
[`1e7ff722`](https://github.com/elle-lisp/elle/commit/1e7ff722) · 2026-02-11 · `language`

Extends `length` to work on all container types: lists, vectors, strings, keywords, tables, and structs. Previously it only handled lists.

---

## [#230](https://github.com/elle-lisp/elle/pull/230) — Rename lambda to fn (with backward compatibility)
[`e3047332`](https://github.com/elle-lisp/elle/commit/e3047332) · 2026-02-11 · `language` `breaking`

The compiler now recognizes both `fn` and `lambda`, with `fn` as the primary keyword. All documentation, examples, and tests switch to `fn`. `lambda` remains as a permanent alias. Ships three new documentation guides (CONTROL_FLOW.md, LANGUAGE_GUIDE.md, WHAT_S_NEW.md) totaling 2000+ lines, plus new examples for exception handling, mutual recursion, and threading operators.

---

## [#229](https://github.com/elle-lisp/elle/pull/229) — jit-stats primitive
[`961046bc`](https://github.com/elle-lisp/elle/commit/961046bc) · 2026-02-11 · `jit` `language`

Returns a struct with JIT statistics fields (compiled-functions, cache-hits, hot-closures, etc.). Currently returns placeholder zeroes; full tracking requires deeper VM integration.

---

## [#228](https://github.com/elle-lisp/elle/pull/228) — LSP: don't respond to notifications
[`2818f4c5`](https://github.com/elle-lisp/elle/commit/2818f4c5) · 2026-02-11 · `bugfix` `lsp`

Completes the notification fix from #202: now checks for the `id` field to distinguish requests from notifications, sending responses only for requests. Three spec compliance tests added.

---

## [#227](https://github.com/elle-lisp/elle/pull/227) — Support spawning JitClosure values
[`ac0f5341`](https://github.com/elle-lisp/elle/commit/ac0f5341) · 2026-02-11 · `jit` `concurrency`

`spawn` now accepts `JitClosure` values by falling back to their source closure for the spawned thread. Extracts `spawn_closure_impl` helper to reduce code duplication between `Closure` and `JitClosure` paths.

---

## [#226](https://github.com/elle-lisp/elle/pull/226) — Cranelift codegen for jit-compile: real native code
[`58736705`](https://github.com/elle-lisp/elle/commit/58736705) · 2026-02-11 · `jit`

Implements `compile_lambda_body` to generate actual native code: creates a function with signature `fn(args_ptr: i64, args_len: i64, env_ptr: i64) -> i64`, binds captured variables and parameters from passed arrays, compiles the lambda body, and updates the VM's `Call`/`TailCall` instructions to dispatch to native code when available.

---

## [#224](https://github.com/elle-lisp/elle/pull/224) — jit-compile primitive for on-demand JIT compilation
[`b4385f21`](https://github.com/elle-lisp/elle/commit/b4385f21) · 2026-02-11 · `jit` `language`

Adds the `jit-compile` primitive that accepts a closure and returns a `JitClosure` backed by native code via Cranelift, with graceful fallback to interpreted execution for unsupported constructs. Introduces `Value::JitClosure` type and a `closure.rs` module in the VM. Ships with 623 closure/lambda unit tests.

---

## [#220](https://github.com/elle-lisp/elle/pull/220) — Remove perf.data from repo
[`081ba202`](https://github.com/elle-lisp/elle/commit/081ba202) · 2026-02-11 · `chore`

Adds `perf.data*` to `.gitignore`.

---

## [#219](https://github.com/elle-lisp/elle/pull/219) — expand-macro and macro? primitives
[`abb3f14e`](https://github.com/elle-lisp/elle/commit/abb3f14e) · 2026-02-11 · `macros` `language`

Runtime macro introspection: `macro?` checks if a symbol is a macro, `expand-macro` returns the expanded form. Uses thread-local symbol table context (same pattern as FFI) for access to macro definitions. Macros still expand at compile time; these are debugging/introspection tools only.

---

## [#218](https://github.com/elle-lisp/elle/pull/218) — JIT: runtime-computed iterables in for loops
[`1213efc8`](https://github.com/elle-lisp/elle/commit/1213efc8) · 2026-02-11 · `jit`

Adds a `runtime_helpers` module with `jit_car`, `jit_cdr`, `jit_is_nil` C-callable helper functions registered with the Cranelift JIT builder. For loops over non-literal iterables (variables, function calls) now compile to native loops that call these helpers for list traversal. Literal cons lists still use the faster unrolling path from #167.

---

## [#217](https://github.com/elle-lisp/elle/pull/217) — JIT: float variable support in stack storage
[`46460a02`](https://github.com/elle-lisp/elle/commit/46460a02) · 2026-02-11 · `jit`

Adds `SlotType` enum (I64/F64) to the stack allocator so float values get correct load/store instructions. Extends let bindings and for loops to handle float types, with type conversion support in `set!` operations.

---

## [#214](https://github.com/elle-lisp/elle/pull/214) — JIT inline variable storage with stack allocation
[`6dafe1cf`](https://github.com/elle-lisp/elle/commit/6dafe1cf) · 2026-02-11 · `jit`

Adds `CompileContext` to centralize compilation state. Implements `compile_var()` (stack load), `compile_set()` (stack store), and proper variable binding in `try_compile_let()` and `try_compile_for()`. JIT-compiled code can now reference and mutate variables through stack slots with proper scoping and shadowing.

---

## [#213](https://github.com/elle-lisp/elle/pull/213) — Trampoline-based TCO eliminates stack overflow
[`d99ca3cf`](https://github.com/elle-lisp/elle/commit/d99ca3cf) · 2026-02-10 · `performance` `vm`

The existing TCO avoided VM call frames but still created Rust stack frames for each tail call via recursive `execute_bytecode()` calls, causing stack overflow after ~50,000 iterations. The fix introduces a trampoline pattern: `pending_tail_call` field on the VM stores tail call info, and an outer loop in `execute_bytecode` handles them iteratively. A cached `tail_call_env_cache` vector avoids repeated allocations. Tested to 50k+ depth with 16 test cases covering simple recursion, accumulators, mutual recursion, and conditional branching.

---

## [#212](https://github.com/elle-lisp/elle/pull/212) — Test cases for closure arithmetic operations
[`3f576b8b`](https://github.com/elle-lisp/elle/commit/3f576b8b) · 2026-02-10 · `testing`

Four tests for the accumulator pattern and nested closures with local arithmetic operations. Tests only, no code changes.

---

## [#211](https://github.com/elle-lisp/elle/pull/211) — REPL returns correct exit code on errors
[`5007952f`](https://github.com/elle-lisp/elle/commit/5007952f) · 2026-02-10 · `bugfix` `repl`

`run_repl()` and `run_repl_fallback()` now return a bool indicating whether errors occurred. `main()` uses this to set exit code 1, matching `run_file()` behavior. Fixes `echo '(+ 1 1' | elle` incorrectly exiting with code 0.

---

## [#210](https://github.com/elle-lisp/elle/pull/210) — Enhanced error messages with source location
[`72f542eb`](https://github.com/elle-lisp/elle/commit/72f542eb) · 2026-02-10 · `errors` `ux`

Adds `src/error/formatting.rs` with `format_source_context()` that displays the source line with a caret pointing to the error column. Reader parse errors now include `file:line:col` information. The REPL extracts location info from error messages and displays visual context. Duplicate error reporting in two-pass compilation is suppressed. Also cleans up two leftover documentation files (`ARCHITECTURAL_ISSUE.md`, `PHASE9_COMPLETE.md`).

---

## [#209](https://github.com/elle-lisp/elle/pull/209) — Begin splitting compile.rs into modular structure
[`2791fca5`](https://github.com/elle-lisp/elle/commit/2791fca5) · 2026-02-10 · `refactor`

Moves `compile.rs` to `compile/mod.rs` and extracts helper methods: `compile_literal`, `compile_if`, `compile_while`, `compile_for`, `compile_cond`, `compile_begin`, `compile_block`, `compile_and`, `compile_or`, `compile_try`, `compile_handler_case`, `compile_handler_bind`, `compile_let`, `compile_letrec`, `compile_call`, `compile_lambda`, `compile_match`. Reduces the main `compile_expr` match from ~1000 lines to ~400 lines.

---

## [#208](https://github.com/elle-lisp/elle/pull/208) — Split converters.rs into modular submodules
[`4cd10e54`](https://github.com/elle-lisp/elle/commit/4cd10e54) · 2026-02-09 · `refactor`

Breaks the 1777-line `converters.rs` into `variable_analysis.rs`, `quasiquote.rs`, `threading.rs`, `binding_forms.rs` (lambda, let/let*/letrec), `control_flow.rs` (cond, match), `exception_handling.rs` (try, handler-case, handler-bind), and `value_to_expr.rs` (core recursive conversion engine). Public API unchanged.

---

## [#206](https://github.com/elle-lisp/elle/pull/206) — REPL accumulates multi-line input for piped stdin
[`b9a4334a`](https://github.com/elle-lisp/elle/commit/b9a4334a) · 2026-02-09 · `bugfix` `repl`

Piped multi-line input was parsed line-by-line, causing "Unterminated list" errors on incomplete expressions. Both `run_repl()` and `run_repl_fallback()` now accumulate lines until a complete s-expression is formed before parsing.

---

## [#203](https://github.com/elle-lisp/elle/pull/203) — LSP: emit syntax errors as diagnostics
[`d5d7f66b`](https://github.com/elle-lisp/elle/commit/d5d7f66b) · 2026-02-09 · `lsp`

Captures lexer, reader, and conversion errors during compilation and converts them to LSP diagnostics with proper severity. Emitted on `didOpen`, `didChange`, and `didClose` events so editors show syntax errors inline.

---

## [#202](https://github.com/elle-lisp/elle/pull/202) — LSP: fix notification responses
[`3557d3b4`](https://github.com/elle-lisp/elle/commit/3557d3b4) · 2026-02-09 · `bugfix` `lsp`

Per LSP 3.17, notifications (messages without an `id` field) must not receive responses. The server was sending empty `{"jsonrpc": "2.0"}` objects for `didOpen`/`didChange`/`didClose`, causing client errors. Now only requests get responses.

---

## [#201](https://github.com/elle-lisp/elle/pull/201) — LSP: symbol renaming
[`6f146361`](https://github.com/elle-lisp/elle/commit/6f146361) · 2026-02-09 · `lsp`

Adds `textDocument/rename` with name validation (format checks, reserved words, builtins), conflict detection, and `WorkspaceEdit` generation covering all symbol occurrences. Single-file scope.

---

## [#200](https://github.com/elle-lisp/elle/pull/200) — LSP: code formatting + workspace integration + spec compliance
[`606c8d58`](https://github.com/elle-lisp/elle/commit/606c8d58) · 2026-02-09 · `lsp` `formatter`

A meaty PR that adds several things at once. The core feature is a new `src/formatter/` module: a recursive s-expression pretty-printer with configurable line length and indentation (defaults 80/2). The LSP gets `textDocument/formatting` support via `elle-lsp/src/formatting.rs`. Beyond formatting, this PR also makes `elle-lsp` a workspace member (previously a standalone project with its own `Cargo.toml`), adds 21 LSP 3.17 spec compliance tests covering the base protocol and all implemented capabilities, adds elle-lsp build+test steps to CI, and cleans up clippy warnings across elle-lint and elle-lsp.

---

## [#199](https://github.com/elle-lisp/elle/pull/199) — LSP: find-references
[`0d7b7c9d`](https://github.com/elle-lisp/elle/commit/0d7b7c9d) · 2026-02-09 · `lsp`

Adds `textDocument/references` handler. Collects all usages of a symbol at cursor position from the `SymbolIndex`, with `includeDeclaration` parameter support. Single-file scope.

---

## [#198](https://github.com/elle-lisp/elle/pull/198) — LSP: go-to-definition
[`ecf9e522`](https://github.com/elle-lisp/elle/commit/ecf9e522) · 2026-02-09 · `lsp`

Adds `textDocument/definition` handler to the LSP server. Queries the `SymbolIndex` to locate definitions at cursor position (single-file, Phase 1 scope). Also fixes the LSP protocol message framing: the Content-Length header was missing the final `\n` in the `\r\n\r\n` separator, causing clients to wait for EOF before processing responses.

---

## [#190](https://github.com/elle-lisp/elle/pull/190) — Fix Token/OwnedToken type mismatch in elle-lint and elle-lsp
[`2f1d2014`](https://github.com/elle-lisp/elle/commit/2f1d2014) · 2026-02-09 · `bugfix`

Both `elle-lint` and `elle-lsp` were creating `Vec<Token<'_>>` but Reader expected `Vec<OwnedToken>`. Adds `OwnedToken::from()` conversions. Also includes elle-lint and elle-lsp Cargo.lock files and source that appears to have been previously untracked.

---

## [#189](https://github.com/elle-lisp/elle/pull/189) — Resident compiler with disk/memory caching
[`bc39bb61`](https://github.com/elle-lisp/elle/commit/bc39bb61) · 2026-02-09 · `compiler` `architecture`

Introduces the `ResidentCompiler` module: a persistent compilation server that caches compiled artifacts in memory (for REPL/LSP) and on disk (via `/dev/shm`). `CompiledDocument` bundles source, AST, bytecode, location map, symbols, and diagnostics. Extends `SourceLoc` with a `file` field and threads it through the lexer. Adds `compile_with_metadata()` and a `location_map` to the VM, laying groundwork for source-mapped error messages. Also introduces `symbol_index.rs` (524 lines) for symbol indexing used by the LSP.

---

## [#188](https://github.com/elle-lisp/elle/pull/188) — Modularize error module
[`8bf953d6`](https://github.com/elle-lisp/elle/commit/8bf953d6) · 2026-02-09 · `refactor`

Splits `error.rs` (688 lines) into `types.rs` (EllError enum), `builders.rs` (constructors), `sourceloc.rs`, `runtime.rs`, and `mod.rs` with 45 error-specific tests.

---

## [#186](https://github.com/elle-lisp/elle/pull/186) — Remove dead scope instructions and AST nodes
[`f9ddd3f3`](https://github.com/elle-lisp/elle/commit/f9ddd3f3) · 2026-02-09 · `refactor`

Removes `LoadScoped`, `StoreScoped`, `ScopeVar`, `ScopeEntry`, `ScopeExit`, and `CompileScope` -- bytecode and AST leftovers that Phase 4 cell boxing made obsolete. Also adds the linter module (261 lines).

---

## [#184](https://github.com/elle-lisp/elle/pull/184) — Modularize JSON module
[`0d77b80e`](https://github.com/elle-lisp/elle/commit/0d77b80e) · 2026-02-09 · `refactor`

Splits the 1079-line `json.rs` (introduced in #177) into `parser.rs`, `serializer.rs`, and `mod.rs`. Same pattern as the reader modularization.

---

## [#183](https://github.com/elle-lisp/elle/pull/183) — Modularize reader module
[`bf24429e`](https://github.com/elle-lisp/elle/commit/bf24429e) · 2026-02-09 · `refactor`

Splits the 737-line `reader.rs` into `token.rs` (Token/OwnedToken/SourceLoc types), `lexer.rs` (tokenization), `parser.rs` (high-level parsing), and `mod.rs` (public API + tests). No functional changes.

---

## [#182](https://github.com/elle-lisp/elle/pull/182) — Fix multi-expression test wrapping
[`a54410bf`](https://github.com/elle-lisp/elle/commit/a54410bf) · 2026-02-09 · `bugfix` `testing`

This PR appears to be a merge artifact that carries the full Phase 4 cell-boxing work from #178 plus the modularization from #188 and #189. The actual bug fix is wrapping multi-expression tests in `begin` so `read_str` (which reads only one expression) can handle them.

---

## [#178](https://github.com/elle-lisp/elle/pull/178) — Shared mutable captures via cell boxing (Phase 4)
[`d08cc9e8`](https://github.com/elle-lisp/elle/commit/d08cc9e8) · 2026-02-09 · `closures` `architecture`

The most architecturally significant closure change in this batch. The problem: `set!` inside lambda bodies failed with "Undefined global variable" because parameters lived in an immutable `Rc<Vec<Value>>` closure environment while locally-defined variables lived on the scope stack, and the compiler could not distinguish between them at runtime.

The solution introduces `Value::Cell` (`Rc<RefCell<Box<Value>>>`) for transparent shared mutable state. Lambda nodes now track locally-defined variables. The closure environment layout becomes `[captures..., params..., locals...]`. New bytecode instructions (`MakeCell`, `UnwrapCell`, `UpdateCell`, `LoadUpvalueRaw`) support the boxing protocol. `MakeCell` is made idempotent to prevent double-wrapping when nested lambdas capture cells from outer scopes. Also fixes self-recursive and mutual-recursive `define` inside lambda bodies. The converter is refactored to extract match arms into `#[inline(never)]` helpers to reduce stack frame size and prevent stack overflow on deep nesting. Reduces pre-existing test failures from 6 to 0.

---

## [#177](https://github.com/elle-lisp/elle/pull/177) — spawn/join concurrency primitives
[`7a555150`](https://github.com/elle-lisp/elle/commit/7a555150) · 2026-02-09 · `concurrency` `language`

First concurrency support: `spawn` takes a closure and runs it on a new OS thread; `join` blocks until the thread completes. A `SendValue` wrapper (with `unsafe impl Send+Sync`) deep-copies values across thread boundaries. Closures with mutable captures (tables, native functions, FFI handles) are rejected at spawn time. Each spawned thread gets a fresh VM with primitives registered but no access to the parent's globals. Ships with 24 concurrency tests, a comprehensive example, and documentation pages for the site generator. Also introduces the `json.rs` module (1078 lines) for JSON serialization/deserialization, which was apparently bundled into the same PR.

---

## [#167](https://github.com/elle-lisp/elle/pull/167) — JIT expression compilation: binops, for-loop unrolling, empty? primitive
[`aad74c38`](https://github.com/elle-lisp/elle/commit/aad74c38) · 2026-02-09 · `jit` `performance`

Threads the `SymbolTable` through the Cranelift pipeline so the compiler can resolve operator names and emit native arithmetic/comparison instructions. Adds closure call profiling infrastructure to the VM (`closure_call_counts`) for identifying hot functions. Introduces the `empty?` predicate (O(1) for all collection types), replacing `(= (length x) 0)` in the N-Queens demo for a 10% speedup (10.6s to 9.6s). Extends JIT compilation to cover and/or expressions, let bindings, cond, while, and for loops. For loops over literal cons lists are unrolled at compile time, eliminating runtime iteration overhead.

---

## [#166](https://github.com/elle-lisp/elle/pull/166) — JIT integration: --jit flag, executor, native code generation
[`0887ea84`](https://github.com/elle-lisp/elle/commit/0887ea84) · 2026-02-09 · `jit` `cli`

Wires the Cranelift infrastructure from #152 into an end-to-end pipeline. Three new modules: `jit_wrapper.rs` (high-level compilation interface with `is_jit_compilable` filtering), `jit_coordinator.rs` (profiling-guided hot-function detection using a 10-call threshold), and `jit_executor.rs` (execution cache and native code dispatch). The `--jit` CLI flag enables opportunistic JIT mode for both REPL and file execution. A `jit_vs_bytecode` benchmark suite measures coordinator overhead (negligible -- sub-1%). The last commit in the squash adds actual native code generation via `ExprCompiler`, calling compiled function pointers and decoding `i64` return values back to Elle `Value`s. Supports literals, binary ops, and conditionals; gracefully falls back to bytecode for everything else.

---

## [#164](https://github.com/elle-lisp/elle/pull/164) — Fix sequential exception-catching bug (Phase 9a)
[`6b2cc915`](https://github.com/elle-lisp/elle/commit/6b2cc915) · 2026-02-08 · `bugfix` `vm`

Root cause: `BindException` was using the raw constant index as the symbol ID for variable binding instead of looking up the `SymbolId` from the constants table. The second handler in a sequence would bind to the wrong symbol, corrupting VM state. Nine regression tests cover sequential catches, nested handlers, and variable persistence across catches.

---

## [#163](https://github.com/elle-lisp/elle/pull/163) — Upgrade GitHub Pages actions to v4
[`8cbc613c`](https://github.com/elle-lisp/elle/commit/8cbc613c) · 2026-02-08 · `ci`

Bumps `configure-pages` v3->v4, `upload-pages-artifact` v2->v3, `deploy-pages` v2->v4 to avoid GitHub's enforcement against deprecated `upload-artifact@v3`. Three lines changed.

---

## [#161](https://github.com/elle-lisp/elle/pull/161) — try/catch/finally (Phase 10)
[`53e9ac54`](https://github.com/elle-lisp/elle/commit/53e9ac54) · 2026-02-08 · `exceptions` `language`

Implements `try/catch/finally` by compiling down to the `handler-case` bytecode from Phase 9a. Catch clauses bind the exception to a variable; finally blocks execute on both success and exception paths. Ships with 25 try/catch tests and 39 handler-case tests. Documents a known bug: multiple sequential exception-catching statements in the same execution context can fail because `BindException` was using a constant index as a symbol ID.

---

## [#160](https://github.com/elle-lisp/elle/pull/160) — GitHub Pages preview deployment for PRs
[`88bcebfd`](https://github.com/elle-lisp/elle/commit/88bcebfd) · 2026-02-08 · `ci`

One-file CI change: a `deploy-pr-preview` job generates full documentation (language docs + rustdoc) for pull requests, uploads it as an artifact with 7-day retention, and comments on the PR with a download link.

---

## [#159](https://github.com/elle-lisp/elle/pull/159) — N-Queens benchmark suite: Elle, Chez Scheme, Common Lisp
[`52f6e9fb`](https://github.com/elle-lisp/elle/commit/52f6e9fb) · 2026-02-08 · `demos` `benchmarks`

Cross-language N-Queens backtracking implementations for performance comparison. All three find the correct 92 solutions for N=8. The Elle version doubles as a smoke test for recursive list processing and serves as a benchmark target for later JIT work.

---

## [#153](https://github.com/elle-lisp/elle/pull/153) — Exception interrupt mechanism (Phase 9a)
[`3e53f60f`](https://github.com/elle-lisp/elle/commit/3e53f60f) · 2026-02-08 · `exceptions` `vm`

Changes how exceptions interact with the VM loop. Previously, arithmetic errors like division-by-zero returned `Err` which immediately exited `execute_bytecode`. Now the VM sets `current_exception` and pushes `Nil`, then checks for pending exceptions after each instruction. If a handler frame exists, it unwinds the stack and jumps to the handler; otherwise it propagates the error. This is the prerequisite for `handler-case` to actually catch runtime exceptions. Also adds parser support for `handler-case` and `handler-bind` keywords.

---

## [#152](https://github.com/elle-lisp/elle/pull/152) — Cranelift JIT integration: 15 phases from IR primitives to escape analysis
[`8c7002fa`](https://github.com/elle-lisp/elle/commit/8c7002fa) · 2026-02-08 · `jit` `cranelift` `architecture`

A marathon PR that builds the entire Cranelift JIT backend in 15 incremental phases across a single squash-merge. The commit adds 30+ files under `src/compiler/cranelift/` and touches nothing in the existing interpreter, so it ships as pure infrastructure.

**Phases 1-4** establish the foundation: `JITContext` wrapping a Cranelift module, `ExprCompiler` for translating AST nodes to CLIF IR, `BinOpCompiler` for constant-folding arithmetic, and `IrEmitter` for actual instruction emission. Phase 4 adds symbol table threading so the compiler can resolve operator names.

**Phases 5-7** add variable scoping (`ScopeManager` with nested stacks), let-binding compilation with both HashMap-based and stack-based backends, user-defined function support (`FunctionCompiler`), and closure capture analysis (`ClosureCompiler` with free-variable detection and environment packing).

**Phases 8-10** introduce optimization passes: expression simplification (dead branch elimination, constant propagation), analysis of additional expression types (cond, while, for, match), and tail-call optimization detection with inlining heuristics.

**Phases 11-15** build the adaptive compilation infrastructure: profiling-guided strategy selection, runtime event collection, feedback-based recompilation, type specialization with monomorphic/polymorphic dispatch, and escape analysis for stack allocation decisions.

Each phase includes its own milestone test file. The test count goes from 693 to 693 (no regressions) with 693 new JIT-specific tests. A benchmark suite (`benches/cranelift_jit_benchmarks.rs`) rounds it out. None of this code generates real native code yet -- it is analysis and planning infrastructure that later PRs will wire up.
