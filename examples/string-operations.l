; String operations examples

; string-split: Split string on delimiter
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

; string-replace: Replace all occurrences
(display "=== string-replace ===")
(newline)
(display "Replace 'world' with 'elle' in 'hello world':")
(display (string-replace "hello world" "world" "elle"))
(newline)

(display "Replace 'a' with 'bb' in 'aaa':")
(display (string-replace "aaa" "a" "bb"))
(newline)

; string-trim: Trim whitespace
(display "=== string-trim ===")
(newline)
(display "Trim '  hello  ':")
(display (string-trim "  hello  "))
(newline)

(display "Trim 'hello' (no whitespace):")
(display (string-trim "hello"))
(newline)

; string-contains?: Check if contains substring
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

; string-starts-with?: Check if starts with prefix
(display "=== string-starts-with? ===")
(newline)
(display "Does 'hello' start with 'hel'?")
(display (string-starts-with? "hello" "hel"))
(newline)

(display "Does 'hello' start with 'world'?")
(display (string-starts-with? "hello" "world"))
(newline)

; string-ends-with?: Check if ends with suffix
(display "=== string-ends-with? ===")
(newline)
(display "Does 'hello' end with 'llo'?")
(display (string-ends-with? "hello" "llo"))
(newline)

(display "Does 'hello' end with 'world'?")
(display (string-ends-with? "hello" "world"))
(newline)

; string-join: Join list of strings with separator
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

; number->string: Convert number to string
(display "=== number->string ===")
(newline)
(display "Convert 42 to string:")
(display (number->string 42))
(newline)

(display "Convert 3.14 to string:")
(display (number->string 3.14))
(newline)

; Practical example: Parse and process CSV-like data
(display "=== Practical Example: CSV Processing ===")
(newline)
(define csv-line "John,Doe,30,Engineer")
(define fields (string-split csv-line ","))
(display "CSV line: ")
(display csv-line)
(newline)
(display "Fields: ")
(display fields)
(newline)

; Practical example: String manipulation chain
(display "=== Practical Example: String Manipulation Chain ===")
(newline)
(define text "  Hello World  ")
(define trimmed (string-trim text))
(define lowercased (string-downcase trimmed))
(define replaced (string-replace lowercased "world" "elle"))
(display "Original: '")
(display text)
(display "'")
(newline)
(display "After trim, downcase, replace: '")
(display replaced)
(display "'")
(newline)
