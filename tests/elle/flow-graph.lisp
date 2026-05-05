(elle/epoch 10)

## ════════════════════════════════════════════════════════════════════════════
## fn/flow and fn/cfg integration tests
## Tests control flow graph introspection and visualization APIs
## ════════════════════════════════════════════════════════════════════════════

## ── fn/flow: Control flow graph introspection ───────────────────────────────

(assert (struct? (fn/flow (fn (x) x))) "fn/flow returns struct")

(let [cfg (fn/flow (fn (x y) (+ x y)))]
  (assert (not (nil? (get cfg :arity))) "fn/flow has arity")
  (assert (not (nil? (get cfg :regs))) "fn/flow has regs")
  (assert (not (nil? (get cfg :locals))) "fn/flow has locals")
  (assert (not (nil? (get cfg :entry))) "fn/flow has entry")
  (assert (not (nil? (get cfg :blocks))) "fn/flow has blocks"))

(assert (string? (get (fn/flow (fn (x y) (+ x y))) :arity))
        "fn/flow arity is string")

(assert (= (get (fn/flow (fn (x y) (+ x y))) :arity) "2") "fn/flow arity value")

(assert (array? (get (fn/flow (fn (x) x)) :blocks)) "fn/flow blocks is array")

(assert (> (length (get (fn/flow (fn (x) x)) :blocks)) 0)
        "fn/flow blocks nonempty")

(let [block (get (get (fn/flow (fn (x) x)) :blocks) 0)]
  (assert (not (nil? (get block :label))) "fn/flow block has label")
  (assert (not (nil? (get block :instrs))) "fn/flow block has instrs")
  (assert (not (nil? (get block :term))) "fn/flow block has term")
  (assert (not (nil? (get block :edges))) "fn/flow block has edges"))

(let [instrs (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :instrs)]
  (assert (array? instrs) "fn/flow instrs is array of strings"))

(let [edges (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :edges)]
  (assert (array? edges) "fn/flow edges is array"))

(let [term (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :term)]
  (assert (string? term) "fn/flow term is string"))

(let [[ok? _] (protect ((fn () (fn/flow 42))))]
  (assert (not ok?) "fn/flow non-closure errors"))

(defn my-add (x y)
  (+ x y))
(assert (nil? (get (fn/flow my-add) :name)) "fn/flow named function")

(assert (nil? (get (fn/flow (fn (x) x)) :name))
        "fn/flow anonymous function name is nil")

(defn my-add (x y)
  "Add two numbers."
  (+ x y))
(assert (= (get (fn/flow my-add) :doc) "Add two numbers.")
        "fn/flow doc with docstring")

(assert (nil? (get (fn/flow (fn (x) x)) :doc)) "fn/flow doc without docstring")

(let [cfg (fn/flow (fn (x) (if x 1 2)))]
  (let [blocks (get cfg :blocks)]
    (assert (> (length blocks) 1) "fn/flow branching has edges")))

## ── fn/cfg: DOT format visualization ────────────────────────────────────────

(def r1 (fn/cfg (fn (x) x) :dot))
(assert (string? r1) "fn/cfg dot returns string")

(def r2 (fn/cfg (fn (a) a) :dot))
(assert (string/starts-with? r2 "digraph {") "fn/cfg dot starts with digraph")

(def r3 (fn/cfg (fn (b) b) :dot))
(assert (string/ends-with? r3 "}\n") "fn/cfg dot ends with closing brace")

(def r4 (fn/cfg (fn (c) c) :dot))
(assert (string/contains? r4 "block0") "fn/cfg dot contains block0")

(def r5 (fn/cfg (fn (d) d) :dot))
(assert (string/contains? r5 "shape=record") "fn/cfg dot contains shape record")

(defn my-fn-1 (x)
  (+ x 1))
(def r6 (fn/cfg my-fn-1 :dot))
(assert (string/contains? r6 "anonymous")
        "fn/cfg dot unnamed defn shows anonymous")

(def r7 (fn/cfg (fn (e) (if e 1 2)) :dot))
(assert (string/contains? r7 "->") "fn/cfg dot branching has edges")

(defn my-fn-2 (x)
  "Does stuff."
  (+ x 1))
