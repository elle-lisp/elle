<!-- changelog-instructions

# Changelog generation

This file is an agent-optimized summary of DEVLOG.md, grouped by
narrative arc rather than chronologically.  To regenerate:

1. Read DEVLOG.md in full.
2. For each PR entry, assign it to one or more thematic arcs.
3. Write one line per PR: `- #PR: summary`
4. Omit trivial PRs (submodule bumps, CI-only, typo fixes, README-only).
5. Write a 2-3 sentence preamble for each arc.

end-changelog-instructions -->

# Elle Changelog (by arc)

Abbreviated from [DEVLOG.md](DEVLOG.md). One line per PR, grouped by
subsystem. Trivial PRs (submodule bumps, CI tweaks, typo fixes) omitted.
Read the DEVLOG entry for full context on any PR.

---

## Memory model

Elle's allocator evolved from leaked Rc pointers through slab free-lists
to the current model: per-fiber bump arenas (64KB pages, no per-slot
dealloc) with inline objects. Immutable collections (strings, arrays,
bytes, sets, closures) store their data as `InlineSlice<T>` contiguous
with the `HeapObject` header — no inner Rust-heap allocations, no Drop.
Memory is reclaimed by scope-based `release(mark)` (escape-analysis-gated
`RegionEnter`/`RegionExit`), two-pool swap rotation at tail-call
boundaries (gated by interprocedural rotation-safety analysis), and
`FlipEnter`/`FlipSwap`/`FlipExit` for explicit per-function rotation.
`SharedAllocator` wraps the same `SlabPool`/`BumpArena` for zero-copy
inter-fiber value exchange. Mutable collections use `Rc<RefCell<_>>`
backing so `deep_copy_to_outbox` preserves cross-fiber sharing.

- #730: Resource measurement library, SharedAllocator rotation, memory model phases 0-2 (sorted Vec structs, bump arena, inline slices)
- #706: Two-pool rotation for tail-call memory reclamation; escape analysis relaxations; interprocedural rotation-safety fixpoint; epoch 7
- #703: SlabPool extraction; SharedAllocator and FiberHeap refactored as SlabPool wrappers
- #669: Slab-only allocation replaces per-scope bumps; JIT RegionEnter/RegionExit wired to runtime helpers
- #576: Root slab free-list allocator replaces root bump allocator
- #572: Root fiber uses FiberHeap; HEAP_ARENA deleted; escape analysis outward-set fix
- #527: Allocation observability (limits, peak, bytes, fiber-stats); per-scope bump allocators
- #490: ScopeStack removed from VM hot path; LoadGlobal/StoreGlobal go direct
- #488: Escape analysis tier 6: while/block/break-aware scope allocation
- #435: Escape analysis tiers 1-5: 48-primitive whitelist, variable-in-result, nested let/match
- #421: Escape analysis infrastructure with primitive whitelist
- #416: Scope escape analysis with 5-condition real analysis replacing stubs
- #414: Per-fiber heaps with bump allocator, RegionEnter/RegionExit, shared allocators for zero-copy fiber exchange
- #405: Thread-local heap arena with mark/release for macro expansion (first real allocator)

---

## Value representation

Values migrated from a 24-byte Rust enum to 8-byte NaN-boxed, then to
16-byte tagged union. Integers are full i64, heap pointers are 64-bit,
keywords use interned strings (later FNV-1a hashes for cross-DSO identity).

- #640: Migrate from 8-byte NaN-boxed to 16-byte tagged-union Value; full i64 integers, full 64-bit pointers
- #630: Replace NaN-boxed Binding with arena-indexed compile-time type (BindingArena)
- #553: Type cleanup: Cell renamed to LBox, blob/buffer purge, @-predicates removed
- #523: Set types (immutable and mutable) with `|1 2 3|` literal syntax
- #519: Implement Ord, Hash, Eq on Value
- #394: Allow fibers, closures, externals as table keys
- #373: Bytes/Blob types, crypto primitives, NaN-box tag reassignment, short string optimization
- #370: Collection literal semantics overhaul: `[...]` immutable, `@[...]` mutable, `{...}` structs, `@{...}` tables
- #321: Keywords reimplemented as interned strings (later reverted to hashes in #607)
- #269: NaN-boxing migration: 24-byte enum to 8-byte Copy value, string interning, nil/empty-list distinction
- #574: Per-value dispatch tables (traits)

---

## Execution backends: VM

