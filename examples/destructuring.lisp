; Destructuring in Elle
;
; This example demonstrates:
; - List destructuring with def and var
; - Array destructuring with def
; - Nested destructuring
; - Destructuring in let and let*
; - Destructuring in function parameters
; - Silent nil semantics for missing values
; - defn with destructured parameters
; - Variadic & rest in function parameters

(import-file "./examples/assertions.lisp")

(display "=== Destructuring ===")
(newline)
(newline)

; ============================================================================
; PART 1: List Destructuring with def
; ============================================================================

(display "PART 1: List Destructuring with def")
(newline)
(newline)

; Basic list destructuring binds names to successive elements
(def (a b c) (list 1 2 3))
(assert-eq a 1 "list destructure first element")
(assert-eq b 2 "list destructure second element")
(assert-eq c 3 "list destructure third element")
(display "  (def (a b c) (list 1 2 3)) => a=")
(display a) (display " b=") (display b) (display " c=") (display c)
(newline)

; Missing elements become nil (no error)
(def (x y z) (list 10))
(assert-eq x 10 "short list first element")
(assert-eq y nil "short list missing second => nil")
(assert-eq z nil "short list missing third => nil")
(display "  (def (x y z) (list 10)) => x=")
(display x) (display " y=") (display y) (display " z=") (display z)
(newline)

; Extra elements are silently ignored
(def (p q) (list 1 2 3 4 5))
(assert-eq p 1 "extra elements: first")
(assert-eq q 2 "extra elements: second")
(display "  (def (p q) (list 1 2 3 4 5)) => p=")
(display p) (display " q=") (display q)
(newline)
(newline)

; ============================================================================
; PART 2: Array Destructuring with def
; ============================================================================

(display "PART 2: Array Destructuring with def")
(newline)
(newline)

(def [i j] [10 20])
(assert-eq i 10 "array destructure first")
(assert-eq j 20 "array destructure second")
(display "  (def [i j] [10 20]) => i=")
(display i) (display " j=") (display j)
(newline)

; Short array gives nil for missing
(def [m n o] [42])
(assert-eq m 42 "short array first")
(assert-eq n nil "short array missing => nil")
(assert-eq o nil "short array missing => nil")
(display "  (def [m n o] [42]) => m=")
(display m) (display " n=") (display n) (display " o=") (display o)
(newline)
(newline)

; ============================================================================
; PART 3: Nested Destructuring
; ============================================================================

(display "PART 3: Nested Destructuring")
(newline)
(newline)

; List within list
(def ((d e) f) (list (list 1 2) 3))
(assert-eq d 1 "nested list inner first")
(assert-eq e 2 "nested list inner second")
(assert-eq f 3 "nested list outer second")
(display "  (def ((d e) f) (list (list 1 2) 3)) => d=")
(display d) (display " e=") (display e) (display " f=") (display f)
(newline)

; Array within list
(def ([g h] k) (list [10 20] 30))
(assert-eq g 10 "array-in-list first")
(assert-eq h 20 "array-in-list second")
(assert-eq k 30 "array-in-list outer")
(display "  (def ([g h] k) (list [10 20] 30)) => g=")
(display g) (display " h=") (display h) (display " k=") (display k)
(newline)
(newline)

; ============================================================================
; PART 4: Mutable Destructuring with var
; ============================================================================

(display "PART 4: Mutable Destructuring with var")
(newline)
(newline)

(var (va vb) (list 1 2))
(assert-eq va 1 "var destructure first")
(set! va 100)
(assert-eq va 100 "var destructure mutated")
(display "  (var (va vb) (list 1 2)) then (set! va 100) => va=")
(display va) (display " vb=") (display vb)
(newline)
(newline)

; ============================================================================
; PART 5: Destructuring in let
; ============================================================================

(display "PART 5: Destructuring in let")
(newline)
(newline)

(var let-result (let (((la lb) (list 10 20))) (+ la lb)))
(assert-eq let-result 30 "let destructure sum")
(display "  (let (((la lb) (list 10 20))) (+ la lb)) => ")
(display let-result)
(newline)

