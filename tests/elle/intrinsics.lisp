(elle/epoch 9)
# %-intrinsic tests
#
# Raw bytecode operations with known type/alloc/escape behavior.
# Each intrinsic is tested for correct results and edge cases.

# ── Arithmetic ────────────────────────────────────────────────

# test_add
(assert (= (%add 1 2) 3) "%add int")
(assert (= (%add 1.5 2.5) 4.0) "%add float")
(assert (= (%add -1 1) 0) "%add negative")

# test_sub_binary
(assert (= (%sub 10 3) 7) "%sub binary")
(assert (= (%sub 1.0 0.5) 0.5) "%sub float")

# test_sub_unary
(assert (= (%sub 5) -5) "%sub unary negation")
(assert (= (%sub -3) 3) "%sub unary double neg")
(assert (= (%sub 0) 0) "%sub unary zero")

# test_mul
(assert (= (%mul 4 5) 20) "%mul int")
(assert (= (%mul -2 3) -6) "%mul negative")
(assert (= (%mul 0 999) 0) "%mul zero")

# test_div
(assert (= (%div 20 4) 5) "%div exact")
(assert (= (%div 7 2) 3) "%div truncates int")

# test_rem
(assert (= (%rem 7 3) 1) "%rem positive")
(assert (= (%rem -7 3) -1) "%rem negative (truncated)")

# test_mod (floored modulus)
(assert (= (%mod 7 3) 1) "%mod positive")
(assert (= (%mod -7 3) 2) "%mod negative dividend (floored)")
(assert (= (%mod 7 -3) -2) "%mod negative divisor (floored)")
(assert (= (%mod 0 5) 0) "%mod zero dividend")
(assert (= (%mod 10 1) 0) "%mod divisor 1")

# ── Comparison ────────────────────────────────────────────────

# test_eq
(assert (%eq 1 1) "%eq true")
(assert (not (%eq 1 2)) "%eq false")

# test_lt
(assert (%lt 1 2) "%lt true")
(assert (not (%lt 2 1)) "%lt false")
(assert (not (%lt 1 1)) "%lt equal")

# test_gt
(assert (%gt 2 1) "%gt true")
(assert (not (%gt 1 2)) "%gt false")

# test_le
(assert (%le 1 2) "%le less")
(assert (%le 1 1) "%le equal")
(assert (not (%le 2 1)) "%le greater")

# test_ge
(assert (%ge 2 1) "%ge greater")
(assert (%ge 1 1) "%ge equal")
(assert (not (%ge 1 2)) "%ge less")

# ── Logical ───────────────────────────────────────────────────

# test_not
(assert (%not false) "%not false")
(assert (not (%not true)) "%not true")
(assert (not (%not 1)) "%not truthy")
(assert (%not nil) "%not nil")

# ── Conversion ────────────────────────────────────────────────

# test_int
(assert (= (%int 3.7) 3) "%int truncates")
(assert (= (%int -2.9) -2) "%int negative truncates")
(assert (= (%int 0.0) 0) "%int zero")

# test_float
(assert (= (%float 3) 3.0) "%float from int")
(assert (= (%float 0) 0.0) "%float zero")

# ── Pair operations ───────────────────────────────────────────

# test_pair
(assert (= (first (%pair 1 2)) 1) "%pair first")
(assert (= (rest (%pair 1 2)) 2) "%pair rest")

# test_pair_builds_list
(let [xs (%pair 1 (%pair 2 (%pair 3 ())))]
  (assert (= (first xs) 1) "%pair list first")
  (assert (= (first (rest xs)) 2) "%pair list second")
  (assert (= (first (rest (rest xs))) 3) "%pair list third"))

# test_first_rest
(let [p (%pair 10 20)]
  (assert (= (%first p) 10) "%first")
  (assert (= (%rest p) 20) "%rest"))

