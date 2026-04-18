# ── @-mutability annotations ──────────────────────────────────────────

# ── def @ ─────────────────────────────────────────────────────────────

# (def @n 3) → mutable binding
(def @n 3)
(assign n (inc n))
(assert (= n 4) "def @n creates mutable binding")

# (def n 3) → immutable (existing behavior)
(def immut-val 42)
(assert (= immut-val 42) "def without @ is immutable")

# (def @name value) with lambda
(def @counter 0)
(def bump (fn [] (assign counter (inc counter)) counter))
(assert (= (bump) 1) "def @ with lambda capture")
(assert (= (bump) 2) "def @ with lambda capture 2")

# ── let @ ─────────────────────────────────────────────────────────────

# (let [[@x 1]] (assign x 2) x) → 2
(assert (= (let [[@x 1]] (assign x 2) x) 2) "let @x is mutable")

# (let [[x 1]] x) → 1 (immutable)
(assert (= (let [[x 1]] x) 1) "let x is immutable")

# Mixed mutable and immutable in one let
(assert (= (let [[@a 10] [b 20]]
             (assign a (+ a b))
             a)
           30)
        "let mixed @mutable and immutable")

# ── letrec @ ──────────────────────────────────────────────────────────

# Immutable letrec binding (function)
(assert (= (letrec ((f (fn [x] (if (= x 0) 1 (* x (f (- x 1)))))))
             (f 5))
           120)
        "letrec immutable by default")

# Mutable letrec binding
(assert (= (letrec ((@counter 0))
             (assign counter 42)
             counter)
           42)
        "letrec @counter is mutable")

# ── defn params ───────────────────────────────────────────────────────

# (defn f [@x] (assign x 10) x) → mutable param
(defn mut-param [@x]
  (assign x 10)
  x)
(assert (= (mut-param 5) 10) "defn @param is mutable")

# (defn f [x] x) → immutable param (no assign needed)
(defn immut-param [x] x)
(assert (= (immut-param 5) 5) "defn param is immutable")

# Optional params with @
(defn opt-mut [@x &opt @y]
  (assign x (+ x 1))
  (if y (assign y (+ y 10)) nil)
  (list x y))
(assert (= (opt-mut 1 2) (list 2 12)) "opt @params are mutable")

# Rest param with @
(defn rest-mut [& @args]
  (assign args (list 99))
  args)
(assert (= (rest-mut 1 2 3) (list 99)) "rest @param is mutable")

# ── destructure @ ─────────────────────────────────────────────────────

# def destructure: immutable by default, @ opts in
(def [@a2 b2] [1 2])
(assign a2 10)
(assert (= a2 10) "destructure @a is mutable")
(assert (= b2 2) "destructure b is immutable")

# let destructure with @
(assert (= (let [[[@x2 y2] [1 2]]]
             (assign x2 99)
             (+ x2 y2))
           101)
        "let destructure @x mutable")

# ── var still works ───────────────────────────────────────────────────

(var v 10)
(assign v 20)
(assert (= v 20) "var still works (backward compat)")

# ── compile-time assertion: assert-immutable ────────────────────────────────

# assert-immutable passes when binding is not assigned
(defn check-immutable [x]
  (assert-immutable x)
  x)
(assert (= (check-immutable 5) 5) "assert-immutable passes for unassigned binding")

# ── all tests passed ─────────────────────────────────────────────────
(println "mutability: all tests passed")
