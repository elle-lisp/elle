# Utility functions for microgpt

(defn array-map [f arr]
  "Apply f to each element of an array, returning a new mutable array."
  (let* ([result @[]])
    (each v in arr
      (push result (f v)))
    result))

(defn array-map2 [f a b]
  "Apply f element-wise to two arrays, returning a new mutable array."
  (let* ([result @[]])
    (each i in (range (length a))
      (push result (f (get a i) (get b i))))
    result))

(defn shuffle! [arr]
  "Shuffle array in place using Fisher-Yates."
   (var i (- (length arr) 1))
    (while (> i 0)
       (let* ([j (floor (* (random/float) (+ i 1)))]
              [tmp (get arr i)])
       (put arr i (get arr j))
      (put arr j tmp))
    (set i (- i 1)))
  arr)

(defn make-2d [rows cols init-fn]
  "Create a rows×cols mutable 2D array, calling (init-fn r c) for each cell."
  (let* ([result @[]])
    (each r in (range rows)
      (let* ([row @[]])
        (each c in (range cols)
          (push row (init-fn r c)))
        (push result row)))
    result))

(defn make-kv-caches [n-layer]
  "Create fresh per-layer KV caches. Returns [keys-cache values-cache]."
  (let* ([ks @[]] [vs @[]])
    (each _ in (range n-layer)
      (push ks @[])
      (push vs @[]))
    [ks vs]))
