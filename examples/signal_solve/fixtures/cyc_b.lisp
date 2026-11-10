(elle/epoch 15)
# audited: 2026-10-26
# The other half of the import cycle that cyc_a.lisp starts.
# examples/signal_solve/main.rs

# expect b |:yield :error :io :fs|
(fn []
  (defn b [n]
    (if n
      ((get ((import/load-file "cyc_a.lisp")) :a) n)
      0))
  {:b b})
