(elle/epoch 9)
## graph-bench.lisp — measure compile/* query times

(def src (file/read "stdlib.lisp"))

# ── Analyze ────────────────────────────────────────────────────────────

(def @t0 (clock/monotonic))
(def a (compile/analyze src {:file "stdlib.lisp"}))
(def @t1 (clock/monotonic))
(println (string/format "compile/analyze:      {} ms" (round (* (- t1 t0) 1000))))

# ── Symbols ────────────────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def syms (compile/symbols a))
(assign t1 (clock/monotonic))
(println (string/format "compile/symbols:      {} ms  ({} symbols)"
                        (round (* (- t1 t0) 1000))
                        (length syms)))

# ── Diagnostics ────────────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def diags (compile/diagnostics a))
(assign t1 (clock/monotonic))
(println (string/format "compile/diagnostics:  {} ms  ({} diagnostics)"
                        (round (* (- t1 t0) 1000))
                        (length diags)))

# ── Signal (single) ───────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def sig (compile/signal a :map))
(assign t1 (clock/monotonic))
(println (string/format "compile/signal:       {} ms" (round (* (- t1 t0) 1000))))

# ── Query signal (bulk) ───────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def silent (compile/query-signal a :silent))
(assign t1 (clock/monotonic))
(println (string/format "query-signal :silent: {} ms  ({} matches)"
                        (round (* (- t1 t0) 1000))
                        (length silent)))

(assign t0 (clock/monotonic))
(def yielding (compile/query-signal a :yields))
(assign t1 (clock/monotonic))
(println (string/format "query-signal :yields: {} ms  ({} matches)"
                        (round (* (- t1 t0) 1000))
                        (length yielding)))

# ── Call graph ─────────────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def g (compile/call-graph a))
(assign t1 (clock/monotonic))
(println (string/format "compile/call-graph:   {} ms  ({} nodes)"
                        (round (* (- t1 t0) 1000))
                        (length (get g :nodes))))

# ── Callers / callees ─────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def callers (compile/callers a :map))
(assign t1 (clock/monotonic))
(println (string/format "compile/callers:      {} ms  ({} callers)"
                        (round (* (- t1 t0) 1000))
                        (length callers)))

(assign t0 (clock/monotonic))
(def callees (compile/callees a :map))
(assign t1 (clock/monotonic))
(println (string/format "compile/callees:      {} ms  ({} callees)"
                        (round (* (- t1 t0) 1000))
                        (length callees)))

# ── Captures ───────────────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def caps (compile/captures a :map))
(assign t1 (clock/monotonic))
(println (string/format "compile/captures:     {} ms  ({} captures)"
                        (round (* (- t1 t0) 1000))
                        (length caps)))

# ── Bindings (bulk) ───────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def binds (compile/bindings a))
(assign t1 (clock/monotonic))
(println (string/format "compile/bindings:     {} ms  ({} bindings)"
                        (round (* (- t1 t0) 1000))
                        (length binds)))

# ── Binding (single) ──────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def b (compile/binding a :map))
(assign t1 (clock/monotonic))
(println (string/format "compile/binding:      {} ms" (round (* (- t1 t0) 1000))))

# ── Primitives ─────────────────────────────────────────────────────────

(assign t0 (clock/monotonic))
(def prims (compile/primitives))
(assign t1 (clock/monotonic))
(println (string/format "compile/primitives:   {} ms  ({} primitives)"
                        (round (* (- t1 t0) 1000))
                        (length prims)))

# ── Full portrait ──────────────────────────────────────────────────────

(def portrait ((import "std/portrait")))

(assign t0 (clock/monotonic))
(def p (portrait:function a :map))
(assign t1 (clock/monotonic))
(println (string/format "portrait:function:    {} ms" (round (* (- t1 t0) 1000))))

(assign t0 (clock/monotonic))
(def m (portrait:module a))
(assign t1 (clock/monotonic))
(println (string/format "portrait:module:      {} ms" (round (* (- t1 t0) 1000))))

# ── RDF generation ─────────────────────────────────────────────────────

(def rdf ((import "std/rdf/elle")))

(assign t0 (clock/monotonic))
(def pt (rdf:primitives))
(assign t1 (clock/monotonic))
(println (string/format "rdf:primitives:       {} ms  ({} bytes)"
                        (round (* (- t1 t0) 1000))
                        (length pt)))

(assign t0 (clock/monotonic))
(def ft (rdf:file a "stdlib.lisp"))
(assign t1 (clock/monotonic))
(println (string/format "rdf:file:             {} ms  ({} bytes)"
                        (round (* (- t1 t0) 1000))
                        (length ft)))
