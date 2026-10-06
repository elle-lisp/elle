(elle/epoch 14)
# audited: 2026-10-06
# One half of an import cycle inside function bodies; cyc_b.lisp is the other.
# examples/signal_solve/main.rs

# The load itself raises :fs and :error.
# expect a |:yield :error :fs|
(fn []
  (defn a [n]
    (if n
      ((get ((import-file "cyc_b.lisp")) :b) n)
      (yield 0)))
  {:a a})