The bytecode VM is the primary backend. Key milestones include the
trampoline-based TCO, fiber-based execution model, and the transition
from scope-stack to direct-global access.

- #775: Widen jump offsets from i16 to i32; fix silent truncation in large functions
- #773: Fix call_stack leak in JIT dispatch path (900 MB RSS reduction on nqueens)
- #771: REPL forward references and mutual recursion via deferred compilation
- #707: REPL rework with form-by-form evaluation and def persistence
- #593: Fiber fuel system for cooperative preemption via instruction budget
- #490: Kill ScopeStack; direct global access
- #482: Racket-style parameters for fiber-scoped dynamic bindings
- #473: Source locations in runtime errors; error values become structs
- #446: Fix letrec binding lost after fiber yield/resume (ExecResult struct)
- #213: Trampoline-based TCO eliminates stack overflow at 50k+ depth

---

## Execution backends: JIT (Cranelift)

The JIT compiles hot functions via Cranelift. It grew from a stub through
multi-function compilation, yield side-exit, polymorphic dispatch, and
cross-thread LIR transfer. Current state: all instruction types supported,
adaptive tiering, direct self-calls and intra-group calls.

- #773: Fix call_stack leak — JIT path never popped CallFrame (900 MB RSS fix)
- #753: Off-thread JIT compilation via background worker thread; eliminates event-loop stalls
- #750: Gate Cranelift behind `jit` feature flag (default on) for Android builds
- #737: JIT tail-call trampoline; squelch enforcement in compile/run-on
- #736: Upgrade Cranelift 0.116 to 0.130 (deduplicate with Wasmtime 43)
- #720: Sendable channels; JIT SuspendingCall fix; LIR transfer for worker-thread JIT
- #714: SDL3 bindings; JIT polymorphic+SuspendingCall fixes; upvalue index widened to u16
- #698: JIT callable-collection dispatch (structs, arrays, sets, strings, bytes)
- #688: Fix scope-allocation use-after-free; JIT yield LBox reconstruction
- #684: Fix block-push heap corruption; JIT io-request leak; dual VM/JIT CI
- #672: Fix JIT LBox wrapping for mutable-captured parameters
- #667: Fix JIT yield-through-call SuspendedFrame reconstruction
- #662: Fiber trampoline switch v2: caller continuation frame, SIG_SWITCH handling
- #661: Fix JIT signal checks: bitwise containment instead of exact equality
- #658: Fix use-after-free in batch JIT compilation (closure_constants lifetime)
- #621: Fix JIT side-exit panic when yielding primitive called from hot function
- #599: Structural correctness: PatternLiteral Hash+Eq, graceful composed-signal suspension
- #592: JIT 20 new instructions (type predicates, destructuring, struct access, MakeClosure)
- #589: JIT rejection diagnostics (jit/rejections, ELLE_JIT_STATS)
- #520: JIT variadic function support
- #465: JIT yield side-exit: spill/resume at yield points
- #464: Skip LocalCell allocations for non-captured let bindings
- #381: JIT cell_locals_mask: 3.2x speedup on N-Queens
- #379: Inline integer fast paths and direct self-calls
- #355: Multi-function JIT compilation for mutually recursive call groups
- #335: JIT-aware tail call resolution for mutual recursion
- #297: 8.9x fib(30) speedup: JIT-to-JIT calls, operator specialization
- #289: JIT phase 4 overflow: Vec-based globals, SmallVec handlers, LoadCapture fix
- #288: JIT phase 4: remove feature gate, TCO, exceptions
- #287: JIT phase 3: full LirInstr coverage, VM pointer in calling convention
- #286: JIT phases 1-2: Cranelift scaffold, hot function detection, fallback
- #152: Cranelift JIT infrastructure: 15 phases of analysis and planning

---

## Execution backends: WASM (Wasmtime)

A complete LIR-to-WASM backend via wasm-encoder and Wasmtime. Per-closure
compilation with disk caching brings startup from 57s to 0.35s. The backend
handles all instruction types including yield/resume via CPS transform.

- #713: Per-closure compilation, LirModule, disk caching; 57s to 0.35s
- #712: CLI args, single-pass regalloc (57s to 2.5s), --json, --eval
- #697: Complete WASM backend: constants through pattern matching, register allocator, trampoline tail calls

---

## Execution backends: MLIR and GPU

