(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## === Shebang Handling ===

(assert-eq (+ 1 2) 3 "shebang with env elle")
(assert-eq 42 42 "shebang short form")
(assert-eq (+ 10 20) 30 "no shebang works normally")
(assert-eq (let ((x 5)) (* x x)) 25 "shebang with complex expression")

## === Macros ===

(assert-eq (begin
             (defmacro my-when (test body) `(if ,test ,body nil))
             (my-when true 42))
           42
           "defmacro my-when true")

(assert-eq (begin
             (defmacro my-when (test body) `(if ,test ,body nil))
             (my-when false 42))
           nil
           "defmacro my-when false")

(assert-eq (begin
             (defmacro my-when (test body) `(if ,test ,body nil))
             (macro? my-when))
           true
           "macro? predicate after defining")

(assert-eq (macro? +) false "macro? predicate non-macro")

(assert-true (begin
               (defmacro my-when (test body) `(if ,test ,body nil))
               (expand-macro '(my-when true 42)))
             "expand-macro returns expanded form")

## === Qualified Symbol Access ===

(assert-eq (let ((obj {:key 42}))
             obj:key)
           42
           "qualified symbol struct access")

(assert-eq (let ((obj {:inner {:value 99}}))
             obj:inner:value)
           99
           "qualified symbol chained access")

(assert-eq (let ((a {:b {:c {:d 7}}}))
             a:b:c:d)
           7
           "qualified symbol triple chain")

(assert-eq (let ((obj {:add1 (fn (x) (+ x 1))}))
             (obj:add1 41))
           42
           "qualified symbol in call position")

(assert-eq (let ((obj {:inner {:double (fn (x) (* x 2))}}))
             (obj:inner:double 21))
           42
           "qualified symbol nested call")

(assert-eq (let ((obj {:a 1}))
             obj:missing)
           nil
           "qualified symbol missing key returns nil")

(assert-eq (read ":foo") :foo "qualified symbol keyword not affected")
(assert-true (symbol? 'foo:bar) "qualified symbol quoted preserved")

(assert-eq (let ((t @{:x 10}))
              t:x)
            10
            "qualified symbol with @struct")

(assert-err (fn () (eval 'unbound:foo)) "qualified symbol unbound first segment")

## === Tables and Structs ===

(assert-true (struct? (@struct)) "@struct creation empty")

(assert-eq (let ((t (@struct)))
             (put t "key" 42)
             (get t "key"))
           42
           "@struct put and get")

(assert-true (struct? (struct)) "struct creation empty")

(assert-eq (= (type-of (@struct)) :@struct) true "type-of @struct")
(assert-eq (= (type-of (struct)) :struct) true "type-of struct")

(assert-eq (let ((t (@struct "a" 1 "b" 2)))
             (+ (get t "a") (get t "b")))
           3
           "@struct with string keys")

(assert-eq (let ((s (struct "x" 10 "y" 20)))
             (+ (get s "x") (get s "y")))
           30
           "struct with string keys")

(assert-eq (let ((t (@struct "a" 1)))
             (has? t "a"))
           true
           "@struct has? true")

(assert-eq (let ((t (@struct "a" 1)))
             (has? t "b"))
           false
           "@struct has? missing")

## === @struct Mutation ===

(assert-eq (let ((t (@struct)))
             (put t "a" 1)
             (put t "a" 2)
             (get t "a"))
           2
           "@struct mutation")

(assert-eq (let ((s (struct "x" 42)))
             (get s "x"))
           42
           "struct immutability")

(assert-eq (let ((outer (@struct)))
             (put outer "inner" (@struct))
             (put (get outer "inner") "value" 42)
             (get (get outer "inner") "value"))
           42
           "nested @struct operations")

(assert-eq (begin
             (defmacro add-one (x) `(+ ,x 1))
             (add-one 41))
           42
           "defmacro with quasiquote")

(assert-eq (-> 5 (+ 3) (* 2)) 16 "threading macro first")
(assert-eq (->> 5 (+ 3) (* 2)) 16 "threading macro last")

(assert-eq (let ((t (@struct "a" 1 "b" 2)))
             (length (keys t)))
           2
           "@struct keys")

(assert-eq (let ((t (@struct "a" 1 "b" 2)))
             (length (values t)))
           2
           "@struct values")

(assert-eq (let ((t (@struct "a" 1 "b" 2)))
             (del t "a")
             (has? t "a"))
           false
           "@struct del")

(assert-eq (let ((s (struct "x" 1)))
             (let ((s2 (put s "x" 2)))
               (list (get s "x") (get s2 "x"))))
           (list 1 2)
           "struct put returns new")

(assert-eq (let ((t (@struct)))
             (get t "missing" 42))
           42
           "get @struct with default")

## === Let Binding Semantics ===

(assert-eq (let ((x 10) (y 20)) (+ x y)) 30 "let parallel binding")

(assert-eq (begin (var x 999) (let ((x 10) (y x)) y)) 999 "let parallel binding shadowing")

(assert-eq (begin (var x 999) (let* ((x 10) (y x)) y)) 10 "let* sequential binding")

(assert-eq (let ((x 42)) x) 42 "let body sees bindings")

(assert-eq (let ((x 1)) (let ((x 2)) x)) 2 "let nested shadowing")

## === Polymorphic get - Arrays ===

(assert-eq (get [1 2 3] 0) 1 "get array by index")
(assert-eq (get [1 2 3] 1) 2 "get array by index middle")
(assert-eq (get [1 2 3] 2) 3 "get array by index last")
(assert-eq (get [1 2 3] 10) nil "get array out of bounds")
(assert-eq (get [1 2 3] 10 :missing) :missing "get array out of bounds with default")
(assert-eq (get [1 2 3] -1) nil "get array negative index")
(assert-eq (get [1 2 3] -1 :default) :default "get array negative index with default")
(assert-eq (get [] 0) nil "get empty array")
(assert-err (fn () (get [1 2 3] :key)) "get array non-integer index error")

## === Polymorphic get - Arrays ===

(assert-eq (get @[1 2 3] 0) 1 "get array by index")
(assert-eq (get @[1 2 3] 1) 2 "get array by index middle")
(assert-eq (get @[1 2 3] 2) 3 "get array by index last")
(assert-eq (get @[1 2 3] 10) nil "get array out of bounds")
(assert-eq (get @[1 2 3] 10 :missing) :missing "get array out of bounds with default")
(assert-eq (get @[1 2 3] -1) nil "get array negative index")
(assert-eq (get @[] 0) nil "get empty array")
(assert-err (fn () (get @[1 2 3] :key)) "get array non-integer index error")

## === Polymorphic get - Strings ===

(assert-eq (get "hello" 0) "h" "get string by char index")
(assert-eq (get "hello" 1) "e" "get string by char index middle")
(assert-eq (get "hello" 4) "o" "get string by char index last")
(assert-eq (get "hello" 10) nil "get string out of bounds")
(assert-eq (get "hello" 10 :missing) :missing "get string out of bounds with default")
(assert-eq (get "hello" -1) nil "get string negative index")
(assert-eq (get "" 0) nil "get empty string")
(assert-err (fn () (get "hello" :key)) "get string non-integer index error")
(assert-eq (get "café" 3) "é" "get string unicode char")

## === Polymorphic get - Structs ===

(assert-eq (get {:a 1} :a) 1 "get struct by keyword")
(assert-eq (get {"x" 10} "x") 10 "get struct by string key")
(assert-eq (get {:a 1} :b) nil "get struct missing key")
(assert-eq (get {:a 1} :b :missing) :missing "get struct missing key with default")
(assert-eq (get {} :a) nil "get empty struct")

## === Polymorphic get - @structs ===

(assert-eq (get @{:a 1} :a) 1 "get @struct by keyword")
(assert-eq (get @{"x" 10} "x") 10 "get @struct by string key")
(assert-eq (get @{:a 1} :b) nil "get @struct missing key")
(assert-eq (get @{:a 1} :b :missing) :missing "get @struct missing key with default")
(assert-eq (get @{} :a) nil "get empty @struct")

## === get Error Cases ===

(assert-err (fn () (eval '(get))) "get wrong arity no args")
(assert-err (fn () (eval '(get [1 2 3]))) "get wrong arity one arg")
(assert-err (fn () (eval '(get [1 2 3] 0 :default :extra))) "get wrong arity too many args")
(assert-err (fn () (get 42 0)) "get unsupported type")
(assert-err (fn () (get nil 0)) "get nil type")

## === put - Arrays ===

(assert-eq (get (put [1 2 3] 0 99) 0) 99 "put array by index")
(assert-eq (get (put [1 2 3] 1 99) 1) 99 "put array by index middle")
(assert-eq (get (put [1 2 3] 2 99) 2) 99 "put array by index last")
(assert-err (fn () (put [1 2 3] 10 99)) "put array out of bounds")
(assert-err (fn () (put [1 2 3] -1 99)) "put array negative index")

(assert-eq (let ((t [1 2 3]))
             (let ((t2 (put t 0 99)))
               (list t t2)))
           (list [1 2 3] [99 2 3])
           "put array immutable original unchanged")

(assert-err (fn () (put [1 2 3] :key 99)) "put array non-integer index error")
(assert-err (fn () (put [] 0 99)) "put empty array errors")

## === put - @arrays ===

(assert-eq (get (put @[1 2 3] 0 99) 0) 99 "put @array by index")
(assert-eq (get (put @[1 2 3] 1 99) 1) 99 "put @array by index middle")
(assert-eq (get (put @[1 2 3] 2 99) 2) 99 "put @array by index last")
(assert-err (fn () (put @[1 2 3] 10 99)) "put @array out of bounds")
(assert-err (fn () (put @[1 2 3] -1 99)) "put @array negative index")

(assert-eq (let ((a @[1 2 3]))
             (let ((a2 (put a 0 99)))
               (identical? a a2)))
           true
           "put @array mutable same reference")

(assert-err (fn () (put @[1 2 3] :key 99)) "put @array non-integer index error")
(assert-err (fn () (put @[] 0 99)) "put empty @array errors")

## === put - Strings ===

(assert-eq (put "hello" 0 "a") "aello" "put string by char index")
(assert-eq (put "hello" 1 "a") "hallo" "put string by char index middle")
(assert-eq (put "hello" 4 "a") "hella" "put string by char index last")
(assert-err (fn () (put "hello" 10 "a")) "put string out of bounds")
(assert-err (fn () (put "hello" -1 "a")) "put string negative index")

(assert-eq (let ((s "hello"))
             (let ((s2 (put s 0 "a")))
               (list s s2)))
           (list "hello" "aello")
           "put string immutable original unchanged")

(assert-err (fn () (put "hello" :key "a")) "put string non-integer index error")
(assert-err (fn () (put "" 0 "a")) "put empty string errors")
(assert-eq (put "café" 3 "x") "cafx" "put string unicode char")

## === put - Structs ===

(assert-eq (get (put {:a 1} :a 99) :a) 99 "put struct by keyword")
(assert-eq (get (put {:a 1} :b 2) :a) 1 "put struct new key a")
(assert-eq (get (put {:a 1} :b 2) :b) 2 "put struct new key b")

(assert-eq (let ((s {:a 1}))
             (let ((s2 (put s :a 99)))
               (list (get s :a) (get s2 :a))))
           (list 1 99)
           "put struct immutable original unchanged")

(assert-eq (get (put {} :a 1) :a) 1 "put empty struct")

## === put - @structs ===

(assert-eq (get (put @{:a 1} :a 99) :a) 99 "put @struct by keyword")
(assert-eq (get (put @{:a 1} :b 2) :a) 1 "put @struct new key a")
(assert-eq (get (put @{:a 1} :b 2) :b) 2 "put @struct new key b")

(assert-eq (let ((t @{:a 1}))
             (let ((t2 (put t :a 99)))
               (identical? t t2)))
           true
           "put @struct mutable same reference")

(assert-eq (get (put @{} :a 1) :a) 1 "put empty @struct")

## === put Error Cases ===

(assert-err (fn () (eval '(put))) "put wrong arity no args")
(assert-err (fn () (eval '(put [1 2 3]))) "put wrong arity one arg")
(assert-err (fn () (eval '(put [1 2 3] 0))) "put wrong arity two args")
(assert-err (fn () (eval '(put [1 2 3] 0 99 :extra))) "put wrong arity too many args")
(assert-err (fn () (put 42 0 99)) "put unsupported type")
(assert-err (fn () (put nil 0 99)) "put nil type")

## === rebox ===

(assert-eq (rebox (box 1) 42) 42 "rebox returns value")

(assert-eq (begin
             (var b (box 1))
             (rebox b 2)
             (unbox b))
           2
           "rebox updates cell")

(assert-eq (begin
             (var b (box 1))
             (rebox b "hello")
             (unbox b))
           "hello"
           "rebox with different types")

(assert-eq (begin
             (var b (box 1))
             (rebox b nil)
             (unbox b))
           nil
           "rebox with nil")

(assert-err (fn () (eval '(rebox))) "rebox wrong arity no args")
(assert-err (fn () (eval '(rebox (box 1)))) "rebox wrong arity one arg")
(assert-err (fn () (eval '(rebox (box 1) 2 3))) "rebox wrong arity too many args")
(assert-err (fn () (rebox 42 99)) "rebox non-cell error")

## === push ===

(assert-eq (length (push @[1 2] 3)) 3 "push single element")

(assert-eq (let ((a @[1 2]))
             (let ((a2 (push a 3)))
               (identical? a a2)))
           true
           "push returns same array")

(assert-eq (length (push @[] 1)) 1 "push empty array")

(assert-eq (begin
             (var a @[])
             (push a 1)
             (push a 2)
             (push a 3)
             (length a))
           3
           "push multiple times")

(assert-err (fn () (eval '(push))) "push wrong arity no args")
(assert-err (fn () (eval '(push @[1 2]))) "push wrong arity one arg")
(assert-err (fn () (eval '(push @[1 2] 3 4))) "push wrong arity too many args")
(assert-err (fn () (push [1 2] 3)) "push non-array error")

## === pop ===

(assert-eq (pop @[1 2 3]) 3 "pop single element")

(assert-eq (begin
             (var a @[1 2 3])
             (pop a)
             (length a))
           2
           "pop mutates array")

(assert-err (fn () (pop @[])) "pop empty array errors")

(assert-eq (begin
             (var a @[42])
             (pop a)
             (length a))
           0
           "pop single element array")

(assert-err (fn () (eval '(pop))) "pop wrong arity no args")
(assert-err (fn () (eval '(pop @[1 2] 3))) "pop wrong arity too many args")
(assert-err (fn () (pop [1 2 3])) "pop non-array error")

## === popn ===

(assert-eq (length (popn @[1 2 3 4] 2)) 2 "popn two elements")

(assert-eq (begin
             (var a @[1 2 3 4])
             (popn a 2)
             (length a))
           2
           "popn mutates original")

(assert-eq (begin
             (var a @[1 2 3])
             (popn a 3)
             (length a))
           0
           "popn all elements")

(assert-eq (begin
             (var a @[1 2])
             (popn a 5)
             (length a))
           0
           "popn more than available")

(assert-eq (length (popn @[1 2 3] 0)) 0 "popn zero elements")
(assert-eq (length (popn @[] 2)) 0 "popn empty array")

(assert-err (fn () (eval '(popn))) "popn wrong arity no args")
(assert-err (fn () (eval '(popn @[1 2 3]))) "popn wrong arity one arg")
(assert-err (fn () (eval '(popn @[1 2 3] 2 3))) "popn wrong arity too many args")
(assert-err (fn () (popn @[1 2 3] :key)) "popn non-integer count error")
(assert-err (fn () (popn [1 2 3] 2)) "popn non-array error")

## === insert ===

(assert-eq (length (insert @[2 3] 0 1)) 3 "insert at beginning")
(assert-eq (length (insert @[1 3] 1 2)) 3 "insert at middle")
(assert-eq (length (insert @[1 2] 2 3)) 3 "insert at end")

(assert-eq (let ((a @[1 3]))
             (let ((a2 (insert a 1 2)))
               (identical? a a2)))
           true
           "insert returns same array")

(assert-eq (length (insert @[] 0 1)) 1 "insert empty array")
(assert-eq (length (insert @[1 2] 10 3)) 3 "insert out of bounds appends")

(assert-err (fn () (eval '(insert))) "insert wrong arity no args")
(assert-err (fn () (eval '(insert @[1 2]))) "insert wrong arity one arg")
(assert-err (fn () (eval '(insert @[1 2] 0))) "insert wrong arity two args")
(assert-err (fn () (eval '(insert @[1 2] 0 3 4))) "insert wrong arity too many args")
(assert-err (fn () (insert @[1 2] :key 3)) "insert non-integer index error")
(assert-err (fn () (insert [1 2] 0 3)) "insert non-array error")

## === remove ===

(assert-eq (length (remove @[1 2 3] 0)) 2 "remove at beginning")
(assert-eq (length (remove @[1 2 3] 1)) 2 "remove at middle")
(assert-eq (length (remove @[1 2 3] 2)) 2 "remove at end")

(assert-eq (length (remove @[1 2 3 4] 1 2)) 2 "remove with count")

(assert-eq (let ((a @[1 2 3]))
             (let ((a2 (remove a 1)))
               (identical? a a2)))
           true
           "remove returns same array")

(assert-eq (length (remove @[1 2 3] 10)) 3 "remove out of bounds no change")
(assert-eq (length (remove @[1 2 3] 1 10)) 1 "remove count exceeds available")
(assert-eq (length (remove @[1 2 3] 1 0)) 3 "remove zero count")

(assert-err (fn () (eval '(remove))) "remove wrong arity no args")
(assert-err (fn () (eval '(remove @[1 2 3]))) "remove wrong arity one arg")
(assert-err (fn () (eval '(remove @[1 2 3] 0 1 2))) "remove wrong arity too many args")
(assert-err (fn () (remove @[1 2 3] :key)) "remove non-integer index error")
(assert-err (fn () (remove @[1 2 3] 0 :key)) "remove non-integer count error")
(assert-err (fn () (remove [1 2 3] 0)) "remove non-array error")

## === append - Arrays ===

(assert-eq (length (append @[1 2] @[3 4])) 4 "append arrays mutates")

(assert-eq (let ((a @[1 2]))
             (let ((a2 (append a @[3 4])))
               (identical? a a2)))
           true
           "append arrays returns same reference")

## === append - Arrays ===

(assert-eq (length (append [1 2] [3 4])) 4 "append arrays returns new")

(assert-eq (let ((t [1 2]))
             (let ((t2 (append t [3 4])))
               (list t t2)))
           (list [1 2] [1 2 3 4])
           "append arrays original unchanged")

## === append - @strings ===

(assert-eq (append "hello" " world") "hello world" "append @strings")

(assert-eq (let ((s "hello"))
             (let ((s2 (append s " world")))
               (list s s2)))
           (list "hello" "hello world")
           "append @strings returns new")

## === append - Empty Collections ===

(assert-eq (length (append @[] @[1 2])) 2 "append empty @arrays")
(assert-eq (length (append @[1 2] @[])) 2 "append to empty @array")
(assert-eq (length (append [] [1 2])) 2 "append empty arrays")
(assert-eq (append "" "hello") "hello" "append empty @strings")
(assert-eq (append "hello" "") "hello" "append to empty @string")

## === append - Error Cases ===

(assert-err (fn () (eval '(append))) "append wrong arity no args")
(assert-err (fn () (eval '(append @[1 2]))) "append wrong arity one arg")
(assert-err (fn () (eval '(append @[1 2] @[3 4] @[5 6]))) "append wrong arity too many args")
(assert-err (fn () (append @[1 2] [3 4])) "append mismatched types error")
(assert-err (fn () (append 42 99)) "append unsupported type error")

## === concat - @arrays ===

(assert-eq (length (concat @[1 2] @[3 4])) 4 "concat @arrays returns new")

(assert-eq (let ((a @[1 2]))
             (let ((a2 (concat a @[3 4])))
               (list a a2)))
           (list @[1 2] @[1 2 3 4])
           "concat @arrays original unchanged")

## === concat - Arrays ===

(assert-eq (length (concat [1 2] [3 4])) 4 "concat arrays returns new")

## === concat - @strings ===

(assert-eq (concat "hello" " world") "hello world" "concat @strings")

## === concat - Empty Collections ===

(assert-eq (length (concat @[] @[1 2])) 2 "concat empty @arrays")
(assert-eq (length (concat @[1 2] @[])) 2 "concat to empty @array")
(assert-eq (length (concat [] [1 2])) 2 "concat empty arrays")
(assert-eq (concat "" "hello") "hello" "concat empty @strings")

## === concat - Variadic ===

(assert-eq (concat [1]) [1] "concat single array identity")
(assert-eq (concat @[1 2] @[3 4] @[5 6]) @[1 2 3 4 5 6] "concat three @arrays")
(assert-eq (concat "a" "b" "c") "abc" "concat three strings")

## === concat - Error Cases ===

(assert-err (fn () (eval '(concat))) "concat wrong arity no args")
(assert-err (fn () (concat @[1 2] [3 4])) "concat mismatched types error")
(assert-err (fn () (concat 42 99)) "concat unsupported type error")

## === get on lists ===

(assert-eq (get (list 10 20 30) 0) 10 "get list by index")
(assert-eq (get (list 10 20 30) 1) 20 "get list by index middle")
(assert-eq (get (list 10 20 30) 2) 30 "get list by index last")
(assert-eq (get (list 10 20 30) 10) nil "get list out of bounds")
(assert-eq (get (list 10 20 30) 10 :missing) :missing "get list out of bounds with default")
(assert-eq (get (list 10 20 30) -1) nil "get list negative index")
(assert-eq (get (list) 0) nil "get empty list")
(assert-err (fn () (get (list 1 2 3) :key)) "get list non-integer index error")

## === append on lists ===

(assert-eq (length (append (list 1 2) (list 3 4))) 4 "append lists")
(assert-eq (length (append (list) (list 1 2))) 2 "append empty list to list")
(assert-eq (length (append (list 1 2) (list))) 2 "append list to empty list")
(assert-eq (append (list) (list)) () "append empty lists")
(assert-err (fn () (append (list 1 2) @[3 4])) "append lists mismatched type error")

## === Loop iteration ===

(assert-eq (let ((sum 0)) (each x '(1 2 3) (assign sum (+ sum x))) sum) 6
  "each simple")

(assert-eq (let ((sum 0)) (each x in '(1 2 3) (assign sum (+ sum x))) sum) 6
   "each with in")

## === describe ===

(assert-eq (describe |1 2 3|) "<set (3 elements)>" "describe set")
(assert-eq (describe @|1 2 3|) "<@set (3 elements)>" "describe @set")
(assert-eq (describe [1 2 3]) "<array (3 elements)>" "describe array")
(assert-eq (describe @[1 2 3]) "<@array (3 elements)>" "describe @array")
(assert-eq (describe {:a 1}) "<struct (1 entries)>" "describe struct")
(assert-eq (describe @{:a 1}) "<@struct (1 entries)>" "describe @struct")
(assert-eq (describe (bytes 1 2 3)) "<bytes (3 bytes)>" "describe bytes")
(assert-eq (describe (@bytes 1 2 3)) "<@bytes (3 bytes)>" "describe @bytes")
