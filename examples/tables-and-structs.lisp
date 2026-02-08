; Tables and Structs Example
; Demonstrates mutable tables and immutable structs with basic operations

(display "=== Testing Tables (Mutable Hash Maps) ===")
(newline)

; Test 1: Create and type-check empty table
(let ((t (table)))
  (display "Created empty table, type is: ")
  (display (type t))
  (newline))

; Test 2: Create table with numeric keys
(let ((t (table 1 "value1" 2 "value2")))
  (display "Created table with integer keys, length: ")
  (display (table-length t))
  (newline))

; Test 3: Get values from table
(let ((t (table 42 "answer")))
  (display "Retrieved value from table: ")
  (display (get t 42))
  (newline))

; Test 4: Get with default
(let ((t (table 1 "exists")))
  (display "Non-existent key with default: ")
  (display (get t 999 "not-found"))
  (newline))

; Test 5: Has? predicate
(let ((t (table 5 "five")))
  (display "Table has key 5? ")
  (display (has-key? t 5))
  (newline)
  (display "Table has key 10? ")
  (display (has-key? t 10))
  (newline))

; Test 6: Put and verify
(let ((t (table)))
  (put t 100 "hundred")
  (display "After put, has key? ")
  (display (has-key? t 100))
  (newline))

; Test 7: Del and verify
(let ((t (table 7 "seven" 8 "eight")))
  (del t 7)
  (display "After delete, has key 7? ")
  (display (has-key? t 7))
  (newline))

; Test 8: Table inspection
(let ((t (table 1 "one" 2 "two" 3 "three")))
  (display "Table length: ")
  (display (table-length t))
  (newline))

(newline)
(display "=== Testing Structs (Immutable Hash Maps) ===")
(newline)

; Test 9: Create and type-check empty struct
(let ((s (struct)))
  (display "Created empty struct, type is: ")
  (display (type s))
  (newline))

; Test 10: Create struct with numeric keys
(let ((s (struct 10 "ten" 20 "twenty")))
  (display "Created struct with integer keys, length: ")
  (display (struct-length s))
  (newline))

; Test 11: Get values from struct
(let ((s (struct 99 "ninetymine")))
  (display "Retrieved value from struct: ")
  (display (struct-get s 99))
  (newline))

; Test 12: Get with default
(let ((s (struct 3 "three")))
  (display "Non-existent key with default: ")
  (display (struct-get s 777 "default-val"))
  (newline))

; Test 13: Struct-has? predicate
(let ((s (struct 2 "two")))
  (display "Struct has key 2? ")
  (display (struct-has? s 2))
  (newline)
  (display "Struct has key 3? ")
  (display (struct-has? s 3))
  (newline))

; Test 14: Struct inspection
(let ((s (struct 11 "eleven" 22 "twentytwo")))
  (display "Struct length: ")
  (display (struct-length s))
  (newline))

(display (newline))
(display "=== All Tests Completed Successfully ===")
(newline)
