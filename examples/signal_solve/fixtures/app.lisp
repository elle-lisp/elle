(elle/epoch 15)
# audited: 2026-10-12
# Calls through module instances: a parametric module, its plugin, and a higher-order export.
# examples/signal_solve/main.rs

# expect go |:error :io :fs|
# expect quiet |:error|
# expect gen |:yield :error|
# expect pure |:error|
(def plug ((import-file "fakeplug.lisp")))
(def mq ((import-file "mq.lisp") plug))
(defn go []
  (mq:connect "somewhere"))
(defn quiet []
  (mq:ping))
(def hof ((import-file "hof.lisp")))
(defn gen []
  (hof:each-with (fn [x] (yield x)) 1))
(defn pure []
  (hof:each-with (fn [x] x) 1))
(fn [] {:go go :quiet quiet :gen gen :pure pure})
