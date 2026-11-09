(elle/epoch 15)
# audited: 2026-11-09
# A def nested in begin, when, if or another form that opens no scope binds in the body around it, under that body's rules.
# docs/bindings.md
#
# The counter-factual: a nested def used to bind a second name of the same
# spelling in the middle of analysis. Every read after it resolved to that
# second binding, even where the def never ran, and the compiler then read the
# def's literal at those reads. `(when false (def num 1)) (def num 7) num`
# answered 1. Each assertion below fails on that compiler.

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

# ── The file: a skipped def leaves its name nil ───────────────────────

(when false (def never-literal 1))
(when false (def never-computed (+ 0 1)))
(assert (nil? never-literal) "a skipped def of a literal reads nil")
(assert (nil? never-computed) "a skipped def of a call reads nil")
(assert (= (type-of never-literal) :nil) "the skipped literal's type is nil")

(when true (def ran 1))
(assert (= ran 1) "a nested def that ran reads its value")

(assert (string/contains? (compile-error
                            "(when true (def kept 1)) (assign kept 2)")
                          "cannot assign immutable binding 'kept'")
        "a nested def stays immutable")

# ── A function body pre-binds a nested def ───────────────────────────

(defn nested-forward []
  (defn call-helper []
    (helper))
  (begin
    (defn helper []
      :found))
  (call-helper))
(assert (= (nested-forward) :found)
        "a lambda earlier in the body calls a def in a later begin")

(assert (string/contains? (compile-error
                            "(fn [c] (def x 7) (when c (def x 1)) x)")
                          "duplicate binding 'x'")
        "a nested def that repeats a body's def is a duplicate")
(assert (string/contains? (compile-error "(fn [] (def x 7) (begin (def x 1)) x)")
                          "duplicate binding 'x'")
        "a def in a nested begin that repeats a body's def is a duplicate")

(defmacro when-hidden [c]
  `(when ,c (def hidden 1)))
(defn hygienic []
  (when-hidden true)
  (def hidden 2)
  hidden)
(assert (= (hygienic) 2)
        "a macro's nested def does not collide with the user's def")

# ── A function body: a skipped def leaves its name nil ────────────────

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

(defn other-branch [c]
  (if c (def chosen :then) chosen))
(assert (= (other-branch true) :then) "the branch that defines reads its value")
(assert (nil? (other-branch false)) "the other branch reads nil")

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

(defn in-loop-body []
  (var i 0)
  (while (< i 0)
    (def inside 1)
    (assign i 1))
  inside)
(assert (nil? (in-loop-body)) "a def in a loop body that never ran reads nil")

(defn in-operand [c]
  (and c (def operand 1))
  operand)
(assert (= (in-operand true) 1)
        "a def in an and operand that ran reads its value")
(assert (nil? (in-operand false)) "a def in a skipped and operand reads nil")

(assert (string/contains? (compile-error
                            "(defn f [c] (when c (def kept 1)) (assign kept 2))")
                          "cannot assign immutable binding 'kept'")
        "a nested def in a function body stays immutable")

# ── A let body at top level: a skipped def leaves its name nil ────────

(assert (nil? (let []
                (when false (def let-literal 1))
                let-literal)) "a skipped def in a let body reads nil")
(assert (let []
          (when false (def let-computed (+ 0 1)))
          (nil? let-computed))
        "nil? sees the nil a skipped def in a let body leaves")
