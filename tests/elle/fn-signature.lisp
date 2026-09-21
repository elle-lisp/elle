(elle/epoch 12)
# audited: 2026-09-21
# fn/signature — the declared shape of a function, from the closure payload.
#
# The counter-factual: a signature diff is only as honest as this record.
# Each assertion pins one field a version-bump verifier reads, so a payload
# change that drops or reshapes a field fails here, not in the verifier.

# A plain function: exact arity, no collector, no metadata beyond shape.
(let [sig (fn/signature (fn [a b] a))]
  (assert (= (get sig :required) 2) "plain fn requires two")
  (assert (= (get sig :optional) 0) "plain fn has no optionals")
  (assert (= (get sig :rest) :none) "plain fn has no collector")
  (assert (= (get sig :named-keys) []) "plain fn has no named keys")
  (assert (nil? (get sig :name)) "anonymous fn carries no name"))

# defn: name and docstring ride the signature.
(defn documented [x]
  "A documented function."
  x)
(let [sig (fn/signature documented)]
  (assert (= (get sig :name) "documented") "defn name")
  (assert (= (get sig :doc) "A documented function.") "defn docstring")
  (assert (= (get sig :required) 1) "defn required count"))

# &opt widens the accepted arity without a collector.
(let [sig (fn/signature (fn [a &opt b c] a))]
  (assert (= (get sig :required) 1) "&opt: one required")
  (assert (= (get sig :optional) 2) "&opt: two optional")
  (assert (= (get sig :rest) :none) "&opt alone is no collector"))

# & collects a list; &rest is the same collector.
(let [sig (fn/signature (fn [a & rest] a))]
  (assert (= (get sig :required) 1) "&: one required")
  (assert (= (get sig :optional) 0) "&: no optionals")
  (assert (= (get sig :rest) :list) "& collects a list"))
(let [sig (fn/signature (fn [&rest xs] xs))]
  (assert (= (get sig :required) 0) "&rest: none required")
  (assert (= (get sig :rest) :list) "&rest collects a list"))

# &keys collects an open struct.
(let [sig (fn/signature (fn [a &keys opts] opts))]
  (assert (= (get sig :rest) :keys) "&keys collects a struct")
  (assert (= (get sig :named-keys) []) "&keys declares no key set"))

# &named collects a struct against a declared key set, reported sorted.
(let [sig (fn/signature (fn [a &named zeta alpha] a))]
  (assert (= (get sig :rest) :named) "&named collector")
  (assert (= (get sig :named-keys) [:alpha :zeta]) "keys are sorted")
  (assert (= (get sig :required) 1) "&named: one required"))

# The signal profile has the shape compile/signal returns.
(let [sigs (get (fn/signature (fn [x] x)) :signals)]
  (assert (get sigs :silent) "identity is silent")
  (assert (empty? (get sigs :bits)) "identity raises nothing")
  (assert (empty? (get sigs :propagates)) "identity propagates nothing"))
(let [sigs (get (fn/signature (fn [] (error {:error :boom}))) :signals)]
  (assert (has? (get sigs :bits) :error) "error shows in :bits")
  (assert (not (get sigs :silent)) "an erroring fn is not silent"))
(let [sigs (get (fn/signature (fn [f] (f))) :signals)]
  (assert (has? (get sigs :propagates) 0) "calling param 0 propagates it"))

# Origin names where the lambda was written.
(let [origin (get (fn/signature documented) :origin)]
  (assert (get origin :file) "origin has :file")
  (assert (get origin :line) "origin has :line")
  (assert (get origin :col) "origin has :col"))

# A native primitive answers from its declared metadata.
(let [sig (fn/signature pair)]
  (assert (= (get sig :name) "pair") "native name")
  (assert (= (get sig :required) 2) "native required count")
  (assert (= (get sig :rest) :none) "native exact arity")
  (assert (get sig :doc) "native doc"))
(let [sig (fn/signature +)]
  (assert (= (get sig :rest) :list) "variadic native collects"))

# Anything that is not callable is a type-error.
(let [[ok? _] (protect (fn/signature 42))]
  (assert (not ok?) "non-function is an error"))
(let [[ok? _] (protect (fn/signature nil))]
  (assert (not ok?) "nil is an error"))

(println "fn-signature: all tests passed")
