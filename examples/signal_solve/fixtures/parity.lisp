(elle/epoch 14)
# audited: 2026-10-05
# Single-file cases for the solver, most of which today's analyzer answers as unknown.
# examples/signal_solve/main.rs

# Every callee here has an exact signal: yield, literals, parameter calls.
# Each `expect` line is checked by `signal_solve --expect`.

# Top level: the file letrec runs a fixpoint, so both are exact.
# expect top-a |:yield|
# expect top-b |:yield|
(defn top-a [n]
  (if n (top-b n) 0))
(defn top-b [n]
  (yield n)
  (top-a n))

# A parameter called inside a nested lambda: the lambda depends on it.
# expect outer || prop 0
(defn apply1 [g x]
  (g x))
(defn outer [f]
  (apply1 (fn [x] (f x)) 1))

# A squelched closure: :yield becomes :error.
# expect use-quiet |:error|
(defn noisy []
  (yield 1))
(def quiet (squelch noisy :yield))
(defn use-quiet []
  (quiet))

(fn []
  # Inside a function body there is no fixpoint: a forward reference reads
  # nothing yet, and a mutual recursion reads the seed.
  # expect fwd ||
  (defn fwd [x]
    (later x))
  (defn later [x]
    x)
  # expect mut-a |:yield|
  # expect mut-b |:yield|
  (defn mut-a [n]
    (if n (mut-b n) 0))
  (defn mut-b [n]
    (yield n)
    (mut-a n))
  {:fwd fwd :mut-a mut-a :outer outer :use-quiet use-quiet})
