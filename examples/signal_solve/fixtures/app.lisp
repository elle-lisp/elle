(elle/epoch 14)
# audited: 2026-10-06
# Calls through module instances: a parametric module, its plugin, and a higher-order export.
# examples/signal_solve/main.rs

# expect go |:error :io :fs|
# expect quiet ||
# expect gen |:yield|
# expect pure ||
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
