
## === Shebang Handling ===

(assert (= (+ 1 2) 3) "shebang with env elle")
(assert (= 42 42) "shebang short form")
(assert (= (+ 10 20) 30) "no shebang works normally")
(assert (= (let ((x 5)) (* x x)) 25) "shebang with complex expression")

## === Macros ===

(assert (= (begin
             (defmacro my-when (test body) `(if ,test ,body nil))
             (my-when true 42)) 42) "defmacro my-when true")

(assert (= (begin
             (defmacro my-when (test body) `(if ,test ,body nil))
             (my-when false 42)) nil) "defmacro my-when false")

(assert (= (begin
             (defmacro my-when (test body) `(if ,test ,body nil))
             (macro? my-when)) true) "macro? predicate after defining")

(assert (= (macro? +) false) "macro? predicate non-macro")

(assert (begin
               (defmacro my-when (test body) `(if ,test ,body nil))
               (expand-macro '(my-when true 42))) "expand-macro returns expanded form")

## === Qualified Symbol Access ===

(assert (= (let ((obj {:key 42}))
             obj:key) 42) "qualified symbol struct access")

(assert (= (let ((obj {:inner {:value 99}}))
             obj:inner:value) 99) "qualified symbol chained access")

(assert (= (let ((a {:b {:c {:d 7}}}))
             a:b:c:d) 7) "qualified symbol triple chain")

(assert (= (let ((obj {:add1 (fn (x) (+ x 1))}))
             (obj:add1 41)) 42) "qualified symbol in call position")

(assert (= (let ((obj {:inner {:double (fn (x) (* x 2))}}))
             (obj:inner:double 21)) 42) "qualified symbol nested call")

(assert (= (let ((obj {:a 1}))
             obj:missing) nil) "qualified symbol missing key returns nil")

(assert (= (read ":foo") :foo) "qualified symbol keyword not affected")
(assert (symbol? 'foo:bar) "qualified symbol quoted preserved")

(assert (= (let ((t @{:x 10}))
              t:x) 10) "qualified symbol with @struct")

(let (([ok? _] (protect ((fn () (eval 'unbound:foo)))))) (assert (not ok?) "qualified symbol unbound first segment"))

## === Tables and Structs ===

(assert (struct? (@struct)) "@struct creation empty")

(assert (= (let ((t (@struct)))
             (put t "key" 42)
             (get t "key")) 42) "@struct put and get")

(assert (struct? (struct)) "struct creation empty")

(assert (= (= (type-of (@struct)) :@struct) true) "type-of @struct")
(assert (= (= (type-of (struct)) :struct) true) "type-of struct")

(assert (= (let ((t (@struct "a" 1 "b" 2)))
             (+ (get t "a") (get t "b"))) 3) "@struct with string keys")

(assert (= (let ((s (struct "x" 10 "y" 20)))
             (+ (get s "x") (get s "y"))) 30) "struct with string keys")

(assert (= (let ((t (@struct "a" 1)))
             (has? t "a")) true) "@struct has? true")

(assert (= (let ((t (@struct "a" 1)))
             (has? t "b")) false) "@struct has? missing")

## === @struct Mutation ===

(assert (= (let ((t (@struct)))
             (put t "a" 1)
             (put t "a" 2)
             (get t "a")) 2) "@struct mutation")

(assert (= (let ((s (struct "x" 42)))
             (get s "x")) 42) "struct immutability")

(assert (= (let ((outer (@struct)))
             (put outer "inner" (@struct))
             (put (get outer "inner") "value" 42)
             (get (get outer "inner") "value")) 42) "nested @struct operations")

(assert (= (begin
             (defmacro add-one (x) `(+ ,x 1))
             (add-one 41)) 42) "defmacro with quasiquote")

(assert (= (-> 5 (+ 3) (* 2)) 16) "threading macro first")
(assert (= (->> 5 (+ 3) (* 2)) 16) "threading macro last")

(assert (= (let ((t (@struct "a" 1 "b" 2)))
             (length (keys t))) 2) "@struct keys")

(assert (= (let ((t (@struct "a" 1 "b" 2)))
             (length (values t))) 2) "@struct values")

(assert (= (let ((t (@struct "a" 1 "b" 2)))
             (del t "a")
             (has? t "a")) false) "@struct del")

(assert (= (let ((s (struct "x" 1)))
             (let ((s2 (put s "x" 2)))
               (list (get s "x") (get s2 "x")))) (list 1 2)) "struct put returns new")

(assert (= (let ((t (@struct)))
             (get t "missing" 42)) 42) "get @struct with default")

## === Let Binding Semantics ===

(assert (= (let ((x 10) (y 20)) (+ x y)) 30) "let parallel binding")

(assert (= (begin (var x 999) (let ((x 10) (y x)) y)) 999) "let parallel binding shadowing")

(assert (= (begin (var x 999) (let* ((x 10) (y x)) y)) 10) "let* sequential binding")

(assert (= (let ((x 42)) x) 42) "let body sees bindings")

(assert (= (let ((x 1)) (let ((x 2)) x)) 2) "let nested shadowing")

## === Polymorphic get - Arrays ===

(assert (= (get [1 2 3] 0) 1) "get array by index")
(assert (= (get [1 2 3] 1) 2) "get array by index middle")
(assert (= (get [1 2 3] 2) 3) "get array by index last")
(assert (= (get [1 2 3] 10) nil) "get array out of bounds")
(assert (= (get [1 2 3] 10 :missing) :missing) "get array out of bounds with default")
(assert (= (get [1 2 3] -1) 3) "get array negative index")
(assert (= (get [1 2 3] -1 :default) 3) "get array negative index with default")
(assert (= (get [] 0) nil) "get empty array")
(let (([ok? _] (protect ((fn () (get [1 2 3] :key)))))) (assert (not ok?) "get array non-integer index error"))

## === Polymorphic get - Arrays ===

(assert (= (get @[1 2 3] 0) 1) "get array by index")
(assert (= (get @[1 2 3] 1) 2) "get array by index middle")
(assert (= (get @[1 2 3] 2) 3) "get array by index last")
(assert (= (get @[1 2 3] 10) nil) "get array out of bounds")
(assert (= (get @[1 2 3] 10 :missing) :missing) "get array out of bounds with default")
(assert (= (get @[1 2 3] -1) 3) "get @array negative index")
(assert (= (get @[] 0) nil) "get empty array")
(let (([ok? _] (protect ((fn () (get @[1 2 3] :key)))))) (assert (not ok?) "get array non-integer index error"))

## === Polymorphic get - Strings ===

(assert (= (get "hello" 0) "h") "get string by char index")
(assert (= (get "hello" 1) "e") "get string by char index middle")
(assert (= (get "hello" 4) "o") "get string by char index last")
(assert (= (get "hello" 10) nil) "get string out of bounds")
(assert (= (get "hello" 10 :missing) :missing) "get string out of bounds with default")
(assert (= (get "hello" -1) "o") "get string negative index")
(assert (= (get "" 0) nil) "get empty string")
(let (([ok? _] (protect ((fn () (get "hello" :key)))))) (assert (not ok?) "get string non-integer index error"))
(assert (= (get "café" 3) "é") "get string unicode char")

## === Polymorphic get - Structs ===

(assert (= (get {:a 1} :a) 1) "get struct by keyword")
(assert (= (get {"x" 10} "x") 10) "get struct by string key")
(assert (= (get {:a 1} :b) nil) "get struct missing key")
(assert (= (get {:a 1} :b :missing) :missing) "get struct missing key with default")
(assert (= (get {} :a) nil) "get empty struct")

## === Polymorphic get - @structs ===

(assert (= (get @{:a 1} :a) 1) "get @struct by keyword")
(assert (= (get @{"x" 10} "x") 10) "get @struct by string key")
(assert (= (get @{:a 1} :b) nil) "get @struct missing key")
(assert (= (get @{:a 1} :b :missing) :missing) "get @struct missing key with default")
(assert (= (get @{} :a) nil) "get empty @struct")

## === get Error Cases ===

(let (([ok? _] (protect ((fn () (eval '(get))))))) (assert (not ok?) "get wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(get [1 2 3]))))))) (assert (not ok?) "get wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(get [1 2 3] 0 :default :extra))))))) (assert (not ok?) "get wrong arity too many args"))
(let (([ok? _] (protect ((fn () (get 42 0)))))) (assert (not ok?) "get unsupported type"))
(let (([ok? _] (protect ((fn () (get nil 0)))))) (assert (not ok?) "get nil type"))

## === put - Arrays ===

(assert (= (get (put [1 2 3] 0 99) 0) 99) "put array by index")
(assert (= (get (put [1 2 3] 1 99) 1) 99) "put array by index middle")
(assert (= (get (put [1 2 3] 2 99) 2) 99) "put array by index last")
(let (([ok? _] (protect ((fn () (put [1 2 3] 10 99)))))) (assert (not ok?) "put array out of bounds"))
(assert (= (put [1 2 3] -1 99) [1 2 99]) "put array negative index")

(assert (= (let ((t [1 2 3]))
             (let ((t2 (put t 0 99)))
               (list t t2))) (list [1 2 3] [99 2 3])) "put array immutable original unchanged")

(let (([ok? _] (protect ((fn () (put [1 2 3] :key 99)))))) (assert (not ok?) "put array non-integer index error"))
(let (([ok? _] (protect ((fn () (put [] 0 99)))))) (assert (not ok?) "put empty array errors"))

## === put - @arrays ===

(assert (= (get (put @[1 2 3] 0 99) 0) 99) "put @array by index")
(assert (= (get (put @[1 2 3] 1 99) 1) 99) "put @array by index middle")
(assert (= (get (put @[1 2 3] 2 99) 2) 99) "put @array by index last")
(let (([ok? _] (protect ((fn () (put @[1 2 3] 10 99)))))) (assert (not ok?) "put @array out of bounds"))
(assert (= (get (put @[1 2 3] -1 99) 2) 99) "put @array negative index")

(assert (= (let ((a @[1 2 3]))
             (let ((a2 (put a 0 99)))
               (identical? a a2))) true) "put @array mutable same reference")

(let (([ok? _] (protect ((fn () (put @[1 2 3] :key 99)))))) (assert (not ok?) "put @array non-integer index error"))
(let (([ok? _] (protect ((fn () (put @[] 0 99)))))) (assert (not ok?) "put empty @array errors"))

## === put - Strings ===

(assert (= (put "hello" 0 "a") "aello") "put string by char index")
(assert (= (put "hello" 1 "a") "hallo") "put string by char index middle")
(assert (= (put "hello" 4 "a") "hella") "put string by char index last")
(let (([ok? _] (protect ((fn () (put "hello" 10 "a")))))) (assert (not ok?) "put string out of bounds"))
(assert (= (put "hello" -1 "a") "hella") "put string negative index")

(assert (= (let ((s "hello"))
             (let ((s2 (put s 0 "a")))
               (list s s2))) (list "hello" "aello")) "put string immutable original unchanged")

(let (([ok? _] (protect ((fn () (put "hello" :key "a")))))) (assert (not ok?) "put string non-integer index error"))
(let (([ok? _] (protect ((fn () (put "" 0 "a")))))) (assert (not ok?) "put empty string errors"))
(assert (= (put "café" 3 "x") "cafx") "put string unicode char")

## === put - Structs ===

(assert (= (get (put {:a 1} :a 99) :a) 99) "put struct by keyword")
(assert (= (get (put {:a 1} :b 2) :a) 1) "put struct new key a")
(assert (= (get (put {:a 1} :b 2) :b) 2) "put struct new key b")

(assert (= (let ((s {:a 1}))
             (let ((s2 (put s :a 99)))
               (list (get s :a) (get s2 :a)))) (list 1 99)) "put struct immutable original unchanged")

(assert (= (get (put {} :a 1) :a) 1) "put empty struct")

## === put - @structs ===

(assert (= (get (put @{:a 1} :a 99) :a) 99) "put @struct by keyword")
(assert (= (get (put @{:a 1} :b 2) :a) 1) "put @struct new key a")
(assert (= (get (put @{:a 1} :b 2) :b) 2) "put @struct new key b")

(assert (= (let ((t @{:a 1}))
             (let ((t2 (put t :a 99)))
               (identical? t t2))) true) "put @struct mutable same reference")

(assert (= (get (put @{} :a 1) :a) 1) "put empty @struct")

## === put Error Cases ===

(let (([ok? _] (protect ((fn () (eval '(put))))))) (assert (not ok?) "put wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(put [1 2 3]))))))) (assert (not ok?) "put wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(put [1 2 3] 0))))))) (assert (not ok?) "put wrong arity two args"))
(let (([ok? _] (protect ((fn () (eval '(put [1 2 3] 0 99 :extra))))))) (assert (not ok?) "put wrong arity too many args"))
(let (([ok? _] (protect ((fn () (put 42 0 99)))))) (assert (not ok?) "put unsupported type"))
(let (([ok? _] (protect ((fn () (put nil 0 99)))))) (assert (not ok?) "put nil type"))

## === rebox ===

(assert (= (rebox (box 1) 42) 42) "rebox returns value")

(assert (= (begin
             (var b (box 1))
             (rebox b 2)
             (unbox b)) 2) "rebox updates cell")

(assert (= (begin
             (var b (box 1))
             (rebox b "hello")
             (unbox b)) "hello") "rebox with different types")

(assert (= (begin
             (var b (box 1))
             (rebox b nil)
             (unbox b)) nil) "rebox with nil")

(let (([ok? _] (protect ((fn () (eval '(rebox))))))) (assert (not ok?) "rebox wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(rebox (box 1)))))))) (assert (not ok?) "rebox wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(rebox (box 1) 2 3))))))) (assert (not ok?) "rebox wrong arity too many args"))
(let (([ok? _] (protect ((fn () (rebox 42 99)))))) (assert (not ok?) "rebox non-cell error"))

## === push ===

(assert (= (length (push @[1 2] 3)) 3) "push single element")

(assert (= (let ((a @[1 2]))
             (let ((a2 (push a 3)))
               (identical? a a2))) true) "push returns same array")

(assert (= (length (push @[] 1)) 1) "push empty array")

(assert (= (begin
             (var a @[])
             (push a 1)
             (push a 2)
             (push a 3)
             (length a)) 3) "push multiple times")

(let (([ok? _] (protect ((fn () (eval '(push))))))) (assert (not ok?) "push wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(push @[1 2]))))))) (assert (not ok?) "push wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(push @[1 2] 3 4))))))) (assert (not ok?) "push wrong arity too many args"))
(let (([ok? _] (protect ((fn () (push [1 2] 3)))))) (assert (not ok?) "push non-array error"))

## === pop ===

(assert (= (pop @[1 2 3]) 3) "pop single element")

(assert (= (begin
             (var a @[1 2 3])
             (pop a)
             (length a)) 2) "pop mutates array")

(let (([ok? _] (protect ((fn () (pop @[])))))) (assert (not ok?) "pop empty array errors"))

(assert (= (begin
             (var a @[42])
             (pop a)
             (length a)) 0) "pop single element array")

(let (([ok? _] (protect ((fn () (eval '(pop))))))) (assert (not ok?) "pop wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(pop @[1 2] 3))))))) (assert (not ok?) "pop wrong arity too many args"))
(let (([ok? _] (protect ((fn () (pop [1 2 3])))))) (assert (not ok?) "pop non-array error"))

## === popn ===

(assert (= (length (popn @[1 2 3 4] 2)) 2) "popn two elements")

(assert (= (begin
             (var a @[1 2 3 4])
             (popn a 2)
             (length a)) 2) "popn mutates original")

(assert (= (begin
             (var a @[1 2 3])
             (popn a 3)
             (length a)) 0) "popn all elements")

(assert (= (begin
             (var a @[1 2])
             (popn a 5)
             (length a)) 0) "popn more than available")

(assert (= (length (popn @[1 2 3] 0)) 0) "popn zero elements")
(assert (= (length (popn @[] 2)) 0) "popn empty array")

(let (([ok? _] (protect ((fn () (eval '(popn))))))) (assert (not ok?) "popn wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(popn @[1 2 3]))))))) (assert (not ok?) "popn wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(popn @[1 2 3] 2 3))))))) (assert (not ok?) "popn wrong arity too many args"))
(let (([ok? _] (protect ((fn () (popn @[1 2 3] :key)))))) (assert (not ok?) "popn non-integer count error"))
(let (([ok? _] (protect ((fn () (popn [1 2 3] 2)))))) (assert (not ok?) "popn non-array error"))

## === insert ===

(assert (= (length (insert @[2 3] 0 1)) 3) "insert at beginning")
(assert (= (length (insert @[1 3] 1 2)) 3) "insert at middle")
(assert (= (length (insert @[1 2] 2 3)) 3) "insert at end")

(assert (= (let ((a @[1 3]))
             (let ((a2 (insert a 1 2)))
               (identical? a a2))) true) "insert returns same array")

(assert (= (length (insert @[] 0 1)) 1) "insert empty array")
(assert (= (length (insert @[1 2] 10 3)) 3) "insert out of bounds appends")

(let (([ok? _] (protect ((fn () (eval '(insert))))))) (assert (not ok?) "insert wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(insert @[1 2]))))))) (assert (not ok?) "insert wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(insert @[1 2] 0))))))) (assert (not ok?) "insert wrong arity two args"))
(let (([ok? _] (protect ((fn () (eval '(insert @[1 2] 0 3 4))))))) (assert (not ok?) "insert wrong arity too many args"))
(let (([ok? _] (protect ((fn () (insert @[1 2] :key 3)))))) (assert (not ok?) "insert non-integer index error"))
(let (([ok? _] (protect ((fn () (insert [1 2] 0 3)))))) (assert (not ok?) "insert non-array error"))

## === remove ===

(assert (= (length (remove @[1 2 3] 0)) 2) "remove at beginning")
(assert (= (length (remove @[1 2 3] 1)) 2) "remove at middle")
(assert (= (length (remove @[1 2 3] 2)) 2) "remove at end")

(assert (= (length (remove @[1 2 3 4] 1 2)) 2) "remove with count")

(assert (= (let ((a @[1 2 3]))
             (let ((a2 (remove a 1)))
               (identical? a a2))) true) "remove returns same array")

(assert (= (length (remove @[1 2 3] 10)) 3) "remove out of bounds no change")
(assert (= (length (remove @[1 2 3] 1 10)) 1) "remove count exceeds available")
(assert (= (length (remove @[1 2 3] 1 0)) 3) "remove zero count")

(let (([ok? _] (protect ((fn () (eval '(remove))))))) (assert (not ok?) "remove wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(remove @[1 2 3]))))))) (assert (not ok?) "remove wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(remove @[1 2 3] 0 1 2))))))) (assert (not ok?) "remove wrong arity too many args"))
(let (([ok? _] (protect ((fn () (remove @[1 2 3] :key)))))) (assert (not ok?) "remove non-integer index error"))
(let (([ok? _] (protect ((fn () (remove @[1 2 3] 0 :key)))))) (assert (not ok?) "remove non-integer count error"))
(let (([ok? _] (protect ((fn () (remove [1 2 3] 0)))))) (assert (not ok?) "remove non-array error"))

## === append - Arrays ===

(assert (= (length (append @[1 2] @[3 4])) 4) "append arrays mutates")

(assert (= (let ((a @[1 2]))
             (let ((a2 (append a @[3 4])))
               (identical? a a2))) true) "append arrays returns same reference")

## === append - Arrays ===

(assert (= (length (append [1 2] [3 4])) 4) "append arrays returns new")

(assert (= (let ((t [1 2]))
             (let ((t2 (append t [3 4])))
               (list t t2))) (list [1 2] [1 2 3 4])) "append arrays original unchanged")

## === append - @strings ===

(assert (= (append "hello" " world") "hello world") "append @strings")

(assert (= (let ((s "hello"))
             (let ((s2 (append s " world")))
               (list s s2))) (list "hello" "hello world")) "append @strings returns new")

## === append - Empty Collections ===

(assert (= (length (append @[] @[1 2])) 2) "append empty @arrays")
(assert (= (length (append @[1 2] @[])) 2) "append to empty @array")
(assert (= (length (append [] [1 2])) 2) "append empty arrays")
(assert (= (append "" "hello") "hello") "append empty @strings")
(assert (= (append "hello" "") "hello") "append to empty @string")

## === append - Error Cases ===

(let (([ok? _] (protect ((fn () (eval '(append))))))) (assert (not ok?) "append wrong arity no args"))
(let (([ok? _] (protect ((fn () (eval '(append @[1 2]))))))) (assert (not ok?) "append wrong arity one arg"))
(let (([ok? _] (protect ((fn () (eval '(append @[1 2] @[3 4] @[5 6]))))))) (assert (not ok?) "append wrong arity too many args"))
(assert (= (append @[1 2] [3 4]) @[1 2 3 4]) "append @array + array (cross-mutability)")
(let (([ok? _] (protect ((fn () (append 42 99)))))) (assert (not ok?) "append unsupported type error"))

## === concat - @arrays ===

(assert (= (length (concat @[1 2] @[3 4])) 4) "concat @arrays returns new")

(assert (= (let ((a @[1 2]))
             (let ((a2 (concat a @[3 4])))
               (list a a2))) (list @[1 2] @[1 2 3 4])) "concat @arrays original unchanged")

## === concat - Arrays ===

(assert (= (length (concat [1 2] [3 4])) 4) "concat arrays returns new")

## === concat - @strings ===

(assert (= (concat "hello" " world") "hello world") "concat @strings")

## === concat - Empty Collections ===

(assert (= (length (concat @[] @[1 2])) 2) "concat empty @arrays")
(assert (= (length (concat @[1 2] @[])) 2) "concat to empty @array")
(assert (= (length (concat [] [1 2])) 2) "concat empty arrays")
(assert (= (concat "" "hello") "hello") "concat empty @strings")

## === concat - Variadic ===

(assert (= (concat [1]) [1]) "concat single array identity")
(assert (= (concat @[1 2] @[3 4] @[5 6]) @[1 2 3 4 5 6]) "concat three @arrays")
(assert (= (concat "a" "b" "c") "abc") "concat three strings")

## === concat - Error Cases ===

(let (([ok? _] (protect ((fn () (eval '(concat))))))) (assert (not ok?) "concat wrong arity no args"))
(let (([ok? _] (protect ((fn () (concat @[1 2] [3 4])))))) (assert (not ok?) "concat mismatched types error"))
(let (([ok? _] (protect ((fn () (concat 42 99)))))) (assert (not ok?) "concat unsupported type error"))

## === concat - bytes ===

(assert (= (concat (bytes 1 2) (bytes 3 4)) (bytes 1 2 3 4)) "concat bytes basic")
(assert (= (concat (bytes) (bytes 1 2)) (bytes 1 2)) "concat bytes empty left")
(assert (= (concat (bytes 1 2) (bytes)) (bytes 1 2)) "concat bytes empty right")
(assert (= (concat (bytes 1) (bytes 2) (bytes 3)) (bytes 1 2 3)) "concat bytes three")
(assert (= (concat (bytes 1)) (bytes 1)) "concat bytes single identity")
(assert (= (let ((a (bytes 1 2)))
             (let ((b (concat a (bytes 3 4))))
               (list (length a) (length b)))) (list 2 4)) "concat bytes non-destructive")

## === concat - @bytes ===

(assert (= (concat (@bytes 1 2) (@bytes 3 4)) (@bytes 1 2 3 4)) "concat @bytes basic")
(assert (= (concat (@bytes) (@bytes 1 2)) (@bytes 1 2)) "concat @bytes empty left")
(assert (= (concat (@bytes 1 2) (@bytes)) (@bytes 1 2)) "concat @bytes empty right")
(assert (= (concat (@bytes 1) (@bytes 2) (@bytes 3)) (@bytes 1 2 3)) "concat @bytes three")
(assert (= (concat (@bytes 1)) (@bytes 1)) "concat @bytes single identity")

## === concat - set ===

(assert (= (concat |1 2| |3 4|) |1 2 3 4|) "concat sets disjoint")
(assert (= (concat |1 2| |2 3|) |1 2 3|) "concat sets overlapping")
(assert (= (concat || |1 2|) |1 2|) "concat set empty left")
(assert (= (concat |1 2| ||) |1 2|) "concat set empty right")
(assert (= (concat |1| |2| |3|) |1 2 3|) "concat sets three")
(assert (= (concat |1|) |1|) "concat set single identity")

## === concat - @set ===

(assert (= (concat @|1 2| @|3 4|) @|1 2 3 4|) "concat @sets disjoint")
(assert (= (concat @|1 2| @|2 3|) @|1 2 3|) "concat @sets overlapping")
(assert (= (concat @|| @|1 2|) @|1 2|) "concat @set empty left")
(assert (= (concat @|1 2| @||) @|1 2|) "concat @set empty right")
(assert (= (concat @|1| @|2| @|3|) @|1 2 3|) "concat @sets three")
(assert (= (concat @|1|) @|1|) "concat @set single identity")

## === concat - struct ===

(assert (= (concat {:a 1} {:b 2}) {:a 1 :b 2}) "concat structs disjoint")
(assert (= (concat {:a 1 :b 2} {:b 3 :c 4}) {:a 1 :b 3 :c 4}) "concat structs right wins")
(assert (= (concat {} {:a 1}) {:a 1}) "concat struct empty left")
(assert (= (concat {:a 1} {}) {:a 1}) "concat struct empty right")
(assert (= (concat {:a 1} {:b 2} {:c 3}) {:a 1 :b 2 :c 3}) "concat structs three")
(assert (= (concat {:a 1} {:a 2} {:a 3}) {:a 3}) "concat structs three last wins")
(assert (= (concat {:a 1}) {:a 1}) "concat struct single identity")

## === concat - @struct ===

(assert (= (concat @{:a 1} @{:b 2}) @{:a 1 :b 2}) "concat @structs disjoint")
(assert (= (concat @{:a 1 :b 2} @{:b 3 :c 4}) @{:a 1 :b 3 :c 4}) "concat @structs right wins")
(assert (= (concat @{} @{:a 1}) @{:a 1}) "concat @struct empty left")
(assert (= (concat @{:a 1} @{}) @{:a 1}) "concat @struct empty right")
(assert (= (concat @{:a 1} @{:b 2} @{:c 3}) @{:a 1 :b 2 :c 3}) "concat @structs three")
(assert (= (concat @{:a 1} @{:a 2} @{:a 3}) @{:a 3}) "concat @structs three last wins")
(assert (= (concat @{:a 1}) @{:a 1}) "concat @struct single identity")

## === concat - new type mismatch errors ===

(let (([ok? _] (protect ((fn () (concat (bytes 1) [2])))))) (assert (not ok?) "concat bytes/array mismatch"))
(let (([ok? _] (protect ((fn () (concat |1| [1])))))) (assert (not ok?) "concat set/array mismatch"))
(let (([ok? _] (protect ((fn () (concat {:a 1} [1])))))) (assert (not ok?) "concat struct/array mismatch"))
(let (([ok? _] (protect ((fn () (concat (bytes 1) (@bytes 2))))))) (assert (not ok?) "concat bytes/@bytes mismatch"))
(let (([ok? _] (protect ((fn () (concat |1| @|1|)))))) (assert (not ok?) "concat set/@set mismatch"))
(let (([ok? _] (protect ((fn () (concat {:a 1} @{:a 1})))))) (assert (not ok?) "concat struct/@struct mismatch"))

## === merge - mutability matching ===

(assert (= (merge {:a 1} {:b 2}) {:a 1 :b 2}) "merge: immutable + immutable")
(assert (= (merge {:a 1 :b 2} {:b 3 :c 4}) {:a 1 :b 3 :c 4}) "merge: immutable right overrides left")
(assert (= (merge @{:a 1} @{:b 2}) @{:a 1 :b 2}) "merge: mutable + mutable")
(assert (= (merge @{:a 1 :b 2} @{:b 3 :c 4}) @{:a 1 :b 3 :c 4}) "merge: mutable right overrides left")
(assert (= (merge {} {:a 1}) {:a 1}) "merge: empty immutable left")
(assert (= (merge {:a 1} {}) {:a 1}) "merge: empty immutable right")
(assert (= (merge @{} @{:a 1}) @{:a 1}) "merge: empty mutable left")
(assert (= (merge @{:a 1} @{}) @{:a 1}) "merge: empty mutable right")

## merge - mutability mismatch errors
(let (([ok? err] (protect ((fn () (merge {:a 1} @{:b 2})))))) (assert (not ok?) "merge: struct/@struct mismatch") (assert (= (get err :error) :type-error) "merge: struct/@struct mismatch"))
(let (([ok? err] (protect ((fn () (merge @{:a 1} {:b 2})))))) (assert (not ok?) "merge: @struct/struct mismatch") (assert (= (get err :error) :type-error) "merge: @struct/struct mismatch"))

## merge - non-struct errors
(let (([ok? err] (protect ((fn () (merge 42 {:a 1})))))) (assert (not ok?) "merge: non-struct first arg") (assert (= (get err :error) :type-error) "merge: non-struct first arg"))
(let (([ok? err] (protect ((fn () (merge {:a 1} 42)))))) (assert (not ok?) "merge: non-struct second arg") (assert (= (get err :error) :type-error) "merge: non-struct second arg"))
(let (([ok? err] (protect ((fn () (merge {:a 1} [1 2])))))) (assert (not ok?) "merge: array second arg") (assert (= (get err :error) :type-error) "merge: array second arg"))

## === get on lists ===

(assert (= (get (list 10 20 30) 0) 10) "get list by index")
(assert (= (get (list 10 20 30) 1) 20) "get list by index middle")
(assert (= (get (list 10 20 30) 2) 30) "get list by index last")
(assert (= (get (list 10 20 30) 10) nil) "get list out of bounds")
(assert (= (get (list 10 20 30) 10 :missing) :missing) "get list out of bounds with default")
(assert (= (get (list 10 20 30) -1) 30) "get list negative index")
(assert (= (get (list) 0) nil) "get empty list")
(let (([ok? _] (protect ((fn () (get (list 1 2 3) :key)))))) (assert (not ok?) "get list non-integer index error"))

## === append on lists ===

(assert (= (length (append (list 1 2) (list 3 4))) 4) "append lists")
(assert (= (length (append (list) (list 1 2))) 2) "append empty list to list")
(assert (= (length (append (list 1 2) (list))) 2) "append list to empty list")
(assert (= (append (list) (list)) ()) "append empty lists")
(let (([ok? _] (protect ((fn () (append (list 1 2) @[3 4])))))) (assert (not ok?) "append lists mismatched type error"))

## === Loop iteration ===

(assert (= (let ((@sum 0)) (each x '(1 2 3) (assign sum (+ sum x))) sum) 6) "each simple")

(assert (= (let ((@sum 0)) (each x in '(1 2 3) (assign sum (+ sum x))) sum) 6) "each with in")

## === describe ===

(assert (= (describe |1 2 3|) "<set (3 elements)>") "describe set")
(assert (= (describe @|1 2 3|) "<@set (3 elements)>") "describe @set")
(assert (= (describe [1 2 3]) "<array (3 elements)>") "describe array")
(assert (= (describe @[1 2 3]) "<@array (3 elements)>") "describe @array")
(assert (= (describe {:a 1}) "<struct (1 entries)>") "describe struct")
(assert (= (describe @{:a 1}) "<@struct (1 entries)>") "describe @struct")
(assert (= (describe (bytes 1 2 3)) "<bytes (3 bytes)>") "describe bytes")
(assert (= (describe (@bytes 1 2 3)) "<@bytes (3 bytes)>") "describe @bytes")
