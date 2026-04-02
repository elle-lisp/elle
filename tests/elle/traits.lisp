(elle/epoch 7)
## Traits test suite
##
## Tests for the per-value trait table mechanism: `with-traits` and `traits`.
## These tests are written BEFORE the implementation (Chunk 2 of the plan).
## They WILL FAIL until Chunk 3 is complete — that is expected and correct.
## Issue #563: Traits (per-value dispatch tables).


# ============================================================================
# Basic attach and retrieve
# ============================================================================

# with-traits attaches a table; traits retrieves it
(begin
  (def tbl {:method (fn (x) x)})
  (def v (with-traits [1 2 3] tbl))
  (assert (= (traits v) tbl) "traits retrieves the attached table"))

# traits on an untraited value returns nil
(assert (= (traits [1 2 3]) nil) "traits returns nil for untraited array")

(assert (= (traits {:a 1}) nil) "traits returns nil for untraited struct")

(assert (= (traits "hello world") nil) "traits returns nil for untraited string")

(assert (= (traits 42) nil) "traits returns nil for integer (immediate)")

(assert (= (traits nil) nil) "traits returns nil for nil (immediate)")

(assert (= (traits :keyword) nil) "traits returns nil for keyword (immediate)")

(assert (= (traits true) nil) "traits returns nil for bool (immediate)")

# ============================================================================
# Falsy / truthy via traits
# ============================================================================

# nil is falsy — (traits untraited) is usable as false branch
(assert (not (traits [1 2 3])) "nil from traits is falsy")

(assert (not (traits "hello world")) "nil from traits on string is falsy")

# the retrieved table is an immutable struct
(assert (struct? (traits (with-traits [1 2] {:a 1}))) "traits result is a struct")

# a trait table (immutable struct) is truthy
(assert (traits (with-traits [1 2 3] {:x 1})) "a trait table is truthy")

(assert (traits (with-traits {:a 1} {:tag :foo})) "a trait table on struct is truthy")

# ============================================================================
# Data transparency — operations see data, not traits
# ============================================================================

(begin
  (def arr (with-traits [10 20 30] {:tag :my-type}))
  (assert (= (get arr 0) 10) "get sees array data through traits")
  (assert (= (length arr) 3) "length sees array data through traits")
  (assert (= (type-of arr) :array) "type-of returns :array, not influenced by traits"))

(begin
  (def s (with-traits {:a 1 :b 2} {:T true}))
  (assert (= (get s :a) 1) "get sees struct data through traits")
  (assert (= (length s) 2) "length sees struct data through traits")
  (assert (= (type-of s) :struct) "type-of returns :struct for traited struct"))

(begin
  (def lst (with-traits (cons 1 (cons 2 ())) {:tag :list}))
  (assert (= (first lst) 1) "first sees cons data through traits")
  (assert (= (type-of lst) :list) "type-of returns :list for traited cons"))

(assert (= (type-of (with-traits (fn (x) x) {:T true})) :closure) "type-of returns :closure for traited closure")

# ============================================================================
# Equality ignores trait tables
# ============================================================================

(begin
  (def tbl1 {:a 1})
  (def tbl2 {:b 2})
  (def x (with-traits [1 2 3] tbl1))
  (def y (with-traits [1 2 3] tbl2))
  # Same data, different trait tables — must be equal
  (assert (= x y) "arrays with different trait tables are equal if data is same")
  # Traited == untraited with same data
  (assert (= x [1 2 3]) "traited array equals untraited array with same data")
  # Symmetric: untraited == traited
  (assert (= [1 2 3] x) "untraited array equals traited array with same data (symmetric)"))

(begin
  (def a (with-traits {:k 1} {:T true}))
  (def b {:k 1})
  (assert (= a b) "traited struct equals untraited struct with same data"))

# ============================================================================
# Replacement — re-attaching replaces, does not merge
# ============================================================================

(begin
  (def t1 {:a 1})
  (def t2 {:b 2})
  (def v1 (with-traits [1 2 3] t1))
  (def v2 (with-traits v1 t2))
  (assert (= (traits v2) t2) "re-attaching replaces the trait table")
  (assert (= (get (traits v2) :a) nil) "old key not present after replacement"))

# ============================================================================
# Mutable sharing — with-traits on @array
# ============================================================================

