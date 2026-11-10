(elle/epoch 15)
# audited: 2026-10-26
# One half of an import cycle inside function bodies; cyc_b.lisp is the other.
# examples/signal_solve/main.rs

# The load reads its file with file/read, so it raises :yield, :io, :fs and
# :error.
# expect a |:yield :error :io :fs|
(fn []
  (defn a [n]
    (if n
      ((get ((import/load-file "cyc_b.lisp")) :b) n)
      (yield 0)))
  {:a a})
