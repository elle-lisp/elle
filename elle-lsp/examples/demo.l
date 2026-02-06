;; Elle Lisp LSP Demo
;; This file demonstrates LSP features including:
;; - Real-time diagnostics from elle-lint
;; - Hover information
;; - Symbol navigation

;; Example 1: Proper naming conventions
(define my-function (fn [x] (+ x 1)))

;; Example 2: Using the function
(define result (my-function 41))

;; Example 3: Pattern matching with proper names
(define process-list
  (fn [items]
    (match items
      [(list:cons head tail) head]
      [_ nil])))

;; Example 4: List operations
(define calculate-sum
  (fn [numbers]
    (if (empty? numbers)
      0
      (+ (first numbers) (calculate-sum (rest numbers))))))

;; Example 5: Using module-qualified names
(define string-demo
  (fn [text]
    (string:upcase text)))

;; Example 6: Complex expression
(define complex-operation
  (fn [x y]
    (if (> x y)
      (* x (calculate-sum (list x y)))
      (+ x y))))

;; Call the function
(my-function 5)

;; List operations
(calculate-sum '(1 2 3 4 5))

;; String operations
(string-demo "hello")

;; Complex operation
(complex-operation 10 5)