MLIR backend lowers LIR to arith/func/cf dialects and JIT-compiles via
ExecutionEngine. Vulkan compute dispatches async via io_uring fence fds.
SPIR-V is generated at runtime from Elle code (no GLSL).

- #784: Fix MLIR capture lowering: separate env_vals from regs map to prevent register collision
- #737: MLIR/SPIR-V float support, differential testing harness (`compile/run-on` across 4 tiers), `LirInstr::Convert`, `ValueConst`
- #727: Vulkan compute (async dispatch, buffer pooling), SPIR-V emitter (runtime bytecode gen), MLIR backend (melior 0.27), GPU eligibility analysis, SignalBits widened to u64

---

## Signal system

Signals (formerly "effects") are Elle's capability/control-flow mechanism.
They evolved from a Pure/Yields enum through interprocedural inference to
a full compile-time tracking and runtime enforcement system with
capabilities, squelch (blacklist), and emit as a special form.

- #761: User-defined signal space widened to bits 32-63 (32 slots, up from 16)
- #759: Sound signal inference for unknown callees; attune (whitelist dual of squelch); SIG_GPU; CAP_MASK structural redefine
- #749: Signal projection (cross-file keyword→signal mapping) and compile-time squelch narrowing
- #723: Capability enforcement (fiber/new :deny), emit special form replaces yield IR, defmacro &opt
- #704: Encapsulate SignalBits (private inner field, named methods) for u32-to-u64 migration
- #590: Squelch redesign: runtime closure transform instead of compile-time special form
- #580: Add squelch: blacklist signal constraint form (open-world composition)
- #564: Grand rename: effects to signals (186 files)
- #552: Complete signals implementation: restrict form, CheckEffectBound instruction
- #517: Rename Effect::none() to Effect::inert() (now Signal::Silent)
- #513: Replace raises/raise with signals/signal terminology
- #506: Effect fixpoint convergence limit (10 iterations)
- #496: Rename Effect::raises to Effect::errors
- #283: Interprocedural effect tracking: effect_env, primitive_effects, Polymorphic resolution
- #284: Default unknown callees to Yields for soundness
- #240: Effect system for colorless coroutines: Pure/Yields/Polymorphic(n) enum, inference engine

---

## I/O and concurrency

Async-first runtime with io_uring backend. All user code runs under the async
scheduler. Structured concurrency (ev/scope, ev/join, ev/select, ev/race),
fibers with signal masks, process scheduler with fuel-based preemption,
channels, and sendable closures for cross-thread transfer.

- #783: Embedding step-based scheduler (ev/step), cdylib C-ABI, Rust + C host demos
- #781: Fix h2 stream leak; add gRPC server-streaming
- #769: Fix h2 writer shutdown race; list_to_array plugin ABI for gRPC
- #763: WebSocket (RFC 6455) and gRPC over HTTP/2, pure Elle
- #761: Full HTTP/2 client and server (RFC 9113 + HPACK), pure Elle
- #741: Fix plugin dispatch in JIT/WASM backends for MCP server
- #710: Fix stdin reads starved when ev/spawn fiber is active
- #709: Cycle detection for mutable containers; SyncBackend removal (1980 lines deleted)
- #695: GenServer, Actor, Task, Supervisor, EventManager (OTP-style abstractions)
- #694: Negative indexing and sequence accessor widening
- #674: Process scheduler with fuel-based preemption, mailboxes, links, monitors
- #664: Async-first runtime unification: ev/run wraps all user code; SIG_WAIT; structured concurrency primitives
- #655: Fiber trampoline v1 (SIG_SWITCH infrastructure)
- #649: IoOp::Task for background thread closures (plugin blocking work)
- #608: Async port/open via io_uring openat with timeout
- #577: SIG_EXEC + subprocess management via io_uring
- #573: Stream combinators (map, filter, take, zip, pipe); Signal::Inert renamed to Silent
- #561: Generalize closure sending across threads (SendBundle)
- #526: I/O Phase 5: TCP, UDP, Unix sockets with io_uring linked timeouts
- #511: I/O Phase 4: async scheduler, io_uring backend, BufferPool
- #505: I/O Phase 3: synchronous I/O, SIG_IO, sync-scheduler trampoline
- #491: Channel primitives wrapping crossbeam-channel
- #489: I/O Phase 2: Port type with OwnedFd, port primitives, stdin/stdout/stderr parameters
- #293: Fiber/signal system (11k-line rewrite): fiber struct, signal masks, coroutine compatibility
- #276: First-class continuations for yield across call boundaries
- #249: Complete coroutine implementation (CPS interpreter, yield propagation)
- #177: spawn/join concurrency primitives (first threading support)
- #711: Process module: readiness protocol, restart limits, subprocess helper

