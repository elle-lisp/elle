(elle/epoch 15)
# audited: 2026-10-12
# A parametric module whose exports call fields of the plugin it is given.
# examples/signal_solve/main.rs

# Qualified access and a call to a field each may raise.
# expect connect |:error| field 0 connect
# expect ping |:error| field 0 ping
(fn [plugin]
  (defn connect [host]
    (plugin:connect host))
  (defn ping []
    (plugin:ping))
  {:connect connect :ping ping})
