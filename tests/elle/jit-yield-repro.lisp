(elle/epoch 7)
## Minimal repro: parameter in range + print + apply-yield

(defn emit [& parts]
  (print (apply concat (map string parts)) "\n"))

(defn wrapper [n & parts]
  (print (string/join (map (fn [_] " ") (range 0 (* n 2))) ""))
  (apply emit parts))

(var i 0)
(while (< i 15)
  (wrapper 1 "line " i)
  (assign i (+ i 1)))

(eprintln "done")
