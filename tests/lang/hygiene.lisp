(elle/epoch 14)
# audited: 2026-09-30
# A macro template resolves its own names where the macro is defined, and datum->syntax is how a template binds a name for its caller.
# docs/macros.md

# ── each: template names are not captured by the caller's ─────────────

## The counter-factual: without hygiene, `rest` in the `each` template
## resolves to the user's `& rest` parameter, and the call fails with
## "Cannot call".
(defn iterate-rest [& rest]
  (let [out @[]]
    (each item in rest
      (push out item))
    (freeze out)))

(assert (= (iterate-rest 1 2 3) [1 2 3])
        "each: template rest not captured by user rest")

## The same for `cur`, another name the `each` template binds.
(defn iterate-cur [& cur]
  (let [out @[]]
    (each item in cur
      (push out item))
    (freeze out)))

(assert (= (iterate-cur "a" "b") ["a" "b"])
        "each: template cur not captured by user cur")

## The same for `seq`.
(defn iterate-seq [& seq]
  (let [out @[]]
    (each item in seq
      (push out item))
    (freeze out)))

(assert (= (iterate-seq :x :y) [:x :y])
        "each: template seq not captured by user seq")

## The `each` template calls `empty?` on a list. The counter-factual: a
## call-site `empty?` that answers true would end the loop before its first
## pass.
(let [empty? (fn [x] true)
      out @[]]
  (each x in (list 1 2)
    (push out x))
  (assert (= (freeze out) [1 2])
          "each: template empty? not captured by a call-site shadow"))

# ── Nested macro expansion ───────────────────────────────────────────

(defn collect-rest [& rest]
  (let [result @[]]
    (each x in rest
      (when (> x 0) (push result x)))
    (freeze result)))

(assert (= (collect-rest -1 2 -3 4) [2 4])
        "nested macros: each+when with shadowed rest")

# ── Struct iteration ─────────────────────────────────────────────────

(let [out @[]]
  (each [k v] in {:a 1 :b 2}
    (push out [k v]))
  (assert (= (length out) 2) "each: struct iteration"))

# ── Inbound capture ──────────────────────────────────────────────────

## A binding a macro template introduces must not capture a free identifier
## of the same name arriving through the macro's arguments
## (src/syntax/expand/macro_expand.rs). The counter-factual: collapse both
## `tmp` into one binding and the first case answers 1998.

(defmacro hyg-m [expr]
  `(let [tmp 999]
     (+ tmp ,expr)))
(def tmp 7)
(assert (= (hyg-m tmp) 1006)
        "macro-introduced binding must not capture an inbound identifier")

## The template's own reference still resolves to the template's binder.
(defmacro hyg-self []
  `(let [tmp 5]
     (* tmp tmp)))
(assert (= (hyg-self) 25) "template references resolve to template binders")

## Each expansion has scopes of its own, so the `tmp` of two macros stay
## distinct from each other and from the caller's.
(defmacro hyg-inner [e]
  `(let [tmp 100]
     (+ tmp ,e)))
(defmacro hyg-outer [e]
  `(let [tmp 10]
     (+ tmp (hyg-inner ,e))))
(assert (= (hyg-outer tmp) 117)
        "nested expansions keep three same-named bindings distinct")

## An identity macro returns its argument with its use-site scopes intact.
(defmacro hyg-id [e]
  e)
(let [x 42]
  (assert (= (hyg-id x) 42) "identity macro preserves use-site resolution"))

## A swap macro binds `tmp` and assigns through both arguments. The caller's
## own `tmp` keeps its value, and the two arguments trade theirs.
(defmacro swap! [a b]
  `(let [tmp ,a]
     (assign ,a ,b)
     (assign ,b tmp)))

(let [tmp 100
      @x 1
      @y 2]
  (swap! x y)
  (assert (= (list tmp x y) (list 100 2 1))
          "swap trades its arguments and leaves the caller's tmp alone"))

# ── Referential transparency ─────────────────────────────────────────

## A free variable in a macro template resolves in the macro's definition
## environment, not at the call site (docs/macros.md). A call-site local that
## shadows the name lacks the template reference's scope, so the reference
## falls through to the top-level binding.
(defn rt-helper [v]
  (* v 10))
(defmacro rt-use [x]
  `(rt-helper ,x))

(assert (= (rt-use 5) 50) "template reference works unshadowed")

(let [rt-helper (fn [v] :hijacked)]
  (assert (= (rt-helper 5) :hijacked) "call-site code still sees its shadow")
  (assert (= (rt-use 5) 50)
          "a call-site shadow must not capture the template's reference"))

# ── datum->syntax: binding a name for the caller ─────────────────────

## An anaphoric if binds `it` with the scopes of its test argument, so the
## caller's branches can name it.
(defmacro aif [test then else]
  `(let [,(datum->syntax test 'it) ,test]
     (if ,(datum->syntax test 'it) ,then ,else)))

(assert (= (aif 42 it 0) 42) "aif binds it to a truthy test")
(assert (= (aif false 42 0) 0) "aif takes the else branch on a falsy test")
(assert (= (aif (+ 1 2) (+ it 10) 0) 13) "aif binds it to a compound test")

(let [it 999]
  (assert (= (aif 42 it 0) 42)
          "the it aif binds shadows a caller's it in its branches"))

## The context may be an atom argument, which arrives as a plain value.
(defmacro bind-as-x [val body]
  `(let [,(datum->syntax val 'x) ,val]
     ,body))

(assert (= (bind-as-x 100 (+ x 1)) 101) "datum->syntax with an atom context")

## The context may be a symbol argument, which arrives as a syntax object
## whose scopes the new name copies.
(defmacro bind-it [name val body]
  `(let [,(datum->syntax name 'it) ,val]
     ,body))

(assert (= (bind-it x 42 (+ it 1)) 43) "datum->syntax with a symbol context")

## The template's own `result` carries the caller's scopes and its own. The
## binding datum->syntax makes carries the caller's alone, so the reference
## sees it.
(defmacro inject-list [ctx]
  `(let [,(datum->syntax ctx 'result) (list 1 2 3)]
     result))

(assert (= (inject-list x) (list 1 2 3))
        "a template reference sees the binding datum->syntax made")
