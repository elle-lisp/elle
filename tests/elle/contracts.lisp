## tests/elle/contracts.lisp — Tests for lib/contract.lisp
## Chunk 1: validators, combinators, validate

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false
      :assert-not-nil assert-not-nil :assert-err assert-err
      :assert-err-kind assert-err-kind :assert-string-eq assert-string-eq}
  ((import-file "tests/elle/assert.lisp")))

(def {:compile-validator compile-validator :validate validate
      :explain explain :contract contract
      :check check :v/and v/and :v/or v/or
      :v/oneof v/oneof :v/optional v/optional
      :v/arrayof v/arrayof :v/mapof v/mapof}
  ((import-file "lib/contract.lisp")))

# ============================================================================
# Test 1-4: Predicate validator
# ============================================================================

# Test 1: predicate pass
(assert-eq (validate (compile-validator integer?) 42) nil
  "predicate: integer? passes for 42")

# Test 2: predicate fail returns non-nil
(assert-not-nil (validate (compile-validator integer?) "x")
  "predicate: integer? fails for string")

# Test 3: predicate failure has :error :validation, :expected, :got
(let [[f (validate (compile-validator integer?) "x")]]
  (assert-eq (get f :error) :validation "predicate fail: :error is :validation")
  (assert-true (has? f :expected) "predicate fail: has :expected")
  (assert-true (has? f :got) "predicate fail: has :got"))

# Test 4: predicate failure :got is (type-of value)
(let [[f (validate (compile-validator integer?) "x")]]
  (assert-eq (get f :got) (type-of "x") "predicate fail: :got is (type-of value)"))

# ============================================================================
# Test 5-7: check combinator
# ============================================================================

# Test 5: check passes when all predicates pass
(assert-eq (validate (compile-validator (check integer? odd?)) 3) nil
  "check: both predicates pass")

# Test 6: check fails on first predicate
(assert-not-nil (validate (compile-validator (check integer? odd?)) "x")
  "check: fails when first predicate fails")

# Test 7: check fails on second predicate
(assert-not-nil (validate (compile-validator (check integer? odd?)) 4)
  "check: fails when second predicate fails")

# ============================================================================
# Test 8-9: compile-validator passthrough and type error
# ============================================================================

# Test 8: validator passthrough — compile-validator is idempotent
(let [[v (compile-validator integer?)]]
  (assert-eq (compile-validator v) v
    "compile-validator: idempotent on already-compiled validator"))

# Test 9: unsupported expression type signals error
(assert-err (fn [] (compile-validator 42))
  "compile-validator: signals error on integer input")

# ============================================================================
# Test 10-11: v/and
# ============================================================================

# Test 10: v/and passes when all sub-validators pass
(assert-eq (validate (v/and integer? odd?) 3) nil
  "v/and: both pass")

# Test 11: v/and accumulates failures — both validators fail
(let [[f (validate (v/and integer? odd?) "x")]]
  (assert-not-nil f "v/and: returns failure")
  (assert-true (has? f :all) "v/and: failure has :all")
  (assert-true (> (length (get f :all)) 0) "v/and: :all is non-empty"))

# ============================================================================
# Test 12-13: v/or
# ============================================================================

# Test 12: v/or short-circuits on first pass
(assert-eq (validate (v/or integer? string?) 42) nil
  "v/or: first validator passes, short-circuits")

# Test 13: v/or returns :any when all validators fail
(let [[f (validate (v/or integer? keyword?) "x")]]
  (assert-not-nil f "v/or: returns failure when all fail")
  (assert-true (has? f :any) "v/or: failure has :any")
  (assert-true (has? f :expected) "v/or: failure has :expected")
  (assert-eq (length (get f :any)) 2 "v/or: :any has one entry per failing validator"))

# ============================================================================
# Test 14-15: v/oneof
# ============================================================================

# Test 14: v/oneof passes for a member
(assert-eq (validate (v/oneof 1 2 3) 2) nil
  "v/oneof: member passes")

# Test 15: v/oneof fails for non-member — :got is the actual value
(let [[f (validate (v/oneof 1 2 3) 99)]]
  (assert-not-nil f "v/oneof: non-member fails")
  (assert-true (has? f :expected) "v/oneof: failure has :expected")
  (assert-eq (get f :got) 99 "v/oneof: :got is the actual value, not a type keyword"))

