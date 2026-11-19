(elle/epoch 18)
# audited: 2026-11-19
# Each def of a name in a body is a new binding, and a lambda keeps the binding it saw.
# docs/bindings.md
#
# The counter-factual: a nested def used to bind a second name of the same
# spelling in the middle of analysis. Every read after it resolved to that
# second binding. A function body also rejected a second def of a name as a
# duplicate. Each subject below fails on that compiler.
#
# A name exists from its def onward. Up to epoch 17 a file and a body were
# each one letrec, so a lambda written before a def read that def. The reads
# marked "from its def onward" below assert the rule that replaced it.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<def-nested>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

# ── The file: a later def of the name wins ────────────────────────────

(begin
  (def spliced 1))
(def spliced 7)
(assert (= spliced 7) "a top-level begin's def is an ordinary top-level def")

(assert (string/contains? (compile-error
                            "(defn read-forward [] forward) (begin (def forward :seen)) (read-forward)")
                          "undefined variable: forward")
        "from its def onward: a lambda before a top-level begin names no def in it")

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

(assert (string/contains? (compile-error
                            "(def level 1) (defn deeper [] (def level (+ level 1)) level) (deeper)")
                          "'level' referenced before its initialization")
        "from its def onward: a def's value cannot read the name it binds")
(assert (string/contains? (compile-error
                            "(defn bump [n] (def n (+ n 1)) n) (bump 1)")
                          "'n' referenced before its initialization")
        "from its def onward: nor when the name is a parameter")

(def greeting :outer)
(defn early-call []
  (defn say []
    greeting)
  (def before (say))
  (def greeting :inner)
  [before (say) greeting])
(assert (= (early-call) [:outer :outer :inner])
        "from its def onward: a lambda before a def reads the outer binding")

(defn early-nil []
  (letrec [peek (fn [] unseen-name)
           before (peek)
           unseen-name 1]
    [before (peek)]))
(assert (= (early-nil) [nil 1])
        "a letrec name that a lambda reads before its value is bound reads nil")

(assert (string/contains? (compile-error "(fn [] (def a b) (def b 7) a)")
                          "undefined variable: b")
        "a direct read before the def with no outer binding is an error")

(assert (string/contains? (compile-error "(fn [] (def x 1) (def @x 2) x)")
                          "both mutable and immutable")
        "a body's defs of one name agree on mutability")
(assert (string/contains? (compile-error "(fn [] (var x 1) (def x 2) x)")
                          "both mutable and immutable") "a var is mutable")

(assert (string/contains? (compile-error
                            "(fn [] (defn call-helper [] (helper)) (begin (defn helper [] :found)) (call-helper))")
                          "undefined variable: helper")
        "from its def onward: a lambda earlier in the body names no def in a later begin")
