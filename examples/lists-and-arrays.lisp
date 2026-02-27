; Lists and Arrays - Sequence operations and comparisons

(import-file "./examples/assertions.lisp")

(display "=== Lists and Arrays ===")
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

(display "Example 1b-6: Get element by index (0-indexed)")
(newline)
(display "List: (a b c d e)")
(newline)
(display "Element at index 2: ")
(display (get (list 'a 'b 'c 'd 'e) 2))
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
; PART 2: Array Operations
; ============================================================================

(display "PART 2: Array Operations")
(newline)
(newline)

; === Array Creation ===
(display "Part 2a: Array Creation")
(newline)

; Create an array with 5 elements
(var my-array (array 10 20 30 40 50))
(display "Created array: ")
(display my-array)
(newline)
(newline)

; === Array Length ===
(display "Part 2b: Array Length")
(newline)

; Get the length of the array
(display "Array length: ")
(display (length my-array))
(newline)
(assert-eq (length my-array) 5 "length returns correct length for array")

; Empty array has length 0
(var empty-arr (array))
(assert-eq (length empty-arr) 0 "empty array has length 0")
(newline)

; === Array Access (get) ===
(display "Part 2c: Array Access (get)")
(newline)

; Access first element (index 0)
(display "Element at index 0: ")
(display (get my-array 0))
(newline)
(assert-eq (get my-array 0) 10 "get index 0 returns first element")

; Access middle element
(display "Element at index 2: ")
(display (get my-array 2))
(newline)
(assert-eq (get my-array 2) 30 "get index 2 returns middle element")

; Access last element
(display "Element at index 4: ")
(display (get my-array 4))
(newline)
(assert-eq (get my-array 4) 50 "get index 4 returns last element")
(newline)

; === Array Mutation (put) ===
(display "Part 2d: Array Mutation (put)")
(newline)

; Create a mutable array
(var mutable-arr (array 1 2 3 4 5))
(display "Original array: ")
(display mutable-arr)
(newline)

; Modify first element - put returns a new array
(var mutable-arr (put mutable-arr 0 100))
(display "After setting index 0 to 100: ")
(display mutable-arr)
(newline)
(assert-eq (get mutable-arr 0) 100 "put returns new array with modified element")

; Modify middle element
(var mutable-arr (put mutable-arr 2 300))
(display "After setting index 2 to 300: ")
(display mutable-arr)
(newline)
(assert-eq (get mutable-arr 2) 300 "put modifies element at index 2")

; Modify last element
(var mutable-arr (put mutable-arr 4 500))
(display "After setting index 4 to 500: ")
(display mutable-arr)
(newline)
(assert-eq (get mutable-arr 4) 500 "put modifies element at index 4")
(newline)

; === Arrays vs Lists ===
(display "Part 2e: Arrays vs Lists")
(newline)

; Create equivalent list and array
(var my-list-2 (list 1 2 3 4 5))
(var my-arr-2 (array 1 2 3 4 5))

(display "List: ")
(display my-list-2)
(newline)
(display "Array: ")
(display my-arr-2)
(newline)

; Note: arrays and lists are different types
; Arrays are mutable, lists are immutable

; Both have length
(display "List length: ")
(display (length my-list-2))
(newline)
(assert-eq (length my-list-2) 5 "list length works")

(display "Array length: ")
(display (length my-arr-2))
(newline)
(assert-eq (length my-arr-2) 5 "array length works")

; Lists use first/rest, arrays use get
(display "List first element: ")
(display (first my-list-2))
(newline)
(assert-eq (first my-list-2) 1 "list first element")

(display "Array first element: ")
(display (get my-arr-2 0))
(newline)
(assert-eq (get my-arr-2 0) 1 "array first element")

; Arrays are mutable, lists are immutable
(var test-list (list 10 20 30))
(var test-arr (array 10 20 30))

; Modify array - put returns a new array
(var test-arr (put test-arr 1 200))
(assert-eq (get test-arr 1) 200 "array mutation works")

; Lists are immutable - cons creates new list
(var modified-list (cons 5 test-list))
(assert-eq (first test-list) 10 "original list unchanged after cons")
(assert-eq (first modified-list) 5 "cons creates new list")
(newline)

; === Array with Different Types ===
(display "Part 2f: Array with Different Types")
(newline)

; Create array with mixed types
(var mixed-arr (array 42 "hello" 'symbol))
(display "Mixed type array: ")
(display mixed-arr)
(newline)

(assert-eq (get mixed-arr 0) 42 "array stores numbers")
(assert-eq (get mixed-arr 1) "hello" "array stores strings")
(assert-eq (get mixed-arr 2) 'symbol "array stores symbols")
(newline)

; ============================================================================
; PART 3: Polymorphic Operations
; ============================================================================

(display "PART 3: Polymorphic Operations")
(newline)

; The `length` function works on both lists and arrays
(var list-len (length (list 1 2 3)))
(var arr-len (length (array 1 2 3)))

(display "List length: ")
(display list-len)
(newline)
(assert-eq list-len 3 "length works on lists")

(display "Array length: ")
(display arr-len)
(newline)
(assert-eq arr-len 3 "length works on arrays")

(display "✓ Polymorphic length verified")
(newline)
(newline)

; ============================================================================
; Summary
; ============================================================================

(display "=== Summary ===")
(newline)
(display "Lists and Arrays in Elle:")
(newline)
(display "1. List operations - cons, first/rest, length, reverse, take/drop, get/last, append")
(newline)
(display "2. List construction - Building lists with cons and list function")
(newline)
(display "3. Nested lists - Working with lists of lists")
(newline)
(display "4. Array creation - Creating arrays with array function")
(newline)
(display "5. Array access - Using get to access elements")
(newline)
(display "6. Array mutation - Using put to modify elements")
(newline)
(display "7. Polymorphic operations - length works on both lists and arrays")
(newline)
(newline)

(display "=== Lists and Arrays Complete - All Assertions Passed ===")
(newline)

(exit 0)
