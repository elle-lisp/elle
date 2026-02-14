#!/usr/bin/env elle

; Syntax Sugar Demo
; =================
; This example demonstrates syntactic sugar features in Elle:
; - -> (thread-first): Inserts the value as the FIRST argument to each form
; - ->> (thread-last): Inserts the value as the LAST argument to each form
; inspired by Clojure and Janet Lisp.

; -> (thread-first): Inserts the value as the FIRST argument to each form
; Example: (-> 5 (+ 10) (* 2)) expands to (* (+ 5 10) 2) = 30

; ->> (thread-last): Inserts the value as the LAST argument to each form
; Example: (->> 5 (+ 10) (* 2)) expands to (* 2 (+ 10 5)) = 30

(import-file "./examples/assertions.lisp")

(define demo-thread-first
  (fn ()
    (display "=== Thread-First (->) Examples ===")
    (newline)
    
    ; Simple arithmetic chain
    (display "Simple chain: (-> 5 (+ 10) (* 2))")
    (newline)
    (display "Expected: 30, Got: ")
    (define result1 (-> 5 (+ 10) (* 2)))
    (display result1)
    (newline)
    (assert-eq result1 30 "Thread-first simple chain should be 30")
    
    ; Multiple arguments
    (newline)
    (display "With multiple args: (-> 5 (+ 10 2) (* 3))")
    (newline)
    (display "Expected: 51, Got: ")
    (define result2 (-> 5 (+ 10 2) (* 3)))
    (display result2)
    (newline)
    (assert-eq result2 51 "Thread-first with multiple args should be 51")
    
    ; Longer chain
    (newline)
    (display "Longer chain: (-> 1 (+ 1) (+ 1) (+ 1))")
    (newline)
    (display "Expected: 4, Got: ")
    (define result3 (-> 1 (+ 1) (+ 1) (+ 1)))
    (display result3)
    (newline)
    (assert-eq result3 4 "Thread-first longer chain should be 4")
    
    ; With list operations
    (newline)
    (display "With lists: (-> (list 1 2 3) (length))")
    (newline)
    (display "Expected: 3, Got: ")
    (define result4 (-> (list 1 2 3) (length)))
    (display result4)
    (newline)
    (assert-eq result4 3 "Thread-first with list length should be 3")
    
    ; Nested operations
    (newline)
    (display "Nested: (-> 10 (- 3) (+ 5))")
    (newline)
    (display "Evaluation: (+ (- 10 3) 5) = (+ 7 5) = 12")
    (newline)
    (display "Got: ")
    (define result5 (-> 10 (- 3) (+ 5)))
    (display result5)
    (newline)
    (assert-eq result5 12 "Thread-first nested should be 12")
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
    (define result6 (->> 5 (+ 10) (* 2)))
    (display result6)
    (newline)
    (assert-eq result6 30 "Thread-last simple chain should be 30")
    
    ; Multiple arguments
    (newline)
    (display "With multiple args: (->> 2 (+ 10) (* 3))")
    (newline)
    (display "Expected: 36, Got: ")
    (define result7 (->> 2 (+ 10) (* 3)))
    (display result7)
    (newline)
    (assert-eq result7 36 "Thread-last with multiple args should be 36")
    
    ; Longer chain
    (newline)
    (display "Longer chain: (->> 1 (+ 1) (+ 1) (+ 1))")
    (newline)
    (display "Expected: 4, Got: ")
    (define result8 (->> 1 (+ 1) (+ 1) (+ 1)))
    (display result8)
    (newline)
    (assert-eq result8 4 "Thread-last longer chain should be 4")
    
    ; With list operations
    (newline)
    (display "With lists: (->> (list 1 2 3) (length))")
    (newline)
    (display "Expected: 3, Got: ")
    (define result9 (->> (list 1 2 3) (length)))
    (display result9)
    (newline)
    (assert-eq result9 3 "Thread-last with list length should be 3")
    
    ; Nested operations with order difference
    (newline)
    (display "Nested: (->> 10 (- 3) (+ 5))")
    (newline)
    (display "Evaluation: (+ 5 (- 3 10)) = (+ 5 -7) = -2")
    (newline)
    (display "Got: ")
    (define result10 (->> 10 (- 3) (+ 5)))
    (display result10)
    (newline)
    (assert-eq result10 -2 "Thread-last nested should be -2")
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
    (define result11 (-> 3 (- 1) (+ 2)))
    (display result11)
    (newline)
    (assert-eq result11 4 "Thread-first comparison should be 4")
    
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
    (define result12 (->> 3 (- 1) (+ 2)))
    (display result12)
    (newline)
    (assert-eq result12 0 "Thread-last comparison should be 0")
  )
)

(define demo-forever
  (fn ()
    (display "=== Forever - Infinite Loop Sugar ===")
    (newline)
    
    ; forever is syntactic sugar for (while #t ...)
    ; Use case: Infinite loops that continue until some condition breaks them
    
    (display "Forever is syntactic sugar for (while #t ...)")
    (newline)
    (newline)
    
    ; Example 1: Forever loop with counter
    (display "Example 1: Forever loop with counter")
    (newline)
    (display "Concept: forever creates an infinite loop")
    (newline)
    (define count 0)
    (define should-continue #t)
    (display "Simulating forever with while #t:")
    (newline)
    (while should-continue
      (begin
        (set! count (+ count 1))
        (display "  count = ")
        (display count)
        (newline)
        (if (= count 3)
          (begin
            (display "  Exiting loop")
            (newline)
            (set! should-continue #f)))))
    (display "Counter after loop: ")
    (display count)
    (newline)
    (assert-eq count 3 "Loop should exit when count reaches 3")
    
    ; Example 2: Forever with multiple statements
    (newline)
    (display "Example 2: Forever with multiple statements")
    (newline)
    (define x 0)
    (define y 0)
    (define keep-looping #t)
    (display "Simulating forever with while #t:")
    (newline)
    (while keep-looping
      (begin
        (set! x (+ x 1))
        (set! y (+ y 2))
        (display "  x = ")
        (display x)
        (display ", y = ")
        (display y)
        (newline)
        (if (>= x 2)
          (set! keep-looping #f))))
    (display "Final: x = ")
    (display x)
    (display ", y = ")
    (display y)
    (newline)
    (assert-eq x 2 "Loop should exit when x reaches 2")
    (assert-eq y 4 "y should be 4 after loop")
    
    ; Example 3: Comparison with while #t
    (newline)
    (display "Example 3: Forever vs (while #t ...)")
    (newline)
    (display "Forever is equivalent to: (while #t body)")
    (newline)
    (define result-forever 0)
    (define loop-active #t)
    (while loop-active
      (begin
        (set! result-forever (+ result-forever 1))
        (if (= result-forever 5)
          (set! loop-active #f))))
    (display "Result from forever simulation: ")
    (display result-forever)
    (newline)
    (assert-eq result-forever 5 "Forever loop should reach value 5")
  )
)

(define main
  (fn ()
    (demo-thread-first)
    (demo-thread-last)
    (demo-comparison)
    (demo-forever)
    (newline)
    (display "=== Demo Complete ===")
    (newline)
    (display "Demonstrated syntactic sugar:")
    (newline)
    (display "1. Thread-first (->) - Insert value as first argument")
    (newline)
    (display "2. Thread-last (->>) - Insert value as last argument")
    (newline)
    (display "3. Forever - Infinite loop sugar for (while #t ...)")
    (newline)
  )
)

(main)
