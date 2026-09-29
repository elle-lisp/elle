(elle/epoch 12)
# Minimal reproducer for UAF: closures in struct returned from function

(defn make-adder [x]
  (let [box @[x]]
    {:get (fn () (box 0))
     :add (fn [y]
            (put box 0 (+ (box 0) y))
            (box 0))}))

(let [a (make-adder 10)]
  ((a :add) 5)
  (let [v ((a :get))]
    (assert (= v 15))))

(println "region-closure-struct: PASS")
