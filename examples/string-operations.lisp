# String operations examples

(import-file "./examples/assertions.lisp")

# string-split: Split string on delimiter
(display "=== string-split ===")
(newline)
(display "Split 'a,b,c' on ',':")
(display (string-split "a,b,c" ","))
(newline)

(display "Split 'hello' on 'll':")
(display (string-split "hello" "ll"))
(newline)

(display "Split 'aaa' on 'a' (preserves empty segments):")
(display (string-split "aaa" "a"))
(newline)

(display "Split 'hello' on 'xyz' (no match):")
(display (string-split "hello" "xyz"))
(newline)

## string-split Assertions
(assert-eq (length (string-split "a,b,c" ",")) 3 "split 'a,b,c' on ',' = 3 parts")
(assert-eq (length (string-split "hello" "ll")) 2 "split 'hello' on 'll' = 2 parts")
(newline)

# string-replace: Replace all occurrences
(display "=== string-replace ===")
(newline)
(display "Replace 'world' with 'elle' in 'hello world':")
(display (string-replace "hello world" "world" "elle"))
(newline)

(display "Replace 'a' with 'bb' in 'aaa':")
(display (string-replace "aaa" "a" "bb"))
(newline)

## string-replace Assertions
(assert-string-eq (string-replace "hello world" "world" "elle") "hello elle" "replace 'world' with 'elle'")
(newline)

# string-trim: Trim whitespace
(display "=== string-trim ===")
(newline)
(display "Trim '  hello  ':")
(display (string-trim "  hello  "))
(newline)

(display "Trim 'hello' (no whitespace):")
(display (string-trim "hello"))
(newline)

## string-trim Assertions
(assert-string-eq (string-trim "  hello  ") "hello" "trim '  hello  ' = 'hello'")
(assert-string-eq (string-trim "hello") "hello" "trim 'hello' = 'hello'")
(newline)

# string-contains?: Check if contains substring
(display "=== string-contains? ===")
(newline)
(display "Does 'hello world' contain 'world'?")
(display (string-contains? "hello world" "world"))
(newline)

(display "Does 'hello' contain 'xyz'?")
(display (string-contains? "hello" "xyz"))
(newline)

(display "Does 'hello' contain '' (empty string)?")
(display (string-contains? "hello" ""))
(newline)

## string-contains? Assertions
(assert-eq (string-contains? "hello world" "world") true "contains 'world' in 'hello world'")
(assert-eq (string-contains? "hello" "xyz") false "does not contain 'xyz' in 'hello'")
(newline)

# string-starts-with?: Check if starts with prefix
(display "=== string-starts-with? ===")
(newline)
(display "Does 'hello' start with 'hel'?")
(display (string-starts-with? "hello" "hel"))
(newline)

(display "Does 'hello' start with 'world'?")
(display (string-starts-with? "hello" "world"))
(newline)

## string-starts-with? Assertions
(assert-eq (string-starts-with? "hello" "hel") true "starts with 'hel'")
(assert-eq (string-starts-with? "hello" "world") false "does not start with 'world'")
(newline)

# string-ends-with?: Check if ends with suffix
(display "=== string-ends-with? ===")
(newline)
(display "Does 'hello' end with 'llo'?")
(display (string-ends-with? "hello" "llo"))
(newline)

(display "Does 'hello' end with 'world'?")
(display (string-ends-with? "hello" "world"))
(newline)

## string-ends-with? Assertions
(assert-eq (string-ends-with? "hello" "llo") true "ends with 'llo'")
(assert-eq (string-ends-with? "hello" "world") false "does not end with 'world'")
(newline)

# string-join: Join list of strings with separator
(display "=== string-join ===")
(newline)
(display "Join (list 'a' 'b' 'c') with ',':")
(display (string-join (list "a" "b" "c") ","))
(newline)

(display "Join (list 'hello') with ' ':")
(display (string-join (list "hello") " "))
(newline)

