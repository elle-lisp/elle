;; Universal length demonstration
;; The length function works on all collection types

;; Lists
(display "Lists: ")
(display (length '(1 2 3)))
(newline)

;; Vectors
(display "Vectors: ")
(display (length [1 2 3 4]))
(newline)

;; Strings
(display "Strings: ")
(display (length "hello"))
(newline)

;; Keywords
(display "Keywords: ")
(display (length :foo))
(newline)

;; Tables
(display "Tables: ")
(display (length (table "a" 1 "b" 2)))
(newline)

;; Structs
(display "Structs: ")
(display (length {"a" 1 "b" 2 "c" 3}))
(newline)
