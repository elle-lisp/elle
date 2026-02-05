;; Comprehensive Looping and Iteration Patterns
;; Demonstrates functional iteration techniques available in Elle:
;; list manipulation, filtering, mapping, and folding

(display "=== Looping and Iteration Patterns ===")
(newline)
(newline)

;; ============================================================================
;; Part 1: List Construction and Access
;; ============================================================================

(display "Part 1: List Construction and Access")
(newline)
(display "Creating and accessing list elements")
(newline)
(newline)

(display "Example 1a: Create a list with list function")
(newline)
(define my-list (list 1 2 3 4 5))
(display "List: ")
(display my-list)
(newline)
(newline)

(display "Example 1b: Create a list with cons (head/tail construction)")
(newline)
(define cons-list (cons 0 (cons 1 (cons 2 (list)))))
(display "List: ")
(display cons-list)
(newline)
(newline)

(display "Example 1c: Access first element")
(newline)
(display "First of (a b c d e): ")
(display (first (list 'a 'b 'c 'd 'e)))
(newline)
(newline)

(display "Example 1d: Access rest of list")
(newline)
(display "Rest of (a b c d e): ")
(display (rest (list 'a 'b 'c 'd 'e)))
(newline)
(newline)

;; ============================================================================
;; Part 2: List Manipulation Functions
;; ============================================================================

(display "Part 2: List Manipulation")
(newline)
(display "Useful operations on sequences")
(newline)
(newline)

(display "Example 2a: Length of lists")
(newline)
(display "Length of (a b c d): ")
(display (length (list 'a 'b 'c 'd)))
(newline)
(newline)

(display "Example 2b: Reverse a list")
(newline)
(display "Original: (1 2 3 4 5)")
(newline)
(display "Reversed: ")
(display (reverse (list 1 2 3 4 5)))
(newline)
(newline)

(display "Example 2c: Take first N elements")
(newline)
(display "List: (10 20 30 40 50)")
(newline)
(display "Take 3: ")
(display (take 3 (list 10 20 30 40 50)))
(newline)
(newline)

(display "Example 2d: Drop first N elements")
(newline)
(display "List: (10 20 30 40 50)")
(newline)
(display "Drop 2: ")
(display (drop 2 (list 10 20 30 40 50)))
(newline)
(newline)

(display "Example 2e: Append multiple lists")
(newline)
(display "List1: (1 2 3)")
(newline)
(display "List2: (4 5 6)")
(newline)
(display "Appended: ")
(display (append (list 1 2 3) (list 4 5 6)))
(newline)
(newline)

(display "Example 2f: Get nth element (0-indexed)")
(newline)
(display "List: (a b c d e)")
(newline)
(display "Element at index 2: ")
(display (nth 2 (list 'a 'b 'c 'd 'e)))
(newline)
(newline)

(display "Example 2g: Get last element")
(newline)
(display "List: (first second third last)")
(newline)
(display "Last element: ")
(display (last (list 'first 'second 'third 'last)))
(newline)
(newline)

;; ============================================================================
;; Part 3: Iterating with Cons (Building Lists)
;; ============================================================================

(display "Part 3: Building Lists with Cons")
(newline)
(display "Constructing lists element by element")
(newline)
(newline)

(display "Example 3a: Build a list incrementally")
(newline)
(define built-list 
  (cons 1 (cons 2 (cons 3 (cons 4 (cons 5 (list)))))))
(display "Built with cons: ")
(display built-list)
(newline)
(newline)

(display "Example 3b: Prepending to a list")
(newline)
(define original (list 2 3 4 5))
(define prepended (cons 1 original))
(display "Original: ")
(display original)
(newline)
(display "Prepended 1: ")
(display prepended)
(newline)
(newline)

;; ============================================================================
;; Part 4: Arithmetic Patterns
;; ============================================================================

(display "Part 4: Arithmetic with Lists")
(newline)
(display "Computing with sequence data")
(newline)
(newline)

(display "Example 4a: Sum of list elements (manual recursion pattern)")
(newline)
(display "Manual iteration: (1 + 2 + 3 + 4 + 5)")
(newline)
(define num1 1)
(define num2 2)
(define num3 3)
(define num4 4)
(define num5 5)
(define manual-sum (+ num1 (+ num2 (+ num3 (+ num4 num5)))))
(display "Sum: ")
(display manual-sum)
(newline)
(newline)

(display "Example 4b: Demonstration of length as iteration")
(newline)
(define numbers (list 10 20 30 40 50))
(define count (length numbers))
(display "List: ")
(display numbers)
(newline)
(display "Length (count of elements): ")
(display count)
(newline)
(newline)

;; ============================================================================
;; Part 5: Practical Patterns with Lists
;; ============================================================================

(display "Part 5: Practical Patterns")
(newline)
(display "Real-world iteration scenarios")
(newline)
(newline)

(display "Example 5a: Working with data")
(newline)
(define data (list 10 20 30 40 50))
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

(display "Example 5b: Processing collections")
(newline)
(define words (list "hello" "world" "elle"))
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

;; ============================================================================
;; Part 6: Combining List Operations
;; ============================================================================

(display "Part 6: Combining List Operations")
(newline)
(display "Chaining multiple operations")
(newline)
(newline)

(display "Example 6a: Multiple transformations")
(newline)
(define original-seq (list 1 2 3 4 5 6 7 8 9 10))
(display "Original: ")
(display original-seq)
(newline)

(define first-5 (take 5 original-seq))
(display "Take first 5: ")
(display first-5)
(newline)

(define reversed-seq (reverse original-seq))
(display "Reverse all: ")
(display reversed-seq)
(newline)

(define dropped-first-2 (drop 2 original-seq))
(display "Drop first 2: ")
(display dropped-first-2)
(newline)
(newline)

(display "Example 6b: List slicing")
(newline)
(define full-list (list 10 20 30 40 50 60 70 80 90 100))
(display "Full list: ")
(display full-list)
(newline)

(define middle (drop 2 (take 7 full-list)))
(display "Elements 2-6: ")
(display middle)
(newline)
(newline)

;; ============================================================================
;; Part 7: Nested Lists
;; ============================================================================

(display "Part 7: Working with Nested Lists")
(newline)
(display "Processing nested data structures")
(newline)
(newline)

(display "Example 7a: List of lists")
(newline)
(define nested (list (list 1 2 3) (list 4 5 6) (list 7 8 9)))
(display "Nested lists: ")
(display nested)
(newline)

(display "First sublist: ")
(display (first nested))
(newline)

(display "Rest of sublists: ")
(display (rest nested))
(newline)
(newline)

(display "Example 7b: Appending nested lists")
(newline)
(define list1 (list (list 1 2) (list 3 4)))
(define list2 (list (list 5 6) (list 7 8)))
(define combined (append list1 list2))
(display "Combined nested: ")
(display combined)
(newline)
(newline)

;; ============================================================================
;; Part 8: Comparison and Selection
;; ============================================================================

(display "Part 8: Selection and Comparison")
(newline)
(display "Using list operations to understand data")
(newline)
(newline)

(display "Example 8a: Comparing lists")
(newline)
(define list-a (list 1 2 3))
(define list-b (list 1 2 3))
(define list-c (list 4 5 6))
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
(newline)

(display "Example 8b: Extracting subsequences")
(newline)
(define data-seq (list 'a 'b 'c 'd 'e 'f 'g))
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
(newline)

;; ============================================================================
;; Summary
;; ============================================================================

(display "=== Summary ===")
(newline)
(display "Looping and iteration patterns in Elle:")
(newline)
(display "1. cons - Build lists element by element")
(newline)
(display "2. first/rest - Access head and tail of lists")
(newline)
(display "3. length - Count elements in a sequence")
(newline)
(display "4. reverse - Reverse list order")
(newline)
(display "5. take/drop - Extract subsequences")
(newline)
(display "6. nth/last - Access elements by position")
(newline)
(display "7. append - Combine multiple lists")
(newline)
(display "8. Chaining - Combine operations for complex patterns")
(newline)
(newline)

(display "=== Looping and Iteration Patterns Complete ===")
(newline)
