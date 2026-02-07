;; Finally Clause Example
;; Demonstrates try/catch/finally expressions in Elle Lisp

;; Example 1: Basic finally block
;; Finally ensures cleanup code runs regardless of success/failure
(display "Example 1: Basic finally")
(newline)
(try 
  (+ 10 20)
  (finally 
    (display "  Cleanup executed")))
(newline)
(newline)

;; Example 2: Finally doesn't change return value
;; The try body's result is returned, finally's result is ignored
(display "Example 2: Finally preserves try value")
(newline)
(display "  Result: ")
(display (try 42 (finally 999)))
(newline)
(newline)

;; Example 3: Finally with side effects
;; Finally blocks can perform operations like display without affecting return
(display "Example 3: Finally with side effects")
(newline)
(display "  Value: ")
(display (try 100 (finally (display "[cleanup]"))))
(newline)
(newline)

;; Example 4: Nested finally blocks
;; Multiple levels of finally all execute
(display "Example 4: Nested finally blocks")
(newline)
(display "  Result: ")
(display (try 
           (try 10 
                (finally 
                  (display "[inner-cleanup]")))
           (finally 
             (display "[outer-cleanup]"))))
(newline)
(newline)

;; Example 5: Finally with simple expressions
;; Finally blocks can contain multiple expressions
(display "Example 5: Finally with expressions")
(newline)
(display "  Result: ")
(display (try 55
              (finally (begin (+ 1 2) (* 3 4) 0))))
(newline)
(newline)

;; Example 6: Finally with list operations
;; Finally can contain complex expressions
(display "Example 6: Finally with list operations")
(newline)
(display "  Result: ")
(display (try (list 1 2 3) 
              (finally (list 4 5 6))))
(newline)
(newline)

;; Example 7: Finally with arithmetic
;; Finally's computed value is discarded
(display "Example 7: Finally with arithmetic")
(newline)
(display "  Result: ")
(display (try 50 
              (finally (* 10 20))))  ;; Result 200 is ignored
(newline)
(newline)

;; Example 8: Try/catch/finally together
;; All three clauses work together
(display "Example 8: Try/catch/finally")
(newline)
(display "  Result: ")
(display (try 30
              (catch e 0)
              (finally (display "[cleanup]"))))
(newline)
(newline)

;; Example 9: Finally with conditional logic
;; Finally blocks can contain if expressions
(display "Example 9: Finally with conditional")
(newline)
(display "  Result: ")
(display (try 77
              (finally (if (> 5 3)
                         (display "[if-true]")
                         (display "[if-false]")))))
(newline)
(newline)

;; Example 10: Finally in sequence
;; Multiple try/finally expressions can be chained
(display "Example 10: Finally in sequence")
(newline)
(display "  First: ")
(display (try 1 (finally 0)))
(display ", Second: ")
(display (try 2 (finally 0)))
(display ", Third: ")
(display (try 3 (finally 0)))
(newline)
(newline)

;; Example 11: Finally with display and newline
;; Practical cleanup pattern
(display "Example 11: Cleanup pattern")
(newline)
(try
  (display "  Processing started")
  (finally
    (begin
      (newline)
      (display "  Processing complete"))))
(newline)
(newline)

;; Example 12: Finally with computation in try and finally
;; Both try and finally have complex expressions
(display "Example 12: Complex try and finally")
(newline)
(display "  Result: ")
(display (try 
           (+ (* 2 5) (* 3 4))  ;; 10 + 12 = 22
           (finally 
             (+ 100 200))))      ;; 300 ignored
(newline)
(newline)

;; Summary
(display "===== Finally Clause Examples Complete =====")
(newline)