---

## Language features

Core language features: destructuring, match, parameters, macros, epochs,
binding forms. The language settled on `def`/`var`/`defn`/`fn` with bracket
syntax for bindings, `true`/`false` literals, and `#` for comments.

- #785: Epoch 9: flat cond/match; immutable push on arrays/strings/bytes
- #767: Make let sequential (Clojure-style); each binding sees previous ones
- #737: Epoch 8: immutable-by-default bindings with `@` prefix for opt-in mutability; `integer`/`float` coercion split from `parse-int`/`parse-float`
- #742: Epoch 7: flat let bindings (Clojure-style `[a 1 b 2]` replaces `[[a 1] [b 2]]`)
- #648: Epoch-based migration system for breaking language changes
- #652: Remove assertions.lisp; consolidate to built-in assert; elle rewrite tool
- #656: Consolidate sync output primitives; print/println/eprint/eprintln; parameterize-aware
- #629: as->, some->, some->> threading macros
- #627: Generalized sort with compare primitive
- #624: Extend concat to bytes, sets, structs
- #571: Strict &keys destructuring + struct rest pattern
- #569: Strict destructuring in binding forms (error on mismatch instead of nil)
- #523: Set types with `|...|` literal syntax; `set` renamed to `assign`
- #521: Remove Pop/Move/Dup from LIR (store-to-slot-then-reload)
- #452: Match overhaul: decision trees, or-patterns, exhaustiveness checking
- #458: Symbol keys in destructuring + letrec destructuring
- #451: Functional programming primitives (sort, range, compose, partial, memoize, etc.)
- #398: &opt, &keys, &named parameter markers
- #397: Bracket syntax in special forms; case, if-let, when-let, forever macros
- #371: Splice special form (`;expr` spreads into calls and constructors)
- #360: Migrate boolean literals from #t/#f to true/false
- #354: Break in while loops (implicit named block)
- #328: Prelude macro migration (defn, let*, ->, ->>), named blocks, array matching, yield*
- #326: Compiler-level destructuring with wildcard and rest patterns
- #322: Rename define to var, const to def (Janet/Clojure convention)
- #319: Add const form for immutable bindings
- #230: Rename lambda to fn
- #497: Variadic string and string/format with positional/named/format-spec support

---


## Compiler pipeline

Five-phase pipeline: **read → expand → analyze → lower → emit**.

**Reader** (`src/reader/`): tokenizes and parses source into `Syntax`
trees. Dispatches by file extension — `.lisp` uses the s-expression
reader, `.js`/`.py`/`.lua` use dedicated lexer+parser pairs (Pratt
precedence for JS/Python, recursive descent for Lua). All surface
syntaxes produce the same `Syntax` type; everything downstream is shared.
Numeric literals support hex, octal, binary, underscores, scientific
notation. `include`/`include-file` splice parsed forms before expansion.

**Expander** (`src/syntax/expand/`): macro expansion with sets-of-scopes
hygiene (Flatt 2016). Transformers are compiled to closures and
VM-evaluated (`pipeline::eval_syntax`). `syntax-case` for pattern-based
macros, `begin-for-syntax` for compile-time definitions, `quasiquote`
wraps template symbols as `SyntaxLiteral` to preserve definition-site
scopes. Transformer closures are cached per `MacroDef`.

**Analyzer** (`src/hir/analyze/`): produces HIR with interprocedural
signal and arity tracking via fixpoint iteration (`fileletrec.rs`).
File-as-letrec compilation model — top-level `def`/`var` forms are
mutually recursive. `BindingArena` indexes bindings by `BindingId`.
Tail-call marking pass (`tailcall.rs`). Accumulates recoverable errors
(undefined variables, signal mismatches) with source locations.

**Lowerer** (`src/lir/lower/`): HIR → LIR with escape analysis
(`escape.rs` — scope allocation, rotation safety, return-params
analysis), intrinsic specialization, decision-tree compilation for
`match` (`decision/`), and `LirModule` with flat `ClosureId`-indexed
closure list.

