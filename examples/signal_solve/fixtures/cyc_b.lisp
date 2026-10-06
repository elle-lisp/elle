(elle/epoch 14)
# audited: 2026-10-05
# The other half of the import cycle that cyc_a.lisp starts.
# examples/signal_solve/main.rs

# expect b |:yield :error :fs|
(fn []
  (defn b [n]
    (if n
      ((get ((import "cyc_a")) :a) n)
      0))
  {:b b})
