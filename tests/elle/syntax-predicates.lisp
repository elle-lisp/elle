(elle/epoch 7)
# Syntax predicate and accessor integration tests (issue #581)
#
# These tests exercise syntax-pair?, syntax-list?, syntax-symbol?,
# syntax-keyword?, syntax-nil?, syntax->list, syntax-first, syntax-rest,
# and syntax-e at runtime using datum->syntax to construct syntax objects.
#
# datum->syntax is the only way to produce syntax objects in non-macro
# runtime code; it is available as a first-class primitive.


# ============================================================================
# Helpers: build syntax objects at runtime via datum->syntax
# ============================================================================

# datum->syntax takes (context datum); use nil as context for synthetic span/scopes.
(def syn-int   (datum->syntax nil 42))
(def syn-bool  (datum->syntax nil true))
(def syn-str   (datum->syntax nil "hello"))
(def syn-sym   (datum->syntax nil 'foo))
(def syn-kw    (datum->syntax nil :bar))
(def syn-nil   (datum->syntax nil nil))
(def syn-list1 (datum->syntax nil (list 1)))
(def syn-list2 (datum->syntax nil (list 1 2)))
(def syn-empty (datum->syntax nil ()))

# ============================================================================
# syntax-keyword? — needs a syntax-wrapped keyword (not reachable via macro arg)
# ============================================================================

(assert (syntax-keyword? syn-kw) "syntax-keyword? true on syntax keyword")
(assert (not (syntax-keyword? syn-int)) "syntax-keyword? false on syntax int")
(assert (not (syntax-keyword? :bar)) "syntax-keyword? false on plain keyword")
(assert (not (syntax-keyword? 42)) "syntax-keyword? false on plain int")

# ============================================================================
# syntax-nil? — needs a syntax-wrapped nil (not reachable via macro arg)
# ============================================================================

(assert (syntax-nil? syn-nil) "syntax-nil? true on syntax nil")
(assert (not (syntax-nil? syn-int)) "syntax-nil? false on syntax int")
(assert (not (syntax-nil? nil)) "syntax-nil? false on plain nil")
(assert (not (syntax-nil? 0)) "syntax-nil? false on plain int 0")

# ============================================================================
# syntax->list — runtime callable
# ============================================================================

# Success: syntax list with one element → array of length 1 whose element is syntax
(let [result (syntax->list syn-list1)]
  (assert (= (length result) 1) "syntax->list: length 1 list")
  (assert (not (nil? (first result))) "syntax->list: element not nil"))

# Success: empty syntax list → empty array
(let [result (syntax->list syn-empty)]
  (assert (= (length result) 0) "syntax->list: empty list → empty array"))

# Error: non-syntax argument
(let [[ok? _] (protect ((fn () (syntax->list 42))))] (assert (not ok?) "syntax->list: non-syntax errors"))

# Error: syntax wrapping a non-list (e.g. an int)
(let [[ok? _] (protect ((fn () (syntax->list syn-int))))] (assert (not ok?) "syntax->list: syntax non-list errors"))

# ============================================================================
# syntax-first — runtime callable
# ============================================================================

# Success: first element of a 2-element syntax list
(let [elem (syntax-first syn-list2)]
  (assert (= (syntax-e elem) 1) "syntax-first: returns first element"))

# Error: empty syntax list
(let [[ok? _] (protect ((fn () (syntax-first syn-empty))))] (assert (not ok?) "syntax-first: empty list errors"))

# Error: syntax wrapping a non-list
(let [[ok? _] (protect ((fn () (syntax-first syn-int))))] (assert (not ok?) "syntax-first: non-list errors"))

# Error: plain non-syntax value
(let [[ok? _] (protect ((fn () (syntax-first 42))))] (assert (not ok?) "syntax-first: non-syntax errors"))

# ============================================================================
# syntax-rest — runtime callable
# ============================================================================

# Success: rest of a 2-element list → syntax list of length 1
(let [tail (syntax-rest syn-list2)]
  (let [items (syntax->list tail)]
    (assert (= (length items) 1) "syntax-rest: rest has 1 element")
    (assert (= (syntax-e (first items)) 2) "syntax-rest: rest element is 2")))

# Error: empty syntax list
(let [[ok? _] (protect ((fn () (syntax-rest syn-empty))))] (assert (not ok?) "syntax-rest: empty list errors"))

# Error: syntax wrapping a non-list
(let [[ok? _] (protect ((fn () (syntax-rest syn-int))))] (assert (not ok?) "syntax-rest: non-list errors"))

# Error: plain non-syntax value
(let [[ok? _] (protect ((fn () (syntax-rest 42))))] (assert (not ok?) "syntax-rest: non-syntax errors"))

# ============================================================================
# syntax-e — runtime callable
# ============================================================================

# Atoms: unwrap to plain value
(assert (= (syntax-e syn-int) 42) "syntax-e: int unwraps")
(assert (= (syntax-e syn-bool) true) "syntax-e: bool unwraps")
(assert (= (syntax-e syn-nil) nil) "syntax-e: nil unwraps")
(assert (= (syntax-e syn-str) "hello") "syntax-e: string unwraps")

# Compound: returns the syntax object unchanged (still a syntax?)
# syntax-e on a list returns the syntax object as-is
(let [result (syntax-e syn-list1)]
  (assert (not (nil? result)) "syntax-e: compound returns non-nil"))

# Error: non-syntax argument
(let [[ok? _] (protect ((fn () (syntax-e 42))))] (assert (not ok?) "syntax-e: non-syntax errors"))
(let [[ok? _] (protect ((fn () (syntax-e :foo))))] (assert (not ok?) "syntax-e: plain keyword errors"))
