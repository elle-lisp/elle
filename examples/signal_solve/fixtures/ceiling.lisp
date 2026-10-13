(elle/epoch 15)
# audited: 2026-10-12
# A ceiling wider than its body: the solver answers what the body raises, and finds no violation.
# examples/signal_solve/main.rs

# A ceiling checks the body and does not replace it, so a caller sees the
# body's |:yield|, not the |:yield :error| the ceiling admits.
# expect produce |:yield|
# expect consume |:yield|
(defn produce [x]
  (attune! |:yield :error|)
  (yield x))
(defn consume []
  (produce 1))
(fn [] {:consume consume})