(def r8 (fn/cfg my-fn-2 :dot))
(assert (string/contains? r8 "Does stuff.")
        "fn/cfg dot shows docstring in label")

## ── fn/cfg: Error handling ──────────────────────────────────────────────────

(let [[ok? _] (protect ((fn () (fn/cfg (fn (k) k) :png))))]
  (assert (not ok?) "fn/cfg invalid format errors"))

(let [[ok? _] (protect ((fn () (fn/cfg (fn (l) l) :dot :extra))))]
  (assert (not ok?) "fn/cfg too many args errors"))

(let [[ok? _] (protect ((fn () (fn/cfg 42))))]
  (assert (not ok?) "fn/cfg non-closure errors"))

## ── fn/cfg: Fiber support ───────────────────────────────────────────────────

(def r14 (fn/cfg (fiber/new (fn (m) 42) 0)))
(assert (string? r14) "fn/cfg fiber")

(def r15 (fn/flow (fiber/new (fn (n) 42) 0)))
(assert (struct? r15) "fn/flow fiber")

## ── fn/cfg: Mermaid format visualization ────────────────────────────────────

(def r9 (fn/cfg (fn (f) f)))
(assert (string/starts-with? r9 "flowchart") "fn/cfg default is mermaid")

(def r10 (fn/cfg (fn (g) g) :mermaid))
(assert (string/starts-with? r10 "flowchart") "fn/cfg mermaid explicit")

(def r11 (fn/cfg (fn (h) h) :mermaid))
(assert (string/contains? r11 "block0") "fn/cfg mermaid contains block")

(def r12 (fn/cfg (fn (i) (if i 1 2)) :mermaid))
(assert (string/contains? r12 "-->") "fn/cfg mermaid branching has edges")

(def f (fn (j) (if j 1 2)))
(def r13a (fn/cfg f))
(def r13b (fn/cfg f :mermaid))
(assert (= r13a r13b) "fn/cfg mermaid default equals explicit")

## ── fn/flow: New field tests ────────────────────────────────────────────────

(def flow1 (fn/flow (fn (o) o)))
(def block1 (get (get flow1 :blocks) 0))
(assert (array? (get block1 :display)) "fn/flow block has display")

(def flow2 (fn/flow (fn (p) p)))
(def block2 (get (get flow2 :blocks) 0))
(def display2 (get block2 :display))
(assert (string/contains? (get display2 0) "r") "fn/flow display is compact")

(def flow3 (fn/flow (fn (q) q)))
(def block3 (get (get flow3 :blocks) 0))
(assert (keyword? (get block3 :term-kind)) "fn/flow block has term-kind")

(def flow4 (fn/flow (fn (r) r)))
(def block4 (get (get flow4 :blocks) 0))
(assert (= (get block4 :term-kind) :return) "fn/flow term-kind return")

(def flow5 (fn/flow (fn (s) (if s 1 2))))
(def entry5 (get (get flow5 :blocks) 0))
(assert (= (get entry5 :term-kind) :branch) "fn/flow term-kind branch")

(def flow6 (fn/flow (fn (t) t)))
(def block6 (get (get flow6 :blocks) 0))
(assert (string? (get block6 :term-display)) "fn/flow block has term-display")

(def flow7 (fn/flow (fn (u) u)))
(def block7 (get (get flow7 :blocks) 0))
(assert (string/starts-with? (get block7 :term-display) "return")
        "fn/flow term-display compact")

## ── fn/cfg: Mermaid visual feature tests ────────────────────────────────────

(def r16 (fn/cfg (fn (v) v) :mermaid))
(assert (string/contains? r16 "classDef") "fn/cfg mermaid has classdef")

(def r17 (fn/cfg (fn (w) (if w 1 2)) :mermaid))
(assert (string/contains? r17 "{") "fn/cfg mermaid branch uses diamond")

(def r18 (fn/cfg (fn (z) z) :mermaid))
(assert (string/contains? r18 "([") "fn/cfg mermaid return uses stadium")

(def r19 (fn/cfg (fn (aa) aa) :mermaid))
(assert (not (string/contains? r19 "Reg("))
        "fn/cfg mermaid compact instructions")
