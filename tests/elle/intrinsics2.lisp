#!/usr/bin/env elle
(elle/epoch 9)
# Tests for the 29 new %-intrinsics

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

(println "All 29 new intrinsic tests passed.")