**Emitters**: bytecode (`src/compiler/`), Cranelift JIT (`src/jit/`),
WASM via wasm-encoder (`src/wasm/`), MLIR via melior (`src/mlir/`).
Each consumes `LirModule`. The WASM backend compiles per-closure with
disk caching. The MLIR backend branches to LLVM JIT (CPU) or SPIR-V
(GPU).

- #775: Widen jump offsets from i16 to i32 to fix silent truncation in large functions
- #768: Call-scoped arena reclamation via fixpoint return-safe analysis (nqueens RSS: 1 GB → 172 MB)
- #755: Fix def-shadow: deferred binding registration order in fileletrec; reject duplicate letrec names
- #749: Signal projection caching for cross-file module imports; compile-time squelch narrowing
- #732: Python surface syntax reader (indentation-aware, Pratt parser)
- #731: JavaScript surface syntax reader (Pratt parser, template literals)
- #728: Macro hygiene fix: SyntaxLiteral preserves definition-site scopes
- #726: Structured errors throughout pipeline; error accumulation; did-you-mean suggestions
- #719: Disambiguate LBox (user) from CaptureCell (compiler captures)
- #713: LirModule with flat ClosureId-indexed closure list; per-closure WASM compilation
- #702: include/include-file for compile-time source splicing
- #705: Bytes literals (`b[1 2 3]` and `@b[1 2 3]`)
- #663: Lua surface syntax reader
- #657: Fix match decision tree duplicating arm bodies for or-patterns
- #613: Fix cond/match register corruption in variadic calls (block ordering)
- #594: Numeric literal formats (hex, octal, binary, underscores, scientific)
- #587: syntax-case, begin-for-syntax, syntax predicates
- #567: Cache compiled macro transformer closures per MacroDef
- #504: File-as-letrec compilation model; module files return closures
- #486: Colon syntax as struct/env access desugaring moved from expander to analyzer
- #452: Match overhaul: decision trees, or-patterns, exhaustiveness checking
- #450: Decompose pipeline.rs into 7 submodules
- #337: Add eval/read primitives; split oversized files
- #325: Compile-time arity checking; declarative primitive registration
- #324: Replace BindingId + HashMap with NaN-boxed Binding type
- #317: Sets-of-scopes hygiene and syntax objects
- #307: Replace template-based macro expansion with VM evaluation
- #298: Remove dead macro infrastructure (pre-VM expansion)
- #272: New pipeline completion: TCO, let semantics fix, old pipeline removal
- #268: New compilation pipeline (Syntax → HIR → LIR → Bytecode)
- #264: Proper lexical scope with compile-time capture resolution

---

## Error handling

Errors evolved from strings through tuples to structured LError with typed
ErrorKind variants. All runtime errors are catchable via fiber signal
dispatch. Source locations with caret context are shown for all error types.

- #726: Structured errors in compilation pipeline; error accumulation; source snippets with carets
- #724: Structured errors across all 11 Elle source files (module category + reason keyword)
- #642: Replace generic :error keywords with specific error types; ERROR_INVENTORY.csv
- #473: Source locations in runtime errors; error values become structs
- #267: Unified error system (LError) replaces Result<T, String>
- #257: Unified error handling; all runtime errors catchable as exceptions
- #210: Enhanced error messages with source location and caret context
- #161: try/catch/finally compiled down to handler-case bytecode
- #153: Exception interrupt mechanism: current_exception + handler frame dispatch

---


## Standard library

Pure Elle libraries for HTTP, DNS, Redis, IRC, TLS, process management,
telemetry, contracts, synchronization, AWS, and more. Libraries use the
closure-as-module pattern and are imported via `(import "std/<name>")`.

- #766: Raylib FFI module (1029 lines: window, drawing, input, audio, collision)
- #762: GTK4 overhaul: 35-test suite, Cairo module, stdlib polish (color, dns, cli)
- #745: Fix TLS read silently dropping final plaintext segment on TCP close
- #735: HTTP: chunked transfer, HTTPS/TLS, query params, redirects, compression, SSE
- #724: IRC module with IRCv3, CAP negotiation, SASL, auto-PONG
- #715: Move 9 plugins from Rust to native Elle (base64, compress, git, glob, semver, sqlite, uuid, cli, watch)
- #716: Declarative GTK4 bindings via FFI (30 widget types, WebKit webview)
- #714: SDL3 bindings via pure FFI (events, textures, audio, TTF)
- #711: Process module: readiness protocol, restart limits, subprocess helper
- #698: lib/sync.lisp: cooperative synchronization toolkit (lock, semaphore, condvar, rwlock, barrier)
- #695: GenServer, Actor, Task, Supervisor, EventManager
- #676: OpenTelemetry metrics library (OTLP/HTTP JSON exporter)
- #674: Process scheduler with I/O integration
- #673: Pure Elle AWS client with SigV4 signing over TLS
- #671: Redis client (RESP2, transactions, pub/sub, pipelining, reconnection)
- #645: TLS plugin via rustls UnbufferedConnection
- #631: Pure Elle DNS client (RFC 1035, wire-format codec, CNAME following)
- #591: Contract library: compile-validator, validate, combinators
- #565: Pure Elle HTTP/1.1 client and server
- #451: Functional programming primitives library

