(elle/epoch 14)
# audited: 2026-10-05
# A ceiling the analyzer accepts, and the solver finds exceeded.
# examples/signal_solve/main.rs

# relay passes its parameter to noisy-apply, which also yields by itself. The
# solver keeps both the parameter and the :yield, so quiet's (silence) is
# exceeded by :yield.
# expect noisy-apply |:yield| prop 0
# expect relay |:yield| prop 0
# expect quiet ||
# violates quiet |:yield|
(defn noisy-apply [g x]
  (yield 1)
  (g x))
(defn relay [g x]
  (noisy-apply g x))
(defn quiet []
  (silence)
  (relay (fn [y] y) 5))
(fn [] {:quiet quiet})