; Mixed simple and destructured bindings
(var mixed-result (let ((s 1) ((t u) (list 2 3))) (+ s t u)))
(assert-eq mixed-result 6 "let mixed bindings")
(display "  (let ((s 1) ((t u) (list 2 3))) (+ s t u)) => ")
(display mixed-result)
(newline)
(newline)

; ============================================================================
; PART 6: Destructuring in let*
; ============================================================================

(display "PART 6: Destructuring in let*")
(newline)
(newline)

; Sequential: second binding uses first
(var star-result (let* (((sa sb) (list 1 2)) (sc (+ sa sb))) sc))
(assert-eq star-result 3 "let* destructure sequential")
(display "  (let* (((sa sb) (list 1 2)) (sc (+ sa sb))) sc) => ")
(display star-result)
(newline)

; Destructure referencing previous binding
(var star2 (let* ((w 10) ((wa wb) (list w 20))) (+ wa wb)))
(assert-eq star2 30 "let* mixed sequential")
(display "  (let* ((w 10) ((wa wb) (list w 20))) (+ wa wb)) => ")
(display star2)
(newline)
(newline)

; ============================================================================
; PART 7: Destructuring in Function Parameters
; ============================================================================

(display "PART 7: Destructuring in Function Parameters")
(newline)
(newline)

(defn add-pair ((pa pb)) (+ pa pb))
(var pair-result (add-pair (list 3 4)))
(assert-eq pair-result 7 "fn param destructure")
(display "  (defn add-pair ((pa pb)) (+ pa pb)) => (add-pair (list 3 4)) = ")
(display pair-result)
(newline)

; Mixed normal and destructured params
(defn weighted-sum (weight (wa wb))
  (+ (* weight wa) (* weight wb)))
(var ws-result (weighted-sum 2 (list 3 4)))
(assert-eq ws-result 14 "fn mixed params")
(display "  (defn weighted-sum (weight (wa wb)) ...) => (weighted-sum 2 (list 3 4)) = ")
(display ws-result)
(newline)

; Nested destructuring in params
(defn deep-sum (((da db) dc))
  (+ da db dc))
(var deep-result (deep-sum (list (list 1 2) 3)))
(assert-eq deep-result 6 "fn nested param destructure")
(display "  (defn deep-sum (((da db) dc)) ...) => (deep-sum (list (list 1 2) 3)) = ")
(display deep-result)
(newline)
(newline)

; ============================================================================
; PART 8: Wildcard _ Pattern
; ============================================================================

(display "PART 8: Wildcard _ Pattern")
(newline)
(newline)

; _ discards the matched value — no binding is created
(def (_ second _) (list 10 20 30))
(assert-eq second 20 "wildcard skips first and third")
(display "  (def (_ second _) (list 10 20 30)) => second=")
(display second)
(newline)

; Wildcard in array destructuring
(def [_ y2] [100 200])
(assert-eq y2 200 "array wildcard")
(display "  (def [_ y2] [100 200]) => y2=")
(display y2)
(newline)

; Wildcard in nested destructuring
(def ((_ nb) nc) (list (list 1 2) 3))
(assert-eq nb 2 "nested wildcard inner")
(assert-eq nc 3 "nested wildcard outer")
(display "  (def ((_ nb) nc) (list (list 1 2) 3)) => nb=")
(display nb) (display " nc=") (display nc)
(newline)

; Wildcard in function parameters
(defn second-of-pair ((_ b)) b)
(assert-eq (second-of-pair (list 10 20)) 20 "fn wildcard param")
(display "  (defn second-of-pair ((_ b)) b) => (second-of-pair (list 10 20)) = ")
(display (second-of-pair (list 10 20)))
(newline)
(newline)

; ============================================================================
; PART 9: & rest Pattern
; ============================================================================

(display "PART 9: & rest Pattern")
(newline)
(newline)

; Collect remaining list elements
(def (head & tail) (list 1 2 3 4))
(assert-eq head 1 "rest: head")
(assert-eq (first tail) 2 "rest: tail first")
(assert-eq (length tail) 3 "rest: tail length")
(display "  (def (head & tail) (list 1 2 3 4)) => head=")
(display head) (display " tail=") (display tail)
(newline)

