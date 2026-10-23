(elle/epoch 15)
# audited: 2026-10-23
# Single-file cases for the solver: mutual and forward calls, a parameter called in a nested lambda, a squelch.
# examples/signal_solve/main.rs

# Every callee here has an exact signal: yield, literals, parameter calls.
# Each `expect` line is checked by `signal_solve --expect`.

# Top level: the file letrec solves its definitions jointly.
# expect top-a |:yield|
# expect top-b |:yield|
(defn top-a [n]
  (if n (top-b n) 0))
(defn top-b [n]
  (yield n)
  (top-a n))

# A parameter called inside a nested lambda: the lambda depends on it. The
# call checks no argument count, so it may raise.
# expect outer |:error| prop 0
(defn apply1 [g x]
  (g x))
(defn outer [f]
  (apply1 (fn [x] (f x)) 1))

# A squelched closure: :yield becomes :error.
# expect use-quiet |:error|
(defn noisy []
  (yield 1))
(def quiet (squelch :yield noisy))
(defn use-quiet []
  (quiet))

(fn []
  # A function body seeds each lambda it defines with its arity, so a forward
  # call reads the later definition, and a mutual recursion settles.
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
