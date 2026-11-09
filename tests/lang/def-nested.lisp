(elle/epoch 15)
# audited: 2026-11-09
# A def binds in the scope around it however deep it sits, and each def of a name is a new binding.
# docs/bindings.md
#
# The counter-factual: a nested def used to bind a second name of the same
# spelling in the middle of analysis. Every read after it resolved to that
# second binding, and `(when false (def num 1)) (def num 7) num` answered 1. A
# function body also rejected a second def of a name as a duplicate. Each
# subject below fails on that compiler.

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
