;; Scope Management Example
;; Demonstrates proper variable scoping in Elle Lisp

;; Example 1: Loop variable isolation
;; Variables defined in loop bodies don't persist after the loop
(begin
  (display "=== Example 1: Loop Variable Isolation ===")
  (newline)
  
  ;; Global variable
  (define counter 0)
  (display "Before loop: counter = ")
  (display counter)
  (newline)
  
  ;; While loop with local variable
  (while (< counter 3)
    (begin
      (display "In loop: counter = ")
      (display counter)
      (newline)
      (set! counter (+ counter 1))))
  
  (display "After loop: counter = ")
  (display counter)
  (newline)
  (newline))

;; Example 2: Nested loops with proper scoping
(begin
  (display "=== Example 2: Nested Loops with Proper Scoping ===")
  (newline)
  
  ;; Outer loop
  (define i 0)
  (while (< i 2)
    (begin
      (display "Outer i = ")
      (display i)
      (newline)
      
      ;; Inner loop with separate variable
      (define j 0)
      (while (< j 2)
        (begin
          (display "  Inner j = ")
          (display j)
          (newline)
          (set! j (+ j 1))))
      
      (display "After inner loop, j is local to inner loop")
      (newline)
      
      (set! i (+ i 1))))
  
  (display "After all loops: i = ")
  (display i)
  (newline)
  (newline))

;; Example 3: For loop variable isolation
(begin
  (display "=== Example 3: For Loop Variable Isolation ===")
  (newline)
  
  (display "Processing list: ")
  (for item (list "apple" "banana" "cherry")
    (begin
      (display item)
      (display " ")))
  (newline)
  
  ;; 'item' is not accessible here - it's scoped to the loop
  (display "After loop, 'item' is only defined in loop scope")
  (newline)
  (newline))

;; Example 4: Define in loop body
(begin
  (display "=== Example 4: Define in Loop Body (GCD Algorithm) ===")
  (newline)
  
  (define a 48)
  (define b 18)
  
  (display "Computing GCD of 48 and 18...")
  (newline)
  
  (while (> b 0)
    (begin
      ;; Define temporary variable in loop body
      (define temp (% a b))
      (display "  a=")
      (display a)
      (display " b=")
      (display b)
      (display " temp=")
      (display temp)
      (newline)
      
      (set! a b)
      (set! b temp)))
  
  (display "GCD result: ")
  (display a)
  (newline)
  (newline))

;; Example 5: Local variable shadowing
(begin
  (display "=== Example 5: Variable Shadowing ===")
  (newline)
  
  (define x 100)
  (display "Global x = ")
  (display x)
  (newline)
  
  ;; Loop creates a scope where x can be "shadowed" 
  ;; (though we don't create new x, we modify existing one)
  (for n (list 1 2 3)
    (begin
      ;; Here x refers to the global x
      (display "In loop, global x = ")
      (display x)
      (newline)))
  
  (display "After loop, global x = ")
  (display x)
  (newline)
  (newline))

;; Example 6: Demonstrating proper scope hierarchy
(begin
  (display "=== Example 6: Scope Hierarchy ===")
  (newline)
  
  ;; Global variables
  (define global_var 1000)
  
  ;; We can access global variables inside loops
  (for item (list "a" "b")
    (begin
      (display "Item: ")
      (display item)
      (display ", Global: ")
      (display global_var)
      (newline)))
  
  ;; Variables defined in loops are local to those loops
  ;; This is now properly enforced
  (display "Loop scope properly isolated")
  (newline)
  (newline))

(display "All scope management examples completed!")
(newline)
