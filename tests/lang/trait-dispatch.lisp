(elle/epoch 12)
## Trait-based dispatch test suite
##
## Tests for dispatching first/rest/length/empty? through trait tables.

# ============================================================================
# Default traitsets are stamped at allocation
# ============================================================================

# Every array has a traitset (not nil)
(assert (not (nil? (traits [1 2 3]))) "array has default traitset")

# Every list has a traitset
(assert (not (nil? (traits (pair 1 ())))) "list has default traitset")

# Every string has a traitset
(assert (not (nil? (traits "hello world"))) "string has default traitset")

# Every set has a traitset
(assert (not (nil? (traits (set 1 2 3)))) "set has default traitset")

# Every struct has a traitset
(assert (not (nil? (traits {:a 1}))) "struct has default traitset")

# Mutable variants too
(assert (not (nil? (traits @[1 2 3]))) "@array has default traitset")

(assert (not (nil? (traits @"hello"))) "@string has default traitset")

(assert (not (nil? (traits (@set 1 2)))) "@set has default traitset")

(assert (not (nil? (traits @{:a 1}))) "@struct has default traitset")

# ============================================================================
# Default traitsets are shared (identical?)
# ============================================================================

# Two arrays share the same traitset pointer
(assert (identical? (traits [1 2 3]) (traits [4 5 6]))
        "arrays share the same default traitset")

# Two lists share the same traitset pointer
(assert (identical? (traits (pair 1 ())) (traits (pair 2 ())))
        "lists share the same default traitset")

# ============================================================================
# Default traitset contains :Sequence and :Collection protocols
# ============================================================================

(begin
  (def t (traits [1 2 3]))
  (assert (not (nil? (t :Sequence))) "array traitset has :Sequence protocol")
  (assert (not (nil? (t :Collection))) "array traitset has :Collection protocol"))

(begin
  (def t (traits (set 1 2 3)))
  (assert (nil? (t :Sequence)) "set traitset has NO :Sequence protocol")
  (assert (not (nil? (t :Collection))) "set traitset has :Collection protocol"))

(begin
  (def t (traits {:a 1}))
  (assert (nil? (t :Sequence)) "struct traitset has NO :Sequence protocol")
  (assert (not (nil? (t :Collection)))
          "struct traitset has :Collection protocol"))

# ============================================================================
# :Sequence method struct has expected keys
# ============================================================================

(begin
  (def seq-methods ((traits [1 2 3]) :Sequence))
  (assert (not (nil? (seq-methods :first))) ":Sequence has :first")
  (assert (not (nil? (seq-methods :rest))) ":Sequence has :rest")
  (assert (not (nil? (seq-methods :last))) ":Sequence has :last")
  (assert (not (nil? (seq-methods :nth))) ":Sequence has :nth")
  (assert (not (nil? (seq-methods :iter))) ":Sequence has :iter"))

# ============================================================================
# :Collection method struct has expected keys
# ============================================================================

(begin
  (def coll-methods ((traits [1 2 3]) :Collection))
  (assert (not (nil? (coll-methods :length))) ":Collection has :length")
  (assert (not (nil? (coll-methods :empty?))) ":Collection has :empty?")
  (assert (not (nil? (coll-methods :has?))) ":Collection has :has?")
  (assert (not (nil? (coll-methods :conj))) ":Collection has :conj")
  (assert (not (nil? (coll-methods :empty))) ":Collection has :empty"))

# ============================================================================
# Primitives dispatch through trait methods
# ============================================================================

# first dispatches through :Sequence :first
(assert (= (first [10 20 30]) 10) "first dispatches on array")
(assert (= (first (pair 10 (pair 20 ()))) 10) "first dispatches on list")
(assert (= (first "abc") "a") "first dispatches on string")

# rest dispatches through :Sequence :rest
(assert (= (rest [10 20 30]) [20 30]) "rest dispatches on array")
(assert (= (rest (pair 10 (pair 20 ()))) (pair 20 ())) "rest dispatches on list")

# length dispatches through :Collection :length
(assert (= (length [1 2 3]) 3) "length dispatches on array")
(assert (= (length "hello") 5) "length dispatches on string")
(assert (= (length (set 1 2 3)) 3) "length dispatches on set")
(assert (= (length {:a 1 :b 2}) 2) "length dispatches on struct")

# empty? dispatches through :Collection :empty?
(assert (empty? []) "empty? on empty array")
(assert (empty? ()) "empty? on empty list")
(assert (not (empty? [1])) "empty? on nonempty array")

# ============================================================================
# with-traits accepts @struct (mutable shell)
# ============================================================================

(begin
  (def tbl @{:Sequence {:first (fn [self] :custom)}})
  (def v (with-traits [1 2 3] tbl))
  (assert (= (traits v) tbl) "with-traits accepts @struct as trait table"))

# ============================================================================
# Per-instance override changes dispatch
# ============================================================================

(begin
  (def custom-first (fn [self] :overridden))
  (def custom-seq
    {:first custom-first
     :rest (fn [self] [])
     :iter (fn [self] (fiber/new (fn [] (yield :overridden)) |:yield|))})
  (def v (with-traits [1 2 3] @{:Sequence custom-seq}))
  (assert (= (first v) :overridden)
          "per-instance override: first dispatches to custom method"))