---

## FFI

FFI rebuilt on libffi with managed pointers, callback trampolines,
`ffi/defbind` macro, and `ffi/callback` for C-to-Elle calls. Pointers
are NaN-boxed (later tagged-union) with ref-counted prevent
use-after-free. `ffi/native nil` loads the current process for libc
access on all Unix platforms.

- #772: Import recognizes .dylib/.dll; falls back to plugin loading on UTF-8 failure
- #765: Fix FFI struct/array marshalling to accept immutable arrays
- #757: Gate libffi behind `ffi` feature flag (default on) for minimal builds
- #756: Link libgcc on Android for `__clear_cache` resolution
- #343: Rebuild FFI on libffi: type descriptors, marshaller, callbacks, ffi/defbind
- #351: Managed FFI pointers prevent use-after-free and double-free
- #566: ffi/signature accepts immutable arrays as arg-types
- #622: ptr/add, ptr/diff, ptr/to-int, ptr/from-int for pointer arithmetic
- #714: ffi/defbind simplified; FFI string nil marshals as NULL

---

## Plugins

Plugin system uses dlopen with `elle_plugin_init` protocol. The stable
ABI crate (`elle-plugin`) provides `extern "C"` types so plugins build
independently. 24 Rust plugins peaked, then 9 were replaced with pure
Elle modules using FFI. Plugins now live in a separate repo
(`elle-lisp/plugins`) as a git submodule.

- #770: Adopt plugin/ prefix convention for all plugin imports
- #769: list_to_array plugin ABI for list-to-array coercion at boundary
- #378: Plugin system: dlopen + elle_plugin_init; HeapObject::External; regex plugin
- #383: Plugin init returns Value; mermaid, sqlite, crypto, random plugins
- #396: Selkie SVG rendering plugin
- #443: Glob plugin
- #607: Oxigraph RDF quad store + SPARQL plugin; keyword hash for cross-DSO identity
- #632: uuid, xml plugins; random plugin expanded
- #634: msgpack plugin (interop + tagged modes)
- #635: base64, compress, csv, toml, yaml, semver plugins
- #639: Syn plugin for Rust syntax parsing
- #645: TLS plugin via rustls
- #646: Protobuf plugin (schema-driven, no codegen)
- #650: Jiff date/time plugin (104 primitives for all 7 jiff types)
- #653: Tree-sitter plugin (query-first API, 16 primitives)
- #654: Regex plugin expanded (replace, split, captures)
- #660: Arrow and Polars columnar data plugins
- #681: MQTT plugin (Rust codec + Elle I/O), ZMQ FFI library
- #689: Hash plugin with streaming API (SHA-256, BLAKE3, etc.)
- #728: Image plugin (35 primitives), SVG plugin (resvg)
- #715: 9 Rust plugins replaced with pure Elle modules using FFI
- #740: Stable plugin ABI (elle-plugin crate); plugins and MCP split to separate repos

---

## MCP server and tooling

The MCP server exposes Elle's compilation pipeline as a queryable semantic
model through JSON-RPC. Tools include eval (monadic bind over persistent
image), portrait, trace, SPARQL, test orchestration, and push gating.
Server lives in a separate repo (elle-lisp/mcp) as a submodule.

- #748: Migrate MCP server to epoch 8; disable static TLS re-exec hack
- #739: Add eval MCP tool: monadic bind over persistent Elle image; 10 operational risk hardening items
- #734: RuntimeConfig on VM; --trace flag; 6 MCP test orchestration tools (test_run/status/history/gate, push_ready/wip)
- #725: MCP server async startup via yield-based population fiber (<10s init)
- #721: Fix MCP server startup (glob plugin deleted); plugin ABI compatibility bug documented
- #696: Living model MCP server with 14 JSON-RPC tools; compile/* primitives (2470 lines); portrait, trace, RDF
- #659: Initial MCP server: RDF extraction, oxigraph store, JSON-RPC over stdin/stdout

