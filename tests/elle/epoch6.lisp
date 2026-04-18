(elle/epoch 8)
## Epoch 6 backward compatibility: nested-pair bindings still work
## via the epoch migration pass (FlattenBindings).

(assert (= (let [a 1 b 2] (+ a b)) 3) "epoch6 nested let")
(assert (= (let [[x y] [3 4]] (+ x y)) 7) "epoch6 nested destructure")
(assert (= (let* [a 1 b (+ a 1)] b) 2) "epoch6 nested let*")
(assert (= (if-let [x 42] x :nope) 42) "epoch6 nested if-let")

(println "epoch6: all passed")