# ============================================================================
# Test 16-18: v/optional
# ============================================================================

# Test 16: v/optional passes for nil
(assert-eq (validate (v/optional integer?) nil) nil
  "v/optional: nil passes")

# Test 17: v/optional passes valid non-nil value
(assert-eq (validate (v/optional integer?) 42) nil
  "v/optional: valid non-nil passes")

# Test 18: v/optional fails invalid non-nil value
(assert-not-nil (validate (v/optional integer?) "x")
  "v/optional: invalid non-nil fails")

# ============================================================================
# Test 19-23: Struct shape validation
# ============================================================================

# Test 19: struct shape passes for correct struct
(assert-eq (validate (compile-validator {:x integer? :y string?}) {:x 1 :y "a"}) nil
  "struct shape: correct struct passes")

# Test 20: struct shape fails for wrong key type — failure has :fields
(let [[f (validate (compile-validator {:x integer? :y string?}) {:x "oops" :y "a"})]]
  (assert-not-nil f "struct shape: wrong key type fails")
  (assert-true (has? f :fields) "struct shape: failure has :fields")
  (assert-eq (length (get f :fields)) 1 "struct shape: one field failed"))

# Test 21: struct shape fails for non-struct input
(let [[f (validate (compile-validator {:x integer?}) 42)]]
  (assert-not-nil f "struct shape: non-struct input fails")
  (assert-eq (get f :error) :validation "struct shape: non-struct :error is :validation")
  (assert-true (has? f :got) "struct shape: non-struct failure has :got"))

# Test 22: struct shape fails for missing key (nil passed to sub-validator)
(let [[f (validate (compile-validator {:x integer?}) {})]]
  (assert-not-nil f "struct shape: missing key fails"))

# Test 23: nested struct shape — failure path includes nesting
(let [[f (validate (compile-validator {:a {:b integer?}}) {:a {:b "oops"}})]]
  (assert-not-nil f "nested struct shape: fails")
  (assert-true (has? f :fields) "nested struct shape: outer failure has :fields")
  (let [[outer-field (get (get f :fields) 0)]]
    (assert-eq (get outer-field :key) :a "nested struct shape: outer field key is :a")
    (assert-true (has? (get outer-field :failure) :fields)
      "nested struct shape: inner failure also has :fields")))

# ============================================================================
# Test 24-27: v/arrayof
# ============================================================================

# Test 24: v/arrayof passes for array of valid elements
(assert-eq (validate (v/arrayof integer?) [1 2 3]) nil
  "v/arrayof: all valid elements pass")

# Test 25: v/arrayof fails for invalid element — failure has :all with :index
(let [[f (validate (v/arrayof integer?) [1 "x" 3])]]
  (assert-not-nil f "v/arrayof: invalid element fails")
  (assert-true (has? f :all) "v/arrayof: failure has :all")
  (let [[entry (get (get f :all) 0)]]
    (assert-eq (get entry :index) 1 "v/arrayof: :index is 1 (0-based)")
    (assert-true (has? entry :failure) "v/arrayof: entry has :failure")))

# Test 26: v/arrayof fails for non-array
(let [[f (validate (v/arrayof integer?) 42)]]
  (assert-not-nil f "v/arrayof: non-array fails")
  (assert-eq (get f :error) :validation "v/arrayof: non-array :error is :validation"))

# Test 27: v/arrayof passes for empty array
(assert-eq (validate (v/arrayof integer?) []) nil
  "v/arrayof: empty array passes")

# ============================================================================
# Test 28-30: v/mapof
# ============================================================================

# Test 28: v/mapof passes for valid struct
(assert-eq (validate (v/mapof keyword? integer?) {:a 1 :b 2}) nil
  "v/mapof: valid keys and values pass")

# Test 29: v/mapof fails for bad value — failure entry has :kind :value
(let [[f (validate (v/mapof keyword? integer?) {:a 1 :b "oops"})]]
  (assert-not-nil f "v/mapof: bad value fails")
  (assert-true (has? f :all) "v/mapof: failure has :all")
  (let [[entry (get (get f :all) 0)]]
    (assert-true (has? entry :kind) "v/mapof: entry has :kind")
    (assert-eq (get entry :kind) :value "v/mapof: entry :kind is :value")
    (assert-true (has? entry :key) "v/mapof: entry has :key")))

