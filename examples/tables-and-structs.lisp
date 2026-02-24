; Tables and Structs Example

(import-file "./examples/assertions.lisp")

(display "=== Testing Tables (Mutable Hash Maps) ===")
(newline)

; Test 1: Create and type-check empty table
(let ((t (table)))
  (display "Created empty table, type is: ")
  (display (type-of t))
  (newline)
  (assert-true (= (type-of t) (type-of (table))) "Empty table has correct type"))

; Test 2: Create table with numeric keys
(let ((t (table 1 "value1" 2 "value2")))
  (display "Created table with integer keys, length: ")
  (display (length t))
  (newline)
  (assert-eq (length t) 2 "Table with 2 keys has length 2"))

; Test 3: Get values from table
(let ((t (table 42 "answer")))
  (display "Retrieved value from table: ")
  (display (get t 42))
  (newline)
  (assert-eq (get t 42) "answer" "Get returns correct value"))

; Test 4: Get with default
(let ((t (table 1 "exists")))
  (display "Non-existent key with default: ")
  (display (get t 999 "not-found"))
  (newline)
  (assert-eq (get t 999 "not-found") "not-found" "Get with default returns default for missing key"))

; Test 5: Has? predicate
(let ((t (table 5 "five")))
  (display "Table has key 5? ")
  (display (has-key? t 5))
  (newline)
  (assert-true (has-key? t 5) "has-key? returns true for existing key")
  (display "Table has key 10? ")
  (display (has-key? t 10))
  (newline)
  (assert-false (has-key? t 10) "has-key? returns false for missing key"))

; Test 6: Put and verify
(let ((t (table)))
  (put t 100 "hundred")
  (display "After put, has key? ")
  (display (has-key? t 100))
  (newline)
  (assert-true (has-key? t 100) "put adds key to table"))

; Test 7: Del and verify
(let ((t (table 7 "seven" 8 "eight")))
  (del t 7)
  (display "After delete, has key 7? ")
  (display (has-key? t 7))
  (newline)
  (assert-false (has-key? t 7) "del removes key from table"))

; Test 8: Table inspection
(let ((t (table 1 "one" 2 "two" 3 "three")))
  (display "Table length: ")
  (display (length t))
  (newline)
  (assert-eq (length t) 3 "Table with 3 keys has length 3"))

(newline)
(display "=== Testing Structs (Immutable Hash Maps) ===")
(newline)

; Test 9: Create and type-check empty struct
(let ((s (struct)))
  (display "Created empty struct, type is: ")
  (display (type-of s))
  (newline)
  (assert-true (= (type-of s) (type-of (struct))) "Empty struct has correct type"))

; Test 10: Create struct with numeric keys
(let ((s (struct 10 "ten" 20 "twenty")))
  (display "Created struct with integer keys, length: ")
  (display (length s))
  (newline)
  (assert-eq (length s) 2 "Struct with 2 keys has length 2"))

; Test 11: Get values from struct
(let ((s (struct 99 "ninetymine")))
  (display "Retrieved value from struct: ")
  (display (get s 99))
  (newline)
  (assert-eq (get s 99) "ninetymine" "get returns correct value"))

; Test 12: Get with default
(let ((s (struct 3 "three")))
  (display "Non-existent key with default: ")
  (display (get s 777 "default-val"))
  (newline)
  (assert-eq (get s 777 "default-val") "default-val" "get with default returns default for missing key"))

; Test 13: has-key? predicate
(let ((s (struct 2 "two")))
  (display "Struct has key 2? ")
  (display (has-key? s 2))
  (newline)
  (assert-true (has-key? s 2) "has-key? returns true for existing key")
  (display "Struct has key 3? ")
  (display (has-key? s 3))
  (newline)
  (assert-false (has-key? s 3) "has-key? returns false for missing key"))

