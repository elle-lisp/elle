(elle/epoch 14)
# audited: 2026-10-05
# Stands in for a native plugin, with exports whose signals are known.
# examples/signal_solve/main.rs

(fn []
  (defn connect [host]
    (port/open host :read))
  (defn ping []
    1)
  {:connect connect :ping ping})
