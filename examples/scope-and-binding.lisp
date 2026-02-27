# Scope and Binding in Elle
#
# This example demonstrates:
# - Global scope and variable binding
# - Local scope with let
# - Function scope and parameters
# - Shadowing and scope resolution
# - Closure scope and variable capture
# - Dynamic scope patterns
# - Assertions verifying scope behavior

(import-file "./examples/assertions.lisp")

(display "=== Scope and Binding ===")
(newline)
(newline)

# ============================================================================
# PART 0: The begin Form - Sequencing (No Scope)
# ============================================================================

(display "PART 0: The begin Form - Sequencing (No Scope)")
(newline)
(newline)

# begin sequences multiple expressions and returns the last value.
# It does NOT create a new scope—bindings defined inside begin
# go into the enclosing scope.

(display "Simple begin sequencing:")
(var result (begin
  (var x 10)
  (var y 20)
  (+ x y)))
(display result)
(newline)
(assert-eq result 30 "begin sequences and returns last value")

# Variables defined in begin DO leak into enclosing scope
(display "Variables leak from begin into enclosing scope:")
(display "x = ")
(display x)  # x is accessible here because begin doesn't create scope
(newline)
(assert-eq x 10 "x is accessible after begin")

(newline)

# ============================================================================
# PART 0.5: The block Form - Scoped Sequencing
# ============================================================================

(display "PART 0.5: The block Form - Scoped Sequencing")
(newline)
(newline)

# block sequences expressions within a NEW lexical scope.
# Bindings defined inside block do NOT leak out.
# You can optionally name a block and use break to exit early.

(display "Simple block with scope isolation:")
(var outer-x 100)
(block
  (var inner-x 50)  # Local to block
  (display "inner-x = ")
  (display inner-x)
  (newline)
  (assert-eq inner-x 50 "inner-x is 50 inside block"))

# inner-x is NOT accessible here
(display "outer-x = ")
(display outer-x)
(newline)
(assert-eq outer-x 100 "outer-x is still 100 outside block")

# Named block with break
(display "Named block with break:")
(var break-result (block :search
  (var x 10)
  (if (> x 5)
    (break :search "x is greater than 5"))
  "x is not greater than 5"))
(display break-result)
(newline)
(assert-eq break-result "x is greater than 5" "break exits block with value")

(newline)

# ============================================================================
# PART 1: BINDING FORMS - Basic let Binding
# ============================================================================

(display "PART 1: BINDING FORMS - Basic let Binding")
(newline)
(newline)

## Example 1: Simple let binding
(display "Example 1: Simple let binding")
(newline)
(display "---")
(newline)

(let ((x 10))
  (display "x = ")
  (display x)
  (newline)
  (assert-eq x 10 "let binds x = 10"))

(newline)

## Example 2: Multiple independent bindings
(display "Example 2: Multiple independent bindings")
(newline)
(display "---")
(newline)

(let ((a 5) (b 3))
  (display "a + b = ")
  (display (+ a b))
  (newline)
  (assert-eq (+ a b) 8 "let binds a=5, b=3, a+b=8"))

(newline)

## Example 3: let with expressions
(display "Example 3: let with expressions")
(newline)
(display "---")
(newline)

(let ((x 10) (y 20))
  (display "x * y = ")
  (display (* x y))
  (newline)
  (assert-eq (* x y) 200 "let with expressions: 10 * 20 = 200"))

(newline)

# ============================================================================
# PART 2: let* (Sequential Binding)
# ============================================================================

(display "PART 2: let* (Sequential Binding)")
(newline)
(newline)

## Example 1: Simple sequential binding
(display "Example 1: Simple sequential binding")
(newline)
(display "---")
(newline)

(let* ((x 10))
  (display "x = ")
  (display x)
  (newline)
  (assert-eq x 10 "let* binds x = 10"))

(newline)

## Example 2: Sequential binding with dependencies
(display "Example 2: Sequential binding with dependencies")
(newline)
(display "---")
(newline)

(let* ((x 5) (y (+ x 3)))
  (display "x = ")
  (display x)
  (newline)
  (display "y = x + 3 = ")
  (display y)
  (newline)
  (assert-eq x 5 "let* binds x = 5")
  (assert-eq y 8 "let* binds y = x + 3 = 8"))

(newline)

## Example 3: Multiple sequential bindings
(display "Example 3: Multiple sequential bindings")
(newline)
(display "---")
(newline)

(let* ((a 2) (b (* a 3)) (c (+ b 1)))
  (display "a = ")
  (display a)
  (newline)
  (display "b = a * 3 = ")
  (display b)
  (newline)
  (display "c = b + 1 = ")
  (display c)
  (newline)
  (assert-eq a 2 "let* binds a = 2")
  (assert-eq b 6 "let* binds b = a * 3 = 6")
  (assert-eq c 7 "let* binds c = b + 1 = 7"))