# ============================================================================
# Mutating a shared default propagates to all instances
# ============================================================================
# Traitsets are shared @struct allocations (one per type, held by the trait
# registry) shared by all instances of a type. Mutating the shared traitset is
# visible through all instances.

(begin
  (def a [1 2 3])
  (def b [4 5 6])
  (def shared (traits a))
  (assert (identical? shared (traits b)) "sanity: arrays share traitset")
  (assert (not (nil? (shared :Sequence))) "shared default has :Sequence")  # Mutate the shared traitset — add a custom protocol
  (put shared :Custom {:greet (fn [self] :hello)})  # Verify it's visible on both instances
  (assert (not (nil? ((traits a) :Custom))) "mutation visible on a")
  (assert (not (nil? ((traits b) :Custom))) "mutation visible on b")
  (assert (identical? ((traits a) :Custom) ((traits b) :Custom))
          "mutation propagates identically")  # Clean up: remove the test protocol so it doesn't affect later tests
  (del shared :Custom))

# ============================================================================
# Fiber-based iterator protocol
# ============================================================================

(begin
  (def arr [10 20 30])
  (def iter-fn (((traits arr) :Sequence) :iter))
  (def fib (iter-fn arr))
  (assert (= (fiber/status fib) :paused) "iterator fiber starts paused")
  (assert (= (fiber/resume fib) 10) "iterator yields first element")
  (assert (= (fiber/resume fib) 20) "iterator yields second element")
  (assert (= (fiber/resume fib) 30) "iterator yields third element")
  (fiber/resume fib)  # fiber completes
  (assert (= (fiber/status fib) :dead) "iterator fiber is dead after exhaustion"))

# List iterator
(begin
  (def lst (pair 1 (pair 2 (pair 3 ()))))
  (def iter-fn (((traits lst) :Sequence) :iter))
  (def fib (iter-fn lst))
  (assert (= (fiber/resume fib) 1) "list iter yields 1")
  (assert (= (fiber/resume fib) 2) "list iter yields 2")
  (assert (= (fiber/resume fib) 3) "list iter yields 3")
  (fiber/resume fib)
  (assert (= (fiber/status fib) :dead) "list iter exhausted"))

# ============================================================================
# Custom Sequence on a struct (user-defined type)
# ============================================================================

(begin
  (def make-range
    (fn [start end]
      (with-traits {:start start :end end}
                   @{:Sequence {:first (fn [self] (self :start))
                                :rest (fn [self]
                                        (if (>= (+ (self :start) 1) (self :end))
                                          ()
                                          (make-range (+ (self :start) 1)
                                          (self :end))))
                                :empty? (fn [self]
                                          (>= (self :start) (self :end)))
                                :iter (fn [self]
                                        (fiber/new (fn []
                                          (def @i (self :start))
                                          (while (< i (self :end))
                                            (yield i)
                                            (assign i (+ i 1)))) |:yield|))}})))
  (def r (make-range 0 5))
  (assert (= (first r) 0) "custom seq: first")
  (assert (= (first (rest r)) 1) "custom seq: first of rest")

  # Iterator — user fiber starts :new, transitions to :paused on yield
  (def iter-fn (((traits r) :Sequence) :iter))
  (def fib (iter-fn r))
  (def acc @[])  # Collect yielded values: resume and push while fiber stays paused
  (def @done false)
  (while (not done)
    (def v (fiber/resume fib))
    (if (= (fiber/status fib) :dead) (assign done true) (push acc v)))
  (assert (= (freeze acc) [0 1 2 3 4])
          "custom seq: iterator collects all elements"))

# ============================================================================
# :Collection :conj
# ============================================================================

(begin
  (def coll-methods ((traits [1 2]) :Collection))
  (def conj-fn (coll-methods :conj))
  (assert (= (conj-fn [1 2] 3) [1 2 3]) "conj appends to array"))

(begin
  (def coll-methods ((traits (pair 1 ())) :Collection))
  (def conj-fn (coll-methods :conj))
  (assert (= (first (conj-fn (pair 2 ()) 1)) 1) "conj prepends to list"))

(begin
  (def coll-methods ((traits (set 1 2)) :Collection))
  (def conj-fn (coll-methods :conj))
  (assert (has? (conj-fn (set 1 2) 3) 3) "conj adds to set"))

# ============================================================================
# :Collection :empty
# ============================================================================

(begin
  (def empty-fn (((traits [1 2 3]) :Collection) :empty))
  (def e (empty-fn [1 2 3]))
  (assert (array? e) "empty of array is array")
  (assert (empty? e) "empty of array is empty"))

(begin
  (def empty-fn (((traits (set 1 2)) :Collection) :empty))
  (def e (empty-fn (set 1 2)))
  (assert (set? e) "empty of set is set")
  (assert (empty? e) "empty of set is empty"))

# ============================================================================
# Immediates have no traitset (unchanged)
# ============================================================================

(assert (nil? (traits 42)) "integer has no traits")
(assert (nil? (traits nil)) "nil has no traits")
(assert (nil? (traits true)) "bool has no traits")
(assert (nil? (traits :kw)) "keyword has no traits")

(println "trait-dispatch: all tests passed")