---

## CLI and configuration

CLI uses subcommands (lint, lsp, rewrite) with all configuration via flags
parsed into a global Config struct. No more ELLE_* environment variables.
The binary includes lint and LSP server.

- #790: Formatter: Align model, depth-limited trivial, --no-epoch, --plm for editor integration
- #787: Formatter: cleanup, cross-Nest fix, epoch rewrite integration, flat cond/match
- #752: Android compilation support (inotify cfg gates, NDK cross-check CI)
- #750: Gate Cranelift behind `jit` feature flag; `--no-default-features` builds
- #757: Gate libffi behind `ffi` feature flag; `smoke-noffi` Makefile target
- #734: RuntimeConfig, --trace=call/signal/fiber/jit, --jit=off/eager/adaptive, --wasm modes
- #712: CLI args replace all ELLE_* env vars; --jit, --wasm, --cache, --json, --eval
- #648: Epoch-based migration; `elle rewrite` for source file updates
- #472: Source-to-source rewrite tool; lint/lsp/rewrite subcommands
- #366: Merge elle-lint and elle-lsp into main binary
- #318: Add (halt) primitive for graceful VM termination

---

## LSP

The LSP server handles go-to-definition, find-references, rename,
completion, hover (with docstrings), formatting, and diagnostics. It
consumes the HIR pipeline directly.

- #273: Migrate elle-lint and elle-lsp to new HIR pipeline
- #228: LSP: don't respond to notifications (spec compliance)
- #203: LSP: emit syntax errors as diagnostics
- #201: LSP: symbol renaming with validation and conflict detection
- #200: LSP: code formatting (s-expression pretty-printer); workspace integration
- #199: LSP: find-references
- #198: LSP: go-to-definition; fix Content-Length framing

---

## Numeric tower

Integers are full i64 (post-#640). Mixed int/float arithmetic, checked
overflow, IEEE 754 compliance (Inf, NaN constants), and canonicalized
hash so `(= 1 1.0)` implies equal hashes.

- #699: Mixed int/float comparisons, checked overflow, IEEE 754, whole-float display, inotify/kqueue watch
- #437: Numeric-aware equality: `(= 1 1.0)` is true; identical? for bitwise identity
- #594: Hex, octal, binary, underscore, scientific notation literals
- #626: number->string radix argument; seq->hex

---

## Testing infrastructure

Tests are primarily Elle scripts run via `make smoke`/`make test`. Rust
tests retained only for bytecode inspection, proptest, and process::Command.
Test migration moved ~5000 assertions from Rust to Elle.

- #508: Migrate ~870 integration tests from Rust to Elle (18 new test files)
- #462: Migrate 60 coroutine tests to Elle
- #455: Migrate property tests to Elle scripts
- #444: Test migration batch 2 (~450 tests)
- #440: Testing strategy: tiered tests, Elle test scripts, proptest_cases() helper

---

## Refactoring milestones

Major cleanup passes that reshape the codebase. These are reference points
for understanding when large structural changes occurred.

- #598: Hammer time: 8-phase cleanup across 58 files (-3010/+2627 lines)
- #547: Global refactoring: visibility narrowing, 15 file splits, primitive renames (291 files)
- #492: Housekeeping: stale files, plugin purge, docs restructure (200 files)
- #407: File renames to single-word lowercase convention; stdlib consolidation
- #285: Split 5 critical files; wire bitwise/remainder bytecode instructions
- #280: Phase B: delete old JIT, migrate value types, implement LocationMap (-17500 lines)
- #277: Delete CPS interpreter (4400 lines)
- #708: Code quality: math.rs dedup, plugin init macro, spawn extraction

---

## Demos and examples

Notable demos that exercise significant language features or serve as
performance benchmarks.

- #753: HTTP server demo with load generator, concurrency-sweep benchmark, SVG charts
- #729: Rewrite microgpt demo: Gaussian init, fused vdot/vsum, 2.7x faster
- #675: Conway's Game of Life (SDL2) and Mandelbrot Explorer (GTK4+Cairo)
- #516: MicroGPT: minimal GPT with scalar autograd
- #159: N-Queens benchmark suite (Elle, Chez Scheme, Common Lisp)