# with-traits on a mutable type creates a new value with its own data copy.
# The data is independent from the original after construction.
# (The heap storage model uses RefCell<Vec<...>>, not Rc<RefCell<...>>,
# so cloning creates an independent copy, not a shared reference.)
(begin
  (def orig @[1 2 3])
  (def traited (with-traits orig {:tag :x}))
  # Both start with length 3
  (assert (= (length orig) 3) "original starts with 3 elements")
  (assert (= (length traited) 3) "traited copy starts with 3 elements")
  # The trait table is attached
  (assert (= (get (traits traited) :tag) :x) "trait table attached to mutable array copy"))

# Mutations to the original after with-traits do not affect the traited copy
(begin
  (def orig @[1 2 3])
  (def traited (with-traits orig {:tag :x}))
  (push orig 4)
  (assert (= (length traited) 3) "mutable array copy is independent: push to original does not affect traited copy"))

(begin
  (def orig @{:a 1})
  (def traited (with-traits orig {:tag :x}))
  (put orig :b 2)
  (assert (= (length traited) 1) "mutable struct copy is independent: put to original does not affect traited copy"))

# ============================================================================
# Constructor pattern — shared table, identical? fast path
# ============================================================================

# When all instances share the same Rc<struct>, identical? is true.
(begin
  (def shared-tbl {:type :my-type})
  (def make (fn (data) (with-traits @{:data data} shared-tbl)))
  (def a (make 1))
  (def b (make 2))
  (assert (identical? (traits a) (traits b)) "instances sharing a constructor table pass identical? check"))

# ============================================================================
# Independent constructors — structural equality, not identity
# ============================================================================

# Two separate allocations of the same struct structure are equal.
# In Elle, identical? checks strict value equality (no numeric coercion),
# not raw pointer identity. Two structs with the same content are identical?.
(begin
  (def make1 (fn (data) (with-traits @{:data data} {:type :t})))
  (def make2 (fn (data) (with-traits @{:data data} {:type :t})))
  (def a (make1 1))
  (def b (make2 1))
  (assert (= (traits a) (traits b)) "independently created tables with same structure are equal")
  # identical? uses value equality (= semantics) not pointer identity
  (assert (identical? (traits a) (traits b)) "independently created tables with same content are identical? (value equality)"))

# ============================================================================
# Private traits via gensym
# ============================================================================

# gensym keys are unique; two gensym calls produce distinct keys.
(begin
  (def k1 (gensym "Seq"))
  (def k2 (gensym "Seq"))
  (def tbl1 {k1 {:first (fn (v) (get v 0))}})
  (def tbl2 {k2 {:first (fn (v) (get v 0))}})
  (def v1 (with-traits [1 2] tbl1))
  (def v2 (with-traits [1 2] tbl2))
  # Tables are structurally different (different gensym keys)
  (assert (not (= (traits v1) (traits v2))) "gensym keys produce distinct trait tables")
  # But both values are data-equal
  (assert (= v1 v2) "values with distinct gensym trait tables are data-equal"))

# ============================================================================
# Composition via merge
# ============================================================================

(begin
  (def t-seq {:Seq {:first (fn (v) (get v :head))}})
  (def t-show {:Show {:show (fn (v) "it")}})
  (def combined (merge t-seq t-show))
  (def v (with-traits @{:head 1} combined))
  (assert (= (get (get (traits v) :Seq) :first) (get (get t-seq :Seq) :first)) "composition via merge: Seq.first preserved")
  (assert (= (get (get (traits v) :Show) :show) (get (get t-show :Show) :show)) "composition via merge: Show.show preserved"))

# ============================================================================
# Manual dispatch — extract operation from trait table and call it
# ============================================================================

(begin
  (def first-op (fn (x) ((get (get (traits x) :Seq) :first) x)))
  (def tbl {:Seq {:first (fn (v) (get v 0))}})
  (def v (with-traits [42 99] tbl))
  (assert (= (first-op v) 42) "manual dispatch: extract and call operation from trait table"))

(begin
  (def show-op (fn (x) ((get (get (traits x) :Show) :show) x)))
  (def struct-tbl {:Show {:show (fn (v) (get v :name))}})
  (def v (with-traits {:name "Alice"} struct-tbl))
  (assert (= (show-op v) "Alice") "manual dispatch on struct: extract and call show"))

# ============================================================================
# All 19 traitable types — with-traits succeeds on each
# ============================================================================

