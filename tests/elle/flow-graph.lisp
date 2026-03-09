(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## ════════════════════════════════════════════════════════════════════════════
## fn/flow and fn/cfg integration tests
## Tests control flow graph introspection and visualization APIs
## ════════════════════════════════════════════════════════════════════════════

## ── fn/flow: Control flow graph introspection ───────────────────────────────

(assert-true (struct? (fn/flow (fn (x) x))) "fn/flow returns struct")

(let ((cfg (fn/flow (fn (x y) (+ x y)))))
  (assert-true (not (nil? (get cfg :arity))) "fn/flow has arity")
  (assert-true (not (nil? (get cfg :regs))) "fn/flow has regs")
  (assert-true (not (nil? (get cfg :locals))) "fn/flow has locals")
  (assert-true (not (nil? (get cfg :entry))) "fn/flow has entry")
  (assert-true (not (nil? (get cfg :blocks))) "fn/flow has blocks"))

(assert-true (string? (get (fn/flow (fn (x y) (+ x y))) :arity)) "fn/flow arity is string")

(assert-eq (get (fn/flow (fn (x y) (+ x y))) :arity) "2" "fn/flow arity value")

(assert-true (tuple? (get (fn/flow (fn (x) x)) :blocks)) "fn/flow blocks is tuple")

(assert-true (> (length (get (fn/flow (fn (x) x)) :blocks)) 0) "fn/flow blocks nonempty")

(let ((block (get (get (fn/flow (fn (x) x)) :blocks) 0)))
  (assert-true (not (nil? (get block :label))) "fn/flow block has label")
  (assert-true (not (nil? (get block :instrs))) "fn/flow block has instrs")
  (assert-true (not (nil? (get block :term))) "fn/flow block has term")
  (assert-true (not (nil? (get block :edges))) "fn/flow block has edges"))

(let ((instrs (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :instrs)))
  (assert-true (tuple? instrs) "fn/flow instrs is tuple of strings"))

(let ((edges (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :edges)))
  (assert-true (tuple? edges) "fn/flow edges is tuple"))

(let ((term (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :term)))
  (assert-true (string? term) "fn/flow term is string"))

(assert-err (fn () (fn/flow 42)) "fn/flow non-closure errors")

(defn my-add (x y) (+ x y))
(assert-true (nil? (get (fn/flow my-add) :name)) "fn/flow named function")

(assert-true (nil? (get (fn/flow (fn (x) x)) :name)) "fn/flow anonymous function name is nil")

(defn my-add (x y) "Add two numbers." (+ x y))
(assert-eq (get (fn/flow my-add) :doc) "Add two numbers." "fn/flow doc with docstring")

(assert-true (nil? (get (fn/flow (fn (x) x)) :doc)) "fn/flow doc without docstring")

(let ((cfg (fn/flow (fn (x) (if x 1 2)))))
  (let ((blocks (get cfg :blocks)))
    (assert-true (> (length blocks) 1) "fn/flow branching has edges")))

## ── fn/cfg: DOT format visualization ────────────────────────────────────────

(def r1 (fn/cfg (fn (x) x) :dot))
(assert-true (string? r1) "fn/cfg dot returns string")

(def r2 (fn/cfg (fn (a) a) :dot))
(assert-true (string/starts-with? r2 "digraph {") "fn/cfg dot starts with digraph")

(def r3 (fn/cfg (fn (b) b) :dot))
(assert-true (string/ends-with? r3 "}\n") "fn/cfg dot ends with closing brace")

(def r4 (fn/cfg (fn (c) c) :dot))
(assert-true (string/contains? r4 "block0") "fn/cfg dot contains block0")

(def r5 (fn/cfg (fn (d) d) :dot))
(assert-true (string/contains? r5 "shape=record") "fn/cfg dot contains shape record")

(defn my-fn-1 (x) (+ x 1))
(def r6 (fn/cfg my-fn-1 :dot))
(assert-true (string/contains? r6 "anonymous") "fn/cfg dot unnamed defn shows anonymous")

(def r7 (fn/cfg (fn (e) (if e 1 2)) :dot))
(assert-true (string/contains? r7 "->") "fn/cfg dot branching has edges")

(defn my-fn-2 (x) "Does stuff." (+ x 1))
(def r8 (fn/cfg my-fn-2 :dot))
(assert-true (string/contains? r8 "Does stuff.") "fn/cfg dot shows docstring in label")

## ── fn/cfg: Error handling ──────────────────────────────────────────────────

(assert-err (fn () (fn/cfg (fn (k) k) :png)) "fn/cfg invalid format errors")

(assert-err (fn () (fn/cfg (fn (l) l) :dot :extra)) "fn/cfg too many args errors")

(assert-err (fn () (fn/cfg 42)) "fn/cfg non-closure errors")

## ── fn/cfg: Fiber support ───────────────────────────────────────────────────

(def r14 (fn/cfg (fiber/new (fn (m) 42) 0)))
(assert-true (string? r14) "fn/cfg fiber")

(def r15 (fn/flow (fiber/new (fn (n) 42) 0)))
(assert-true (struct? r15) "fn/flow fiber")

## ── fn/cfg: Mermaid format visualization ────────────────────────────────────

(def r9 (fn/cfg (fn (f) f)))
(assert-true (string/starts-with? r9 "flowchart") "fn/cfg default is mermaid")

(def r10 (fn/cfg (fn (g) g) :mermaid))
(assert-true (string/starts-with? r10 "flowchart") "fn/cfg mermaid explicit")

(def r11 (fn/cfg (fn (h) h) :mermaid))
(assert-true (string/contains? r11 "block0") "fn/cfg mermaid contains block")

(def r12 (fn/cfg (fn (i) (if i 1 2)) :mermaid))
(assert-true (string/contains? r12 "-->") "fn/cfg mermaid branching has edges")

(def f (fn (j) (if j 1 2)))
(def r13a (fn/cfg f))
(def r13b (fn/cfg f :mermaid))
(assert-eq r13a r13b "fn/cfg mermaid default equals explicit")

## ── fn/flow: New field tests ────────────────────────────────────────────────

(def flow1 (fn/flow (fn (o) o)))
(def block1 (get (get flow1 :blocks) 0))
(assert-true (tuple? (get block1 :display)) "fn/flow block has display")

(def flow2 (fn/flow (fn (p) p)))
(def block2 (get (get flow2 :blocks) 0))
(def display2 (get block2 :display))
(assert-true (string/contains? (get display2 0) "r") "fn/flow display is compact")

(def flow3 (fn/flow (fn (q) q)))
(def block3 (get (get flow3 :blocks) 0))
(assert-true (keyword? (get block3 :term-kind)) "fn/flow block has term-kind")

(def flow4 (fn/flow (fn (r) r)))
(def block4 (get (get flow4 :blocks) 0))
(assert-eq (get block4 :term-kind) :return "fn/flow term-kind return")

(def flow5 (fn/flow (fn (s) (if s 1 2))))
(def entry5 (get (get flow5 :blocks) 0))
(assert-eq (get entry5 :term-kind) :branch "fn/flow term-kind branch")

(def flow6 (fn/flow (fn (t) t)))
(def block6 (get (get flow6 :blocks) 0))
(assert-true (string? (get block6 :term-display)) "fn/flow block has term-display")

(def flow7 (fn/flow (fn (u) u)))
(def block7 (get (get flow7 :blocks) 0))
(assert-true (string/starts-with? (get block7 :term-display) "return") "fn/flow term-display compact")

## ── fn/cfg: Mermaid visual feature tests ────────────────────────────────────

(def r16 (fn/cfg (fn (v) v) :mermaid))
(assert-true (string/contains? r16 "classDef") "fn/cfg mermaid has classdef")

(def r17 (fn/cfg (fn (w) (if w 1 2)) :mermaid))
(assert-true (string/contains? r17 "{") "fn/cfg mermaid branch uses diamond")

(def r18 (fn/cfg (fn (z) z) :mermaid))
(assert-true (string/contains? r18 "([") "fn/cfg mermaid return uses stadium")

(def r19 (fn/cfg (fn (aa) aa) :mermaid))
(assert-true (not (string/contains? r19 "Reg(")) "fn/cfg mermaid compact instructions")
