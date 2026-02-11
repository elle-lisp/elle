#!/usr/bin/env elle

; Threading Operators Demo
; ========================
; This example demonstrates the -> and ->> threading operators
; inspired by Clojure and Janet Lisp.

; -> (thread-first): Inserts the value as the FIRST argument to each form
; Example: (-> 5 (+ 10) (* 2)) expands to (* (+ 5 10) 2) = 30

; ->> (thread-last): Inserts the value as the LAST argument to each form
; Example: (->> 5 (+ 10) (* 2)) expands to (* 2 (+ 10 5)) = 30

(define demo-thread-first
  (fn ()
    (display "=== Thread-First (->) Examples ===")
    (newline)
    
    ; Simple arithmetic chain
    (display "Simple chain: (-> 5 (+ 10) (* 2))")
    (newline)
    (display "Expected: 30, Got: ")
    (display (-> 5 (+ 10) (* 2)))
    (newline)
    
    ; Multiple arguments
    (newline)
    (display "With multiple args: (-> 5 (+ 10 2) (* 3))")
    (newline)
    (display "Expected: 51, Got: ")
    (display (-> 5 (+ 10 2) (* 3)))
    (newline)
    
    ; Longer chain
    (newline)
    (display "Longer chain: (-> 1 (+ 1) (+ 1) (+ 1))")
    (newline)
    (display "Expected: 4, Got: ")
    (display (-> 1 (+ 1) (+ 1) (+ 1)))
    (newline)
    
    ; With list operations
    (newline)
    (display "With lists: (-> (list 1 2 3) (length))")
    (newline)
    (display "Expected: 3, Got: ")
    (display (-> (list 1 2 3) (length)))
    (newline)
    
    ; Nested operations
    (newline)
    (display "Nested: (-> 10 (- 3) (+ 5))")
    (newline)
    (display "Evaluation: (+ (- 10 3) 5) = (+ 7 5) = 12")
    (newline)
    (display "Got: ")
    (display (-> 10 (- 3) (+ 5)))
    (newline)
  )
)

(define demo-thread-last
  (fn ()
    (display "=== Thread-Last (->>) Examples ===")
    (newline)
    
    ; Simple arithmetic chain
    (display "Simple chain: (->> 5 (+ 10) (* 2))")
    (newline)
    (display "Expected: 30, Got: ")
    (display (->> 5 (+ 10) (* 2)))
    (newline)
    
    ; Multiple arguments
    (newline)
    (display "With multiple args: (->> 2 (+ 10) (* 3))")
    (newline)
    (display "Expected: 36, Got: ")
    (display (->> 2 (+ 10) (* 3)))
    (newline)
    
    ; Longer chain
    (newline)
    (display "Longer chain: (->> 1 (+ 1) (+ 1) (+ 1))")
    (newline)
    (display "Expected: 4, Got: ")
    (display (->> 1 (+ 1) (+ 1) (+ 1)))
    (newline)
    
    ; With list operations
    (newline)
    (display "With lists: (->> (list 1 2 3) (length))")
    (newline)
    (display "Expected: 3, Got: ")
    (display (->> (list 1 2 3) (length)))
    (newline)
    
    ; Nested operations with order difference
    (newline)
    (display "Nested: (->> 10 (- 3) (+ 5))")
    (newline)
    (display "Evaluation: (+ 5 (- 3 10)) = (+ 5 -7) = -2")
    (newline)
    (display "Got: ")
    (display (->> 10 (- 3) (+ 5)))
    (newline)
  )
)

(define demo-comparison
  (fn ()
    (display "=== Thread-First vs Thread-Last Comparison ===")
    (newline)
    
    ; Show how the same threading path gives different results
    ; with different operators
    
    (display "Value: 3")
    (newline)
    (display "Operations: (- 1), (+ 2)")
    (newline)
    
    (newline)
    (display "Thread-first: (-> 3 (- 1) (+ 2))")
    (newline)
    (display "  = (+ (- 3 1) 2)")
    (newline)
    (display "  = (+ 2 2)")
    (newline)
    (display "  = 4")
    (newline)
    (display "Result: ")
    (display (-> 3 (- 1) (+ 2)))
    (newline)
    
    (newline)
    (display "Thread-last: (->> 3 (- 1) (+ 2))")
    (newline)
    (display "  = (+ 2 (- 1 3))")
    (newline)
    (display "  = (+ 2 -2)")
    (newline)
    (display "  = 0")
    (newline)
    (display "Result: ")
    (display (->> 3 (- 1) (+ 2)))
    (newline)
  )
)

(define main
  (fn ()
    (demo-thread-first)
    (demo-thread-last)
    (demo-comparison)
    (newline)
    (display "=== Demo Complete ===")
    (newline)
  )
)

(main)