(begin
  (def t {:x 1})

  # LArray
  (assert (= (traits (with-traits [1 2 3] t)) t) "with-traits works on LArray")

  # LArrayMut
  (assert (= (traits (with-traits @[1 2 3] t)) t) "with-traits works on LArrayMut")

  # LStruct
  (assert (= (traits (with-traits {:a 1} t)) t) "with-traits works on LStruct")

  # LStructMut
  (assert (= (traits (with-traits @{:a 1} t)) t) "with-traits works on LStructMut")

  # LString — must be heap-allocated (>6 bytes to exceed inline threshold)
  (assert (= (traits (with-traits "hello world" t)) t) "with-traits works on LString (heap-allocated)")

  # LStringMut
  (assert (= (traits (with-traits @"hello" t)) t) "with-traits works on LStringMut")

  # LBytes
  (assert (= (traits (with-traits (bytes 1 2 3) t)) t) "with-traits works on LBytes")

  # LBytesMut
  (assert (= (traits (with-traits (@bytes 1 2 3) t)) t) "with-traits works on LBytesMut")

  # LSet
  (assert (= (traits (with-traits (set 1 2 3) t)) t) "with-traits works on LSet")

  # LSetMut
  (assert (= (traits (with-traits (@set 1 2 3) t)) t) "with-traits works on LSetMut")

  # Cons
  (assert (= (traits (with-traits (cons 1 2) t)) t) "with-traits works on Cons")

  # Closure
  (assert (= (traits (with-traits (fn (x) x) t)) t) "with-traits works on Closure")

  # LBox (user-accessible via `box`)
  (assert (= (traits (with-traits (box 1) t)) t) "with-traits works on LBox (box)")

  # Parameter
  (assert (= (traits (with-traits (make-parameter 0) t)) t) "with-traits works on Parameter")

  # Fiber (user-constructible via fiber/new)
  (assert (= (traits (with-traits (fiber/new || (fn () 1)) t)) t) "with-traits works on Fiber")

  # Syntax, ManagedPointer, External, ThreadHandle:
  # These types require FFI, plugins, or thread spawning to construct in Elle
  # scripts. They are not testable in pure Elle scripts.
)

# traits returns nil for untraited fiber
(assert (nil? (traits (fiber/new || (fn () 1)))) "traits returns nil for untraited fiber")

# traits returns nil for untraited parameter
(assert (nil? (traits (make-parameter 0))) "traits returns nil for untraited parameter")

# ============================================================================
# Validation errors
# ============================================================================

# Trait table must be an immutable struct — not a mutable struct
(let (([ok? err] (protect ((fn () (with-traits [1 2 3] @{:a 1})))))) (assert (not ok?) "with-traits rejects mutable struct as table") (assert (= (get err :error) :type-error) "with-traits rejects mutable struct as table"))

# Trait table must be a struct — not an array
(let (([ok? err] (protect ((fn () (with-traits [1 2 3] [1 2])))))) (assert (not ok?) "with-traits rejects array as table") (assert (= (get err :error) :type-error) "with-traits rejects array as table"))

# Trait table must be a struct — not a string
(let (([ok? err] (protect ((fn () (with-traits [1 2 3] "str")))))) (assert (not ok?) "with-traits rejects string as table") (assert (= (get err :error) :type-error) "with-traits rejects string as table"))

# Trait table must be a struct — not a keyword
(let (([ok? err] (protect ((fn () (with-traits [1 2 3] :tag)))))) (assert (not ok?) "with-traits rejects keyword as table") (assert (= (get err :error) :type-error) "with-traits rejects keyword as table"))

# Trait table must be a struct — not an integer
(let (([ok? err] (protect ((fn () (with-traits [1 2 3] 42)))))) (assert (not ok?) "with-traits rejects integer as table") (assert (= (get err :error) :type-error) "with-traits rejects integer as table"))

# Arity: with-traits requires exactly 2 arguments
# (compile-time arity errors are wrapped by eval, so use assert-err not assert-err-kind)
(let (([ok? _] (protect ((fn () (eval '(with-traits [1 2 3]))))))) (assert (not ok?) "with-traits arity error: too few args"))

(let (([ok? _] (protect ((fn () (eval '(with-traits [1 2 3] {:a 1} :extra))))))) (assert (not ok?) "with-traits arity error: too many args"))

# Arity: traits requires exactly 1 argument
(let (([ok? _] (protect ((fn () (eval '(traits))))))) (assert (not ok?) "traits arity error: zero args"))

(let (([ok? _] (protect ((fn () (eval '(traits [1] [2]))))))) (assert (not ok?) "traits arity error: two args"))

# Infrastructure types (NativeFn): with-traits should return a type error.
# NativeFn values are exposed as primitives; `+` is a NativeFn.
(let (([ok? err] (protect ((fn () (with-traits + {:a 1})))))) (assert (not ok?) "with-traits rejects NativeFn (infrastructure type)") (assert (= (get err :error) :type-error) "with-traits rejects NativeFn (infrastructure type)"))
