(elle/epoch 15)
# audited: 2026-10-12
# A module whose one export calls its argument.
# examples/signal_solve/main.rs

# Its signal is its argument's, and each call checks no argument count.
# expect each-with |:error| prop 0
(fn []
  (defn each-with [f x]
    (f x)
    (f x))
  {:each-with each-with})