; Test 14: Struct inspection
(let ((s (struct 11 "eleven" 22 "twentytwo")))
  (display "Struct length: ")
  (display (length s))
  (newline)
  (assert-eq (length s) 2 "Struct with 2 keys has length 2"))

(display (newline))
(display "=== Testing Struct Sugar Syntax ===")
(newline)

; Test 15: Basic struct with integer keys
(var data1 {1 "one" 2 "two" 3 "three"})
(display "Created struct with sugar: ")
(display data1)
(newline)
(assert-eq (length data1) 3 "Struct sugar creates struct with 3 keys")

; Test 16: Access struct values
(display "Get value for key 1: ")
(display (get data1 1))
(newline)
(assert-eq (get data1 1) "one" "Struct sugar value for key 1 is 'one'")

(display "Get value for key 2: ")
(display (get data1 2))
(newline)
(assert-eq (get data1 2) "two" "Struct sugar value for key 2 is 'two'")

; Test 17: Check struct has key
(display "Struct has key 2? ")
(display (has-key? data1 2))
(newline)
(assert-true (has-key? data1 2) "Struct sugar has key 2")

(display "Struct has key 10? ")
(display (has-key? data1 10))
(newline)
(assert-false (has-key? data1 10) "Struct sugar doesn't have key 10")

; Test 18: Get struct length
(display "Struct length: ")
(display (length data1))
(newline)
(assert-eq (length data1) 3 "Struct sugar length is 3")

; Test 19: Empty struct
(var empty-struct {})
(display (newline))
(display "Empty struct: ")
(display empty-struct)
(newline)
(assert-eq (length empty-struct) 0 "Empty struct has length 0")

(display "Empty struct type: ")
(display (type-of empty-struct))
(newline)
(assert-true (= (type-of empty-struct) (type-of (struct))) "Empty struct has correct type")

(display "Empty struct length: ")
(display (length empty-struct))
(newline)
(assert-eq (length empty-struct) 0 "Empty struct length is 0")

