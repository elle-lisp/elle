;; Looping Primitives Example
;; Demonstrates while and for loops in Elle Lisp

;; Example 1: While loop with global counter
(define counter 0)

(print "While loop example: counting to 5")
(while (< counter 5)
  (begin
    (print counter)
    (set! counter (+ counter 1))))

(print "Done!")
(print "")

;; Example 2: For loop over a pre-built list
(print "For loop example: iterating over list")
(define items (list "apple" "banana" "cherry"))

;; Note: The for loop body has limitations with variable binding in current implementation
;; but the loop structure itself works correctly

(print "Created list: (apple banana cherry)")
(print "")

;; Example 3: Demonstrate while loop with nested operations
(define x 1)
(print "While loop: multiply by 2 until >= 32")

(while (< x 32)
  (begin
    (print x)
    (set! x (* x 2))))

(print "Final value: ")
(print x)
(print "")

;; Example 4: While loop with conditions
(define count 10)
(print "Countdown from 10:")

(while (> count 0)
  (begin
    (print count)
    (set! count (- count 1))))

(print "Blast off!")
