# ── compile/run-on smoke test ─────────────────────────────────────────
#
# The differential harness depends on (compile/run-on tier f & args).
# This test asserts the primitive exists and works on :bytecode (the
# tier always available). The other tiers are exercised by the
# directed tests that import lib/differential.lisp.

# Primitive must be registered.
(def has-run-on?
  (not (empty? (filter (fn [p] (= (get p :name) "compile/run-on"))
                       (compile/primitives)))))
(assert has-run-on?
        "compile/run-on must be a registered primitive")

# Bytecode tier is always available.
(defn add [a b] (+ a b))

(assert (= (compile/run-on :bytecode add 3 4) 7)
        "compile/run-on :bytecode add 3 4 = 7")

(assert (= (compile/run-on :bytecode add -10 30) 20)
        "compile/run-on :bytecode add -10 30 = 20")

# A function with no args.
(defn answer [] 42)
(assert (= (compile/run-on :bytecode answer) 42)
        "compile/run-on :bytecode answer = 42")

# Branching closure (covers Branch terminator, Compare, Const).
(defn abs1 [x] (if (< x 0) (- 0 x) x))
(assert (= (compile/run-on :bytecode abs1 -7) 7) "abs1(-7) = 7")
(assert (= (compile/run-on :bytecode abs1 5)  5) "abs1(5)  = 5")
(assert (= (compile/run-on :bytecode abs1 0)  0) "abs1(0)  = 0")

# Unknown tier rejected with a structured error.
(def [ok? err] (protect (compile/run-on :no-such-tier add 1 2)))
(assert (not ok?) "unknown tier must signal an error")

# Non-keyword tier rejected.
(def [ok2? _] (protect (compile/run-on "bytecode" add 1 2)))
(assert (not ok2?) "tier must be a keyword, not a string")

# Non-closure target rejected.
(def [ok3? _] (protect (compile/run-on :bytecode 42 1 2)))
(assert (not ok3?) "target must be a closure")

(println "compile/run-on: OK")
