(elle/epoch 15)
# audited: 2026-11-10
# After a def that may not run, a read finds the latest def that ran, or else the binding visible before it.
# docs/bindings.md
#
# The counter-factual: a nested def used to bind a second name of the same
# spelling in the middle of analysis. Every read after it resolved to that
# second binding, even where the def never ran, and the compiler then read the
# def's literal at those reads. Each subject below fails on that compiler.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<def-merge>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

# ── The file: after a def that may not run ────────────────────────────

(when false (def never-literal 1))
(when false (def never-computed (+ 0 1)))
(assert (nil? never-literal) "a skipped def of a literal reads nil")
(assert (nil? never-computed) "a skipped def of a call reads nil")
(assert (= (type-of never-literal) :nil) "the skipped literal's type is nil")

(def file-prior 1)
(when false (def file-prior 2))
(assert (= file-prior 1) "a skipped redefinition leaves the earlier def")

(def file-ran 1)
(when true (def file-ran 2))
(assert (= file-ran 2) "a redefinition that ran wins")

(assert (string/contains? (compile-error
                            "(when true (def kept 1)) (assign kept 2)")
                          "cannot assign immutable binding 'kept'")
        "a nested def stays immutable")

# ── A function body: after a def that may not run ─────────────────────

(defn branch [c]
  (def x 1)
  (def p (fn [] x))
  (when c (def x 2))
  [x (p)])
(assert (= (branch true) [2 1]) "a redefinition that ran wins")
(assert (= (branch false) [1 1]) "a skipped redefinition leaves the earlier def")

(def shade-level 1)
(defn maybe-shade [c]
  (when c (def shade-level 2))
  shade-level)
(assert (= (maybe-shade true) 2) "a def that ran wins over the outer binding")
(assert (= (maybe-shade false) 1) "a skipped def leaves the outer binding")

(defn maybe-literal [c]
  (when c (def found 1))
  found)
(defn maybe-computed [c]
  (when c (def found (+ 0 1)))
  found)
(defn maybe-nil? [c]
  (when c (def found 1))
  (nil? found))
(assert (= (maybe-literal true) 1) "a def that ran reads its literal")
(assert (nil? (maybe-literal false)) "a skipped def of a literal reads nil")
(assert (nil? (maybe-computed false)) "a skipped def of a call reads nil")
(assert (maybe-nil? false) "nil? sees the nil a skipped def leaves")
(assert (not (maybe-nil? true)) "nil? sees the value a def that ran leaves")

(assert (string/contains? (compile-error
                            "(fn [c] (if c (def chosen :then) chosen))")
                          "'chosen' referenced before its initialization")
        "the other branch reads the name before its def")

(defn maybe-call [c]
  (when c
    (defn helper []
      :called))
  (helper))
(assert (= (maybe-call true) :called) "a nested defn that ran is called")
(assert (not (first (protect (maybe-call false))))
        "calling a skipped nested defn raises")

(defn maybe-closure [c]
  (when c (def captured 5))
  (fn [] captured))
(assert (= ((maybe-closure true)) 5) "a closure reads a nested def that ran")
(assert (nil? ((maybe-closure false))) "a closure reads nil for a skipped def")

(defn maybe-loop [c]
  (when c
    (defn count-down [n]
      (if (= n 0) :done (count-down (- n 1)))))
  (if c (count-down 3) :skipped))
(assert (= (maybe-loop true) :done) "a nested self-recursive defn recurses")
(assert (= (maybe-loop false) :skipped)
        "a skipped self-recursive defn is not called")

(defn maybe-pair [c]
  (when c (def [p q] [1 2]))
  [p q])
(assert (= (maybe-pair true) [1 2])
        "a nested destructuring def that ran binds its leaves")
(assert (= (maybe-pair false) [nil nil])
        "a skipped destructuring def leaves its leaves nil")

(defn in-operand [c]
  (and c (def operand 1))
  operand)
