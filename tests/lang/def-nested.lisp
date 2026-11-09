(elle/epoch 15)
# audited: 2026-11-09
# A def binds in the scope around it however deep it sits, and a read after a def that may not run finds the latest def that ran.
# docs/bindings.md
#
# The counter-factual: a nested def used to bind a second name of the same
# spelling in the middle of analysis. Every read after it resolved to that
# second binding, even where the def never ran, and the compiler then read the
# def's literal at those reads. `(when false (def num 1)) (def num 7) num`
# answered 1. A function body also rejected a second def of a name as a
# duplicate. Each subject below fails on that compiler.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<def-nested>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

# ── The file: a later def of the name wins ────────────────────────────

(when false (def num 1))
(def num 7)
(defn read-num []
  num)
(assert (= num 7) "a skipped nested def does not shadow the later top-level def")
(assert (= (read-num) 7) "a lambda after the later def reads it")

(begin
  (def spliced 1))
(def spliced 7)
(assert (= spliced 7) "a top-level begin's def is an ordinary top-level def")

(defn read-forward []
  forward)
(begin
  (def forward :seen))
(assert (= (read-forward) :seen)
        "a lambda written before a top-level begin sees its def")

(def again 1)
(defn read-first []
  again)
(def again 2)
(defn read-second []
  again)
(assert (= (read-first) 1) "a lambda before a redefinition reads the first def")
(assert (= (read-second) 2) "a lambda after a redefinition reads the second def")

# A lambda the fixpoint re-analyzes keeps the binding it first resolved, so it
# calls the redefinition and yields, rather than calling the first def.
(def answer (fn [] 1))
(def answer
  (fn []
    (yield :yielded)
    2))
(defn call-answer []
  (answer))
(def answering (fiber/new call-answer |:yield|))
(assert (= (fiber/resume answering) :yielded)
        "the call reaches the redefinition")
(assert (= (fiber/resume answering) 2)
        "the redefinition returns after its yield")

(assert (string/contains? (compile-error "(def @v @[1]) (def v (freeze v))")
                          "both mutable and immutable")
        "the file's defs of one name agree on mutability")

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

# ── A function body redefines a name ─────────────────────────────────

(defn redefine []
  (def x 3)
  (def p (fn [] (= x 3)))
  (def x 4)
  [x (p)])
(assert (= (redefine) [4 true])
        "code after a redefinition reads it, and a lambda keeps its binding")

(def level 1)
(defn deeper []
  (def level (+ level 1))
  level)
(assert (= (deeper) 2) "a def's value reads the outer binding of its name")

(defn bump [n]
  (def n (+ n 1))
  n)
(assert (= (bump 1) 2) "a def's value reads the parameter of its name")

(def greeting :outer)
(defn early-call []
  (defn say []
    greeting)
  (def before (say))
  (def greeting :inner)
  [before (say)])
(assert (= (early-call) [:outer :inner])
        "a forward reference reads the outer binding until the def runs")

(defn early-nil []
  (defn peek []
    unseen-name)
  (def before (peek))
  (def unseen-name 1)
  [before (peek)])
(assert (= (early-nil) [nil 1])
        "a forward reference with no outer binding reads nil until the def runs")

(assert (string/contains? (compile-error "(fn [] (def a b) (def b 7) a)")
                          "'b' referenced before its initialization")
        "a direct read before the def with no outer binding is an error")

(assert (string/contains? (compile-error "(fn [] (def x 1) (def @x 2) x)")
                          "both mutable and immutable")
        "a body's defs of one name agree on mutability")
(assert (string/contains? (compile-error "(fn [] (var x 1) (def x 2) x)")
                          "both mutable and immutable") "a var is mutable")

(defn nested-forward []
  (defn call-helper []
    (helper))
  (begin
    (defn helper []
      :found))
  (call-helper))
(assert (= (nested-forward) :found)
        "a lambda earlier in the body calls a def in a later begin")

(defmacro when-hidden [c]
  `(when ,c (def hidden 1)))
(defn hygienic []
  (when-hidden true)
  (def hidden 2)
  hidden)
(assert (= (hygienic) 2)
        "a macro's nested def does not collide with the user's def")

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
