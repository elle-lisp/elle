(elle/epoch 19)
# audited: 2026-11-24
# The syntax predicates and accessors answer for syntax objects, and every macro argument is a syntax object, an atom included.
# docs/macros/syntax.md

# ── Syntax objects built at run time ─────────────────────────────────

# datum->syntax takes a context and a datum. A nil context gives the
# object no span and no scopes.
(def syn-int (syntax/<-datum nil 42))
(def syn-bool (syntax/<-datum nil true))
(def syn-str (syntax/<-datum nil "hello"))
(def syn-sym (syntax/<-datum nil 'foo))
(def syn-kw (syntax/<-datum nil :bar))
(def syn-nil (syntax/<-datum nil nil))
(def syn-list1 (syntax/<-datum nil (list 1)))
(def syn-list2 (syntax/<-datum nil (list 1 2)))
(def syn-empty (syntax/<-datum nil ()))

# ── syntax-keyword? ──────────────────────────────────────────────────

(assert (syntax/keyword? syn-kw) "syntax-keyword? true on syntax keyword")
(assert (not (syntax/keyword? syn-int)) "syntax-keyword? false on syntax int")
(assert (not (syntax/keyword? :bar)) "syntax-keyword? false on plain keyword")
(assert (not (syntax/keyword? 42)) "syntax-keyword? false on plain int")

# ── syntax-nil? ──────────────────────────────────────────────────────

(assert (syntax/nil? syn-nil) "syntax-nil? true on syntax nil")
(assert (not (syntax/nil? syn-int)) "syntax-nil? false on syntax int")
(assert (not (syntax/nil? nil)) "syntax-nil? false on plain nil")
(assert (not (syntax/nil? 0)) "syntax-nil? false on plain int 0")

# ── syntax->list ──

# Success: syntax list with one element → array of length 1 whose element is syntax
(let [result (syntax/->list syn-list1)]
  (assert (= (length result) 1) "syntax->list: length 1 list")
  (assert (not (nil? (first result))) "syntax->list: element not nil"))

# Success: empty syntax list → empty array
(let [result (syntax/->list syn-empty)]
  (assert (= (length result) 0) "syntax->list: empty list → empty array"))

# Error: non-syntax argument
(let [[ok? _] (protect ((fn () (syntax/->list 42))))]
  (assert (not ok?) "syntax->list: non-syntax errors"))

# Error: syntax wrapping a non-list (e.g. an int)
(let [[ok? _] (protect ((fn () (syntax/->list syn-int))))]
  (assert (not ok?) "syntax->list: syntax non-list errors"))

# ── syntax-first ──

# Success: first element of a 2-element syntax list
(let [elem (syntax/first syn-list2)]
  (assert (= (syntax/e elem) 1) "syntax-first: returns first element"))

# Error: empty syntax list
(let [[ok? _] (protect ((fn () (syntax/first syn-empty))))]
  (assert (not ok?) "syntax-first: empty list errors"))

# Error: syntax wrapping a non-list
(let [[ok? _] (protect ((fn () (syntax/first syn-int))))]
  (assert (not ok?) "syntax-first: non-list errors"))

# Error: plain non-syntax value
(let [[ok? _] (protect ((fn () (syntax/first 42))))]
  (assert (not ok?) "syntax-first: non-syntax errors"))

# ── syntax-rest ──

# Success: rest of a 2-element list → syntax list of length 1
(let [tail (syntax/rest syn-list2)]
  (let [items (syntax/->list tail)]
    (assert (= (length items) 1) "syntax-rest: rest has 1 element")
    (assert (= (syntax/e (first items)) 2) "syntax-rest: rest element is 2")))

# Error: empty syntax list
(let [[ok? _] (protect ((fn () (syntax/rest syn-empty))))]
  (assert (not ok?) "syntax-rest: empty list errors"))

# Error: syntax wrapping a non-list
(let [[ok? _] (protect ((fn () (syntax/rest syn-int))))]
  (assert (not ok?) "syntax-rest: non-list errors"))

# Error: plain non-syntax value
(let [[ok? _] (protect ((fn () (syntax/rest 42))))]
  (assert (not ok?) "syntax-rest: non-syntax errors"))

# ── syntax-e ──

# Atoms: unwrap to plain value
(assert (= (syntax/e syn-int) 42) "syntax-e: int unwraps")
(assert (= (syntax/e syn-bool) true) "syntax-e: bool unwraps")
(assert (= (syntax/e syn-nil) nil) "syntax-e: nil unwraps")
(assert (= (syntax/e syn-str) "hello") "syntax-e: string unwraps")

# A compound syntax object answers itself.
(let [result (syntax/e syn-list1)]
  (assert (not (nil? result)) "syntax-e: compound returns non-nil"))

# Error: non-syntax argument
(let [[ok? _] (protect ((fn () (syntax/e 42))))]
  (assert (not ok?) "syntax-e: non-syntax errors"))
(let [[ok? _] (protect ((fn () (syntax/e :foo))))]
  (assert (not ok?) "syntax-e: plain keyword errors"))

# ── Macro arguments ──────────────────────────────────────────────────

# Every argument arrives at a macro as a syntax object: a symbol, a compound
# form, and an atom (nil, a boolean, a number, a string or a keyword).
# The trap: a false or nil argument is a syntax object, and so truthy. A body
# that tests the literal unwraps it with syntax-e.

(defmacro arg-type [x]
  (type-of x))
(assert (= (arg-type 42) :syntax) "an int argument is a syntax object")
(assert (= (arg-type "s") :syntax) "a string argument is a syntax object")
(assert (= (arg-type :k) :syntax) "a keyword argument is a syntax object")

(defmacro truthy? [x]
  (if x true false))
(assert (truthy? false) "a false argument is truthy")
(assert (truthy? nil) "a nil argument is truthy")

(defmacro unwrapped-truthy? [x]
  (if (syntax/e x) true false))
(assert (not (unwrapped-truthy? false)) "an unwrapped false argument is falsy")
(assert (not (unwrapped-truthy? nil)) "an unwrapped nil argument is falsy")
(assert (unwrapped-truthy? 0) "an unwrapped 0 argument is truthy")

(defmacro left-out? [&opt x]
  (nil? x))
(assert (left-out?) "an &opt parameter the call leaves out is plain nil")
(assert (not (left-out? nil)) "an &opt parameter the call gives is syntax")

(defmacro test-pair? [x]
  (syntax/pair? x))
(assert (test-pair? (a b c)) "syntax-pair? on a list argument")
(assert (not (test-pair? ())) "syntax-pair? on an empty list argument")
(assert (not (test-pair? 42)) "syntax-pair? on an int argument")

(defmacro test-list? [x]
  (syntax/list? x))
(assert (test-list? (a b)) "syntax-list? on a list argument")
(assert (test-list? ()) "syntax-list? on an empty list argument")
(assert (not (test-list? 42)) "syntax-list? on an int argument")

(defmacro test-sym? [x]
  (syntax/symbol? x))
(assert (test-sym? foo) "syntax-symbol? on a symbol argument")
(assert (not (test-sym? 42)) "syntax-symbol? on an int argument")
(assert (not (test-sym? :kw)) "syntax-symbol? on a keyword argument")

(defmacro test-kw? [x]
  (syntax/keyword? x))
(assert (test-kw? :foo) "syntax-keyword? on a keyword argument")
(assert (not (test-kw? foo)) "syntax-keyword? on a symbol argument")

(defmacro test-nil? [x]
  (syntax/nil? x))
(assert (test-nil? nil) "syntax-nil? on a nil argument")
(assert (not (test-nil? 42)) "syntax-nil? on an int argument")

# ── syntax->datum ────────────────────────────────────────────────────

(defmacro get-datum [x]
  (syntax/->datum x))
(assert (= (get-datum 42) 42) "syntax->datum strips a macro argument")
(assert (= (syntax/->datum 42) 42) "syntax->datum answers a plain value as is")
