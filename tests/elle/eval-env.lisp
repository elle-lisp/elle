(elle/epoch 10)
# Tests for eval env argument and (environment) special form

# ──────────────────────────────────────────────────────────
# eval with explicit env struct
# ──────────────────────────────────────────────────────────

# eval uses symbol-keyed struct entries as bindings
(assert (= 43 (eval '(+ x 1) {'x 42})) "eval with env struct")

# eval with multiple bindings
(assert (= 30 (eval '(+ a b) {'a 10 'b 20})) "eval with multiple env bindings")

# eval env overrides: env binding takes precedence over nothing
(assert (= 1 (eval '(identity x) {'x 1})) "eval env provides binding")

# ──────────────────────────────────────────────────────────
# (environment) captures lexical bindings
# ──────────────────────────────────────────────────────────

# environment captures def bindings
(def x 42)
(def y 99)
(def env (environment))
(assert (= 42 (get env 'x)) "environment captures x")
(assert (= 99 (get env 'y)) "environment captures y")

# round-trip: eval + environment
(def a 10)
(assert (= 20 (eval '(+ a a) (environment))) "eval with environment round-trip")

# environment does NOT include primitives (eval binds those itself)
(def env2 (environment))
(assert (not (has? env2 '+)) "environment excludes primitives")

# nested scope: let bindings appear in environment
(def x2 1)
(let [y2 2]
  (def env3 (environment))
  (assert (has? env3 'y2) "environment captures let binding")
  (assert (has? env3 'x2) "environment captures outer binding"))

# eval with nil env works (same as no env)
(assert (= 42 (eval '42 nil)) "eval with nil env")
