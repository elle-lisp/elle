; Struct and Table Sugar Syntax Example
; {:key value} creates a struct (immutable)
; @{:key value} creates a table (mutable)

(display "=== Testing Struct Sugar Syntax ===")
(newline)

; Basic struct with integer keys
(define data1 {1 "one" 2 "two" 3 "three"})
(display "Created struct with sugar: ")
(display data1)
(newline)

; Access struct values
(display "Get value for key 1: ")
(display (struct-get data1 1))
(newline)

(display "Get value for key 2: ")
(display (struct-get data1 2))
(newline)

; Check struct has key
(display "Struct has key 2? ")
(display (struct-has? data1 2))
(newline)

(display "Struct has key 10? ")
(display (struct-has? data1 10))
(newline)

; Get struct length
(display "Struct length: ")
(display (struct-length data1))
(newline)

; Empty struct
(define empty-struct {})
(display (newline))
(display "Empty struct: ")
(display empty-struct)
(newline)

(display "Empty struct type: ")
(display (type empty-struct))
(newline)

(display "Empty struct length: ")
(display (struct-length empty-struct))
(newline)

; Struct with mixed value types
(define mixed {1 42 2 3.14 3 "text" 4 #t})
(display (newline))
(display "Struct with mixed types: ")
(display mixed)
(newline)

(display "Value for key 1 (int): ")
(display (struct-get mixed 1))
(newline)

(display "Value for key 3 (string): ")
(display (struct-get mixed 3))
(newline)

; ========== TABLE SUGAR SYNTAX ==========
(display (newline))
(display "=== Testing Table Sugar Syntax ===")
(newline)

; Basic table with integer keys
(define table1 @{1 "first" 2 "second" 3 "third"})
(display "Created table with sugar: ")
(display table1)
(newline)

(display "Get value for key 1: ")
(display (get table1 1))
(newline)

(display "Get value for key 2: ")
(display (get table1 2))
(newline)

; Check table has key
(display "Table has key 3? ")
(display (has? table1 3))
(newline)

(display "Table has key 10? ")
(display (has? table1 10))
(newline)

; Get table length
(display "Table length: ")
(display (table-length table1))
(newline)

; Empty table
(define empty-table @{})
(display (newline))
(display "Empty table: ")
(display empty-table)
(newline)

(display "Empty table type: ")
(display (type empty-table))
(newline)

(display "Empty table length: ")
(display (table-length empty-table))
(newline)

; Table mutability
(display (newline))
(display "Table mutability test:")
(newline)
(put table1 10 "new-key")
(display "After put with key 10, value: ")
(display (get table1 10))
(newline)

; ========== COMPARISON ==========
(display (newline))
(display "=== Struct vs Table ===")
(newline)

(define s {100 "struct"})
(define t @{100 "table"})

(display "Struct type: ")
(display (type s))
(newline)

(display "Table type: ")
(display (type t))
(newline)

(display "Struct value: ")
(display (struct-get s 100))
(newline)

(display "Table value: ")
(display (get t 100))
(newline)

; ========== EQUIVALENCE ==========
(display (newline))
(display "=== Sugar vs Explicit Calls ===")
(newline)

; These should be equivalent
(define s1 {1 "a" 2 "b"})
(define s2 (struct 1 "a" 2 "b"))

(display "Struct from sugar: ")
(display s1)
(newline)

(display "Struct from explicit: ")
(display s2)
(newline)

(display "Are they equal? ")
(display (= s1 s2))
(newline)

; Same for tables
(define t1 @{1 "a" 2 "b"})
(define t2 (table 1 "a" 2 "b"))

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

; ========== NESTING ==========
(display (newline))
(display "=== Nested Structures ===")
(newline)

; Nested struct with list values
(define nested {1 (list 10 20 30) 2 (list "a" "b" "c")})
(display "Struct with list values: ")
(display nested)
(newline)

(display "First list value: ")
(display (struct-get nested 1))
(newline)

; Table with nested table
(define outer @{1 @{10 "inner"}})
(display (newline))
(display "Table with nested table: ")
(display outer)
(newline)

(display "Get outer table key 1: ")
(display (get outer 1))
(newline)

; All tests completed successfully
(display (newline))
(display "=== All Struct and Table Sugar Tests Completed Successfully ===")
(newline)
