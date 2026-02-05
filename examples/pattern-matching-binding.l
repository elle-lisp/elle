#!/usr/bin/env elle
; -*- mode: lisp -*-
; Pattern Matching with Variable Binding Example
; 
; Demonstrates the new variable binding feature in pattern matching,
; where matched values can be bound to variables for use in the result expression.

(display "=== Pattern Matching with Variable Binding Examples ===")
(newline)
(newline)

; ============================================================================
; Simple Variable Binding Examples
; ============================================================================

(display "Simple binding - return the matched value:")
(display (match 42 (x x)))
(newline)

(display "Add 10 to matched value:")
(display (match 32 (n (+ n 10))))
(newline)

(display "Double the matched value:")
(display (match 7 (n (* n 2))))
(newline)
(newline)

; ============================================================================
; String Operations with Binding
; ============================================================================

(display "Prepend to matched string:")
(display (match "world" (s (string-append "hello " s))))
(newline)

(display "Get length of matched string:")
(display (match "hello" (s (string-length s))))
(newline)
(newline)

; ============================================================================
; List Operations with Binding
; ============================================================================

(display "Get first element of list:")
(display (match (list 10 20 30) (l (first l))))
(newline)

(display "Get length of list:")
(display (match (list 1 2 3 4 5) (l (length l))))
(newline)

(display "Get rest of list length:")
(display (match (list 10 20 30 40) (l (length (rest l)))))
(newline)

(display "Check list equality:")
(display (match (list 1 2 3) (l (= l (list 1 2 3)))))
(newline)
(newline)

; ============================================================================
; Arithmetic with Binding
; ============================================================================

(display "Complex arithmetic with binding:")
(display (match 5 (x (+ (+ x 10) (* x 2)))))
(newline)

(display "Nested arithmetic:")
(display (match 3 (x (* (+ x 2) x))))
(newline)

(display "Multiple uses of bound variable:")
(display (match 4 (x (+ x (+ x x)))))
(newline)
(newline)

; ============================================================================
; Conditional Logic with Binding
; ============================================================================

(display "Use binding in comparison:")
(display (match 50 (x (> x 40))))
(newline)

(display "Use binding in if expression:")
(display (match 100 (x (if (> x 50) "large" "small"))))
(newline)

(display "Use binding with not:")
(display (match 30 (x (not (> x 100)))))
(newline)
(newline)

; ============================================================================
; Multiple Pattern Clauses with Bindings
; ============================================================================

(display "Match multiple patterns, use binding in second:")
(display (match 99 (1 "one") (2 "two") (x (+ x 1))))
(newline)

(display "Fall through to variable binding pattern:")
(display (match 42 (10 "ten") (20 "twenty") (x (* x 2))))
(newline)
(newline)

; ============================================================================
; Real-World Examples
; ============================================================================

(display "Apply discount to price:")
(display (match 100 (price (* price 0.9))))
(newline)

(display "Scale coordinates:")
(display (match (list 3 4) (coords (list (* (first coords) 2) 
                                          (* (first (rest coords)) 2)))))
(newline)

(display "Get second element:")
(display (match (list 10 20 30) (l (first (rest l)))))
(newline)

(display "Build pair from single value:")
(display (match 5 (x (list x x))))
(newline)

(display "Calculate hypotenuse components:")
(display (match 3 (x (list x (+ x 1) (+ x 2)))))
(newline)
(newline)

; ============================================================================
; Type-Based Operations with Binding
; ============================================================================

(display "Numbers: bind and increment:")
(display (match 99 (n (+ n 1))))
(newline)

(display "Floats: bind and compute:")
(display (match 2.5 (f (+ f 1.5))))
(newline)

(display "Nil pattern:")
(display (match nil (x x)))
(newline)

(display "Check nil equality:")
(display (match nil (x (= x nil))))
(newline)
(newline)

; ============================================================================
; Sequential Operations
; ============================================================================

(display "Combine multiple match results:")
(display (+ (match 10 (x (+ x 5))) (match 20 (y (+ y 5)))))
(newline)

(display "Nested matches with binding:")
(display (match 8 (x (match 4 (y (+ x y))))))
(newline)

(display "Use match result as list element:")
(display (list (match 1 (x x)) (match 2 (x x)) (match 3 (x x))))
(newline)
(newline)

; ============================================================================
; Completion Message
; ============================================================================

(display "=== All pattern matching binding examples completed! ===")
(newline)
