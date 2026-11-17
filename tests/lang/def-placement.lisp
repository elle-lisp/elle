(elle/epoch 16)
# audited: 2026-11-17
# A def binds only directly in a body; anywhere else it is a compile error naming the form that holds it.
# docs/bindings.md
#
# The counter-factual: a def in a branch, a loop or an operand used to compile.
# A read after it stood for whichever def had run, or nil. Each error subject
# below compiles on that compiler, and its assertion fails.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<p>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

(defn refused [src where]
  "Assert that SRC fails to compile because of a def placed outside a body,
   and that the message names WHERE, the form that holds it and its place."
  (let [message (compile-error src)]
    (assert (string/contains? message "a def binds only directly in a body")
            (string "the message names the rule: " message))
    (assert (string/contains? message where)
            (string "the message names " where ": " message))))

# ── A form that may skip or repeat its operands ───────────────────────

(refused "(defn f [c] (when c (def x 1)) 2)" "the (if ...) form at <p>:1:13")
(refused "(defn f [c] (if c (def x 1) 2))" "the (if ...) form at <p>:1:13")
(refused "(defn f [n] (cond (= n 0) (def label :zero)) 1)"
         "the (cond ...) form at <p>:1:13")
(refused "(defn f [c] (and c (def x 1)))" "the (and ...) form at <p>:1:13")
(refused "(defn f [c] (or c (def x 1)))" "the (or ...) form at <p>:1:13")
(refused "(defn f [] (while false (def x 1)))"
         "the (while ...) form at <p>:1:12")
(refused "(when true (def kept 1))" "the (if ...) form at <p>:1:1")

(let [message (compile-error "(defn f [] (while false (var x 1)))")]
  (assert (string/contains? message "a var binds only directly in a body")
          "a var names itself"))

(let [message (compile-error "(defn f [] (forever (def y 1) (break)))")]
  (assert (string/contains? message "a def binds only directly in a body")
          "a forever body is a while body"))

# ── A form that runs its operand once, but is no body ─────────────────

(refused "(def k (do (defn inner [] 1) (inner)))"
         "the (def ...) form at <p>:1:1")
(refused "(defn f [] (println (def x 1)))" "the (println ...) form at <p>:1:12")
(refused "(defn f [] (let [a (def x 1)] a))" "the (let ...) form at <p>:1:12")
(refused "(defn f [c] (letrec [h (when c (def x 7))] h))"
         "the (if ...) form at <p>:1:24")

(let [message (compile-error "(defmacro when-hidden [c] `(when ,c (def hidden 1)))
(defn f [] (when-hidden true) 2)")]
  (assert (string/contains? message "a def binds only directly in a body")
          "a def a macro writes outside a body is refused too"))

# ── A body, and a begin that stands directly in one ───────────────────

(defn in-begins []
  (begin
    (def a 1)
    (begin
      (def b 2)))
  (+ a b))
(assert (= (in-begins) 3) "a begin directly in a body passes its defs to it")

(begin
  (def spliced 1))
(assert (= spliced 1) "a top-level begin's def is a top-level def")

(assert (= (block (def z 1)
             (+ z 1)) 2) "a block is a body")
(assert (= (let []
             (def q 1)
             (+ q 1)) 2) "a let body is a body")
(assert (= (match 1
             1
               (begin
                 (def m 2)
                 m)
             _ 0) 2) "a match arm is a body")

(defn branch-local [c]
  (if c
    (block (def x 1)
      (+ x 1))
    0))
(assert (= (branch-local true) 2) "a block in a branch holds its own def")
(assert (= (branch-local false) 0) "and the other branch reads nothing of it")

# ── A parameterize body is a scope of its own ─────────────────────────

(def *depth* (make-parameter 0))
(defn scoped []
  (parameterize ((*depth* 1))
    (def seen (*depth*))
    seen))
(assert (= (scoped) 1) "a def in a parameterize body binds there")

(assert (string/contains? (compile-error
                            "(def p (make-parameter 0))
(parameterize ((p 1)) (def inner 2))
inner")
                          "undefined variable: inner")
        "a parameterize body's def ends with the body")
