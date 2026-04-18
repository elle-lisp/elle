(elle/epoch 7)
## examples/portrait-demo.lisp — deep analysis of the pipeline example
##
## Shows how the living model follows data relationships across the
## Elle/Rust boundary, revealing properties the programmer never declared.

(def portrait ((import "std/portrait")))
(def rdf ((import "std/rdf/elle")))

(def src (file/read "examples/pipeline.lisp"))
(def a (compile/analyze src {:file "examples/pipeline.lisp"}))

# ── Section helper ────────────────────────────────────────────────────

(defn section [title]
  (println)
  (println (string/format "── {} ──────────────────────────────────────" title)))

# ── Module portrait ───────────────────────────────────────────────────

(section "Module topology")
(println (portrait:render-module (portrait:module a)))

# ── Follow the signal boundary ────────────────────────────────────────

(section "Signal boundary: process-pipeline")
(println (portrait:render (portrait:function a :process-pipeline)))

(section "Signal boundary: read-records")
(println (portrait:render (portrait:function a :read-records)))

(section "Pure core: parse-record")
(println (portrait:render (portrait:function a :parse-record)))

(section "Pure core: normalize")
(println (portrait:render (portrait:function a :normalize)))

(section "Delegating: transform")
(println (portrait:render (portrait:function a :transform)))

# ── Cross-boundary call graph ─────────────────────────────────────────

(section "Cross-boundary call graph")

(def graph (compile/call-graph a))

(each node in (get graph :nodes)
  (var name (get node :name))
  (var callees (get node :callees))
  (when (not (empty? callees))
    (println (string/format "  {} calls:" name))
    (each callee in callees
      # Classify: is this callee a Rust primitive or an Elle function?
      (var is-prim false)
      (each p in (compile/primitives)
        (when (= (get p :name) callee)
          (assign is-prim true)))
      (println (string/format "    {} {}" callee
        (if is-prim "[Rust]" "[Elle]"))))))

# ── Rust primitives used by this module ───────────────────────────────

(section "Rust primitives called from this module")

(var prim-names @{})
(each p in (compile/primitives)
  (put prim-names (get p :name) (get p :signal)))

(var used-prims @{})
(each node in (get graph :nodes)
  (each callee in (get node :callees)
    (when (not (nil? (get prim-names callee)))
      (put used-prims callee (get prim-names callee)))))

(each [name sig] in (pairs (freeze used-prims))
  (println (string/format "  {:20} silent={} io={} yields={}"
    name (get sig :silent) (get sig :io) (get sig :yields))))

# ── Impact analysis: what if parse-record gained I/O? ─────────────────

(section "Impact: what if parse-record added I/O?")

(def callers (compile/callers a :parse-record))
(println "  Direct callers of parse-record:")
(each c in callers
  (var caller-name (get c :name))
  (var caller-sig (compile/signal a (keyword caller-name)))
  (println (string/format "    {} — currently silent={} jit={}"
    caller-name (get caller-sig :silent) (get caller-sig :jit-eligible)))
  (when (get caller-sig :jit-eligible)
    (println "      ⚠ would lose JIT eligibility"))
  (when (get caller-sig :silent)
    (println "      ⚠ would become impure")))

# ── Capture analysis: make-accumulator ────────────────────────────────

(section "Capture analysis: make-accumulator closures")

(def syms (compile/symbols a))
(each sym in syms
  (when (= (get sym :kind) :function)
    (var caps (compile/captures a (keyword (get sym :name))))
    (when (not (empty? caps))
      (println (string/format "  {} captures:" (get sym :name)))
      (each cap in caps
        (println (string/format "    {} (kind={} mutated={})"
          (get cap :name) (get cap :kind) (get cap :mutated)))))))

# ── RDF output sample ────────────────────────────────────────────────

(section "RDF: unified graph sample")

(def triples (rdf:file a "examples/pipeline.lisp"))
(var lines (string/split triples "\n"))
(var shown 0)
(each line in lines
  (when (and (< shown 15)
             (or (string/contains? line "calls")
                 (string/contains? line "Primitive")
                 (string/contains? line "capture")))
    (println (string/format "  {}" line))
    (assign shown (+ shown 1))))

(println)
(println (string/format "Total triples for this module: {}" (length lines)))
(println (string/format "Rust primitive nodes available: {}" (length (compile/primitives))))
