(elle/epoch 14)
# audited: 2026-10-06
# A per-file summary, checked with --no-follow, whose imports stay open.
# examples/signal_solve/main.rs

# With the imports open, each function's answer names the exports it calls instead of their signals.
# A call to an open export lets its arguments run, which is sound but
# conservative: a literal argument adds :error, because calling it raises.
# The link that supplies the export makes the answer exact (see app.lisp).
# expect go2 |:error| import connect
# expect gen2 |:yield :error| import each-with
# expect pure2 |:error| import each-with
(def mq ((import-file "mq.lisp") ((import-file "fakeplug.lisp"))))
(def hof ((import-file "hof.lisp")))
(defn go2 []
  (mq:connect "somewhere"))
(defn gen2 []
  (hof:each-with (fn [x] (yield x)) 1))
(defn pure2 []
  (hof:each-with (fn [x] x) 1))
(fn [] {:go2 go2 :gen2 gen2 :pure2 pure2})
