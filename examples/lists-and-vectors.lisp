; Lists and Vectors - Sequence operations and comparisons

(import-file "./examples/assertions.lisp")

(display "=== Lists and Vectors ===")
(newline)
(newline)

; ============================================================================
; PART 1: List Construction and Access
; ============================================================================

(display "PART 1: List Construction and Access")
(newline)
(display "Creating and accessing list elements")
(newline)
(newline)

(display "Example 1a-1: Create a list with list function")
(newline)
(var my-list (list 1 2 3 4 5))
(display "List: ")
(display my-list)
(newline)
(newline)

(display "Example 1a-2: Create a list with cons (head/tail construction)")
(newline)
(var cons-list (cons 0 (cons 1 (cons 2 (list)))))
(display "List: ")
(display cons-list)
(newline)
(newline)

(display "Example 1a-3: Access first element")
(newline)
(display "First of (a b c d e): ")
(display (first (list 'a 'b 'c 'd 'e)))
(newline)
(newline)

(display "Example 1a-4: Access rest of list")
(newline)
(display "Rest of (a b c d e): ")
(display (rest (list 'a 'b 'c 'd 'e)))
(newline)

; Part 1a Assertions
(assert-eq (length my-list) 5 "list length = 5")
(assert-eq (length cons-list) 3 "cons-list length = 3")
(assert-eq (first (list 'a 'b 'c 'd 'e)) 'a "first element = a")
(newline)

; ============================================================================
; PART 1b: List Manipulation Functions
; ============================================================================

(display "PART 1b: List Manipulation")
(newline)
(display "Useful operations on sequences")
(newline)
(newline)

(display "Example 1b-1: Length of lists")
(newline)
(display "Length of (a b c d): ")
(display (length (list 'a 'b 'c 'd)))
(newline)
(newline)

(display "Example 1b-2: Reverse a list")
(newline)
(display "Original: (1 2 3 4 5)")
(newline)
(display "Reversed: ")
(display (reverse (list 1 2 3 4 5)))
(newline)
(newline)

(display "Example 1b-3: Take first N elements")
(newline)
(display "List: (10 20 30 40 50)")
(newline)
(display "Take 3: ")
(display (take 3 (list 10 20 30 40 50)))
(newline)
(newline)

(display "Example 1b-4: Drop first N elements")
(newline)
(display "List: (10 20 30 40 50)")
(newline)
(display "Drop 2: ")
(display (drop 2 (list 10 20 30 40 50)))
(newline)
(newline)

(display "Example 1b-5: Append multiple lists")
(newline)
(display "List1: (1 2 3)")
(newline)
(display "List2: (4 5 6)")
(newline)
(display "Appended: ")
(display (append (list 1 2 3) (list 4 5 6)))
(newline)
(newline)

(display "Example 1b-6: Get nth element (0-indexed)")
(newline)
(display "List: (a b c d e)")
(newline)
(display "Element at index 2: ")
(display (nth 2 (list 'a 'b 'c 'd 'e)))
(newline)
(newline)

(display "Example 1b-7: Get last element")
(newline)
(display "List: (first second third last)")
(newline)
(display "Last element: ")
(display (last (list 'first 'second 'third 'last)))
(newline)
(newline)

; Part 1b Assertions
(assert-eq (length (list 'a 'b 'c 'd)) 4 "length of 4-element list = 4")
(assert-eq (length (reverse (list 1 2 3 4 5))) 5 "reversed list length = 5")
(assert-eq (length (take 3 (list 10 20 30 40 50))) 3 "take 3 length = 3")
(assert-eq (length (drop 2 (list 10 20 30 40 50))) 3 "drop 2 length = 3")
(newline)

; ============================================================================
; PART 1c: Building Lists with Cons
; ============================================================================

(display "PART 1c: Building Lists with Cons")
(newline)
(display "Constructing lists element by element")
(newline)
(newline)

(display "Example 1c-1: Build a list incrementally")
(newline)
(var built-list 
  (cons 1 (cons 2 (cons 3 (cons 4 (cons 5 (list)))))))
(display "Built with cons: ")
(display built-list)
(newline)
(newline)

(display "Example 1c-2: Prepending to a list")
(newline)
(var original (list 2 3 4 5))
(var prepended (cons 1 original))
(display "Original: ")
(display original)
(newline)
(display "Prepended 1: ")
(display prepended)
(newline)
(newline)

; ============================================================================
; PART 1d: Arithmetic Patterns
; ============================================================================

(display "PART 1d: Arithmetic with Lists")
(newline)
(display "Computing with sequence data")
(newline)
(newline)

(display "Example 1d-1: Sum of list elements (manual recursion pattern)")
(newline)
(display "Manual iteration: (1 + 2 + 3 + 4 + 5)")
(newline)
(var num1 1)
(var num2 2)
(var num3 3)
(var num4 4)
(var num5 5)
(var manual-sum (+ num1 (+ num2 (+ num3 (+ num4 num5)))))
(display "Sum: ")
(display manual-sum)
(newline)
(newline)

(display "Example 1d-2: Demonstration of length as iteration")
(newline)
(var numbers (list 10 20 30 40 50))
(var count (length numbers))
(display "List: ")
(display numbers)
(newline)
(display "Length (count of elements): ")
(display count)
(newline)
(newline)

; ============================================================================
; PART 1e: Practical Patterns with Lists
; ============================================================================

(display "PART 1e: Practical Patterns")
(newline)
(display "Real-world iteration scenarios")
(newline)
(newline)

(display "Example 1e-1: Working with data")
(newline)
(var data (list 10 20 30 40 50))
(display "Data: ")
(display data)
(newline)
(display "Count: ")
(display (length data))
(newline)
(display "First element: ")
(display (first data))
(newline)
(display "Last element: ")
(display (last data))
(newline)
(display "All but first: ")
(display (rest data))
(newline)
(newline)

(display "Example 1e-2: Processing collections")
(newline)
(var words (list "hello" "world" "elle"))
(display "Words: ")
(display words)
(newline)
(display "Count of words: ")
(display (length words))
(newline)
(display "Reversed order: ")
(display (reverse words))
(newline)
(display "All but first word: ")
(display (rest words))
(newline)
(newline)

; ============================================================================
; PART 1f: Combining List Operations
; ============================================================================

(display "PART 1f: Combining List Operations")
(newline)
(display "Chaining multiple operations")
(newline)
(newline)

(display "Example 1f-1: Multiple transformations")
(newline)
(var original-seq (list 1 2 3 4 5 6 7 8 9 10))
(display "Original: ")
(display original-seq)
(newline)

(var first-5 (take 5 original-seq))
(display "Take first 5: ")
(display first-5)
(newline)

(var reversed-seq (reverse original-seq))
(display "Reverse all: ")
(display reversed-seq)
(newline)

(var dropped-first-2 (drop 2 original-seq))
(display "Drop first 2: ")
(display dropped-first-2)
(newline)

; Part 1f Assertions
(assert-eq (length original-seq) 10 "original-seq length = 10")
(assert-eq (length first-5) 5 "first-5 length = 5")
(assert-eq (length reversed-seq) 10 "reversed-seq length = 10")
(assert-eq (length dropped-first-2) 8 "dropped-first-2 length = 8")
(newline)

(display "Example 1f-2: List slicing")
(newline)
(var full-list (list 10 20 30 40 50 60 70 80 90 100))
(display "Full list: ")
(display full-list)
(newline)

(var middle (drop 2 (take 7 full-list)))
(display "Elements 2-6: ")
(display middle)
(newline)

; Part 1f Assertions
(assert-eq (length full-list) 10 "full-list length = 10")
(assert-eq (length middle) 5 "middle slice length = 5")
(newline)

; ============================================================================
; PART 1g: Nested Lists
; ============================================================================

(display "PART 1g: Working with Nested Lists")
(newline)
(display "Processing nested data structures")
(newline)
(newline)

(display "Example 1g-1: List of lists")
(newline)
(var nested (list (list 1 2 3) (list 4 5 6) (list 7 8 9)))
(display "Nested lists: ")
(display nested)
(newline)

(display "First sublist: ")
(display (first nested))
(newline)

(display "Rest of sublists: ")
(display (rest nested))
(newline)

; Part 1g Assertions
(assert-eq (length nested) 3 "nested list length = 3")
(assert-eq (length (first nested)) 3 "first sublist length = 3")
(newline)

(display "Example 1g-2: Appending nested lists")
(newline)
(var list1 (list (list 1 2) (list 3 4)))
(var list2 (list (list 5 6) (list 7 8)))
(var combined (append list1 list2))
(display "Combined nested: ")
(display combined)
(newline)

; Part 1g Assertions
(assert-eq (length list1) 2 "list1 length = 2")
(assert-eq (length list2) 2 "list2 length = 2")
(assert-eq (length combined) 4 "combined nested length = 4")
(newline)

; ============================================================================
; PART 1h: Selection and Comparison
; ============================================================================

(display "PART 1h: Selection and Comparison")
(newline)
(display "Using list operations to understand data")
(newline)
(newline)

(display "Example 1h-1: Comparing lists")
(newline)
(var list-a (list 1 2 3))
(var list-b (list 1 2 3))
(var list-c (list 4 5 6))
(display "List A: ")
(display list-a)
(newline)
(display "List B: ")
(display list-b)
(newline)
(display "List C: ")
(display list-c)
(newline)
(display "Length of A: ")
(display (length list-a))
(newline)
(display "Length of C: ")
(display (length list-c))
(newline)

; Part 1h Assertions
(assert-eq (length list-a) 3 "list-a length = 3")
(assert-eq (length list-c) 3 "list-c length = 3")
(newline)

(display "Example 1h-2: Extracting subsequences")
(newline)
(var data-seq (list 'a 'b 'c 'd 'e 'f 'g))
(display "Full sequence: ")
(display data-seq)
(newline)
(display "First 3 elements: ")
(display (take 3 data-seq))
(newline)
(display "Last 3 elements: ")
(display (take 3 (reverse data-seq)))
(newline)
(display "Middle elements (skip 2, take 3): ")
(display (take 3 (drop 2 data-seq)))
(newline)

; Part 1h Assertions
(assert-eq (length data-seq) 7 "data-seq length = 7")
(assert-eq (length (take 3 data-seq)) 3 "take 3 length = 3")
(assert-eq (length (take 3 (drop 2 data-seq))) 3 "middle slice length = 3")
(newline)

; ============================================================================
; PART 2: Vector Operations
; ============================================================================

(display "PART 2: Vector Operations")
(newline)
(newline)

; === Vector Creation ===
(display "Part 2a: Vector Creation")
(newline)

; Create a vector with 5 elements
(var my-vector (vector 10 20 30 40 50))
(display "Created vector: ")
(display my-vector)
(newline)
(newline)

; === Vector Length ===
(display "Part 2b: Vector Length")
(newline)

; Get the length of the vector
(display "Vector length: ")
(display (length my-vector))
(newline)
(assert-eq (length my-vector) 5 "length returns correct length for vector")

; Empty vector has length 0
(var empty-vec (vector))
(assert-eq (length empty-vec) 0 "empty vector has length 0")
(newline)

; === Vector Access (vector-ref) ===
(display "Part 2c: Vector Access (vector-ref)")
(newline)

; Access first element (index 0)
(display "Element at index 0: ")
(display (vector-ref my-vector 0))
(newline)
(assert-eq (vector-ref my-vector 0) 10 "vector-ref index 0 returns first element")

; Access middle element
(display "Element at index 2: ")
(display (vector-ref my-vector 2))
(newline)
(assert-eq (vector-ref my-vector 2) 30 "vector-ref index 2 returns middle element")

; Access last element
(display "Element at index 4: ")
(display (vector-ref my-vector 4))
(newline)
(assert-eq (vector-ref my-vector 4) 50 "vector-ref index 4 returns last element")
(newline)

; === Vector Mutation (vector-set!) ===
(display "Part 2d: Vector Mutation (vector-set!)")
(newline)

; Create a mutable vector
(var mutable-vec (vector 1 2 3 4 5))
(display "Original vector: ")
(display mutable-vec)
(newline)

; Modify first element - vector-set! returns a new vector
(var mutable-vec (vector-set! mutable-vec 0 100))
(display "After setting index 0 to 100: ")
(display mutable-vec)
(newline)
(assert-eq (vector-ref mutable-vec 0) 100 "vector-set! returns new vector with modified element")

; Modify middle element
(var mutable-vec (vector-set! mutable-vec 2 300))
(display "After setting index 2 to 300: ")
(display mutable-vec)
(newline)
(assert-eq (vector-ref mutable-vec 2) 300 "vector-set! modifies element at index 2")

; Modify last element
(var mutable-vec (vector-set! mutable-vec 4 500))
(display "After setting index 4 to 500: ")
(display mutable-vec)
(newline)
(assert-eq (vector-ref mutable-vec 4) 500 "vector-set! modifies element at index 4")
(newline)

; === Vectors vs Lists ===
(display "Part 2e: Vectors vs Lists")
(newline)

; Create equivalent list and vector
(var my-list-2 (list 1 2 3 4 5))
(var my-vec-2 (vector 1 2 3 4 5))

(display "List: ")
(display my-list-2)
(newline)
(display "Vector: ")
(display my-vec-2)
(newline)

; Note: vectors and lists are different types
; Vectors are mutable, lists are immutable

; Both have length
(display "List length: ")
(display (length my-list-2))
(newline)
(assert-eq (length my-list-2) 5 "list length works")

(display "Vector length: ")
(display (length my-vec-2))
(newline)
(assert-eq (length my-vec-2) 5 "vector length works")

; Lists use first/rest, vectors use vector-ref
(display "List first element: ")
(display (first my-list-2))
(newline)
(assert-eq (first my-list-2) 1 "list first element")

(display "Vector first element: ")
(display (vector-ref my-vec-2 0))
(newline)
(assert-eq (vector-ref my-vec-2 0) 1 "vector first element")

; Vectors are mutable, lists are immutable
(var test-list (list 10 20 30))
(var test-vec (vector 10 20 30))

; Modify vector - vector-set! returns a new vector
(var test-vec (vector-set! test-vec 1 200))
(assert-eq (vector-ref test-vec 1) 200 "vector mutation works")

; Lists are immutable - cons creates new list
(var modified-list (cons 5 test-list))
(assert-eq (first test-list) 10 "original list unchanged after cons")
(assert-eq (first modified-list) 5 "cons creates new list")
(newline)

; === Vector with Different Types ===
(display "Part 2f: Vector with Different Types")
(newline)

; Create vector with mixed types
(var mixed-vec (vector 42 "hello" 'symbol))
(display "Mixed type vector: ")
(display mixed-vec)
(newline)

(assert-eq (vector-ref mixed-vec 0) 42 "vector stores numbers")
(assert-eq (vector-ref mixed-vec 1) "hello" "vector stores strings")
(assert-eq (vector-ref mixed-vec 2) 'symbol "vector stores symbols")
(newline)

; ============================================================================
; PART 3: Polymorphic Operations
; ============================================================================

(display "PART 3: Polymorphic Operations")
(newline)

; The `length` function works on both lists and vectors
(var list-len (length (list 1 2 3)))
(var vec-len (length (vector 1 2 3)))

(display "List length: ")
(display list-len)
(newline)
(assert-eq list-len 3 "length works on lists")

(display "Vector length: ")
(display vec-len)
(newline)
(assert-eq vec-len 3 "length works on vectors")

(display "✓ Polymorphic length verified")
(newline)
(newline)

; ============================================================================
; Summary
; ============================================================================

(display "=== Summary ===")
(newline)
(display "Lists and Vectors in Elle:")
(newline)
(display "1. List operations - cons, first/rest, length, reverse, take/drop, nth/last, append")
(newline)
(display "2. List construction - Building lists with cons and list function")
(newline)
(display "3. Nested lists - Working with lists of lists")
(newline)
(display "4. Vector creation - Creating vectors with vector function")
(newline)
(display "5. Vector access - Using vector-ref to access elements")
(newline)
(display "6. Vector mutation - Using vector-set! to modify elements")
(newline)
(display "7. Polymorphic operations - length works on both lists and vectors")
(newline)
(newline)

(display "=== Lists and Vectors Complete - All Assertions Passed ===")
(newline)

(exit 0)
