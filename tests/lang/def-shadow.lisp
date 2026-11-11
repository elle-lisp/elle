(elle/epoch 15)
# audited: 2026-11-09
# A file's redefinition reads the previous binding in its value, agrees with it on mutability, and an explicit letrec rejects a duplicate.
# docs/bindings.md
#
# The counter-factual: the file registered a redefinition before analyzing its
# value, so `(def a (+ a 1))` read the new, uninitialized `a`.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<def-shadow>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

# ── file scope: the redefinition's value reads the previous binding ──
(def a 10)
(def a (+ a 1))
(assert (= a 11) "file-scope: (+ a 1) should see previous a=10")

(def @x @[1 2 3])
(def @x (freeze x))
(assert (= x [1 2 3]) "file-scope: freeze of @x should produce [1 2 3]")

# ── file scope: defs of one name agree on mutability ──
(assert (string/contains? (compile-error "(def @x @[1 2 3]) (def x (freeze x))")
                          "both mutable and immutable")
        "file-scope: @x then x is a compile error")

# ── explicit letrec: duplicate names rejected ──
(let [[ok? _] (protect ((fn []
                          (eval '(letrec [x 1
                                          x 2]
                                   x)))))]
  (assert (not ok?) "letrec: duplicate x should be a compile error"))

(let [[ok? _] (protect ((fn []
                          (eval '(letrec [@x @[1 2 3]
                                          x (freeze x)]
                                   x)))))]
  (assert (not ok?) "letrec: @x then x should be a compile error"))