(let [xs '(a b c)]
  (assert (= (%first xs) 'a) "%first of quoted list")
  (assert (= (%first (%rest xs)) 'b) "%rest then %first"))

# ── Bitwise ───────────────────────────────────────────────────

# test_bit_and
(assert (= (%bit-and 0xFF 0x0F) 0x0F) "%bit-and")
(assert (= (%bit-and 0 0xFF) 0) "%bit-and zero")

# test_bit_or
(assert (= (%bit-or 0xF0 0x0F) 0xFF) "%bit-or")

# test_bit_xor
(assert (= (%bit-xor 0xFF 0x0F) 0xF0) "%bit-xor")
(assert (= (%bit-xor 42 42) 0) "%bit-xor self")

# test_shl
(assert (= (%shl 1 8) 256) "%shl")
(assert (= (%shl 3 4) 48) "%shl 3<<4")

# test_shr
(assert (= (%shr 256 8) 1) "%shr")
(assert (= (%shr 48 4) 3) "%shr 48>>4")

# ── Region inference: intrinsics are non-escaping ─────────────

# test_intrinsic_scope_allocation
# %add returns an immediate — the let scope should be reclaimable
(let [x 1]
  (assert (= (%add x 2) 3) "intrinsic in let body"))

# test_intrinsic_in_loop
# Intrinsics in a loop body should not prevent scope allocation
(def @sum 0)
(def @i 0)
(while (< i 10)
  (assign sum (%add sum i))
  (assign i (%add i 1)))
(assert (= sum 45) "intrinsic in loop body")

# ── Arity validation ─────────────────────────────────────────
# Arity errors for %-intrinsics are compile-time. We test them
# via eval which triggers compilation; errors propagate as signals.

# test_arity_error_too_few
(def [ok1? _] (protect (eval '(%add 1))))
(assert (not ok1?) "%add with 1 arg should be compile error")

# test_arity_error_too_many
(def [ok2? _] (protect (eval '(%not true false))))
(assert (not ok2?) "%not with 2 args should be compile error")

# test_unknown_intrinsic
(def [ok3? _] (protect (eval '(%bogus 1 2))))
(assert (not ok3?) "unknown %-intrinsic should be compile error")

# ── Intrinsic + stdlib interop ────────────────────────────────

# test_intrinsic_with_stdlib
(assert (= (map (fn [x] (%mul x x)) '(1 2 3 4)) '(1 4 9 16))
        "intrinsic inside map callback")

# test_intrinsic_in_fold
(assert (= (fold (fn [a b] (%add a b)) 0 '(1 2 3 4 5)) 15)
        "%add wrapped in lambda for fold")

# test_intrinsic_in_filter
(assert (= (filter (fn [x] (%gt x 3)) '(1 2 3 4 5)) '(4 5))
        "intrinsic in filter predicate")

# ── Pair/First/Rest rename verification ──────────────────────
# The Elle-level primitives are now pair/first/rest (not cons/car/cdr).

# test_pair_primitive
(assert (= (pair 1 2) (%pair 1 2)) "pair primitive matches %pair intrinsic")
(assert (pair? (pair 1 2)) "pair produces a pair")
(assert (not (pair? 42)) "pair? false for non-pair")
(assert (not (pair? nil)) "pair? false for nil")
(assert (pair? '(1 2)) "quoted list is a pair")

# test_first_rest_primitives
(assert (= (first '(1 2 3)) 1) "first of list")
(assert (= (first (pair :a :b)) :a) "first of pair")
(assert (= (rest '(1 2 3)) '(2 3)) "rest of list")
(assert (= (rest (pair :a :b)) :b) "rest of pair")

# test_list_construction_with_pair
(let [xs (pair 1 (pair 2 (pair 3 ())))]
  (assert (= (length xs) 3) "pair-built list length")
  (assert (= xs '(1 2 3)) "pair-built list equals quoted"))

# ── Mixed intrinsic/primitive expressions ─────────────────────

# test_nested_intrinsic_expressions
(assert (= (%add (%mul 3 4) (%sub 10 5)) 17) "nested intrinsics: (3*4) + (10-5)")

# test_intrinsic_in_conditional
(assert (= (if (%lt 1 2) (%add 10 20) (%sub 10 20)) 30)
        "intrinsic in if condition and branches")

# test_intrinsic_with_let_bindings
(let [a 10
      b 20]
  (assert (= (%add a b) 30) "intrinsic with let-bound vars")
  (assert (%lt a b) "comparison intrinsic with bindings"))

# test_intrinsic_in_letrec
(letrec [double (fn [x] (%mul x 2))
         quad (fn [x] (double (double x)))]
  (assert (= (quad 3) 12) "intrinsic in letrec-bound functions"))

# test_intrinsic_in_match
(let [x 5]
  (assert (= (match (%rem x 2)
               0 :even
               _ :odd) :odd) "intrinsic in match scrutinee"))

# ── Edge cases ────────────────────────────────────────────────

# test_float_arithmetic
(assert (= (%add 0.1 0.2) (+ 0.1 0.2)) "%add float matches + float")
(assert (= (%mul 2.5 4.0) 10.0) "%mul float")
(assert (= (%div 7.0 2.0) 3.5) "%div float exact")

# test_int_float_promotion
(assert (= (%add 1 2.0) 3.0) "%add int+float promotes")
(assert (= (%mul 3 1.5) 4.5) "%mul int*float promotes")

# test_comparison_mixed_types
(assert (%lt 1 2.0) "%lt int vs float")
(assert (%ge 3.0 3) "%ge float vs int")

# test_pair_with_various_types
(assert (= (%first (%pair :key "value")) :key) "%pair with keyword")
(assert (= (%rest (%pair :key "value")) "value") "%pair with string")
(assert (= (%first (%pair nil true)) nil) "%pair with nil")
(assert (= (%rest (%pair nil true)) true) "%pair with bool")

# test_deeply_nested_pair
(let [deep (%pair 1 (%pair 2 (%pair 3 (%pair 4 ()))))]
  (assert (= (%first (%rest (%rest (%rest deep)))) 4)
          "deeply nested %pair/%rest/%first"))

# ── Intrinsics as building blocks ─────────────────────────────

# test_manual_sum_with_intrinsics
(defn manual-sum [xs]
  (fold (fn [acc x] (%add acc x)) 0 xs))
(assert (= (manual-sum '(1 2 3 4 5)) 15) "manual sum with %add")

# test_manual_map_with_intrinsics
(defn manual-map [f xs]
  (if (empty? xs)
    ()
    (%pair (f (%first xs)) (manual-map f (%rest xs)))))
(assert (= (manual-map (fn [x] (%mul x x)) '(1 2 3)) '(1 4 9))
        "manual map with %pair/%first/%rest/%mul")

# ── %bit-not and %ne ─────────────────────────────────────────────────

(assert (= (%bit-not 0) -1) "%bit-not 0 → -1")
(assert (= (%bit-not -1) 0) "%bit-not -1 → 0")
(assert (= (%bit-not 42) -43) "%bit-not 42 → -43")

(assert (%ne 1 2) "%ne 1 2 → true")
(assert (not (%ne 1 1)) "%ne 1 1 → false")
(assert (not (%ne 1 1.0)) "%ne 1 1.0 → false (numeric coercion)")

# ── Type predicates ──────────────────────────────────────────────────

(assert (%nil? nil) "%nil? nil")
(assert (not (%nil? 0)) "%nil? 0 → false")
(assert (not (%nil? false)) "%nil? false → false")

(assert (%empty? ()) "%empty? ()")
(assert (not (%empty? nil)) "%empty? nil → false")
(assert (not (%empty? (pair 1 ()))) "%empty? pair → false")

(assert (%bool? true) "%bool? true")
(assert (%bool? false) "%bool? false")
(assert (not (%bool? nil)) "%bool? nil → false")
(assert (not (%bool? 1)) "%bool? 1 → false")

(assert (%int? 42) "%int? 42")
(assert (not (%int? 3.14)) "%int? 3.14 → false")
(assert (not (%int? nil)) "%int? nil → false")

(assert (%float? 3.14) "%float? 3.14")
(assert (not (%float? 42)) "%float? 42 → false")

(assert (%string? "hello") "%string? string")
(assert (%string? @"mutable") "%string? @string")
(assert (not (%string? 42)) "%string? 42 → false")

(assert (%keyword? :foo) "%keyword? :foo")
(assert (not (%keyword? "foo")) "%keyword? string → false")

(assert (%symbol? 'x) "%symbol? 'x")
(assert (not (%symbol? :x)) "%symbol? :x → false")

(assert (%pair? (pair 1 2)) "%pair? pair")
(assert (not (%pair? ())) "%pair? () → false")
(assert (not (%pair? nil)) "%pair? nil → false")

(assert (%array? [1 2 3]) "%array? array")
(assert (%array? @[1 2 3]) "%array? @array")
(assert (not (%array? ())) "%array? () → false")

(assert (%struct? {:a 1}) "%struct? struct")
(assert (%struct? @{:a 1}) "%struct? @struct")
(assert (not (%struct? [1])) "%struct? array → false")

(assert (%set? |1 2 3|) "%set? set")
(assert (%set? @|1 2 3|) "%set? @set")
(assert (not (%set? [1])) "%set? array → false")

(assert (%bytes? (bytes 3)) "%bytes? bytes")
(assert (not (%bytes? "hello")) "%bytes? string → false")

(assert (%closure? (fn [] 1)) "%closure? closure")
(assert (not (%closure? 42)) "%closure? 42 → false")

# %box? — boxes are internal; skip direct test

# %fiber? — test with a fiber
(def f (fiber/new (fn [] (yield 1)) |:yield|))
(assert (%fiber? f) "%fiber? fiber")
(assert (not (%fiber? 42)) "%fiber? 42 → false")

# %type-of
(assert (= (%type-of 42) :integer) "%type-of 42 → :integer")
(assert (= (%type-of 3.14) :float) "%type-of 3.14 → :float")
(assert (= (%type-of "hi") :string) "%type-of string → :string")
(assert (= (%type-of nil) :nil) "%type-of nil → :nil")
(assert (= (%type-of true) :boolean) "%type-of true → :boolean")
(assert (= (%type-of :foo) :keyword) "%type-of :foo → :keyword")
(assert (= (%type-of [1]) :array) "%type-of array → :array")
(assert (= (%type-of @[1]) :@array) "%type-of @array → :@array")

# ── Data access ──────────────────────────────────────────────────────

# %length
(assert (= (%length [1 2 3]) 3) "%length array")
(assert (= (%length "hello") 5) "%length string")
(assert (= (%length {:a 1 :b 2}) 2) "%length struct")
(assert (= (%length ()) 0) "%length empty list")
(assert (= (%length |1 2 3|) 3) "%length set")

# %get
(assert (= (%get [10 20 30] 1) 20) "%get array by index")
(assert (= (%get {:a 1 :b 2} :b) 2) "%get struct by key")

# %put
(assert (= (get (%put {:a 1} :b 2) :b) 2) "%put struct assoc")

# %del
(assert (not (has? (%del {:a 1 :b 2} :a) :a)) "%del struct dissoc")

# %has?
(assert (%has? {:a 1} :a) "%has? struct key exists")
(assert (not (%has? {:a 1} :b)) "%has? struct key missing")

# %push — mutates @array in place, returns new for immutable
(def arr1 @[1 2])
(%push arr1 3)
(assert (= (length arr1) 3) "%push @array mutates in place")
(def arr1b (%push [1 2] 3))
(assert (= (length arr1b) 3) "%push immutable returns new array")

# %pop
(def arr2 @[1 2 3])
(def popped (%pop arr2))
(assert (= popped 3) "%pop returns last element")
(assert (= (length arr2) 2) "%pop shrinks array")

# ── Mutability ───────────────────────────────────────────────────────

# %freeze
(def frozen (%freeze @[1 2 3]))
(assert (%array? frozen) "%freeze @array → array")
(assert (= (%type-of frozen) :array) "%freeze produces immutable array")

# %thaw
(def thawed (%thaw [1 2 3]))
(assert (%array? thawed) "%thaw array → @array")
(assert (= (%type-of thawed) :@array) "%thaw produces mutable array")

# ── Identity ─────────────────────────────────────────────────────────

(assert (%identical? 1 1) "%identical? same int")
(assert (not (%identical? 1 1.0)) "%identical? int vs float → false")
(assert (%identical? :foo :foo) "%identical? same keyword")
# Different heap allocations should not be identical
(def a1 @[1 2 3])
(def a2 @[1 2 3])
(assert (not (%identical? a1 a2)) "%identical? different heap allocs")

(println "all intrinsic tests passed")
