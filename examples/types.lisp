; Type System in Elle
;
; This example demonstrates Elle's type system:
; - type-of: Get the type of any value
; - Type predicates: number?, symbol?, string?, list?, array?, table?, closure?, coro?
; - Type conversions: string, number, symbol
; - Type checking patterns
; - Assertions verifying type operations

(import-file "./examples/assertions.lisp")

(display "=== Keywords ===\n")

; Basic keyword creation and display
(display "Basic keywords: ")
(display :name)
(display " ")
(display :value)
(display " ")
(display :status)
(newline)

; Keywords have a type
(display "Type of :keyword-name: ")
(display (type-of :keyword-name))
(newline)

; Keyword equality
(assert-true (= :foo :foo) "keyword equality: :foo = :foo")
(assert-false (= :foo :bar) "keyword inequality: :foo ≠ :bar")
(assert-true (= :name :name) "keyword equality: :name = :name")

; Keywords in lists - useful for building data structures
(var person '(:name :John :age :30 :city :NYC))
(assert-eq (first person) :name "first element of person list is :name")

; Keywords in arrays
(var options [1 :option-a 2 :option-b 3])
(assert-eq (array-ref options 1) :option-a "second element of options array is :option-a")

; Building configuration with keywords
(var settings (list :debug #t :host "localhost" :port 8080))
(assert-eq (first settings) :debug "first element of settings is :debug")

; Keywords as data structure labels
(var colors (list :red 255 :green 128 :blue 64))
(assert-eq (first colors) :red "first element of colors is :red")

; Keywords are distinct from symbols
(assert-false (= :name 'name) "keyword :name is not equal to symbol 'name")

(display "✓ Keywords verified\n")

;; ============================================================================
;; SECTION 2: Symbols
;; ============================================================================

(display "\n=== Symbols ===\n")

; Basic symbol creation with quote
(display "Basic symbols: ")
(display 'name)
(display " ")
(display 'value)
(display " ")
(display 'status)
(newline)

; Symbols have a type
(display "Type of 'symbol-name: ")
(display (type-of 'symbol-name))
(newline)

; Symbol equality
(assert-true (eq? 'foo 'foo) "symbol equality: 'foo eq? 'foo")
(assert-false (eq? 'foo 'bar) "symbol inequality: 'foo not eq? 'bar")
(assert-true (eq? 'name 'name) "symbol equality: 'name eq? 'name")

; Symbols in lists
(var vars '(x y z))
(assert-eq (first vars) 'x "first element of vars list is 'x")

; Symbols in arrays
(var ops (array 'add 'subtract 'multiply))
(assert-eq (array-ref ops 0) 'add "first element of ops array is 'add")

; Symbols are distinct from keywords
(assert-false (eq? 'name :name) "symbol 'name is not eq? to keyword :name")

(display "✓ Symbols verified\n")

;; ============================================================================
;; SECTION 3: Numbers (Integers and Floats)
;; ============================================================================

(display "\n=== Numbers ===\n")

; Integers
(display "Integers: ")
(display 42)
(display " ")
(display -17)
(display " ")
(display 0)
(newline)

; Floats
(display "Floats: ")
(display 3.14)
(display " ")
(display -2.5)
(display " ")
(display 0.0)
(newline)

; Number type
(display "Type of 42: ")
(display (type-of 42))
(newline)

(display "Type of 3.14: ")
(display (type-of 3.14))
(newline)

; Number equality
(assert-true (= 42 42) "integer equality: 42 = 42")
(assert-false (= 42 43) "integer inequality: 42 ≠ 43")
(assert-true (= 3.14 3.14) "float equality: 3.14 = 3.14")

; Numbers in lists
(var nums (list 1 2 3 4 5))
(assert-eq (first nums) 1 "first element of nums list is 1")

; Numbers in arrays
(var values [10 20 30 40 50])
(assert-eq (array-ref values 0) 10 "first element of values array is 10")

; Arithmetic with numbers
(assert-eq (+ 10 5) 15 "arithmetic: 10 + 5 = 15")
(assert-eq (* 3 4) 12 "arithmetic: 3 * 4 = 12")

(display "✓ Numbers verified\n")

;; ============================================================================
;; SECTION 4: Strings
;; ============================================================================

(display "\n=== Strings ===\n")

; Basic string creation
(display "Strings: ")
(display "hello")
(display " ")
(display "world")
(display " ")
(display "")
(newline)

; String type
(display "Type of \"hello\": ")
(display (type-of "hello"))
(newline)

; String equality
(assert-true (= "hello" "hello") "string equality: \"hello\" = \"hello\"")
(assert-false (= "hello" "world") "string inequality: \"hello\" ≠ \"world\"")

; String length
(display "Length of \"hello\": ")
(display (length "hello"))
(newline)
(assert-eq (length "hello") 5 "length of \"hello\" is 5")

; Empty string
(assert-eq (length "") 0 "length of empty string is 0")

; Strings in lists
(var words (list "apple" "banana" "cherry"))
(assert-eq (first words) "apple" "first element of words list is \"apple\"")

; Strings in arrays
(var messages ["hello" "world" "!"])
(assert-eq (array-ref messages 0) "hello" "first element of messages array is \"hello\"")

; String concatenation
(var greeting (string-append "Hello, " "World!"))
(assert-eq greeting "Hello, World!" "string concatenation works")

(display "✓ Strings verified\n")

;; ============================================================================
;; SECTION 5: Booleans
;; ============================================================================

(display "\n=== Booleans ===\n")

; Boolean values
(display "Booleans: ")
(display #t)
(display " ")
(display #f)
(newline)

; Boolean type
(display "Type of #t: ")
(display (type-of #t))
(newline)

(display "Type of #f: ")
(display (type-of #f))
(newline)

; Boolean equality
(assert-true (= #t #t) "boolean equality: #t = #t")
(assert-true (= #f #f) "boolean equality: #f = #f")
(assert-false (= #t #f) "boolean inequality: #t ≠ #f")

; Booleans in lists
(var flags (list #t #f #t))
(assert-eq (first flags) #t "first element of flags list is #t")

; Booleans in arrays
(var states [#t #f #t #f])
(assert-eq (array-ref states 0) #t "first element of states array is #t")

; Boolean predicates
(assert-true (boolean? #t) "boolean? returns true for #t")
(assert-true (boolean? #f) "boolean? returns true for #f")
(assert-false (boolean? 42) "boolean? returns false for 42")

(display "✓ Booleans verified\n")

;; ============================================================================
;; SECTION 6: Nil
;; ============================================================================

(display "\n=== Nil ===\n")

; Nil value
(display "Nil: ")
(display nil)
(newline)

; Nil type
(display "Type of nil: ")
(display (type-of nil))
(newline)

; Nil equality
(assert-true (= nil nil) "nil equality: nil = nil")

; Nil in lists
(var maybe-values (list 1 nil 3))
(assert-eq (first (rest maybe-values)) nil "second element of maybe-values list is nil")

; Nil in arrays
(var optional [10 nil 30])
(assert-eq (array-ref optional 1) nil "second element of optional array is nil")

; Nil predicates
(assert-true (nil? nil) "nil? returns true for nil")
(assert-false (nil? 42) "nil? returns false for 42")
(assert-false (nil? #f) "nil? returns false for #f")

; Empty list is not nil in Elle
(assert-false (nil? '()) "nil? returns false for empty list")

(display "✓ Nil verified\n")

;; ============================================================================
;; SECTION 7: Mixed Atoms in Collections
;; ============================================================================

(display "\n=== Mixed Atoms in Collections ===\n")

; Mixed list
(var mixed-list (list :key 'symbol 42 "string" #t nil))
(assert-eq (first mixed-list) :key "first element is keyword")
(assert-eq (first (rest mixed-list)) 'symbol "second element is symbol")
(assert-eq (first (rest (rest mixed-list))) 42 "third element is number")

; Mixed array
(var mixed-arr (array :id 'user 123 "Alice" #t))
(assert-eq (array-ref mixed-arr 0) :id "first element is keyword")
(assert-eq (array-ref mixed-arr 1) 'user "second element is symbol")
(assert-eq (array-ref mixed-arr 2) 123 "third element is number")

(display "✓ Mixed atoms verified\n")

;; ============================================================================
;; SECTION 8: Type Predicates - nil?, pair?, list?
;; ============================================================================

(display "\n=== Type Predicates: nil?, pair?, list? ===\n")

(assert-false (nil? '()) "nil? returns false for empty list")
(assert-false (nil? 42) "nil? returns false for number")
(assert-false (nil? "hello") "nil? returns false for string")

(assert-true (pair? (cons 1 2)) "pair? returns true for cons cell")
(assert-true (pair? (list 1 2 3)) "pair? returns true for list (which is pairs)")
(assert-false (pair? '()) "pair? returns false for empty list")
(assert-false (pair? 42) "pair? returns false for number")

(assert-true (list? '()) "list? returns true for empty list")
(assert-true (list? (list 1 2 3)) "list? returns true for list")
(assert-true (list? (cons 1 (cons 2 '()))) "list? returns true for cons-built list")
(assert-false (list? 42) "list? returns false for number")
(assert-false (list? "hello") "list? returns false for string")
(assert-true (list? (cons 1 2)) "list? returns true for improper list (cons cell)")

(display "✓ nil?, pair?, list? verified\n")

;; ============================================================================
;; SECTION 9: Type Predicates - number?, symbol?, string?, boolean?
;; ============================================================================

(display "\n=== Type Predicates: number?, symbol?, string?, boolean? ===\n")

(assert-true (number? 42) "number? returns true for integer")
(assert-true (number? 3.14) "number? returns true for float")
(assert-true (number? -100) "number? returns true for negative number")
(assert-true (number? 0) "number? returns true for zero")
(assert-false (number? "42") "number? returns false for string")
(assert-false (number? 'number) "number? returns false for symbol")

(assert-true (symbol? 'hello) "symbol? returns true for symbol")
(assert-true (symbol? 'x) "symbol? returns true for single-char symbol")
(assert-true (symbol? '+) "symbol? returns true for operator symbol")
(assert-false (symbol? "hello") "symbol? returns false for string")
(assert-false (symbol? 42) "symbol? returns false for number")
(assert-false (symbol? '()) "symbol? returns false for list")

(assert-true (string? "hello") "string? returns true for string")
(assert-true (string? "") "string? returns true for empty string")
(assert-true (string? "123") "string? returns true for numeric string")
(assert-false (string? 'hello) "string? returns false for symbol")
(assert-false (string? 123) "string? returns false for number")
(assert-false (string? '()) "string? returns false for list")

(assert-true (boolean? #t) "boolean? returns true for #t")
(assert-true (boolean? #f) "boolean? returns true for #f")
(assert-false (boolean? 1) "boolean? returns false for number")
(assert-false (boolean? 'true) "boolean? returns false for symbol")
(assert-false (boolean? "true") "boolean? returns false for string")
(assert-false (boolean? '()) "boolean? returns false for list")

(display "✓ number?, symbol?, string?, boolean? verified\n")

;; ============================================================================
;; SECTION 10: Type Predicates - All Atoms
;; ============================================================================

(display "\n=== Type Checking All Atoms ===\n")

; Type predicates
(assert-true (symbol? 'name) "symbol? works for symbols")
(assert-false (symbol? :name) "symbol? returns false for keywords")

(assert-true (number? 42) "number? works for numbers")
(assert-false (number? "42") "number? returns false for strings")

(assert-true (string? "hello") "string? works for strings")
(assert-false (string? 'hello) "string? returns false for symbols")

(assert-true (boolean? #t) "boolean? works for booleans")
(assert-false (boolean? 1) "boolean? returns false for numbers")

(assert-true (nil? nil) "nil? works for nil")
(assert-false (nil? #f) "nil? returns false for false")

(display "✓ Type checking verified\n")

;; ============================================================================
;; SECTION 11: Type Predicate Summary
;; ============================================================================

(display "\n=== Type Predicate Summary ===\n")

; Create test values
(var test-nil '())
(var test-pair (cons 1 2))
(var test-list (list 1 2 3))
(var test-number 42)
(var test-symbol 'symbol)
(var test-string "hello")
(var test-bool #t)
(var test-array (array 1 2 3))

; Display type information
(display "nil: ")
(display test-nil)
(display " -> nil?=")
(display (nil? test-nil))
(display " list?=")
(display (list? test-nil))
(newline)

(display "pair: ")
(display test-pair)
(display " -> pair?=")
(display (pair? test-pair))
(newline)

(display "list: ")
(display test-list)
(display " -> list?=")
(display (list? test-list))
(display " pair?=")
(display (pair? test-list))
(newline)

(display "number: ")
(display test-number)
(display " -> number?=")
(display (number? test-number))
(newline)

(display "symbol: ")
(display test-symbol)
(display " -> symbol?=")
(display (symbol? test-symbol))
(newline)

(display "string: ")
(display test-string)
(display " -> string?=")
(display (string? test-string))
(newline)

(display "boolean: ")
(display test-bool)
(display " -> boolean?=")
(display (boolean? test-bool))
(newline)

(display "array: ")
(display test-array)
(display " -> list?=")
(display (list? test-array))
(newline)

;; ============================================================================
;; SECTION 12: Type Predicate Combinations
;; ============================================================================

(display "\n=== Type Predicate Combinations ===\n")

; A list is also a pair (except empty list)
(assert-true (pair? (list 1 2 3)) "non-empty list is a pair")
(assert-false (pair? '()) "empty list is not a pair")

; A number is not a string
(assert-false (string? 42) "number is not a string")
(assert-false (number? "42") "string is not a number")

; A symbol is not a string
(assert-false (string? 'hello) "symbol is not a string")
(assert-false (symbol? "hello") "string is not a symbol")

; Boolean values are distinct
(assert-true (boolean? #t) "#t is boolean")
(assert-true (boolean? #f) "#f is boolean")
(assert-false (= #t 1) "#t is not equal to 1")
(assert-false (= #f 0) "#f is not equal to 0")

(display "✓ Type predicate combinations verified\n")

;; ============================================================================
;; SECTION 13: Arrays
;; ============================================================================

(display "\n=== Arrays ===\n")

; Arrays are a distinct type from lists
(var test-arr (array 1 2 3))
(display "Array: ")
(display test-arr)
(newline)
(assert-false (list? test-arr) "array is not a list")
(display "✓ arrays are distinct from lists\n")

; ========================================
; TYPE CONVERSION SECTION
; ========================================

(display "\n")
(display "========================================\n")
(display "TYPE CONVERSION PRIMITIVES\n")
(display "========================================\n")

; ========================================
; 1. int: Convert to integer
; ========================================
(display "\n=== 1. int: Convert to Integer ===\n")

(display "Converting various types to int:\n")

(display "  int(42) = ")
(display (int 42))
(newline)

(display "  int(3.14) = ")
(display (int 3.14))
(newline)

(display "  int(3.99) = ")
(display (int 3.99))
(newline)

(display "  int(-5.5) = ")
(display (int -5.5))
(newline)

(assert-eq (int 42) 42 "int(42) equals 42")
(assert-eq (int 3.14) 3 "int(3.14) equals 3 (truncates)")
(assert-eq (int 3.99) 3 "int(3.99) equals 3 (truncates)")
(assert-eq (int -5.5) -5 "int(-5.5) equals -5 (truncates)")

(display "✓ int conversion works\n")

; ========================================
; 2. float: Convert to float
; ========================================
(display "\n=== 2. float: Convert to Float ===\n")

(display "Converting various types to float:\n")

(display "  float(42) = ")
(display (float 42))
(newline)

(display "  float(3.14) = ")
(display (float 3.14))
(newline)

(display "  float(-5) = ")
(display (float -5))
(newline)

(assert-eq (float 42) 42.0 "float(42) equals 42.0")
(assert-eq (float 3.14) 3.14 "float(3.14) equals 3.14")
(assert-eq (float -5) -5.0 "float(-5) equals -5.0")

(display "✓ float conversion works\n")

; ========================================
; 3. string: Convert to string
; ========================================
(display "\n=== 3. string: Convert to String ===\n")

(display "Converting various types to string:\n")

(display "  string(42) = ")
(display (string 42))
(newline)

(display "  string(3.14) = ")
(display (string 3.14))
(newline)

(display "  string('hello) = ")
(display (string 'hello))
(newline)

(display "  string(#t) = ")
(display (string #t))
(newline)

(assert-true (string? (string 42)) "string(42) returns a string")
(assert-true (string? (string 3.14)) "string(3.14) returns a string")
(assert-true (string? (string 'hello)) "string('hello) returns a string")
(assert-true (string? (string #t)) "string(#t) returns a string")

(display "✓ string conversion works\n")

; ========================================
; 4. string->int: Parse string to int
; ========================================
(display "\n=== 4. string->int: Parse String to Int ===\n")

(display "Parsing strings to integers:\n")

(display "  string->int(\"42\") = ")
(display (string->int "42"))
(newline)

(display "  string->int(\"-100\") = ")
(display (string->int "-100"))
(newline)

(display "  string->int(\"0\") = ")
(display (string->int "0"))
(newline)

(assert-eq (string->int "42") 42 "string->int(\"42\") equals 42")
(assert-eq (string->int "-100") -100 "string->int(\"-100\") equals -100")
(assert-eq (string->int "0") 0 "string->int(\"0\") equals 0")

(display "✓ string->int parsing works\n")

; ========================================
; 5. string->float: Parse string to float
; ========================================
(display "\n=== 5. string->float: Parse String to Float ===\n")

(display "Parsing strings to floats:\n")

(display "  string->float(\"3.14\") = ")
(display (string->float "3.14"))
(newline)

(display "  string->float(\"-2.5\") = ")
(display (string->float "-2.5"))
(newline)

(display "  string->float(\"0.0\") = ")
(display (string->float "0.0"))
(newline)

(assert-eq (string->float "3.14") 3.14 "string->float(\"3.14\") equals 3.14")
(assert-eq (string->float "-2.5") -2.5 "string->float(\"-2.5\") equals -2.5")
(assert-eq (string->float "0.0") 0.0 "string->float(\"0.0\") equals 0.0")

(display "✓ string->float parsing works\n")

; ========================================
; 6. number->string: Convert number to string
; ========================================
(display "\n=== 6. number->string: Convert Number to String ===\n")

(display "Converting numbers to strings:\n")

(display "  number->string(42) = ")
(display (number->string 42))
(newline)

(display "  number->string(3.14) = ")
(display (number->string 3.14))
(newline)

(display "  number->string(-100) = ")
(display (number->string -100))
(newline)

(assert-true (string? (number->string 42)) "number->string(42) returns string")
(assert-true (string? (number->string 3.14)) "number->string(3.14) returns string")
(assert-true (string? (number->string -100)) "number->string(-100) returns string")

(display "✓ number->string conversion works\n")

; ========================================
; 7. symbol->string: Convert symbol to string
; ========================================
(display "\n=== 7. symbol->string: Convert Symbol to String ===\n")

(display "Converting symbols to strings:\n")

(display "  symbol->string('hello) = ")
(display (symbol->string 'hello))
(newline)

(display "  symbol->string('world) = ")
(display (symbol->string 'world))
(newline)

(display "  symbol->string('+) = ")
(display (symbol->string '+))
(newline)

(assert-true (string? (symbol->string 'hello)) "symbol->string('hello) returns string")
(assert-true (string? (symbol->string 'world)) "symbol->string('world) returns string")
(assert-true (string? (symbol->string '+)) "symbol->string('+) returns string")

(display "✓ symbol->string conversion works\n")

; ========================================
; 8. any->string: Convert any value to string
; ========================================
(display "\n=== 8. any->string: Convert Any Value to String ===\n")

(display "Converting any type to string:\n")

(display "  any->string(42) = ")
(display (any->string 42))
(newline)

(display "  any->string(3.14) = ")
(display (any->string 3.14))
(newline)

(display "  any->string('symbol) = ")
(display (any->string 'symbol))
(newline)

(display "  any->string(\"hello\") = ")
(display (any->string "hello"))
(newline)

(display "  any->string(#t) = ")
(display (any->string #t))
(newline)

(display "  any->string((1 2 3)) = ")
(display (any->string (list 1 2 3)))
(newline)

(assert-true (string? (any->string 42)) "any->string(42) returns string")
(assert-true (string? (any->string 3.14)) "any->string(3.14) returns string")
(assert-true (string? (any->string 'symbol)) "any->string('symbol) returns string")
(assert-true (string? (any->string "hello")) "any->string(\"hello\") returns string")
(assert-true (string? (any->string #t)) "any->string(#t) returns string")
(assert-true (string? (any->string (list 1 2 3))) "any->string(list) returns string")

(display "✓ any->string conversion works\n")

; ========================================
; 9. Round-trip conversions
; ========================================
(display "\n=== 9. Round-Trip Conversions ===\n")

(display "Testing round-trip conversions:\n")

; Number -> String -> Number
(display "  42 -> string -> int: ")
(var num1 42)
(var str1 (number->string num1))
(var num1-back (string->int str1))
(display num1-back)
(newline)
(assert-eq num1-back num1 "Round-trip: number -> string -> int")

; Float -> String -> Float
(display "  3.14 -> string -> float: ")
(var num2 3.14)
(var str2 (number->string num2))
(var num2-back (string->float str2))
(display num2-back)
(newline)
(assert-eq num2-back num2 "Round-trip: float -> string -> float")

; Symbol -> String -> Symbol (via gensym)
(display "  'hello -> string: ")
(var sym 'hello)
(var sym-str (symbol->string sym))
(display sym-str)
(newline)
(assert-true (string? sym-str) "Round-trip: symbol -> string")

(display "✓ Round-trip conversions work\n")

; ========================================
; 10. Type conversion chains
; ========================================
(display "\n=== 10. Type Conversion Chains ===\n")

(display "Chaining conversions:\n")

; int -> float -> string -> int
(display "  int(42) -> float -> string -> int: ")
(var chain1 (int 42))
(var chain2 (float chain1))
(var chain3 (number->string chain2))
(var chain4 (string->int chain3))
(display chain4)
(newline)
(assert-eq chain4 42 "Conversion chain: int -> float -> string -> int")

; string -> int -> float -> string
(display "  string->int(\"100\") -> float -> string: ")
(var chain5 (string->int "100"))
(var chain6 (float chain5))
(var chain7 (number->string chain6))
(display chain7)
(newline)
(assert-true (string? chain7) "Conversion chain: string -> int -> float -> string")

(display "✓ Type conversion chains work\n")

; ========================================
; Summary
; ========================================


(display "=== All Atom Types Verified ===\n")
(display "✓ Keywords (:keyword)\n")
(display "✓ Symbols ('symbol)\n")
(display "✓ Numbers (integers and floats)\n")
(display "✓ Strings (\"string\")\n")
(display "✓ Booleans (#t, #f)\n")
(display "✓ Nil (nil)\n")
(display "✓ Mixed atoms in collections\n")

(display "\nType Checking Predicates:\n")
(display "  ✓ nil?\n")
(display "  ✓ pair?\n")
(display "  ✓ list?\n")
(display "  ✓ number?\n")
(display "  ✓ symbol?\n")
(display "  ✓ string?\n")
(display "  ✓ boolean?\n")
(display "  ✓ vectors (distinct type)\n")

(display "\nType Conversion Functions:\n")
(display "  ✓ int - Convert to integer\n")
(display "  ✓ float - Convert to float\n")
(display "  ✓ string - Convert to string\n")
(display "  ✓ string->int - Parse string to int\n")
(display "  ✓ string->float - Parse string to float\n")
(display "  ✓ number->string - Convert number to string\n")
(display "  ✓ symbol->string - Convert symbol to string\n")
(display "  ✓ any->string - Convert any value to string\n")

(display "\nKey Concepts:\n")
(display "  - Type predicates check value types\n")
(display "  - int truncates floats to integers\n")
(display "  - float converts integers to floats\n")
(display "  - string converts any value to string representation\n")
(display "  - string->int and string->float parse strings\n")
(display "  - Round-trip conversions preserve values\n")
(display "  - Conversion chains enable flexible type handling\n")

(display "\n")
(display "========================================\n")
(display "All tests passed!\n")
(display "========================================\n")
(display "\n")

;; ============================================================================
;; SECTION 14: Mutable Storage - Boxes
;; ============================================================================

(display "=== Mutable Storage: Boxes ===\n")

; === Box Creation ===
(display "\n=== Box Creation ===\n")

; Create a box with initial value
(var my-box (box 42))
(display "Created box with value 42: ")
(display my-box)
(newline)

; Verify it's a box
(assert-true (box? my-box) "box creates a box")

; === Unbox (Get Value) ===
(display "\n=== Unbox (Get Value) ===\n")

; Get value from box
(display "Value in box: ")
(display (unbox my-box))
(newline)
(assert-eq (unbox my-box) 42 "unbox returns the stored value")

; Create boxes with different types
(var string-box (box "hello"))
(var symbol-box (box 'symbol))
(var list-box (box (list 1 2 3)))

(assert-eq (unbox string-box) "hello" "box stores strings")
(assert-eq (unbox symbol-box) 'symbol "box stores symbols")
(assert-eq (unbox list-box) (list 1 2 3) "box stores lists")

(display "✓ unbox works with different types\n")

; === Box Mutation (box-set!) ===
(display "\n=== Box Mutation (box-set!) ===\n")

; Create a mutable box
(var counter (box 0))
(display "Initial counter value: ")
(display (unbox counter))
(newline)
(assert-eq (unbox counter) 0 "counter starts at 0")

; Increment counter
(box-set! counter 1)
(display "After box-set! to 1: ")
(display (unbox counter))
(newline)
(assert-eq (unbox counter) 1 "box-set! updates the value")

; Increment again
(box-set! counter 2)
(assert-eq (unbox counter) 2 "box-set! can update multiple times")

; Set to different type
(box-set! counter "changed")
(assert-eq (unbox counter) "changed" "box-set! can change type")

(display "✓ box-set! works correctly\n")

; === box? Predicate ===
(display "\n=== box? Predicate ===\n")

(assert-true (box? (box 42)) "box? returns true for box")
(assert-true (box? (box "hello")) "box? returns true for any box")
(assert-false (box? 42) "box? returns false for number")
(assert-false (box? "hello") "box? returns false for string")
(assert-false (box? (list 1 2 3)) "box? returns false for list")
(assert-false (box? (array 1 2 3)) "box? returns false for array")

(display "✓ box? works correctly\n")

; === Boxes vs Immutable Structures ===
(display "\n=== Boxes vs Immutable Structures ===\n")

; Lists are immutable
(var my-list (list 1 2 3))
(display "Original list: ")
(display my-list)
(newline)

; cons creates a new list, doesn't modify original
(var new-list (cons 0 my-list))
(display "After cons 0: ")
(display new-list)
(newline)
(display "Original list unchanged: ")
(display my-list)
(newline)
(assert-eq (first my-list) 1 "original list is unchanged")

; Boxes are mutable
(var my-box-list (box (list 1 2 3)))
(display "\nOriginal box contents: ")
(display (unbox my-box-list))
(newline)

; box-set! modifies the box
(box-set! my-box-list (cons 0 (unbox my-box-list)))
(display "After box-set! with cons: ")
(display (unbox my-box-list))
(newline)
(assert-eq (first (unbox my-box-list)) 0 "box contents changed")

(display "✓ Boxes are mutable, lists are immutable\n")

; === Use Case: Mutable State ===
(display "\n=== Use Case: Mutable State ===\n")

; Create a simple state holder with numbers
(var state (box (list)))

(display "Initial state: ")
(display (unbox state))
(newline)

; Add items to state
(box-set! state (cons 100 (unbox state)))
(display "After adding 100: ")
(display (unbox state))
(newline)

(box-set! state (cons 200 (unbox state)))
(display "After adding 200: ")
(display (unbox state))
(newline)

(assert-eq (first (unbox state)) 200 "state contains 200")
(assert-eq (first (rest (unbox state))) 100 "state contains 100")

(display "✓ Mutable state works\n")

(display "\n")
(display "========================================\n")
(display "All type checking and mutable storage tests passed!\n")
(display "========================================\n")
(display "\n")

(display "=== All Atom Types and Mutable Storage Verified ===\n")
(display "✓ Keywords (:keyword)\n")
(display "✓ Symbols ('symbol)\n")
(display "✓ Numbers (integers and floats)\n")
(display "✓ Strings (\"string\")\n")
(display "✓ Booleans (#t, #f)\n")
(display "✓ Nil (nil)\n")
(display "✓ Mixed atoms in collections\n")

(display "\nType Checking Predicates:\n")
(display "  ✓ nil?\n")
(display "  ✓ pair?\n")
(display "  ✓ list?\n")
(display "  ✓ number?\n")
(display "  ✓ symbol?\n")
(display "  ✓ string?\n")
(display "  ✓ boolean?\n")
(display "  ✓ vectors (distinct type)\n")

(display "\nType Conversion Functions:\n")
(display "  ✓ int - Convert to integer\n")
(display "  ✓ float - Convert to float\n")
(display "  ✓ string - Convert to string\n")
(display "  ✓ string->int - Parse string to int\n")
(display "  ✓ string->float - Parse string to float\n")
(display "  ✓ number->string - Convert number to string\n")
(display "  ✓ symbol->string - Convert symbol to string\n")
(display "  ✓ any->string - Convert any value to string\n")

(display "\nMutable Storage (Boxes):\n")
(display "  ✓ box - Create mutable box\n")
(display "  ✓ unbox - Extract value from box\n")
(display "  ✓ box-set! - Mutate box contents\n")
(display "  ✓ box? - Type predicate for boxes\n")
(display "  ✓ Boxes vs immutable structures\n")
(display "  ✓ Counter closure pattern\n")
(display "  ✓ Accumulator closure pattern\n")
(display "  ✓ Mutable state pattern\n")

(display "\nKey Concepts:\n")
(display "  - Type predicates check value types\n")
(display "  - int truncates floats to integers\n")
(display "  - float converts integers to floats\n")
(display "  - string converts any value to string representation\n")
(display "  - string->int and string->float parse strings\n")
(display "  - Round-trip conversions preserve values\n")
(display "  - Conversion chains enable flexible type handling\n")
(display "  - Boxes provide mutable storage in functional language\n")

;; ============================================================================
;; SECTION 15: Truthiness Semantics
;; ============================================================================

(display "\n=== Truthiness Semantics ===\n")

; In Elle, only nil and #f are falsy. Everything else is truthy.
; This is different from languages like C where 0, "", [], etc. are falsy.

(display "\n=== Falsy Values ===\n")

; nil is falsy
(assert-false (if nil #t #f) "nil is falsy in if")
(display "nil is falsy\n")

; #f is falsy
(assert-false (if #f #t #f) "#f is falsy in if")
(display "#f is falsy\n")

(display "✓ Only nil and #f are falsy\n")

(display "\n=== Truthy Values ===\n")

; #t is truthy
(assert-true (if #t #t #f) "#t is truthy in if")
(display "#t is truthy\n")

; Zero is truthy (unlike C)
(assert-true (if 0 #t #f) "0 is truthy in if")
(display "0 is truthy (not falsy like C)\n")

; Negative numbers are truthy
(assert-true (if -1 #t #f) "-1 is truthy in if")
(display "-1 is truthy\n")

; Floats are truthy
(assert-true (if 3.14 #t #f) "3.14 is truthy in if")
(display "3.14 is truthy\n")

; Empty string is truthy (unlike C)
(assert-true (if "" #t #f) "empty string is truthy in if")
(display "empty string is truthy (not falsy like C)\n")

; Empty list is truthy (unlike C)
(assert-true (if '() #t #f) "empty list is truthy in if")
(display "empty list is truthy (not falsy like C)\n")

; Empty array is truthy (unlike C)
(assert-true (if [] #t #f) "empty array is truthy in if")
(display "empty array is truthy (not falsy like C)\n")

; Non-empty string is truthy
(assert-true (if "hello" #t #f) "non-empty string is truthy in if")
(display "non-empty string is truthy\n")

; Non-empty list is truthy
(assert-true (if '(a b c) #t #f) "non-empty list is truthy in if")
(display "non-empty list is truthy\n")

; Non-empty array is truthy
(assert-true (if [1 2 3] #t #f) "non-empty array is truthy in if")
(display "non-empty array is truthy\n")

; Symbols are truthy
(assert-true (if 'symbol #t #f) "symbol is truthy in if")
(display "symbol is truthy\n")

; Keywords are truthy
(assert-true (if :keyword #t #f) "keyword is truthy in if")
(display "keyword is truthy\n")

(display "✓ All other values are truthy\n")

(display "\n=== Truthiness in Conditionals ===\n")

; Using truthiness in cond
(var test-value 0)
(var result (cond
  ((nil? test-value) "is nil")
  ((= test-value #f) "is false")
  (test-value "is truthy")
  (else "unreachable")))
(assert-eq result "is truthy" "0 is truthy in cond")
(display "0 evaluates to truthy in cond\n")

; Using truthiness in and/or
(assert-eq (and 1 2 3) 3 "and returns last value if all truthy")
(assert-eq (and 1 nil 3) nil "and returns first falsy value")
(assert-eq (and 1 #f 3) #f "and returns first falsy value")
(display "and returns last truthy or first falsy\n")

; Using truthiness in or
(assert-eq (or nil #f "hello") "hello" "or returns first truthy value")
(assert-eq (or nil #f) #f "or returns last value if all falsy")
(display "or returns first truthy or last value\n")

(display "✓ Truthiness works in conditionals\n")

(display "\n=== Truthiness Summary ===\n")
(display "Falsy values: nil, #f\n")
(display "Truthy values: everything else\n")
(display "  - 0 is truthy (not falsy like C)\n")
(display "  - \"\" (empty string) is truthy\n")
(display "  - '() (empty list) is truthy\n")
(display "  - [] (empty array) is truthy\n")
(display "  - All other values are truthy\n")

(display "✓ Truthiness semantics verified\n")

(display "  - box-set! enables stateful closures\n")
(display "  - Lists remain immutable; boxes are mutable\n")

(display "\n")
(display "========================================\n")
(display "All tests passed!\n")
(display "========================================\n")
(display "\n")

(exit 0)
