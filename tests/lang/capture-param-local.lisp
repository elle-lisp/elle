(elle/epoch 15)
# audited: 2026-11-18
# A parameter and a local that a closure mutates each keep a cell of their own.
# The counter-factual: the local took the parameter's cell, so both names
# changed together, or it took a plain parameter's slot and the store failed.

(defn one-param [@a]
  (let [@b 10]
    (let [g (fn []
              (assign a (+ a 1))
              (assign b (+ b 1))
              [a b])]
      (g))))
(assert (= (one-param 1) [2 11])
        "a celled parameter and a celled local hold separate cells")

(defn plain-after [@a x]
  (let [@b 10]
    (let [g (fn []
              (assign a (+ a 1))
              (assign b (+ b x))
              [a b])]
      (g))))
(assert (= (plain-after 1 5) [2 15])
        "a plain parameter after the celled one keeps its value")

(defn plain-before [x @a]
  (let [@b 10]
    (let [g (fn []
              (assign a (+ a x))
              (assign b (+ b 1))
              [x a b])]
      (g))))
(assert (= (plain-before 5 1) [5 6 11])
        "a plain parameter before the celled one keeps its value")

(defn two-of-each [@a @b]
  (let [@c 100
        plain 7
        @d 1000]
    (let [g (fn []
              (assign a (+ a 1))
              (assign b (+ b 2))
              (assign c (+ c 3))
              (assign d (+ d 4))
              [a b c d plain])]
      (g)
      (g))))
(assert (= (two-of-each 1 10) [3 14 106 1008 7])
        "two celled parameters and two celled locals hold four cells")

(defn with-rest [@a & more]
  (let [@b 10]
    (let [g (fn []
              (assign a (+ a 1))
              (assign b (+ b (length more)))
              [a b])]
      (g))))
(assert (= (with-rest 1 :x :y :z) [2 13])
        "a rest parameter after the celled one keeps its list")

# The frame that owns the cells reads them after the closure returns.
(defn read-back [@a]
  (let [@b 10]
    (let [g (fn []
              (assign a (+ a 1))
              (assign b (+ b 1)))]
      (g)
      (g)
      [a b])))
(assert (= (read-back 1) [3 12])
        "the defining frame reads each cell after the closure wrote it")