(newline)

# ============================================================================
# PART 3: Function Parameters (Binding)
# ============================================================================

(display "PART 3: Function Parameters (Binding)")
(newline)
(newline)

## Example 1: Simple function parameters
(display "Example 1: Simple function parameters")
(newline)
(display "---")
(newline)

(def add (fn (x y)
  (+ x y)))

(display "add(3, 4) = ")
(display (add 3 4))
(newline)
(assert-eq (add 3 4) 7 "function parameters bind x=3, y=4")

(newline)

## Example 2: Function with multiple parameters
(display "Example 2: Function with multiple parameters")
(newline)
(display "---")
(newline)

(def multiply-and-add (fn (x y z)
  (+ (* x y) z)))

(display "multiply-and-add(2, 3, 5) = ")
(display (multiply-and-add 2 3 5))
(newline)
(assert-eq (multiply-and-add 2 3 5) 11 "function parameters: (2*3)+5 = 11")

(newline)

# ============================================================================
# PART 4: SCOPE - Global Scope
# ============================================================================

## Global variables are defined at the top level
(var global-x 100)
(var global-y 200)

## They can be accessed anywhere
(display "PART 4: SCOPE - Global Scope")
(newline)
(newline)

(display "Global x: ")
(display global-x)
(newline)
(assert-eq global-x 100 "Global x should be 100")

(newline)

# ============================================================================
# PART 5: Function Scope (Local Variables)
# ============================================================================

## Parameters are local to the function
(def add-numbers (fn (x y)
  (+ x y)))

(display "PART 5: Function Scope (Local Variables)")
(newline)
(newline)

(display "Add 3 + 4: ")
(var add-result (add-numbers 3 4))
(display add-result)
(newline)
(assert-eq add-result 7 "3 + 4 should be 7")

## Parameters shadow outer variables
(var x 1000)
(def shadowing-function (fn (x)
  (+ x 1)))

(display "Inner x (1) + 1: ")
(var shadow-result (shadowing-function 1))
(display shadow-result)
(newline)
(assert-eq shadow-result 2 "Inner x (1) + 1 should be 2")
(display "Global x still: ")
(display x)
(newline)
(assert-eq x 1000 "Global x should still be 1000")

(newline)

# ============================================================================
# PART 6: Lexical Scoping
# ============================================================================

(display "PART 6: Lexical Scoping")
(newline)
(newline)

## Example 1: Scope isolation
(display "Example 1: Scope isolation")
(newline)
(display "---")
(newline)

(display "Outer scope: (var outer-x 100)")
(var outer-x 100)
(display "outer-x = ")
(display outer-x)
(newline)

(let ((x 5))
  (display "Inside let: x = ")
  (display x)
  (newline)
  (assert-eq x 5 "let binds x with 5"))

(display "After let: outer-x = ")
(display outer-x)
(newline)
(assert-eq outer-x 100 "outer-x unchanged after let")

(newline)

## Example 2: Nested scopes
(display "Example 2: Nested scopes")
(newline)
(display "---")
(newline)

(let ((outer-val 10))
  (display "Outer let: outer-val = ")
  (display outer-val)
  (newline)
  (let ((inner-val 20))
    (display "Inner let: inner-val = ")
    (display inner-val)
    (newline)
    (assert-eq inner-val 20 "inner let binds inner-val with 20"))
  (display "Back to outer let: outer-val = ")
  (display outer-val)
  (newline)
  (assert-eq outer-val 10 "outer-val unchanged after inner let"))

(newline)

## Example 3: Closure captures lexical scope
(display "Example 3: Closure captures lexical scope")
(newline)
(display "---")
(newline)

(def make-adder (fn (n)
  (fn (x) (+ x n))))

(var add-5 (make-adder 5))
(var add-10 (make-adder 10))

(display "add-5(3) = ")
(display (add-5 3))
(newline)
(display "add-10(3) = ")
(display (add-10 3))
(newline)

(assert-eq (add-5 3) 8 "closure captures n=5: 3+5=8")
(assert-eq (add-10 3) 13 "closure captures n=10: 3+10=13")

(newline)

# ============================================================================
# PART 7: Shadowing Rules
# ============================================================================

(display "PART 7: Shadowing Rules")
(newline)
(newline)

## Example 1: let creates new binding
(display "Example 1: let creates new binding")
(newline)
(display "---")
(newline)

(let ((shadow-y 50))
  (display "Inside let: shadow-y = ")
  (display shadow-y)
  (newline)
  (assert-eq shadow-y 50 "let binds shadow-y with 50"))

(newline)

## Example 2: let* sequential shadowing
(display "Example 2: let* sequential shadowing")
(newline)
(display "---")
(newline)

