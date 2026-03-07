# Utility functions for microgpt

(defn slice [arr start end]
  "Return a new mutable array containing arr[start..end)."
  (let* ([result @[]])
    (var i start)
    (while (< i end)
      (push result (get arr i))
      (set i (+ i 1)))
    result))

(defn map2 [f a b]
  "Apply f element-wise to two arrays, returning a new mutable array."
  (let* ([n (length a)]
         [result @[]])
    (var i 0)
    (while (< i n)
      (push result (f (get a i) (get b i)))
      (set i (+ i 1)))
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

(defn argmax [arr]
  "Return the index of the maximum element."
  (var best-i 0)
  (var best-v (get arr 0))
  (var i 1)
  (while (< i (length arr))
    (when (> (get arr i) best-v)
      (set best-i i)
      (set best-v (get arr i)))
    (set i (+ i 1)))
  best-i)

(defn make-2d [rows cols init-fn]
  "Create a 2D mutable array (array of arrays).
   init-fn is called with (row col) and returns the initial value."
  (let* ([result @[]])
    (var r 0)
    (while (< r rows)
      (let* ([row @[]])
        (var c 0)
        (while (< c cols)
          (push row (init-fn r c))
          (set c (+ c 1)))
        (push result row))
      (set r (+ r 1)))
    result))
