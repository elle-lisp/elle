;; List utility library
(define (sum lst)
  (if (nil? lst)
    0
    (+ (first lst) (sum (rest lst)))))

(define (product lst)
  (if (nil? lst)
    1
    (* (first lst) (product (rest lst)))))
