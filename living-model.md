# The Living Model

A specification for exposing Elle's compilation pipeline as a queryable,
persistent semantic model of running programs — for both human developers
and AI agents.

## Contents

- [Vision](#vision)
- [Architecture](#architecture)
- [Phase 1: Compiler as library](#phase-1-compiler-as-library)
- [Phase 2: Semantic portrait](#phase-2-semantic-portrait)
- [Phase 3: Knowledge graph enrichment](#phase-3-knowledge-graph-enrichment)
- [Phase 4: MCP tools for agents](#phase-4-mcp-tools-for-agents)
- [Phase 5: Live model](#phase-5-live-model)
- [Phase 6: Program transformation](#phase-6-program-transformation)
- [Language changes](#language-changes)
- [Appendix A: Signal bit reference](#appendix-a-signal-bit-reference)
- [Appendix B: Data model](#appendix-b-data-model)

---

## Vision

A programmer writes four lines of Elle:

```lisp
(defn fetch-and-transform [url transform]
  (-> (http/get url)
      (get :body)
      (json/parse)
      (transform)))
```

The compilation pipeline already knows:

- The function does I/O (`http/get`), can error (network, parse), and
  propagates whatever `transform` does (parameter 1 is polymorphic).
- There is a silent nil path: `(get :body)` returns nil on missing key,
  which feeds into `json/parse`, which errors on nil input.  No guard
  exists between them.
- The I/O is not idempotent in general.  The pure middle section (field
  extraction, JSON parsing) could be memoized independently.
- If `transform` is silent, the JIT can compile the tail call.  If it
  yields, the entire call chain suspends.

None of this was declared.  All of it was inferred.  Today it lives inside
the compiler and is discarded after bytecode emission.

The living model **retains this analysis, makes it queryable, keeps it in
sync with the source, and projects it into views** that reveal the
computation the programmer specified — including the parts they didn't
realize they specified.

### What the user sees

For a single function: an **effect profile** showing phases (I/O → pure →
delegated), failure modes, composition properties (retry-safe? timeout-safe?
parallelizable?), and latent issues (unguarded nil paths, unsandboxed
delegation).

For a module: a **signal topology** showing where effects live, where the
boundaries are between pure and impure code, and which functions delegate
to user-controlled closures.

For a whole program: an **architecture map** derived from signal
transitions, capture coupling, and trust boundaries — not from declarations
or diagrams, but from what the code actually does.

### Why this is defensible

Every view is powered by information that only exists because of Elle's
compilation pipeline: signal inference, capture analysis, binding arena,
escape analysis.  You cannot bolt this onto Python (no static signals),
JavaScript (no binding arena), or Rust (information is distributed across
type signatures, not centrally queryable).  Reproducing the living model
means reproducing the language.

---

## Architecture

```
┌──────────────┐     ┌──────────────┐     ┌──────────────────┐
│  Elle source │────▸│  Pipeline    │────▸│  Model store     │
│  (.lisp)     │     │  analyze()   │     │  (oxigraph)      │
└──────────────┘     └──────────────┘     └──────────────────┘
                           │                       │
                    ┌──────┴──────┐         ┌──────┴──────┐
                    │ Primitives  │         │ MCP server  │
                    │ compile/*   │         │ SPARQL      │
                    └─────────────┘         └─────────────┘
                           │                       │
                    ┌──────┴──────┐         ┌──────┴──────┐
                    │ Elle code   │         │ AI agents   │
                    │ (portraits, │         │ (queries,   │
                    │  transforms)│         │  transforms)│
                    └─────────────┘         └─────────────┘
```

Two access paths to the same underlying analysis:

1. **Primitives** (`compile/*`) — Elle code calls directly into the
   pipeline.  Returns structured Elle values.  Immediate, synchronous,
   scoped to a single analysis.  Used by Elle programs that reason about
   other Elle programs.

2. **MCP server** — AI agents query via SPARQL over a persistent knowledge
   graph.  The graph is populated by running the pipeline extractors.
   Used for cross-file queries, historical analysis, and agent tooling.

Both paths use the same pipeline entry point: `analyze_file()`.  The
primitives wrap it for in-process use; the extractors run it and emit
triples.

---

## Phase 1: Compiler as library

**Goal:** Elle code can analyze Elle source and query the results.
**Effort:** ~3 evenings.  Core primitives + struct construction.
**Value:** Immediate.  Replaces `read-all`-based extractors with full
analysis.  Agents get signals, bindings, captures, diagnostics.

### 1.1 New primitive module: `src/primitives/compile.rs`

Register as category `"compile"` in `ALL_TABLES`.

#### `compile/analyze`

```lisp
(compile/analyze source)
(compile/analyze source {:file "name.lisp"})
```

Runs `analyze_file(source, symbols, vm, file_name)`.  Returns an opaque
`External` value wrapping the `AnalyzeResult` and associated metadata:

```rust
struct AnalysisHandle {
    hir: Hir,
    arena: BindingArena,
    symbols: SymbolIndex,
    diagnostics: Vec<Diagnostic>,
    call_graph: CallGraph,        // built lazily on first query
    signal_map: SignalMap,        // built lazily on first query
}
```

Wrapped as `Value::external("analysis", handle)`.

**Why opaque?**  The HIR tree is large.  Converting the entire tree to Elle
values would be expensive and rarely useful.  Instead, the analysis handle
is a query target: other `compile/*` primitives accept it and extract
specific views.

**Signal:** `Signal::errors()` — analysis can fail on malformed source.

**Implementation notes:**
- Uses `context::get_symbol_table()` for symbol interning.
- Uses `context::get_vm_context()` for macro expansion (the expander
  needs a VM).
- The handle stores the `BindingArena` by value (moved from `AnalyzeResult`).
- The `SymbolIndex` is built eagerly via `extract_symbols_from_hir`.
- The `Diagnostic` list is built eagerly via `HirLinter`.
- `CallGraph` and `SignalMap` are built on first access (see §1.4).

#### `compile/diagnostics`

```lisp
(compile/diagnostics analysis)
#=> [{:severity :warning
#     :code "W0002"
#     :rule "unused-binding"
#     :message "binding 'tmp' is never used"
#     :line 14 :col 8
#     :suggestions ["remove the binding" "prefix with _"]}]
```

Returns an immutable array of immutable structs.  Each struct maps
directly from `Diagnostic`:

```rust
fn diagnostic_to_value(d: &Diagnostic) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(kw("severity"), Value::keyword(match d.severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }));
    fields.insert(kw("code"), Value::string(&d.code));
    fields.insert(kw("rule"), Value::string(&d.rule));
    fields.insert(kw("message"), Value::string(&d.message));
    if let Some(loc) = &d.location {
        fields.insert(kw("line"), Value::int(loc.line as i64));
        fields.insert(kw("col"), Value::int(loc.col as i64));
    }
    fields.insert(kw("suggestions"), Value::array(
        d.suggestions.iter().map(|s| Value::string(s)).collect()
    ));
    Value::struct_from(fields)
}
```

**Signal:** `Signal::silent()` — pure extraction from handle.

#### `compile/symbols`

```lisp
(compile/symbols analysis)
#=> [{:name "fetch-page" :kind :function :line 12 :col 0
#     :arity 2 :doc "Fetch a URL and return parsed body."}
#    {:name "config" :kind :variable :line 3 :col 0}]
```

Returns an immutable array of `SymbolDef` structs.  Maps from
`SymbolIndex.definitions`.

#### `compile/bindings`

```lisp
(compile/bindings analysis)
#=> [{:name "url" :scope :parameter :mutated false
#     :captured false :immutable false :line 12 :col 20}
#    {:name "config" :scope :local :mutated true
#     :captured true :immutable false :line 3 :col 5}]
```

Returns an immutable array from the `BindingArena`.  Each binding maps:

| Field | Source |
|-------|--------|
| `:name` | `symbols.name(inner.name)` |
| `:scope` | `:parameter` or `:local` |
| `:mutated` | `inner.is_mutated` |
| `:captured` | `inner.is_captured` |
| `:immutable` | `inner.is_immutable` |
| `:needs-lbox` | `inner.needs_lbox()` |
| `:line`, `:col` | from `SymbolIndex.symbol_locations` |

### 1.2 Signal query primitives

These are the highest-value primitives.  No other language can offer them.

#### `compile/signal`

```lisp
(compile/signal analysis :fetch-page)
#=> {:bits |:io :error| :propagates || :silent false :yields true
#    :io true :jit-eligible false}
```

Looks up the named function in the analysis.  Returns its `Signal` as a
struct.

**Implementation:**

1. Look up the name in `SymbolIndex.definitions` to get the `SymbolId`.
2. Find the corresponding `Define` or `Letrec` binding in the HIR.
3. If the value is a `Lambda`, read `inferred_signals`.
4. Format `bits` as a set of keywords using `SignalRegistry::format_signal_bits`.
5. Format `propagates` as a set of parameter indices.
6. Compute derived fields: `silent` = bits == 0 && propagates == 0,
   `yields` = `may_suspend()`, `io` = bits contains `SIG_IO`,
   `jit-eligible` = `!may_suspend()`.

```rust
fn signal_to_value(sig: &Signal, registry: &SignalRegistry) -> Value {
    let mut fields = BTreeMap::new();

    // :bits as keyword set
    let mut bit_set = BTreeSet::new();
    for entry in registry.entries() {
        if sig.bits.0 & (1 << entry.bit_position) != 0 {
            bit_set.insert(Value::keyword(&entry.name));
        }
    }
    fields.insert(kw("bits"), Value::set(bit_set));

    // :propagates as integer set
    let mut prop_set = BTreeSet::new();
    for i in 0..32 {
        if sig.propagates & (1 << i) != 0 {
            prop_set.insert(Value::int(i));
        }
    }
    fields.insert(kw("propagates"), Value::set(prop_set));

    // Derived convenience booleans
    fields.insert(kw("silent"), Value::bool(sig.is_silent()));
    fields.insert(kw("yields"), Value::bool(sig.may_suspend()));
    fields.insert(kw("io"), Value::bool(sig.bits.0 & SIG_IO.0 != 0));
    fields.insert(kw("jit-eligible"), Value::bool(!sig.may_suspend()));

    Value::struct_from(fields)
}
```

#### `compile/query-signal`

```lisp
# All functions with a specific signal
(compile/query-signal analysis :io)
#=> [{:name "fetch-page" :line 42} {:name "save-data" :line 87}]

# All silent functions
(compile/query-signal analysis :silent)
#=> [{:name "transform" :line 12} {:name "validate" :line 30}]

# All JIT-eligible functions
(compile/query-signal analysis :jit-eligible)
#=> [{:name "transform" :line 12} ...]
```

Walks the symbol index.  For each function-kind symbol, checks whether its
signal matches the query.  `:silent` and `:jit-eligible` are virtual
queries (not signal bits).

#### `compile/signal-of`

```lisp
# Signal of an arbitrary expression (not just named functions)
(compile/signal-of analysis 42 8)  # line 42, col 8
#=> {:bits |:io| :propagates ||}
```

Walks the HIR to find the node at the given source location.  Returns
that node's `hir.signal`.  Useful for understanding what a specific
subexpression contributes.

### 1.3 Capture and binding query primitives

#### `compile/captures`

```lisp
(compile/captures analysis :make-handler)
#=> [{:name "config" :kind :value :mutated false}
#    {:name "counter" :kind :lbox :mutated true}]
```

Finds the Lambda for the named function.  Returns its `captures` vec
as structs.

**Implementation:** For each `CaptureInfo`, look up the binding in the
arena:

```rust
fn capture_to_value(cap: &CaptureInfo, arena: &BindingArena, symbols: &SymbolTable) -> Value {
    let inner = arena.get(cap.binding);
    let mut fields = BTreeMap::new();
    fields.insert(kw("name"), Value::string(symbols.name(inner.name).unwrap_or("?")));
    fields.insert(kw("kind"), Value::keyword(match cap.kind {
        CaptureKind::Local => if inner.needs_lbox() { "lbox" } else { "value" },
        CaptureKind::Capture { .. } => "transitive",
    }));
    fields.insert(kw("mutated"), Value::bool(inner.is_mutated));
    Value::struct_from(fields)
}
```

#### `compile/captured-by`

```lisp
(compile/captured-by analysis :config)
#=> [{:name "make-handler" :line 20} {:name "setup" :line 45}]
```

Reverse lookup: walks all Lambdas in the HIR, checks each capture list
for the named binding, returns the enclosing function names.

#### `compile/binding`

```lisp
(compile/binding analysis :counter)
#=> {:scope :local :mutated true :captured true :immutable false
#    :needs-lbox true :captured-by ["make-handler" "reset"]
#    :usages [{:line 25 :col 4} {:line 30 :col 8}]}
```

Combines arena metadata with symbol index usage data.  Single-binding
deep dive.

### 1.4 Call graph primitives

Built lazily on first query from the `AnalysisHandle`.

#### `compile/callers`

```lisp
(compile/callers analysis :fetch-page)
#=> [{:name "main" :line 50 :tail false}
#    {:name "retry-loop" :line 35 :tail true}]
```

#### `compile/callees`

```lisp
(compile/callees analysis :main)
#=> [{:name "fetch-page" :line 50 :tail false}
#    {:name "process" :line 51 :tail true}]
```

#### `compile/call-graph`

```lisp
(compile/call-graph analysis)
#=> {:nodes [{:name "main" :callees ["fetch" "process"] :callers []}
#            {:name "fetch" :callees ["http/get"] :callers ["main"]}]
#    :roots ["main"]
#    :leaves ["http/get" "json/parse"]}
```

Returns the full graph as a struct.  `roots` = functions with no callers.
`leaves` = functions that call nothing (or only primitives).

**Implementation:**

```rust
struct CallGraph {
    edges: HashMap<String, Vec<CallEdge>>,
}

struct CallEdge {
    callee: String,
    span: Span,
    is_tail: bool,
}
```

Built by walking the HIR once.  For each `HirKind::Call`, resolve the func
position:
- `Var(binding)` → look up name in arena.  Named edge.
- `Lambda` → anonymous call.  Skip or label as `<lambda>`.
- Other expression → indirect call.  Mark as `<indirect>`.

Track the "current function" as the enclosing `Define` or `Letrec` binding
name.

### 1.5 Registration

In `src/primitives/compile.rs`:

```rust
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "compile/analyze",
        func: prim_compile_analyze,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Analyze Elle source text, returning an analysis handle for queries.",
        params: &["source", "opts"],
        category: "compile",
        example: r#"(compile/analyze "(defn f [x] (+ x 1))")"#,
        aliases: &[],
    },
    // ... compile/diagnostics, compile/symbols, compile/bindings,
    //     compile/signal, compile/query-signal, compile/signal-of,
    //     compile/captures, compile/captured-by, compile/binding,
    //     compile/callers, compile/callees, compile/call-graph
];
```

Add `compile::PRIMITIVES` to `ALL_TABLES` in `registration.rs`.

### 1.6 Validation

New test file: `tests/elle/compile-analyze.lisp`

```lisp
# Basic analysis
(def src "(defn add [a b] (+ a b))")
(def a (compile/analyze src))

# Diagnostics
(assert (array? (compile/diagnostics a)))

# Symbols
(def syms (compile/symbols a))
(assert (= (get (first syms) :name) "add"))
(assert (= (get (first syms) :kind) :function))

# Signal — add is silent (just arithmetic)
(def sig (compile/signal a :add))
(assert (get sig :silent))
(assert (get sig :jit-eligible))

# Bindings
(def b (compile/binding a :a))
(assert (= (get b :scope) :parameter))
(assert (not (get b :mutated)))

# I/O function
(def src2 "(defn greet [name] (println name))")
(def a2 (compile/analyze src2))
(def sig2 (compile/signal a2 :greet))
(assert (not (get sig2 :silent)))
(assert (get sig2 :io))
```

---

## Phase 2: Semantic portrait

**Goal:** Given an analysis, produce human-readable and agent-readable
summaries that surface non-obvious properties.
**Effort:** ~2 evenings.  Pure Elle, built on Phase 1 primitives.
**Value:** High.  This is what makes the model "living" — the user sees
their code annotated with derived knowledge.

### 2.1 Function portrait

A pure Elle function that takes an analysis handle and a function name
and returns a structured description:

```lisp
# lib/portrait.lisp

(defn function-portrait [analysis name]
  "Build a semantic portrait of a named function."
  (let* [[sig     (compile/signal analysis name)]
         [caps    (compile/captures analysis name)]
         [callers (compile/callers analysis name)]
         [callees (compile/callees analysis name)]
         [binding (compile/binding analysis name)]]

    # Classify phases by walking callees and their signals
    (let* [[phases (classify-phases analysis callees)]
           [failures (detect-failure-modes analysis name callees)]
           [composition (assess-composition sig caps)]
           [observations (find-observations analysis name sig caps
                                            callees phases failures)]]

      {:name name
       :signal sig
       :phases phases
       :captures caps
       :failures failures
       :composition composition
       :observations observations
       :callers callers
       :callees callees})))
```

#### Phase classification

Walk the callees in source order.  Classify each into a phase based on
its signal:

```lisp
(defn classify-phases [analysis callees]
  "Group callees into sequential phases by effect type."
  (var current-kind :pure)
  (var current-fns @[])
  (var phases @[])

  (each callee in callees
    (let* [[sig (compile/signal analysis (get callee :name))]
           [kind (cond
                   ((get sig :io)     :io)
                   ((get sig :yields) :suspending)
                   ((not (empty? (get sig :propagates))) :delegated)
                   (true              :pure))]]
      (when (not (= kind current-kind))
        (when (not (empty? current-fns))
          (push phases {:kind current-kind :functions (freeze current-fns)}))
        (assign current-kind kind)
        (assign current-fns @[]))
      (push current-fns (get callee :name))))

  (when (not (empty? current-fns))
    (push phases {:kind current-kind :functions (freeze current-fns)}))

  (freeze phases))
```

#### Failure mode detection

```lisp
(defn detect-failure-modes [analysis name callees]
  "Identify how this function can fail."
  (var modes @[])

  (each callee in callees
    (let [[sig (compile/signal analysis (get callee :name))]]
      (when (contains? (get sig :bits) :error)
        (push modes {:source (get callee :name)
                     :line (get callee :line)
                     :kind :error}))))

  # Detect unguarded nil paths (see §2.3)
  (each path in (detect-nil-paths analysis name)
    (push modes {:source (get path :producer)
                 :line (get path :line)
                 :kind :unguarded-nil
                 :consumer (get path :consumer)}))

  (freeze modes))
```

#### Composition assessment

```lisp
(defn assess-composition [sig captures]
  "Determine composition properties from signal and captures."
  (let* [[has-mutable-capture (some? (fn [c] (get c :mutated)) captures)]
         [has-io (get sig :io)]
         [has-any-capture (not (empty? captures))]]
    {:retry-safe (not has-io)
     :timeout-safe (not has-mutable-capture)
     :parallelizable (not has-mutable-capture)
     :memoizable (and (get sig :silent) (not has-any-capture))
     :jit-eligible (get sig :jit-eligible)
     :stateless (empty? captures)}))
```

#### Observation generation

This is the most valuable part: **surfacing implicit decisions.**

```lisp
(defn find-observations [analysis name sig caps callees phases failures]
  "Generate observations about non-obvious properties."
  (var obs @[])

  # 1. Almost-pure detection
  (when (and (not (get sig :silent))
             (= 1 (length (filter (fn [c] (= (get c :kind) :io))
                                  callees))))
    (let [[io-callee (first (filter (fn [c]
                              (get (compile/signal analysis (get c :name)) :io))
                            callees))]]
      (push obs {:kind :almost-pure
                 :message (string/format
                   "Only I/O source is {} at line {}. \
                    Factoring it out makes the rest JIT-eligible."
                   (get io-callee :name) (get io-callee :line))})))

  # 2. Mutable capture shared across fibers
  (each cap in caps
    (when (get cap :mutated)
      (let [[captured-by (compile/captured-by analysis
                           (keyword (get cap :name)))]]
        (when (> (length captured-by) 1)
          (push obs {:kind :shared-mutable
                     :message (string/format
                       "Mutable binding '{}' is captured by {} functions. \
                        Concurrent fibers will race."
                       (get cap :name) (length captured-by))})))))

  # 3. Unsandboxed delegation
  (when (not (empty? (get sig :propagates)))
    (let [[param-indices (get sig :propagates)]]
      (each idx in param-indices
        (push obs {:kind :unsandboxed-delegation
                   :message (string/format
                     "Parameter {} is called without signal bounds. \
                      A malicious or buggy closure could do arbitrary I/O, \
                      yield indefinitely, or error to crash the caller. \
                      Consider (silence param) or a fuel budget."
                     idx)}))))

  # 4. All-tail-call chain (could be a loop)
  (when (every? (fn [c] (get c :tail)) callees)
    (when (> (length callees) 0)
      (push obs {:kind :tail-chain
                 :message "All calls are in tail position. \
                           This function is a state machine candidate."})))

  # 5. Capture-by-value of mutable source
  (each cap in caps
    (when (and (= (get cap :kind) :value) (not (get cap :mutated)))
      (let [[binding (compile/binding analysis (keyword (get cap :name)))]]
        (when (get binding :mutated)
          (push obs {:kind :stale-capture
                     :message (string/format
                       "Captures '{}' by value, but '{}' is mutated elsewhere. \
                        This closure sees the value at capture time, not mutations."
                       (get cap :name) (get cap :name))})))))

  (freeze obs))
```

### 2.2 Module portrait

```lisp
(defn module-portrait [analysis]
  "Build a signal topology for an entire module."
  (let* [[syms (compile/symbols analysis)]
         [fns (filter (fn [s] (= (get s :kind) :function)) syms)]
         [fn-names (map (fn [s] (get s :name)) fns)]]

    # Classify functions by signal
    (var pure @[])
    (var io-boundary @[])
    (var delegating @[])
    (var yielding @[])

    (each name in fn-names
      (let [[sig (compile/signal analysis (keyword name))]]
        (cond
          ((get sig :silent) (push pure name))
          ((not (empty? (get sig :propagates))) (push delegating name))
          ((get sig :io) (push io-boundary name))
          ((get sig :yields) (push yielding name))
          (true (push io-boundary name)))))

    # Find signal boundaries: edges where signal changes
    (var boundaries @[])
    (let [[graph (compile/call-graph analysis)]]
      (each node in (get graph :nodes)
        (let [[caller-sig (compile/signal analysis (keyword (get node :name)))]]
          (each callee-name in (get node :callees)
            (let [[callee-sig (compile/signal analysis (keyword callee-name))]]
              (when (not (= (get caller-sig :silent) (get callee-sig :silent)))
                (push boundaries {:caller (get node :name)
                                  :callee callee-name
                                  :transition (if (get caller-sig :silent)
                                                :pure-to-impure
                                                :impure-to-pure)})))))))

    {:pure (freeze pure)
     :io-boundary (freeze io-boundary)
     :delegating (freeze delegating)
     :yielding (freeze yielding)
     :boundaries (freeze boundaries)
     :roots (get (compile/call-graph analysis) :roots)
     :leaves (get (compile/call-graph analysis) :leaves)}))
```

### 2.3 Nil path detection

One of the most valuable non-obvious observations.  Detects paths where a
nil-producing expression feeds into a nil-intolerant expression with no
guard between them.

**This requires tracking which primitives can return nil and which error
on nil input.**  We encode this as metadata on primitive definitions.

```lisp
(defn detect-nil-paths [analysis name]
  "Find data flow paths where nil can silently propagate into an error."
  # Implementation requires the data flow graph from Phase 3.
  # Placeholder for Phase 1: return empty.
  [])
```

Full implementation deferred to Phase 3, which adds data flow tracking.

### 2.4 Portrait rendering

Portraits are data.  Rendering is separate.  Multiple renderers:

```lisp
# Text rendering (for terminal / agent consumption)
(defn render-portrait-text [portrait]
  (let [[name (get portrait :name)]
        [sig (get portrait :signal)]
        [phases (get portrait :phases)]
        [comp (get portrait :composition)]]
    (var out @"")
    (push out (string/format "{} : ({}) → value\n\n"
               name (string/join ", " (map (fn [c] (get c :name))
                                           (get portrait :captures)))))
    (push out (string/format "Effects:    {}\n" (format-signal sig)))
    (push out (string/format "Phases:     {}\n" (format-phases phases)))

    (when (not (empty? (get portrait :failures)))
      (push out "\nFailure modes:\n")
      (each f in (get portrait :failures)
        (push out (string/format "  - {} at {} (line {})\n"
                   (get f :kind) (get f :source) (get f :line)))))

    (push out "\nComposition:\n")
    (each [k v] in (pairs comp)
      (push out (string/format "  {}: {}\n" k v)))

    (when (not (empty? (get portrait :observations)))
      (push out "\nObservations:\n")
      (each o in (get portrait :observations)
        (push out (string/format "  [{:8}] {}\n"
                   (get o :kind) (get o :message)))))

    (freeze out)))

# Struct rendering (for programmatic consumption by agents)
# The portrait itself IS the struct rendering — it's already structured data.

# RDF rendering (for knowledge graph ingestion)
(defn render-portrait-triples [portrait file]
  # Emits ntriples for loading into oxigraph — see Phase 3.
  ...)
```

---

## Phase 3: Knowledge graph enrichment

**Goal:** The knowledge graph contains full semantic data, not just
syntactic facts.  Signal topology, capture graphs, call graphs, and
composition properties are queryable via SPARQL.
**Effort:** ~2 evenings.  New extractor + schema.
**Value:** Transforms the MCP server from a code search tool into a
semantic reasoning substrate.

### 3.1 New graph schema

Namespace: `urn:elle:` (extending existing schema).

#### Function-level predicates

| Predicate | Range | Source |
|-----------|-------|--------|
| `elle:signal-bits` | keyword set literal | `Signal.bits` |
| `elle:signal-silent` | `"true"` / `"false"` | computed |
| `elle:signal-yields` | `"true"` / `"false"` | computed |
| `elle:signal-io` | `"true"` / `"false"` | computed |
| `elle:signal-propagates` | integer (param index) | `Signal.propagates` |
| `elle:jit-eligible` | `"true"` / `"false"` | computed |
| `elle:capture` | binding IRI | from Lambda captures |
| `elle:capture-kind` | `"value"` / `"lbox"` / `"transitive"` | `CaptureKind` |
| `elle:calls` | function IRI | call graph edge |
| `elle:calls-tail` | function IRI | tail call edge |
| `elle:phase` | `"pure"` / `"io"` / `"delegated"` | from portrait |
| `elle:stateless` | `"true"` / `"false"` | empty captures |
| `elle:retry-safe` | `"true"` / `"false"` | from composition |
| `elle:parallelizable` | `"true"` / `"false"` | from composition |
| `elle:memoizable` | `"true"` / `"false"` | from composition |

#### Binding-level predicates

| Predicate | Range | Source |
|-----------|-------|--------|
| `elle:scope` | `"parameter"` / `"local"` | `BindingScope` |
| `elle:mutated` | `"true"` / `"false"` | `is_mutated` |
| `elle:captured` | `"true"` / `"false"` | `is_captured` |
| `elle:immutable` | `"true"` / `"false"` | `is_immutable` |
| `elle:needs-lbox` | `"true"` / `"false"` | `needs_lbox()` |

#### Signal boundary predicates

| Predicate | Range | Source |
|-----------|-------|--------|
| `elle:signal-boundary` | boundary IRI | caller→callee signal transition |
| `elle:boundary-caller` | function IRI | the pure/impure caller |
| `elle:boundary-callee` | function IRI | the impure/pure callee |
| `elle:boundary-kind` | `"pure-to-impure"` etc. | transition type |

### 3.2 Semantic extractor

Replace `elle-graph.lisp` with a new extractor that uses `compile/analyze`:

```lisp
# tools/semantic-graph.lisp

(defn extract-file [file]
  "Extract full semantic triples from an Elle source file."
  (let [[[ok? src] (protect (file/read file))]]
    (when ok?
      (let [[[ok? analysis] (protect (compile/analyze src {:file file}))]]
        (when ok?
          (let [[syms (compile/symbols analysis)]
                [graph (compile/call-graph analysis)]]

            # Emit function-level triples
            (each sym in syms
              (when (= (get sym :kind) :function)
                (let* [[name (get sym :name)]
                       [subj (elle-iri "fn" name)]
                       [sig (compile/signal analysis (keyword name))]
                       [caps (compile/captures analysis (keyword name))]]

                  # Existing syntactic triples
                  (triple subj rdf-type (iri "urn:elle:Fn"))
                  (triple subj (pred "name") (lit name))
                  (triple subj (pred "file") (lit file))
                  (triple subj (pred "arity") (lit (string (get sym :arity))))
                  (when (get sym :doc)
                    (triple subj (pred "doc") (lit (get sym :doc))))

                  # NEW: Signal triples
                  (triple subj (pred "signal-silent")
                          (lit (string (get sig :silent))))
                  (triple subj (pred "signal-yields")
                          (lit (string (get sig :yields))))
                  (triple subj (pred "signal-io")
                          (lit (string (get sig :io))))
                  (triple subj (pred "jit-eligible")
                          (lit (string (get sig :jit-eligible))))
                  (each bit in (get sig :bits)
                    (triple subj (pred "signal-bits") (lit (string bit))))
                  (each idx in (get sig :propagates)
                    (triple subj (pred "signal-propagates")
                            (lit (string idx))))

                  # NEW: Capture triples
                  (each cap in caps
                    (triple subj (pred "capture")
                            (lit (get cap :name)))
                    (triple subj (pred "capture-kind")
                            (lit (string (get cap :kind)))))

                  # NEW: Composition triples
                  (let [[comp (assess-composition sig caps)]]
                    (triple subj (pred "stateless")
                            (lit (string (get comp :stateless))))
                    (triple subj (pred "retry-safe")
                            (lit (string (get comp :retry-safe))))
                    (triple subj (pred "parallelizable")
                            (lit (string (get comp :parallelizable))))
                    (triple subj (pred "memoizable")
                            (lit (string (get comp :memoizable))))))))

            # Emit call graph edges
            (each node in (get graph :nodes)
              (let [[caller-iri (elle-iri "fn" (get node :name))]]
                (each callee in (get node :callees)
                  (triple caller-iri (pred "calls")
                          (elle-iri "fn" callee)))))))))))
```

### 3.3 Example SPARQL queries

With the enriched graph, agents can ask questions that were previously
impossible:

```sparql
# All functions that do I/O and are called from pure functions
# (signal boundaries — architecture discovery)
SELECT ?pure_fn ?io_fn ?file WHERE {
  ?caller a <urn:elle:Fn> ;
          <urn:elle:signal-silent> "true" ;
          <urn:elle:calls> ?callee ;
          <urn:elle:name> ?pure_fn .
  ?callee a <urn:elle:Fn> ;
          <urn:elle:signal-io> "true" ;
          <urn:elle:name> ?io_fn ;
          <urn:elle:file> ?file .
}
```

```sparql
# Functions with mutable captures that are called from multiple places
# (potential race conditions)
SELECT ?fn ?capture ?caller_count WHERE {
  ?f a <urn:elle:Fn> ;
     <urn:elle:name> ?fn ;
     <urn:elle:capture> ?capture ;
     <urn:elle:capture-kind> "lbox" .
  {
    SELECT ?f (COUNT(DISTINCT ?caller) AS ?caller_count) WHERE {
      ?caller <urn:elle:calls> ?f .
    } GROUP BY ?f
  }
  FILTER(?caller_count > 1)
}
```

```sparql
# Memoization candidates: pure, stateless, called more than once
SELECT ?fn ?call_count WHERE {
  ?f a <urn:elle:Fn> ;
     <urn:elle:name> ?fn ;
     <urn:elle:memoizable> "true" .
  {
    SELECT ?f (COUNT(?caller) AS ?call_count) WHERE {
      ?caller <urn:elle:calls> ?f .
    } GROUP BY ?f
  }
  FILTER(?call_count > 1)
}
ORDER BY DESC(?call_count)
```

```sparql
# Find the deepest I/O call chains (latency-sensitive paths)
# This is a recursive query — oxigraph supports property paths
SELECT ?root ?leaf (COUNT(?mid) AS ?depth) WHERE {
  ?root a <urn:elle:Fn> ;
        <urn:elle:signal-io> "true" .
  ?root <urn:elle:calls>+ ?mid .
  ?mid <urn:elle:calls> ?leaf .
  ?leaf <urn:elle:signal-io> "true" .
  FILTER NOT EXISTS { ?leaf <urn:elle:calls> ?deeper .
                      ?deeper <urn:elle:signal-io> "true" }
} GROUP BY ?root ?leaf
ORDER BY DESC(?depth)
```

---

## Phase 4: MCP tools for agents

**Goal:** AI agents interact with the living model through high-level MCP
tools, not raw SPARQL.  The MCP server becomes the semantic interface for
agentic development.
**Effort:** ~3 evenings.  New MCP tools + portrait integration.
**Value:** Very high.  This is the agent-facing product.

### 4.1 New MCP tools

Added to the MCP server alongside existing SPARQL tools:

#### `analyze_file`

```json
{
  "name": "analyze_file",
  "description": "Analyze an Elle source file and load its semantic data into the knowledge graph. Returns a summary of what was found.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "Path to .lisp file" }
    },
    "required": ["path"]
  }
}
```

Reads the file, runs `compile/analyze`, generates the semantic portrait,
emits triples into the store, and returns a summary:

```json
{
  "functions": 12,
  "pure": 8,
  "io": 3,
  "delegating": 1,
  "diagnostics": 2,
  "observations": [
    "make-handler captures mutable 'counter' shared by 2 closures",
    "transform parameter unsandboxed in handle-request"
  ]
}
```

#### `portrait`

```json
{
  "name": "portrait",
  "description": "Generate a semantic portrait of a function, module, or expression. Shows effect profile, failure modes, composition properties, and non-obvious observations.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": { "type": "string" },
      "function": { "type": "string", "description": "Function name (omit for module portrait)" }
    },
    "required": ["path"]
  }
}
```

Returns the rendered portrait text (for function) or module topology (for
file).

#### `signal_query`

```json
{
  "name": "signal_query",
  "description": "Query functions by their signal properties. Find all pure functions, all I/O functions, all functions that propagate a parameter, etc.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": { "type": "string" },
      "query": {
        "type": "string",
        "enum": ["silent", "io", "yields", "jit-eligible", "delegating", "errors"],
        "description": "Signal property to filter by"
      }
    },
    "required": ["path", "query"]
  }
}
```

#### `impact`

```json
{
  "name": "impact",
  "description": "Assess the impact of changing a function. Returns callers, downstream signal changes, and affected tests.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": { "type": "string" },
      "function": { "type": "string" },
      "proposed_signal": {
        "type": "string",
        "description": "Proposed new signal (e.g., 'io' if adding I/O to a pure function)"
      }
    },
    "required": ["path", "function"]
  }
}
```

Impact analysis:
1. Find all callers (transitive) of the named function.
2. If `proposed_signal` is given, compute what the callers' signals would
   become if the function's signal changed.
3. Flag callers that currently rely on the function being silent (JIT,
   `silence` bounds, etc.).
4. Return the set of affected functions with before/after signals.

#### `verify_invariants`

```json
{
  "name": "verify_invariants",
  "description": "Check project invariants encoded as SPARQL queries. Returns pass/fail for each invariant.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "invariants_path": {
        "type": "string",
        "description": "Path to .lisp file defining invariants (default: .elle-invariants.lisp)"
      }
    }
  }
}
```

Invariant file format:

```lisp
# .elle-invariants.lisp

[{:name "no-exec-in-lib"
  :description "Library functions must not spawn subprocesses"
  :query "ASK { ?f a <urn:elle:Fn> ;
                   <urn:elle:file> ?file ;
                   <urn:elle:signal-bits> \":exec\" .
                FILTER(STRSTARTS(?file, \"lib/\")) }"
  :expect false}

 {:name "all-public-documented"
  :description "All exported functions have docstrings"
  :query "ASK { ?f a <urn:elle:Fn> ;
                   <urn:elle:file> \"lib/http.lisp\" .
                FILTER NOT EXISTS { ?f <urn:elle:doc> ?d } }"
  :expect false}

 {:name "no-mutable-captures-in-handlers"
  :description "Request handlers should not capture mutable state"
  :query "ASK { ?f a <urn:elle:Fn> ;
                   <urn:elle:name> ?name ;
                   <urn:elle:capture-kind> \"lbox\" .
                FILTER(CONTAINS(?name, \"handle\")) }"
  :expect false}]
```

### 4.2 Integration with graph extractors

The existing `tools/load-all.lisp` is updated to use the semantic
extractor (Phase 3).  The MCP server's `analyze_file` tool calls the same
code path, so the graph stays consistent whether populated by batch
extraction or incremental analysis.

### 4.3 Agent workflow

A complete agent workflow using these tools:

```
Agent: "Add caching to fetch-page"

1. Call `portrait` on fetch-page
   → Learns: does I/O, not memoizable (has I/O), stateless
   → Observes: "fetch-page is called from 3 places, all pass constant URLs"

2. Call `signal_query` for silent functions
   → Finds: transform, validate are pure → cache after fetch, before transform

3. Call `impact` with proposed_signal="silent" on a new cached-fetch
   → Learns: callers' signals change from io to silent
   → Flags: 2 callers have test assertions checking for I/O signal

4. Write the cached version, call `analyze_file` to verify
   → Diagnostics clean, signal matches expectation

5. Call `verify_invariants` to check project rules
   → All pass

6. Submit PR with semantic annotation:
   "fetch-page → cached-fetch: signal changed from {:io :error} to {:error}.
    3 callers affected. 2 test updates needed."
```

---

## Phase 5: Live model

**Status:** Complete.

**Done:**
- Rust primitives are first-class RDF nodes (`urn:elle:Primitive`), loaded
  into the store on MCP server startup via `compile/primitives`.
- `analyze_file` populates the RDF store incrementally (clear old triples,
  insert new), bridging the gap between analysis cache and knowledge graph.
- Shared triple-generation library (`lib/rdf.lisp`) used by both the batch
  extractor and the MCP server — single canonical schema.
- Watch plugin (`plugins/watch/`) wrapping `notify` crate for filesystem
  events, with debouncing and `Arc<Mutex<VecDeque>>` event queue.
- Fiber-aware wrapper (`lib/watch.lisp`) with `watch:for-each`, extension
  filtering, and `ev/sleep`-based polling.
- MCP server spawns a watcher fiber on startup that re-analyzes `.lisp`
  files on change, diffs signals, and emits `notifications/model/updated`.
- Graceful degradation: MCP server works without the watch plugin.

**Goal:** The model updates in real-time as source changes.  No manual
re-extraction needed.
**Value:** The model becomes the persistent development companion.

### 5.1 File watcher integration

The MCP server gains a file watcher (using `notify` crate or polling):

```lisp
# In MCP server startup:
(defn watch-directory [store dir]
  "Watch for .lisp file changes and re-analyze."
  (let [[watcher (file/watch dir)]]
    (forever
      (let [[event (file/watch-next watcher)]]
        (when (and (string/ends-with? (get event :path) ".lisp")
                   (contains? |:create :modify| (get event :kind)))
          (log "re-analyzing: " (get event :path))
          (re-analyze-file store (get event :path)))))))
```

### 5.2 Incremental graph update

When a file changes:

1. Delete all triples with `elle:file` = changed file.
2. Re-analyze the file.
3. Insert new triples.
4. Recompute affected cross-file edges (callers from other files).

```sparql
# Step 1: Clear old data for a file
DELETE WHERE {
  ?s <urn:elle:file> "lib/http.lisp" .
  ?s ?p ?o .
}
```

### 5.3 Notification protocol

The MCP server emits notifications when the model changes:

```json
{
  "jsonrpc": "2.0",
  "method": "model/updated",
  "params": {
    "file": "lib/http.lisp",
    "functions_changed": ["fetch-page", "http/get"],
    "signal_changes": [
      {"name": "fetch-page", "before": {"io": true}, "after": {"io": true, "exec": true}}
    ],
    "new_observations": [
      "fetch-page now spawns subprocesses — verify this is intentional"
    ]
  }
}
```

Agents subscribed to the MCP server see these notifications and can react:
re-run invariant checks, update downstream analyses, flag unexpected
signal changes.

---

## Phase 6: Program transformation ✓

**Status:** Complete.  Four transformation primitives implemented using
span-based patching (approach A from 6.5).

**Goal:** Agents request semantic transformations through the model.  The
compiler guarantees correctness.
**Value:** The moat.  No other language can offer verified program
transformation through a query interface.

### 6.1 Safe rename

```lisp
(compile/rename analysis :old-name :new-name)
#=> {:diff "..." :affected-files ["lib/http.lisp" "tests/http-test.lisp"]}
```

Uses the binding arena to find every reference to the binding.  Generates
a diff.  No false positives (shadowed names are different bindings), no
false negatives (all references are in the arena).

**Implementation:** Walk the HIR.  For each `Var(binding)` where
`arena.get(binding).name` matches the target, record the span.  Generate
text replacements from spans.

### 6.2 Extract function

```lisp
(compile/extract analysis {:from :process-data :lines [15 20]
                            :name :parse-section})
#=> {:new-function "(defn parse-section [data config] ...)"
#    :replacement "(parse-section data config)"
#    :captures ["data" "config"]
#    :signal {:bits |:error| :propagates ||}}
```

1. Find the HIR subtree spanning lines 15-20.
2. Compute its free variables (references to bindings defined outside the
   range).  These become parameters.
3. Compute its signal from the HIR node.
4. Generate the new function definition.
5. Generate the replacement call site.

**The captures list and signal are computed by the compiler**, not guessed.
The extracted function is guaranteed to have the correct arity and signal.

### 6.3 Add signal handling

```lisp
(compile/add-handler analysis :fetch-page :error)
#=> {:diff "..."
#    :wraps "(protect (fetch-page url))"
#    :handler "(if ok? result (begin (log \"fetch failed\") nil))"}
```

Knows what signals `fetch-page` can emit (from the analysis).  Generates
appropriate wrapping.  If the function only errors, generates `protect`.
If it yields, generates fiber wrapping.  If it does I/O, generates
timeout wrapping.

### 6.4 Parallelize

```lisp
(compile/parallelize analysis [:fetch-a :fetch-b :fetch-c])
#=> {:safe true
#    :reason "No shared mutable captures between any pair."
#    :code "(let [[results (ev/map (fn [f] (f)) [fetch-a fetch-b fetch-c])]] ...)"
#    :signal {:bits |:io :error| :propagates ||}}
```

Or:

```lisp
(compile/parallelize analysis [:update-counter :update-state])
#=> {:safe false
#    :reason "Both capture mutable binding 'state' via lbox."
#    :shared-captures [{:name "state" :kind :lbox}]}
```

Uses capture analysis to verify no mutable capture overlap.  Uses signal
analysis to verify no ordering dependency.

### 6.5 HIR-to-source reconstruction

All transformations need to produce valid Elle source text.  This requires
a **HIR-to-source** function.

Two approaches:

**A. Span-based patching** (simpler, Phase 6a).  Use the original source
text and replace specific spans.  Works for rename, extract (cut+paste),
and wrapping (insert around span).  Doesn't work for transformations that
create new code.

**B. HIR pretty-printing** (fuller, Phase 6b).  Walk the HIR and emit
Elle source.  Requires handling all `HirKind` variants.  Loses original
formatting but can generate entirely new code.

Recommend: start with (A) for rename/extract/wrap, add (B) when needed
for synthetic code generation.

---

## Language changes

Some observations worth considering — changes that would make the living
model more powerful.  None are blockers for Phases 1-4; all would enhance
Phase 5-6 and beyond.

### L1. `compile/analyze` as a special form

Currently, `compile/analyze` would be a primitive that accesses the symbol
table and VM via thread-local context.  If it were a **special form**
recognized by the analyzer, the compiler could track the signal
implications:

```lisp
# The compiler knows that compile/analyze is pure analysis — no I/O,
# no side effects, no yield.  It can inline the result in JIT contexts.
(def analysis (compile/analyze src))  # Signal: errors
```

This is a minor optimization but establishes the precedent that the
compiler knows about itself.

### L2. Signal annotations as values

Currently signals are bitmasks.  If signal descriptions were first-class
Elle values (keyword sets), the portrait code could work with them more
naturally:

```lisp
# Today (would need to be):
(contains? (get sig :bits) :io)

# With signal-as-set:
(contains? (signal-of fetch-page) :io)
```

Consider: `(signal-of name)` as a primitive that returns the closure's
runtime signal bits as a keyword set.  This is different from
`compile/signal` (which takes an analysis handle and works on source text)
— `signal-of` works on live closures at runtime:

```lisp
(defn safe-call [f]
  "Call f only if it's silent."
  (if (empty? (signal-of f))
    (f)
    (error :signal-error "expected silent function")))
```

This gives Elle a **runtime signal query** in addition to the compile-time
inference.  The information is already on the closure (`closure.signal`) —
this just exposes it.

### L3. Data flow tracking in HIR

Phase 2's nil-path detection requires knowing which expressions can
produce nil and which are nil-intolerant.  This information isn't in the
current HIR.

**Option A: Type tags on primitives.**  Add a `returns` field to
`PrimitiveDef`:

```rust
pub struct PrimitiveDef {
    // ... existing fields ...
    /// What this primitive might return (for data flow analysis).
    pub returns: ReturnSpec,
}

pub enum ReturnSpec {
    /// Unknown / varies
    Any,
    /// Always returns a value of this type
    Always(&'static str),
    /// May return nil (e.g., get on missing key)
    Nullable,
    /// Never returns nil
    NonNil,
}
```

This is a small, backward-compatible change.  The analyzer can use it to
propagate nullability through the HIR.

**Option B: Full type inference.**  Out of scope for this spec, but the
natural long-term evolution.  Start with nilability tracking (Option A),
expand to a type inference pass when the need is clear.

### L4. `(portrait name)` in the REPL

A REPL command that prints the semantic portrait of a function defined in
the current session:

```
elle> (defn add [a b] (+ a b))
elle> (portrait add)

add : (a, b) → value

Effects:    silent
Phases:     [pure: +]
Captures:   none
Composition:
  retry-safe: true
  timeout-safe: true
  parallelizable: true
  memoizable: true
  jit-eligible: true
```

This requires the REPL to retain the analysis handle for the current
session.  Not difficult — the REPL already has a persistent `VM` and
`SymbolTable`.  Add a persistent `AnalysisHandle` that accumulates
definitions.

### L5. Inline signal assertions

A new form that asserts signal properties at compile time:

```lisp
(defn handler [request transform]
  (assert-signal transform :silent)  # compile error if transform isn't bounded
  ...)
```

Today this is done with `(silence transform)` inside a lambda body.  But
`assert-signal` could work at any scope level and support partial
assertions (e.g., "no I/O" without requiring full silence).

```lisp
(assert-signal f :no-io)     # f must not have SIG_IO
(assert-signal f :no-exec)   # f must not have SIG_EXEC
(assert-signal f :silent)    # f must have zero signal bits
```

This gives programmers direct control over the signal contracts that the
living model surfaces as observations.  Instead of "unsandboxed
delegation" being an observation, it becomes an enforceable invariant.

### L6. Effect boundaries as module interfaces

When Elle gains a module system beyond `import`, consider:

```lisp
(module http
  (export fetch-page :signal |:io :error|)
  (export parse-url  :signal ||))
```

Explicit signal declarations on module exports.  The compiler verifies
that the implementation's inferred signal is a subset of the declared
signal.  Callers from other modules use the declared signal (stable
interface) rather than the inferred signal (implementation detail).

This is the natural endpoint of the signal system: **signals as module
interface contracts**, enforced by the compiler, visible in the knowledge
graph, stable across refactoring.

### L7. `@analysis` syntax for inline model queries

Speculative.  An annotation syntax that embeds model queries in source:

```lisp
(defn handler [request transform]
  @(assert-silent transform)
  @(assert-no-io inner-logic)
  ...)
```

The `@(...)` forms are evaluated at analysis time against the current
compilation unit.  They're compile-time assertions, not runtime code.
They vanish from the emitted bytecode.

This makes the living model bidirectional: not just "the compiler tells
you what your code does" but "you tell the compiler what your code must
do, and the compiler verifies."

---

## Appendix A: Signal bit reference

| Bit | Name | Constant | Compile-time | Description |
|-----|------|----------|--------------|-------------|
| 0 | error | `SIG_ERROR` | yes | May raise an error |
| 1 | yield | `SIG_YIELD` | yes | May suspend (cooperative) |
| 2 | debug | `SIG_DEBUG` | yes | May hit breakpoint |
| 3 | resume | `SIG_RESUME` | no | VM-internal: fiber resume |
| 4 | ffi | `SIG_FFI` | yes | Calls foreign code |
| 5 | propagate | `SIG_PROPAGATE` | no | VM-internal: re-raise |
| 6 | abort | `SIG_ABORT` | no | VM-internal: fiber kill |
| 7 | query | `SIG_QUERY` | no | VM-internal: state query |
| 8 | halt | `SIG_HALT` | yes | Graceful VM termination |
| 9 | io | `SIG_IO` | yes | I/O request to scheduler |
| 10 | terminal | `SIG_TERMINAL` | no | Non-resumable |
| 11 | exec | `SIG_EXEC` | yes | Subprocess capability |
| 12 | fuel | `SIG_FUEL` | yes | Budget exhaustion |
| 13 | switch | `SIG_SWITCH` | no | VM-internal: fiber switch |
| 14 | wait | `SIG_WAIT` | yes | Structured concurrency wait |
| 15 | — | — | — | Reserved |
| 16-31 | user | — | yes | User-defined signals |

"Compile-time" = tracked by signal inference in HIR analysis.

---

## Appendix B: Data model

### AnalysisHandle (Rust, wrapped as External)

```rust
pub struct AnalysisHandle {
    pub hir: Hir,
    pub arena: BindingArena,
    pub symbol_index: SymbolIndex,
    pub diagnostics: Vec<Diagnostic>,
    // Lazy-initialized on first query:
    pub call_graph: OnceCell<CallGraph>,
    pub signal_map: OnceCell<SignalMap>,
}
```

### CallGraph (Rust)

```rust
pub struct CallGraph {
    pub nodes: HashMap<String, CallGraphNode>,
    pub roots: Vec<String>,   // functions with no callers
    pub leaves: Vec<String>,  // functions that call no user-defined functions
}

pub struct CallGraphNode {
    pub name: String,
    pub callees: Vec<CallEdge>,
    pub callers: Vec<String>,
    pub span: Span,
}

pub struct CallEdge {
    pub callee: String,
    pub span: Span,
    pub is_tail: bool,
}
```

### SignalMap (Rust)

```rust
pub struct SignalMap {
    /// Maps function name → inferred Signal
    pub functions: HashMap<String, Signal>,
}
```

Built by walking the HIR once.  For each `Define { binding, value }` where
value is a `Lambda`, record `(arena.name(binding), lambda.inferred_signals)`.

### Portrait (Elle struct)

```lisp
{:name "fetch-page"
 :signal {:bits |:io :error| :propagates |1|
          :silent false :yields true :io true :jit-eligible false}
 :phases [{:kind :io :functions ["http/get"]}
          {:kind :pure :functions ["get" "json/parse"]}
          {:kind :delegated :functions ["transform"]}]
 :captures [{:name "config" :kind :value :mutated false}]
 :failures [{:source "http/get" :line 3 :kind :error}
            {:source "json/parse" :line 5 :kind :error}]
 :composition {:retry-safe false :timeout-safe true
               :parallelizable true :memoizable false
               :jit-eligible false :stateless false}
 :observations [{:kind :unsandboxed-delegation
                 :message "Parameter 1 is called without signal bounds..."}]
 :callers [{:name "main" :line 50 :tail false}]
 :callees [{:name "http/get" :line 3 :tail false}
           {:name "get" :line 4 :tail false}
           {:name "json/parse" :line 5 :tail false}
           {:name "transform" :line 6 :tail true}]}
```

### RDF triple schema (ntriples)

Subject IRIs follow existing convention:

```
<urn:elle:fn:fetch-page>
<urn:elle:def:config>
<urn:elle:macro:when-let>
<urn:elle:binding:url:lib/http.lisp:12:20>
<urn:elle:boundary:fetch-page:http/get>
```

Binding IRIs include file:line:col to disambiguate same-named bindings
in different scopes.

---

## Implementation order

| Phase | Effort | Depends on | Delivers |
|-------|--------|------------|----------|
| **1: Compiler as library** | ~3 evenings | nothing | `compile/*` primitives; agents get signals, bindings, captures |
| **2: Semantic portrait** | ~2 evenings | Phase 1 | Function/module portraits; observation engine |
| **3: Knowledge graph** | ~2 evenings | Phase 1 | Enriched SPARQL; cross-file semantic queries |
| **4: MCP tools** | ~3 evenings | Phase 2, 3 | Agent-facing MCP tools; impact analysis; invariant checking |
| **5: Live model** | ~3 evenings | Phase 3, 4 | File watching; incremental update; change notifications |
| **6: Transformation** | ~5 evenings | Phase 1, 2 | Safe rename/extract/parallelize/handler generation |

**Phase 1 is the foundation.**  Everything else builds on `compile/analyze`
returning an opaque handle that other primitives query.

**Phases 2 and 3 can proceed in parallel** after Phase 1.  Phase 2 is pure
Elle (portrait library).  Phase 3 is graph extraction (tooling).

**Phase 4 requires both 2 and 3** — the MCP tools call the portrait
functions and query the enriched graph.

**Phase 5 requires Phase 3+4** — incremental update assumes the graph and
MCP server exist.

**Phase 6 is semi-independent** — it needs Phase 1 (analysis handle) and
Phase 2 (to verify transformations don't break portraits), but doesn't
need the MCP server.  Could be built as early as after Phase 2.
