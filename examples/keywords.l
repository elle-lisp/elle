; Keyword demonstration - Keywords are self-evaluating values prefixed with :
; Keywords are useful for map keys, configuration options, and pattern matching

; Basic keyword creation and display
(display "=== Basic Keywords ===")
(newline)
(display :name)
(newline)
(display :value)
(newline)
(display :status)
(newline)

; Keywords have a type
(display (newline))
(display "=== Type of Keywords ===")
(newline)
(display (type :keyword-name))
(newline)

; Keyword equality
(display (newline))
(display "=== Keyword Equality ===")
(newline)
(display (= :foo :foo))
(newline)
(display (= :foo :bar))
(newline)
(display (= :name :name))
(newline)

; Keywords in lists - useful for building data structures
(display (newline))
(display "=== Keywords in Lists ===")
(newline)
(define person '(:name :John :age :30 :city :NYC))
(display person)
(newline)

; Keywords in vectors
(display (newline))
(display "=== Keywords in Vectors ===")
(newline)
(define options [1 :option-a 2 :option-b 3])
(display options)
(newline)

; Building configuration with keywords
(display (newline))
(display "=== Configuration with Keywords ===")
(newline)
(define settings (list :debug #t :host "localhost" :port 8080))
(display settings)
(newline)

; Keywords as data structure labels
(display (newline))
(display "=== Using Keywords for Data ===")
(newline)
(define colors (list :red 255 :green 128 :blue 64))
(display colors)
(newline)

; Keywords are distinct from symbols
(display (newline))
(display "=== Keywords vs Symbols ===")
(newline)
(display (= :name 'name))
(newline)
(display (= (type :name) (type 'name)))
(newline)

; Keywords work in vectors
(display (newline))
(display "=== Keywords in Vectors for Named Access ===")
(newline)
(define config [:timeout 5000 :retries 3 :debug #t :host "localhost"])
(display config)
(newline)

; String representation of keywords
(display (newline))
(display "=== Display Representation ===")
(newline)
(display :greeting)
(display " ")
(display :farewell)
(newline)