; Empty rest when all elements consumed
(def (ra rb & rc) (list 1 2))
(assert-eq ra 1 "rest empty: first")
(assert-eq rb 2 "rest empty: second")
(assert-eq rc (list) "rest empty: rest is empty list")
(display "  (def (ra rb & rc) (list 1 2)) => ra=")
(display ra) (display " rb=") (display rb) (display " rc=") (display rc)
(newline)

; Array rest collects into a sub-array
(def [ax & ar] [10 20 30])
(assert-eq ax 10 "array rest: first")
(assert-eq (array-ref ar 0) 20 "array rest: rest[0]")
(assert-eq (array-ref ar 1) 30 "array rest: rest[1]")
(display "  (def [ax & ar] [10 20 30]) => ax=")
(display ax) (display " ar=") (display ar)
(newline)

; Wildcard + rest combined
(def (_ & wr) (list 1 2 3))
(assert-eq (first wr) 2 "wildcard+rest: first of rest")
(display "  (def (_ & wr) (list 1 2 3)) => wr=")
(display wr)
(newline)

; Rest in function parameters (via list destructuring)
(defn sum-head-next ((hd & r))
  (if (empty? r)
    hd
    (+ hd (first r))))
(assert-eq (sum-head-next (list 10 20)) 30 "fn rest param")
(display "  (defn sum-head-next ((hd & r)) ...) => (sum-head-next (list 10 20)) = ")
(display (sum-head-next (list 10 20)))
(newline)
(newline)

; ============================================================================
; PART 10: Variadic & rest in Function Parameters
; ============================================================================

(display "PART 10: Variadic & rest in Function Parameters")
(newline)
(newline)

; Collect all arguments into a list
(defn my-list (& items) items)
(assert-eq (my-list 1 2 3) (list 1 2 3) "variadic: collect all")
(display "  (defn my-list (& items) items) => (my-list 1 2 3) = ")
(display (my-list 1 2 3))
(newline)

; No extra args → empty list
(assert-eq (my-list) (list) "variadic: no args => empty list")
(display "  (my-list) => ")
(display (my-list))
(newline)

; Fixed params + rest
(defn head-and-rest (x & rest) (list x rest))
(def (hd rst) (head-and-rest 1 2 3))
(assert-eq hd 1 "variadic: fixed head")
(assert-eq rst (list 2 3) "variadic: rest list")
(display "  (defn head-and-rest (x & rest) ...) => (head-and-rest 1 2 3) = ")
(display (head-and-rest 1 2 3))
(newline)

; Rest with no extra args
(def (hd2 rst2) (head-and-rest 42))
(assert-eq hd2 42 "variadic: fixed only")
(assert-eq rst2 (list) "variadic: rest empty")
(display "  (head-and-rest 42) => ")
(display (head-and-rest 42))
(newline)

; Variadic with closure capture
(def multiplier 10)
(defn scale-all (& nums)
  (if (empty? nums) (list)
      (cons (* multiplier (first nums))
            (scale-all))))
(display "  Variadic with closure capture: scale-all defined")
(newline)

; Variadic higher-order: apply-fn
(defn apply-fn (f & args)
  (f (first args)))
(assert-eq (apply-fn (fn (x) (+ x 1)) 10) 11 "variadic higher-order")
(display "  (defn apply-fn (f & args) (f (first args))) => (apply-fn inc 10) = ")
(display (apply-fn (fn (x) (+ x 1)) 10))
(newline)
(newline)

; ============================================================================
; PART 11: Practical Examples
; ============================================================================

(display "PART 11: Practical Examples")
(newline)
(newline)

; Swap two values
(def (first-val second-val) (list 1 2))
(def (swapped-b swapped-a) (list second-val first-val))
(assert-eq swapped-a 1 "swap a")
(assert-eq swapped-b 2 "swap b")
(display "  Swap: (1, 2) => (")
(display swapped-a) (display ", ") (display swapped-b) (display ")")
(newline)

; Destructure a function's return value
(defn divide-with-remainder (a b)
  (list (/ a b) (% a b)))
(def (quotient remainder) (divide-with-remainder 17 5))
(assert-eq quotient 3 "divmod quotient")
(assert-eq remainder 2 "divmod remainder")
(display "  17 / 5 => quotient=")
(display quotient) (display " remainder=") (display remainder)
(newline)

(newline)
(display "=== All destructuring tests passed ===")
(newline)
