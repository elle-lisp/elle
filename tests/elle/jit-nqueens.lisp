(elle/epoch 10)
(defn check-safe-helper [col remaining row-offset]
  (if (empty? remaining)
    true
    (let [placed-col (first remaining)]
      (if (or (= col placed-col) (= row-offset (abs (- col placed-col))))
        false
        (check-safe-helper col (rest remaining) (+ row-offset 1))))))

(defn safe? [col queens]
  (check-safe-helper col queens 1))

(defn try-cols-helper [n col queens row]
  (if (= col n)
    (list)
    (if (safe? col queens)
      (let [nq (pair col queens)]
        (append (solve-helper n (+ row 1) nq)
                (try-cols-helper n (+ col 1) queens row)))
      (try-cols-helper n (+ col 1) queens row))))

(defn solve-helper [n row queens]
  (if (= row n) (list (reverse queens)) (try-cols-helper n 0 queens row)))

(defn solve-nqueens [n]
  (solve-helper n 0 (list)))

(def result (length (solve-nqueens 8)))
(assert (= result 92) (string "expected 92, got " result))
(println "jit-nqueens: ok")
