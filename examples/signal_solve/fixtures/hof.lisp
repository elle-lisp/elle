(elle/epoch 14)
# audited: 2026-10-05
# A module whose one export calls its argument.
# examples/signal_solve/main.rs

# Its signal is its argument's.
# expect each-with || prop 0
(fn []
  (defn each-with [f x]
    (f x)
    (f x))
  {:each-with each-with})
