#!/usr/bin/env elle

# Semantic portraits — the living model
#
# Demonstrates:
#   compile/analyze   — analyze source text without executing it
#   compile/signal    — query a function's inferred signal
#   compile/captures  — query what a closure captures
#   compile/callees   — query what a function calls
#   compile/call-graph — get the full call graph
#   portrait:function — build a semantic portrait of a function
#   portrait:module   — build a signal topology of a module
#   portrait:render   — render a portrait as text
#
# The living model exposes everything the compiler knows about your code:
# effect profiles, capture analysis, composition properties, and
# observations about implicit decisions you didn't realize you made.

(def portrait ((import "lib/portrait.lisp")))

# ── The program under analysis ──────────────────────────────────────────
#
# We analyze this source as a string — no execution needed.  The compiler
# resolves bindings, infers signals, computes captures, and builds the
# call graph.  All of that is queryable.

(def src "
(defn validate [data]
  (when (nil? (get data :name))
    (error {:error :validation-error :message \"missing name\"}))
  data)

(defn transform [data f]
  (f (get data :value)))

(defn make-processor [config]
  (var count 0)
  (defn process [data]
    (assign count (+ count 1))
    (validate data)
    (transform data (get config :transform-fn)))
  process)
")

(def a (compile/analyze src {:file "example.lisp"}))

# ── Raw signal queries ──────────────────────────────────────────────────

(println "── Signal queries ──")
(println)

(def validate-sig (compile/signal a :validate))
(println "  validate:  silent=" (get validate-sig :silent)
                     " jit=" (get validate-sig :jit-eligible))

(def transform-sig (compile/signal a :transform))
(println "  transform: silent=" (get transform-sig :silent)
                     " propagates=" (get transform-sig :propagates))

(def process-sig (compile/signal a :process))
(println "  process:   silent=" (get process-sig :silent)
                     " yields=" (get process-sig :yields))

# ── Capture analysis ────────────────────────────────────────────────────

(println)
(println "── Captures ──")
(println)

(def process-caps (compile/captures a :process))
(each cap in process-caps
  (println (string/format "  process captures {} as {}{}"
    (get cap :name) (get cap :kind)
    (if (get cap :mutated) " (mutable)" ""))))

# ── Function portraits ──────────────────────────────────────────────────

(println)
(println "── Function portrait: validate ──")
(println (portrait:render (portrait:function a :validate)))

(println "── Function portrait: transform ──")
(println (portrait:render (portrait:function a :transform)))

(println "── Function portrait: process ──")
(println (portrait:render (portrait:function a :process)))

# ── Module topology ─────────────────────────────────────────────────────

(println "── Module topology ──")
(println (portrait:render-module (portrait:module a)))

# ── What the living model surfaced ──────────────────────────────────────
#
# Without running the code, the compiler told us:
#
# 1. transform delegates to parameter f with no signal bounds.
#    A malicious closure could do arbitrary I/O.
#
# 2. process captures mutable 'count' — not parallelizable,
#    not timeout-safe.
#
# 3. process yields (because transform propagates f's signal,
#    and process calls transform).
#
# 4. validate is pure — JIT-eligible, memoizable, stateless.
#
# 5. The signal boundary between process and validate marks
#    where the architecture transitions from impure to pure.