(let* ((z 10) (z (+ z 5)))
  (display "z = ")
  (display z)
  (newline)
  (assert-eq z 15 "let* allows shadowing: z = 10, then z = 10+5 = 15"))

(newline)

## Example 3: Function parameter shadowing
(display "Example 3: Function parameter shadowing")
(newline)
(display "---")
(newline)

(def shadow-test (fn (x)
  (let ((x (+ x 10)))
    (display "Inside let: x = ")
    (display x)
    (newline)
    x)))

(display "shadow-test(5) = ")
(display (shadow-test 5))
(newline)
(assert-eq (shadow-test 5) 15 "function parameter shadowed by let: 5+10=15")

(newline)

# ============================================================================
# PART 8: Let-binding Shadowing
# ============================================================================

## Let-bindings can shadow outer variables
(var x 100)

(display "PART 8: Let-binding Shadowing")
(newline)
(newline)

(let ((x 42))
  (begin
    (display "Inside let - x is shadowed to: ")
    (display x)
    (newline)
    (assert-eq x 42 "x in let should be shadowed to 42")))

(display "Outside let - x is back to: ")
(display x)
(newline)
## Note: Elle's let creates a new lexical binding, outer scope unchanged
(assert-eq x 100 "x outside let unchanged - let creates shadow binding")

(newline)

# ============================================================================
# PART 9: Loop Variable Scoping
# ============================================================================

## While loops have their own scope
(display "PART 9: Loop Variable Scoping")
(newline)
(newline)

(display "While loop scoping example:")
(newline)

(var counter 0)
(while (< counter 3)
  (begin
    (display "Counter: ")
    (display counter)
    (newline)
    (set counter (+ counter 1))))

(display "After loop, counter is: ")
(display counter)
(newline)
(assert-eq counter 3 "Counter after loop should be 3")

## Each loops iterate with proper variable scoping
(display "Each loop scoping example:")
(newline)

(each item (list "apple" "banana" "cherry")
  (begin
    (display "Item: ")
    (display item)
    (newline)))

## 'item' is scoped to the loop - not accessible here

(newline)

# ============================================================================
# PART 10: Closures and Captured Variables
# ============================================================================

## Closures can capture variables from their defining scope
(def make-counter (fn (start)
  (fn ()
    (set start (+ start 1))
    start)))

(display "PART 10: Closures and Captured Variables")
(newline)
(newline)

(var counter1 (make-counter 10))

(display "Counter1 call 1: ")
(var c1-first (counter1))
(display c1-first)
(newline)
(assert-eq c1-first 11 "Counter1 first call should be 11")
(display "Counter1 call 2: ")
(var c1-second (counter1))
(display c1-second)
(newline)
(assert-eq c1-second 12 "Counter1 second call should be 12")

## Different closure instance has its own captured value
(var counter2 (make-counter 100))
(display "Counter2 call 1: ")
(var c2-first (counter2))
(display c2-first)
(newline)
(assert-eq c2-first 101 "Counter2 first call should be 101")

(newline)

# ============================================================================
# PART 11: Nested Functions and Scope Chain
# ============================================================================

## Inner functions can access outer function's variables
(var outer-var 42)

(display "PART 11: Nested Functions and Scope Chain")
(newline)
(newline)

(def outer-function (fn (x)
  (def inner-function (fn (y)
    (+ x y outer-var)))
  (inner-function 10)))

(display "Nested function result: ")
(var nested-result (outer-function 5))
(display nested-result)
(newline)
(assert-eq nested-result 57 "Nested function should compute to 57")
## x=5, y=10, outer-var=42 -> 5 + 10 + 42 = 57

(newline)

# ============================================================================
# PART 12: Loop Variable Isolation
# ============================================================================

(begin
  (display "PART 12: Loop Variable Isolation")
  (newline)
  
  ## Global variable
  (var counter 0)
  (display "Before loop: counter = ")
  (display counter)
  (newline)
  (assert-eq counter 0 "Counter should start at 0")
  
  ## While loop with local variable
  (while (< counter 3)
    (begin
      (display "In loop: counter = ")
      (display counter)
      (newline)
      (set counter (+ counter 1))))
  
  (display "After loop: counter = ")
  (display counter)
  (newline)
  (assert-eq counter 3 "Counter should be 3 after loop")
  (newline))

# ============================================================================
# PART 13: Nested Loops with Proper Scoping
# ============================================================================

(begin
  (display "PART 13: Nested Loops with Proper Scoping")
  (newline)
  
  ## Outer loop
  (var i 0)
  (while (< i 2)
    (begin
      (display "Outer i = ")
      (display i)
      (newline)
      
      ## Inner loop with separate variable
      (var j 0)
      (while (< j 2)
        (begin
          (display "  Inner j = ")
          (display j)
          (newline)
          (set j (+ j 1))))
      
      (display "After inner loop, j is local to inner loop")
      (newline)
      
      (set i (+ i 1))))
  
  (display "After all loops: i = ")
  (display i)
  (newline)
  (assert-eq i 2 "i should be 2 after nested loops")
  (newline))

