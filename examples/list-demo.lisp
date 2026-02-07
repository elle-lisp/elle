; List operations demo
(define my-list (list 1 2 3 4 5))

(display "First element: ")
(display (first my-list))
(newline)

(display "Rest: ")
(display (rest my-list))
(newline)

(display "Building a list: ")
(display (cons 0 my-list))
(newline)
