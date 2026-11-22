(elle/epoch 18)
# audited: 2026-11-21
# One half of an import cycle inside function bodies; cyc_b.lisp is the other.
# examples/signal_solve/main.rs

# The load reads its file with file/read, so it raises :yield, :io, :fs and
# :error. Its purity walk reads each closure with fn/captures, so it raises
# :inspect too.
# expect a |:yield :error :io :fs :inspect|
(fn []
  (defn a [n]
    (if n
      ((get ((import/load-file "cyc_b.lisp")) :b) n)
      (yield 0)))
  {:a a})