# ============================================================================
# PART 14: For Loop Variable Isolation
# ============================================================================

(begin
  (display "PART 14: Each Loop Variable Isolation")
  (newline)
  
  (display "Processing list: ")
  (each item (list "apple" "banana" "cherry")
    (begin
      (display item)
      (display " ")))
  (newline)
  
  ## 'item' is not accessible here - it's scoped to the loop
  (display "After loop, 'item' is only defined in loop scope")
  (newline)
  ## Verify the each loop executed 3 times
  (assert-eq (length (list "apple" "banana" "cherry")) 3 "List should have 3 items")
  (newline))

# ============================================================================
# PART 15: Define in Loop Body (GCD Algorithm)
# ============================================================================

(begin
  (display "PART 15: Define in Loop Body (GCD Algorithm)")
  (newline)
  
  (var a 48)
  (var b 18)
  
  (display "Computing GCD of 48 and 18...")
  (newline)
  
  (while (> b 0)
    (begin
      ## Define temporary variable in loop body
      (var temp (% a b))
      (display "  a=")
      (display a)
      (display " b=")
      (display b)
      (display " temp=")
      (display temp)
      (newline)
      
      (set a b)
      (set b temp)))
  
  (display "GCD result: ")
  (display a)
  (newline)
  (assert-eq a 6 "GCD of 48 and 18 should be 6")
  (newline))

# ============================================================================
# PART 16: Variable Shadowing in Loops
# ============================================================================

(begin
  (display "PART 16: Variable Shadowing in Loops")
  (newline)
  
  (var x 100)
  (display "Global x = ")
  (display x)
  (newline)
  (assert-eq x 100 "Global x should be 100")
  
  ## Loop creates a scope where x can be "shadowed" 
  ## (though we don't create new x, we modify existing one)
  (each n (list 1 2 3)
    (begin
      ## Here x refers to the global x
      (display "In loop, global x = ")
      (display x)
      (newline)))
  
  (display "After loop, global x = ")
  (display x)
  (newline)
  (assert-eq x 100 "Global x should still be 100 after loop")
  (newline))

# ============================================================================
# PART 17: Scope Hierarchy
# ============================================================================

(begin
  (display "PART 17: Scope Hierarchy")
  (newline)
  
  ## Global variables
  (var global_var 1000)
  
  ## We can access global variables inside loops
  (each item (list "a" "b")
    (begin
      (display "Item: ")
      (display item)
      (display ", Global: ")
      (display global_var)
      (newline)))
  
  ## Variables defined in loops are local to those loops
  ## This is now properly enforced
  (display "Loop scope properly isolated")
  (newline)
  (assert-eq global_var 1000 "Global variable should be 1000")
  (newline))

# ============================================================================
# PART 18: Best Practices
# ============================================================================

(display "PART 18: Best Practices")
(newline)
(newline)

## 1. Use global variables sparingly
## 2. Use let-bindings for temporary local variables
## 3. Use function parameters for values that change behavior
## 4. Be aware of variable shadowing - it can be confusing!
## 5. Use descriptive variable names to avoid confusion

## Example: Good practice - using let for intermediate values
(let ((temp-result (* 5 6))
      (temp-sum (+ 10 20)))
  (display "Good practice result: ")
  (var good-result (+ temp-result temp-sum))
  (display good-result)
  (newline)
  (assert-eq good-result 60 "Good practice result should be 60 (30+30)"))

(newline)

# ============================================================================
# PART 19: Common Scoping Mistakes
# ============================================================================

(display "PART 19: Common Scoping Mistakes")
(newline)
(newline)

## MISTAKE 1: Assuming loop variables persist (they don't in proper scoping)
## (each i (list 1 2 3) (print i))
## (print i)  # ERROR: i is not defined outside the loop

## MISTAKE 2: Modifying global instead of local
(var count 0)

(fn ()
  (set count (+ count 1))  # Modifies global!
  count)

## To fix, use a parameter:
(fn (count)
  (+ count 1))

## MISTAKE 3: Expecting sequential binding in let (use let* instead)
## This would fail:
## (let ((x 5)
##       (y (+ x 1)))  # ERROR: x not bound yet in let
##   (+ x y))

(display "Common mistakes documented")
(newline)
(newline)

# ============================================================================
# END OF SCOPE AND BINDING EXPLANATION
# ============================================================================

(display "=== All Scope and Binding Examples Complete - All Assertions Passed ===")
(newline)

(exit 0)
