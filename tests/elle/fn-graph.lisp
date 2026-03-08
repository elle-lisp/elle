(import-file "tests/elle/assert.lisp")

## fn/cfg integration tests - Part 1
## Tests the control flow graph visualization (DOT and Mermaid formats)
## Tests DOT format, error handling, and fiber support.

## ── DOT format tests (via fn/cfg with :dot) ─────────────────────────

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

## ── Error handling ──────────────────────────────────────────────────

(assert-err (fn () (fn/cfg (fn (k) k) :png)) "fn/cfg invalid format errors")

(assert-err (fn () (fn/cfg (fn (l) l) :dot :extra)) "fn/cfg too many args errors")

(assert-err (fn () (fn/cfg 42)) "fn/cfg non-closure errors")

## ── Fiber support ───────────────────────────────────────────────────

(def r14 (fn/cfg (fiber/new (fn (m) 42) 0)))
(assert-true (string? r14) "fn/cfg fiber")

(def r15 (fn/flow (fiber/new (fn (n) 42) 0)))
(assert-true (struct? r15) "fn/flow fiber")
