(elle/epoch 19)
# audited: 2026-11-24
# if-let tests its bindings in order, and both of its forms see every name it binds.
# docs/control.md

## ── The bindings and the then form ─────────────────────────────────

(assert (= (if-let [x 42] x :else) 42) "if-let truthy")
(assert (= (if-let [x nil] :then :else) :else) "if-let falsy")
(assert (= (if-let [x false] :then :else) :else) "if-let false is falsy")
(assert (= (if-let [x 1
                    y 2]
             (+ x y)
             :else) 3) "if-let: every value truthy")
(assert (= (if-let [x 1
                    y nil]
             (+ x y)
             :else) :else) "if-let: the second value falsy")
(assert (= (if-let [x 1
                    y (+ x 1)]
             y
             :else) 2) "if-let: each expression sees the names before it")
(assert (= (if-let [x nil
                    y (error :ran)]
             :then
             :else) :else)
        "if-let: the expressions after the first falsy value do not run")

## ── The else form sees every name ───────────────────────────────────

## The counter-factual: an if-let that binds no name around its else form
## reads the outer a and b.
(let [a :outer-a
      b :outer-b]
  (assert (= (if-let [a 1
                      b nil]
               :then
               [a b]) [1 nil])
          "if-let: the else form sees the names the if-let binds"))
(assert (= (if-let [a nil
                    b 2]
             :then
             [a b]) [nil nil])
        "if-let: a name whose expression did not run is nil")
(assert (= (if-let [a 1
                    b false]
             :then
             [a b]) [1 false])
        "if-let: a name holds the value its expression gave")
(assert (= (if-let [@a nil]
             :then
             (begin
               (assign a 3)
               a)) 3) "if-let: an @ name is mutable in the else form")

## ── A destructuring pattern in the else form ────────────────────────

(assert (= (if-let [[a b] [1 2]
                    c nil]
             :then
             [a b c]) [1 2 nil])
        "if-let: a pattern whose value is truthy destructures")
(assert (= (if-let [[a b] nil] :then [a b]) [nil nil])
        "if-let: a pattern whose value is falsy binds nil")
(assert (= (if-let [x nil
                    [a & r] [1 2]]
             :then
             [a r]) [nil nil])
        "if-let: a pattern that did not run binds nil, its rest name included")
(assert (= (if-let [{:k k & more} false] :then [k more]) [nil nil])
        "if-let: a struct pattern binds nil, its rest name included")
(assert (= (if-let [{:p [x y]} {:p [1 2]}
                    c nil]
             :then
             [x y c]) [1 2 nil])
        "if-let: a nested pattern that ran destructures")
(assert (= (if-let [{:p [x (y)]} nil] :then [x y]) [nil nil])
        "if-let: a nested pattern that did not run binds nil")
(assert (= (if-let [[_ b] [1 2]
                    c nil]
             :then
             b) 2) "if-let: a wildcard binds no name")
(assert (= (if-let [[@a] [1]
                    c nil]
             :then
             (begin
               (assign a (+ a 1))
               a)) 2) "if-let: an @ name in a pattern that ran is mutable")
(assert (= (if-let [[@a] nil]
             :then
             (begin
               (assign a 5)
               a)) 5)
        "if-let: an @ name in a pattern that did not run is mutable")

## The trap: read as a datum, a struct pattern is a list that starts with
## `struct`, and a list pattern that starts with `struct` is a struct pattern.
## The first would bind `struct` to nil; the second would miss `a`.
(assert (= (if-let [{:k k} nil] :then (struct :k k)) {:k nil})
        "if-let: a struct pattern binds no name struct")
(let [a :outer]
  (assert (= (if-let [(struct a b) nil] :then [a b]) [nil nil])
          "if-let: a list pattern that starts with struct binds each name"))
(let [k :outer]
  (assert (= (if-let [{'k v} nil] :then [k v]) [:outer nil])
          "if-let: a quoted struct key binds no name"))

## The trap: a name the caller binds captures a template's call placed in its
## scope, so the else form builds each pattern's names before it binds any.
(assert (= (if-let [list 1
                    [a] [2]
                    c nil]
             :then
             [list a]) [1 2])
        "if-let: a name the if-let binds captures no call of its own")

## ── Placement ───────────────────────────────────────────────────────

(assert (empty? (filter (fn [d] (= (get d :rule) "argument-placed-twice"))
                        (compile/diagnostics
                          (compile/analyze
                            "(if-let [[a b] [1 2] c nil] (println a) (list b c))"))))
        "if-let: a pattern places the else form once")

## The trap: a `block` around the then form would catch an unlabeled break
## that the caller meant for its own loop.
(assert (= (each x in [1 2 3]
             (if-let [y x
                      z (> y 1)]
               (break y)
               nil)) 2)
        "if-let: an unlabeled break in the then form leaves the caller's loop")
(assert (= (each x in [1 nil 3]
             (if-let [y x] nil (break 0))) 0)
        "if-let: an unlabeled break in the else form leaves the caller's loop")
(assert (= (each x in [[1] nil]
             (if-let [[y] x] nil (break y))) nil)
        "if-let: an unlabeled break in the else form of a pattern leaves the caller's loop")
