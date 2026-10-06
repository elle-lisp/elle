(elle/epoch 14)
# audited: 2026-10-05
# A parametric module whose exports call fields of the plugin it is given.
# examples/signal_solve/main.rs

# expect connect || field 0 connect
# expect ping || field 0 ping
(fn [plugin]
  (defn connect [host]
    (plugin:connect host))
  (defn ping []
    (plugin:ping))
  {:connect connect :ping ping})