(assert (= (in-operand true) 1)
        "a def in an and operand that ran reads its value")
(assert (nil? (in-operand false)) "a def in a skipped and operand reads nil")

(defn fallback [a]
  (or a (def used :fallback))
  used)
(assert (nil? (fallback true)) "a def in a skipped or operand reads nil")
(assert (= (fallback false) :fallback)
        "a def in an or operand that ran reads its value")

(defn pick [n]
  (cond
    (= n 0) (def label :zero)
    (= n 1) (def label :one))
  label)
(assert (= (pick 0) :zero) "the first cond clause's def wins when it runs")
(assert (= (pick 1) :one) "a later cond clause's def wins when it runs")
(assert (nil? (pick 2)) "no cond clause ran, so the name reads nil")

(defn count-either [up]
  (def @acc @[])
  (if up
    (begin
      (def @i 0)
      (while (< i 2)
        (push acc i)
        (assign i (+ i 1))))
    (begin
      (def @i 2)
      (while (> i 0)
        (push acc i)
        (assign i (- i 1)))))
  (freeze acc))
(assert (= (count-either true) [0 1]) "a mutable def in one branch")
(assert (= (count-either false) [2 1]) "a mutable def of the name in the other")

(assert (string/contains? (compile-error "(fn [c] (when c (def @n 0)) n)")
                          "one of them is mutable")
        "a read that may find a mutable binding is an error")
(assert (string/contains? (compile-error
                            "(fn [c] (when c (def @n 0)) (assign n 1))")
                          "one of them is mutable")
        "an assign that may find a mutable binding is an error")
(assert (string/contains? (compile-error
                            "(defn f [c] (when c (def kept 1)) (assign kept 2))")
                          "cannot assign immutable binding 'kept'")
        "a nested def in a function body stays immutable")

# ── A function body: a def in a loop ──────────────────────────────────

(defn in-loop-body []
  (var i 0)
  (while (< i 0)
    (def inside 1)
    (assign i 1))
  inside)
(assert (nil? (in-loop-body)) "a def in a loop body that never ran reads nil")

(defn last-pass []
  (def @i 0)
  (def seen :none)
  (while (< i 3)
    (def seen i)
    (assign i (+ i 1)))
  seen)
(assert (= (last-pass) 2) "after a loop, the name reads the last pass's def")

(defn previous-passes []
  (def @i 0)
  (def @seen @[])
  (def latest :start)
  (while (< i 3)
    (push seen latest)
    (def latest i)
    (assign i (+ i 1)))
  (freeze seen))
(assert (= (previous-passes) [:start 0 1])
        "a read before the def in a pass finds the earlier pass's def")

(defn pass-closures []
  (def @i 0)
  (def @fs @[])
  (while (< i 3)
    (def k i)
    (push fs (fn [] k))
    (assign i (+ i 1)))
  [((get fs 0)) ((get fs 1)) ((get fs 2))])
(assert (= (pass-closures) [0 1 2])
        "each pass's closure keeps its pass's binding")

# ── A let body at top level: a skipped def leaves its name nil ────────

(assert (nil? (let []
                (when false (def let-literal 1))
                let-literal)) "a skipped def in a let body reads nil")
(assert (let []
          (when false (def let-computed (+ 0 1)))
          (nil? let-computed))
        "nil? sees the nil a skipped def in a let body leaves")

# ── A letrec initializer: after a def that may not run ────────────────
#
# The counter-factual: the letrec's signal fixpoint analyzed its lambda again
# while the merged binding of the last initializer waited, and the lambda took
# it in. The merged binding was then defined before the merge var it reads,
# and the body's read of the name failed to compile.

(defn in-letrec [c]
  (letrec [helper (fn [] 1)
           holder (when c (def from-init 7))]
    from-init))
(assert (= (in-letrec true) 7) "a def in a letrec initializer that ran")
(assert (nil? (in-letrec false))
        "a skipped def in a letrec initializer reads nil")