# Test 30: v/mapof passes for empty struct
(assert-eq (validate (v/mapof keyword? integer?) {}) nil
  "v/mapof: empty struct passes")

# ============================================================================
# Test 31-34: explain
# ============================================================================

# Test 31: explain returns nil on pass
(assert-eq (explain (compile-validator integer?) 42) nil
  "explain: returns nil when validation passes")

# Test 32: explain returns a string on fail
(let [[result (explain (compile-validator integer?) "x")]]
  (assert-not-nil result "explain: returns non-nil on failure")
  (assert-true (string? result) "explain: returns a string on failure"))

# Test 33: explain leaf format contains "expected"
(let [[result (explain (compile-validator integer?) "x")]]
  (assert-true (not (nil? (string/find result "expected")))
    "explain: leaf format contains 'expected'"))

# Test 34: explain struct failure contains field key name
(let* [[v (compile-validator {:port integer? :host string?})]
       [result (explain v {:port "oops" :host "localhost"})]]
  (assert-not-nil result "explain: struct failure returns non-nil")
  (assert-true (string? result) "explain: struct failure returns string")
  (assert-true (not (nil? (string/find result "port")))
    "explain: struct failure string contains key name 'port'"))

# ============================================================================
# Test 35-42: contract
# ============================================================================

# Helper: a simple add function to wrap
(defn raw-add [x y] (+ x y))

# Helper: a function that returns wrong type for testing return blame
(defn bad-return [x] (string x))

(def safe-add (contract raw-add [integer? integer?] integer? "safe-add"))
(def bad-safe (contract bad-return [integer?] integer? "bad-safe"))

# Test 35: contract passes through valid call
(assert-eq (safe-add 1 2) 3
  "contract: valid args produce correct result")

# Test 36: contract arity error signals :contract-error with :blame :caller
(let [[[ok? err] (protect (safe-add 1))]]
  (assert-false ok? "contract: wrong arity signals error")
  (assert-eq (get err :error) :contract-error "contract: arity error is :contract-error")
  (assert-eq (get err :blame) :caller "contract: arity error blame is :caller"))

# Test 37: contract arg validation failure — :blame :caller, :arg is 0-indexed
(let [[[ok? err] (protect (safe-add 1 "two"))]]
  (assert-false ok? "contract: bad arg signals error")
  (assert-eq (get err :error) :contract-error "contract: arg error is :contract-error")
  (assert-eq (get err :blame) :caller "contract: arg error blame is :caller")
  (assert-eq (get err :arg) 1 "contract: arg error :arg is 1 (0-indexed second arg)")
  (assert-true (has? err :failure) "contract: arg error has :failure"))

# Test 38: contract return blame — :blame :function, no :arg
(let [[[ok? err] (protect (bad-safe 42))]]
  (assert-false ok? "contract: bad return signals error")
  (assert-eq (get err :error) :contract-error "contract: return error is :contract-error")
  (assert-eq (get err :blame) :function "contract: return error blame is :function")
  (assert-false (has? err :arg) "contract: return error has no :arg key"))

# Test 39: contract preserves behavior for valid inputs
(assert-eq (safe-add 10 20) 30 "contract: preserves function behavior")
(assert-eq (safe-add -5 5)   0 "contract: preserves behavior with negative")

# Test 40: contract error is catchable via protect
(let [[[ok? _] (protect (safe-add 1 "x"))]]
  (assert-false ok? "contract: error is catchable with protect"))

# Test 41: contract error struct shape is complete
(let [[[_ err] (protect (safe-add 1 "x"))]]
  (assert-eq (get err :error)    :contract-error  "contract: err has :error")
  (assert-eq (get err :blame)    :caller           "contract: err has :blame")
  (assert-eq (get err :function) "safe-add"        "contract: err has :function name")
  (assert-true (has? err :arg) "contract: err has :arg")
  (assert-true (has? err :failure) "contract: err has :failure"))

# Test 42: contract with nil ret-expr skips return validation
(def identity-contract (contract (fn [x] x) [integer?] nil "identity-contract"))
(assert-eq (identity-contract 42) 42
  "contract: nil ret-expr — valid call returns correct value")
(let [[[ok? _] (protect (identity-contract "x"))]]
  (assert-false ok? "contract: nil ret-expr still validates args"))
