(elle/epoch 15)
# audited: 2026-11-18
# A function takes at most 64 parameters, a collector counting as one.
# The counter-factual: a 65th parameter compiled, and when it was `@` and the
# body assigned it, the call stopped the process instead of raising.

(defn plain [n]
  (map (fn [_] (gensym "p")) (range n)))
(defn fn-form [params body]
  (list 'fn (apply array params) ;body))
(defn refused? [form]
  (let [[ok? err] (protect (eval form))]
    (and (not ok?) (string/contains? (get err :message) "at most 64 parameters"))))

(def widest (eval (fn-form (plain 64) [:ok])))
(assert (= (widest ;(range 64)) :ok) "64 parameters compile and run")

(assert (refused? (fn-form (plain 65) [:ok])) "65 parameters do not compile")

(assert (refused? (fn-form (append (plain 64) ['@q]) ['(assign q 1) 'q]))
        "a 65th parameter the body assigns does not compile")

(def last-celled
  (eval (fn-form (append (plain 63) ['@q]) ['(assign q (+ q 1)) 'q])))
(assert (= (last-celled ;(range 64)) 64)
        "the 64th parameter takes its cell when the body assigns it")

(def last-captured
  (eval (fn-form (append (plain 63) ['@q])
                 ['(let [get (fn [] q)]
                     (assign q (* q 2))
                     (get))])))
(assert (= (last-captured ;(range 64)) 126)
        "a closure reads the cell of the 64th parameter")

(def rest (eval (fn-form (append (plain 63) ['& 'more]) ['more])))
(assert (= (rest ;(range 66)) (list 63 64 65))
        "a rest collector after 63 parameters is the 64th")

(assert (refused? (fn-form (append (plain 64) ['& 'more]) ['more]))
        "a rest collector after 64 parameters does not compile")

(assert (refused? (fn-form (append (plain 64) ['&named 'k]) ['k]))
        "a &named collector after 64 parameters does not compile")

(assert (refused? (fn-form (append (append (plain 60) ['&opt]) (plain 5)) [:ok]))
        "&opt parameters count toward the limit")
