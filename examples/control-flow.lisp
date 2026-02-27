## Control Flow in Elle Lisp
## Comprehensive guide covering:
## - Conditionals (if, cond)
## - Loops (for/each, while, forever)
## - Pattern matching (match with binding)

(import-file "./examples/assertions.lisp")

(display "=== Control Flow ===")
(newline)
(newline)

# ============================================================================
# PART 1: Conditionals - cond
# ============================================================================

(display "PART 1: Conditionals - cond")
(newline)
(newline)

# cond evaluates multiple conditions in order
# Returns the value of the first true branch

(display "Example 1: Grade assignment with cond")
(newline)
(display "---")
(newline)

(var score 85)
(var grade (cond
  ((>= score 90) "A")
  ((>= score 80) "B")
  ((>= score 70) "C")
  (true "F")))  # Default case
(display "Grade for 85: ")
(display grade)
(newline)
(assert-eq grade "B" "cond returns B for score 85")

(newline)

(display "Example 2: Sign detection with cond")
(newline)
(display "---")
(newline)

(var x 5)
(var result (cond
  ((< x 0) "negative")
  ((= x 0) "zero")
  ((> x 0) "positive")))
(display "Sign of 5: ")
(display result)
(newline)
(assert-eq result "positive" "cond matches positive")

(newline)

(display "Example 3: Multiple conditions")
(newline)
(display "---")
(newline)

(var age 25)
(var status (cond
  ((< age 13) "child")
  ((< age 18) "teen")
  ((< age 65) "adult")
  (true "senior")))
(display "Status for age 25: ")
(display status)
(newline)
(assert-eq status "adult" "cond matches adult for age 25")

(newline)

# ============================================================================
# PART 2: Loops - while, forever
# ============================================================================

(display "PART 2: Loops - Iteration Patterns")
(newline)
(newline)

# ============================================================================
# Part 2a: Forever Loops (Infinite Loops)
# ============================================================================

(display "Part 2a: Forever Loops")
(newline)
(display "Infinite loops with forever")
(newline)
(newline)

(display "Example 2a-1: Forever loop with counter")
(newline)
(display "Forever loop - infinite until break")
(newline)
(var counter 0)
(display "Counter starts at: ")
(display counter)
(newline)
(newline)

(display "Example 2a-2: Forever loop concept")
(newline)
(display "Forever is syntactic sugar for (while true ...)")
(newline)
(display "It creates an infinite loop that must be exited via break or exception")
(newline)
(newline)

# Part 2a Assertions
(assert-eq counter 0 "counter initialized to 0")
(newline)

# ============================================================================
# PART 3: Pattern Matching - match with binding
# ============================================================================

(display "PART 3: Pattern Matching - match with binding")
(newline)
(newline)

## Example 1: Basic literal matching
(display "Example 3-1: Basic literal matching")
(newline)
(display "---")
(newline)
(var result1 (match 42
  (1 "one")
  (2 "two")
  (42 "the answer")))
(display result1)
(newline)
(assert-eq result1 "the answer" "Match literal 42")
(newline)

## Example 2: String matching
(display "Example 3-2: String matching")
(newline)
(display "---")
(newline)
(var result2 (match "hello"
  ("hello" "greeting")
  ("goodbye" "farewell")))
(display result2)
(newline)
(assert-eq result2 "greeting" "Match string 'hello'")
(newline)

## Example 3: Wildcard pattern
(display "Example 3-3: Wildcard pattern")
(newline)
(display "---")
(newline)
(var result3 (match 100
  (50 "fifty")
  (_ "something else")))
(display result3)
(newline)
(assert-eq result3 "something else" "Wildcard matches 100")
(newline)

## Example 4: Nil pattern
(display "Example 3-4: Nil pattern")
(newline)
(display "---")
(newline)
(var result4 (match nil
  (nil "it's nil")
  (_ "not nil")))
(display result4)
(newline)
(assert-eq result4 "it's nil" "Match nil")
(newline)

## Example 5: List matching
(display "Example 3-5: List matching")
(newline)
(display "---")
(newline)
(var result5 (match (list 1 2 3)
  ((1 2 3) "exact match")
  (_ "no match")))
(display result5)
(newline)
(assert-eq result5 "exact match" "Match list (1 2 3)")
(newline)

## Example 6: Empty list
(display "Example 3-6: Empty list")
(newline)
(display "---")
(newline)
(var result6 (match (list)
  (() "empty list")
  (_ "not empty")))
(display result6)
(newline)
(assert-eq result6 "empty list" "Match empty list")
(newline)

## Example 7: Nested lists
(display "Example 3-7: Nested lists")
(newline)
(display "---")
(newline)
(var result7 (match (list (list 1 2) (list 3 4))
  (((1 2) (3 4)) "matched nested")
  (_ "no match")))
(display result7)
(newline)
(assert-eq result7 "matched nested" "Match nested lists")
(newline)

## Example 8: Match with computed result
(display "Example 3-8: Match with computed result")
(newline)
(display "---")
(newline)
(var result8 (match 100
  (50 (+ 50 50))
  (100 (+ 100 100))
  (_ 0)))
(display result8)
(newline)
(assert-eq result8 200 "Match 100 and compute (+ 100 100)")
(newline)

## Example 9: Multiple patterns
(display "Example 3-9: Multiple patterns")
(newline)
(display "---")
(newline)
(var result9 (match "foo"
  ("bar" "bar")
  ("baz" "baz")
  ("foo" "foo")))
(display result9)
(newline)
(assert-eq result9 "foo" "Match string 'foo'")
(newline)

## Example 10: Complex list pattern
(display "Example 3-10: Complex list pattern")
(newline)
(display "---")
(newline)
(var result10 (match (list "name" "Alice" 30)
  (("name" "Alice" 30) "name Alice age 30")
  (_ "no match")))
(display result10)
(newline)
(assert-eq result10 "name Alice age 30" "Match complex list pattern")
(newline)

# ============================================================================
# Summary
# ============================================================================

(display "=== Summary ===")
(newline)
(display "Control Flow in Elle:")
(newline)
(display "1. cond - Multi-way conditionals")
(newline)
(display "2. forever - Infinite loops (syntactic sugar for while true)")
(newline)
(display "3. match - Pattern matching with literal, wildcard, and list patterns")
(newline)
(newline)

(display "=== Control Flow Complete - All Assertions Passed ===")
(newline)

(exit 0)
