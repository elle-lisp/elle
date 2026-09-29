(elle/epoch 12)
# audited: 2026-09-21
# compile/symbols — sorted output, and :arity on exact-arity functions.
#
# The counter-factual: the symbol list came from hash-map iteration, so
# two runs could disagree on order and a diff over it churned; :arity was
# declared on the record and never populated.

(def a
  (compile/analyze (string "(defn zeta [a b] a)\n" "(defn alpha [x] x)\n"
                           "(defn vary [a &opt b] a)\n" "(def data 42)")))
(def syms (compile/symbols a))

# Sorted by name.
(let [names (map (fn [s] (get s :name)) syms)]
  (assert (= names (sort names)) "symbols arrive sorted by name"))

# :arity is the declared parameter count of an exact-arity function.
(let [zeta (find (fn [s] (= (get s :name) "zeta")) syms)]
  (assert (= (get zeta :arity) 2) "zeta's arity"))

# A shape one number cannot state carries no :arity.
(let [vary (find (fn [s] (= (get s :name) "vary")) syms)]
  (assert (nil? (get vary :arity)) "&opt shape has no single arity"))

# A non-function symbol carries none either.
(let [data (find (fn [s] (= (get s :name) "data")) syms)]
  (assert (nil? (get data :arity)) "a value binding has no arity"))

(println "compile-symbols-order: all tests passed")