; Test 20: Struct with mixed value types
(var mixed {1 42 2 3.14 3 "text" 4 #t})
(display (newline))
(display "Struct with mixed types: ")
(display mixed)
(newline)
(assert-eq (length mixed) 4 "Mixed struct has 4 keys")

(display "Value for key 1 (int): ")
(display (get mixed 1))
(newline)
(assert-eq (get mixed 1) 42 "Mixed struct value for key 1 is 42")

(display "Value for key 3 (string): ")
(display (get mixed 3))
(newline)
(assert-eq (get mixed 3) "text" "Mixed struct value for key 3 is 'text'")

; ========== TABLE SUGAR SYNTAX ==========
(display (newline))
(display "=== Testing Table Sugar Syntax ===")
(newline)

; Test 21: Basic table with integer keys
(var table1 @{1 "first" 2 "second" 3 "third"})
(display "Created table with sugar: ")
(display table1)
(newline)
(assert-eq (length table1) 3 "Table sugar creates table with 3 keys")

(display "Get value for key 1: ")
(display (get table1 1))
(newline)
(assert-eq (get table1 1) "first" "Table sugar value for key 1 is 'first'")

(display "Get value for key 2: ")
(display (get table1 2))
(newline)
(assert-eq (get table1 2) "second" "Table sugar value for key 2 is 'second'")

; Test 22: Check table has key
(display "Table has key 3? ")
(display (has-key? table1 3))
(newline)
(assert-true (has-key? table1 3) "Table sugar has key 3")

(display "Table has key 10? ")
(display (has-key? table1 10))
(newline)
(assert-false (has-key? table1 10) "Table sugar doesn't have key 10")

; Test 23: Get table length
(display "Table length: ")
(display (length table1))
(newline)
(assert-eq (length table1) 3 "Table sugar length is 3")

; Test 24: Empty table
(var empty-table @{})
(display (newline))
(display "Empty table: ")
(display empty-table)
(newline)
(assert-eq (length empty-table) 0 "Empty table has length 0")

(display "Empty table type: ")
(display (type-of empty-table))
(newline)
(assert-true (= (type-of empty-table) (type-of (table))) "Empty table has correct type")

(display "Empty table length: ")
(display (length empty-table))
(newline)
(assert-eq (length empty-table) 0 "Empty table length is 0")

; Test 25: Table mutability
(display (newline))
(display "Table mutability test:")
(newline)
(put table1 10 "new-key")
(display "After put with key 10, value: ")
(display (get table1 10))
(newline)
(assert-eq (get table1 10) "new-key" "Table put adds new key")
(assert-eq (length table1) 4 "Table length increased to 4")

; ========== COMPARISON ==========
(display (newline))
(display "=== Struct vs Table ===")
(newline)

(var s {100 "struct"})
(var t @{100 "table"})

(display "Struct type: ")
(display (type-of s))
(newline)
(assert-true (= (type-of s) (type-of (struct))) "Struct sugar has struct type")

(display "Table type: ")
(display (type-of t))
(newline)
(assert-true (= (type-of t) (type-of (table))) "Table sugar has table type")

(display "Struct value: ")
(display (get s 100))
(newline)
(assert-eq (get s 100) "struct" "Struct sugar value is 'struct'")

(display "Table value: ")
(display (get t 100))
(newline)
(assert-eq (get t 100) "table" "Table sugar value is 'table'")

; ========== EQUIVALENCE ==========
(display (newline))
(display "=== Sugar vs Explicit Calls ===")
(newline)

; Test 26: These should be equivalent
(var s1 {1 "a" 2 "b"})
(var s2 (struct 1 "a" 2 "b"))

(display "Struct from sugar: ")
(display s1)
(newline)

(display "Struct from explicit: ")
(display s2)
(newline)

(display "Are they equal? ")
(display (= s1 s2))
(newline)
(assert-true (= s1 s2) "Struct sugar equals explicit struct call")

; Test 27: Same for tables
(var t1 @{1 "a" 2 "b"})
(var t2 (table 1 "a" 2 "b"))

(display (newline))
(display "Table from sugar: ")
(display t1)
(newline)

(display "Table from explicit: ")
(display t2)
(newline)

(display "Are they equal? ")
(display (= t1 t2))
(newline)
(assert-true (and (= (get t1 1) (get t2 1)) (= (get t1 2) (get t2 2))) "Table sugar and explicit table have same contents")

; ========== NESTING ==========
(display (newline))
(display "=== Nested Structures ===")
(newline)

; Test 28: Nested struct with list values
(var nested {1 (list 10 20 30) 2 (list "a" "b" "c")})
(display "Struct with list values: ")
(display nested)
(newline)
(assert-eq (length nested) 2 "Nested struct has 2 keys")

(display "First list value: ")
(display (get nested 1))
(newline)
(var first-list (get nested 1))
(assert-eq (length first-list) 3 "First list has 3 elements")
(assert-eq (nth 0 first-list) 10 "First list first element is 10")

; Test 29: Table with nested table
(var outer @{1 @{10 "inner"}})
(display (newline))
(display "Table with nested table: ")
(display outer)
(newline)
(assert-eq (length outer) 1 "Outer table has 1 key")

(display "Get outer table key 1: ")
(display (get outer 1))
(newline)
(var inner-table (get outer 1))
(assert-true (= (type-of inner-table) (type-of (table))) "Nested value is a table")
(assert-eq (get inner-table 10) "inner" "Inner table value is 'inner'")

; All tests completed successfully
(display (newline))
(display "=== All Tables and Structs Tests Completed Successfully ===")
(newline)

;; NOTE: The `length` function is polymorphic and works on all sequence types
;; (lists, strings, arrays, tables, structs, keywords). See list-operations.lisp,
;; string-operations.lisp, and array-operations.lisp for examples with other types.
