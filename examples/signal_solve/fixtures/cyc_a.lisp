(elle/epoch 14)
# audited: 2026-10-05
# One half of an import cycle inside function bodies; cyc_b.lisp is the other.
# examples/signal_solve/main.rs

# The import call itself raises :fs and :error.
# expect a |:yield :error :fs|
(fn []
  (defn a [n]
    (if n
      ((get ((import "cyc_b")) :b) n)
      (yield 0)))
  {:a a})