(display "Join empty list with ',':")
(display (string-join (list) ","))
(newline)

## string-join Assertions
(assert-string-eq (string-join (list "a" "b" "c") ",") "a,b,c" "join with ','")
(assert-string-eq (string-join (list "hello") " ") "hello" "join single element")
(newline)

# number->string: Convert number to string
(display "=== number->string ===")
(newline)
(display "Convert 42 to string:")
(display (number->string 42))
(newline)

(display "Convert 3.14 to string:")
(display (number->string 3.14))
(newline)

## number->string Assertions
(assert-string-eq (number->string 42) "42" "convert 42 to string")
(newline)

# Practical example: Parse and process CSV-like data
(display "=== Practical Example: CSV Processing ===")
(newline)
(var csv-line "John,Doe,30,Engineer")
(var fields (string-split csv-line ","))
(display "CSV line: ")
(display csv-line)
(newline)
(display "Fields: ")
(display fields)
(newline)

## CSV Assertions
(assert-eq (length fields) 4 "CSV has 4 fields")
(newline)

# Practical example: String manipulation chain
(display "=== Practical Example: String Manipulation Chain ===")
(newline)
(var text "  Hello World  ")
(var trimmed (string-trim text))
(var lowercased (string-downcase trimmed))
(var replaced (string-replace lowercased "world" "elle"))
(display "Original: '")
(display text)
(display "'")
(newline)
(display "After trim, downcase, replace: '")
(display replaced)
(display "'")
(newline)

## String manipulation chain Assertions
(assert-string-eq trimmed "Hello World" "trim removes whitespace")
(assert-string-eq lowercased "hello world" "downcase converts to lowercase")
(assert-string-eq replaced "hello elle" "replace 'world' with 'elle'")
(newline)

(display "=== All String Operations Assertions Passed ===")
(newline)

# === String Module (Built-in) ===
(display "\n=== String Module (Built-in) ===\n")

(display "Elle's String Module provides:")
(newline)
(display "  - string-upcase, string-downcase")
(newline)
(display "  - substring, length (for strings)")
(newline)
(display "  - string-split, string-join")
(newline)
(display "  - string-replace, string-trim")
(newline)
(display "  - string-contains?, string-starts-with?, string-ends-with?")
(newline)
(display "  - char-at, string-index")
(newline)
(display "  - append, number->string")
(newline)

(display "\nModule examples:")
(newline)

(display "  string-upcase(\"hello\") = ")
(let ((upper (string-upcase "hello")))
  (display upper)
  (newline)
  (assert-string-eq upper "HELLO" "Module: string-upcase"))

(display "  string-downcase(\"WORLD\") = ")
(let ((lower (string-downcase "WORLD")))
  (display lower)
  (newline)
  (assert-string-eq lower "world" "Module: string-downcase"))

(display "  substring(\"Elle\", 0, 2) = ")
(let ((sub (substring "Elle" 0 2)))
  (display sub)
  (newline)
  (assert-string-eq sub "El" "Module: substring"))

(display "  length(\"Hello\") = ")
(let ((len (length "Hello")))
  (display len)
  (newline)
  (assert-eq len 5 "Module: length for string"))

(display "  char-at(\"Elle\", 0) = ")
(let ((ch (char-at "Elle" 0)))
  (display ch)
  (newline)
  (assert-string-eq ch "E" "Module: char-at"))

(display "  string-index(\"Elle\", \"l\") = ")
(let ((idx (string-index "Elle" "l")))
  (display idx)
  (newline)
  (assert-eq idx 1 "Module: string-index"))

(newline)
(display "✓ String Module functions verified")
(newline)

## NOTE: The `length` function is polymorphic and works on all sequence types
## (lists, strings, arrays, tables, structs, keywords). See list-operations.lisp,
## array-operations.lisp, and tables-and-structs.lisp for examples with other types.
