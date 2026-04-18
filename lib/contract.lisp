(elle/epoch 7)
## lib/contract.lisp — Compositional validation system for function boundaries.
##
## Loaded via:
##   (def cv ((import-file "lib/contract.lisp")))
##   (def compile-validator (get cv :compile-validator))
##   ...
##
## Exported functions:
##   compile-validator  — convert expr to validator struct
##   validate           — run validator, return nil or failure struct
##   explain            — run validator, return nil or human-readable string
##   contract           — wrap fn with arg/return validation and blame
##   check              — build validator from predicate functions (all must pass)
##   v/and              — all sub-validators must pass (accumulates failures)
##   v/or               — any sub-validator must pass (short-circuits on first pass)
##   v/oneof            — value must equal one of the given literals
##   v/optional         — nil passes; otherwise delegates to sub-validator
##   v/arrayof          — every array element must pass the sub-validator
##   v/mapof            — every struct key/value must pass their respective validators
##
## Validator struct shape:
##   {:check (fn [value] ...) :describe "human-readable description"}
##   :check returns nil on success, a failure struct on failure.
##
## Failure struct shapes:
##   Leaf:      {:error :validation :expected "desc" :got :type-keyword}
##   Fields:    {:error :validation :fields [{:key :k :failure {...}}]}
##   Aggregate: {:error :validation :all [{...} {...}]}           (v/and, v/arrayof, v/mapof)
##   Any:       {:error :validation :any [{...}] :expected "a | b"} (v/or)
##   Oneof:     {:error :validation :expected "one of: a, b" :got value}

# ============================================================================
# Internal helpers
# ============================================================================

(def validator?
  (fn [x]
    "Return true if x is a compiled validator struct."
    (and (struct? x) (has? x :check) (fn? (get x :check)))))

(def make-validator
  (fn [check-fn describe-str]
    "Construct a validator struct from a check function and a describe string."
    {:check check-fn :describe describe-str}))

# ============================================================================
# compile-validator
# ============================================================================

(def compile-validator
  (fn [expr]
    "Convert a validator expression into a validator struct.
   Dispatches on expression type:
   - fn?       → predicate validator (truthy=pass, falsy=fail)
   - validator? → pass-through (already compiled)
   - struct?   → struct shape validator (validates declared keys; extra keys ignored)
   - other     → signals :type-error"
    (cond
      ((fn? expr)
       # Wrap a raw predicate: truthy = pass, falsy = fail.
       # Use protect to treat thrown errors as failures (e.g. odd? on non-integer).
       (let [desc (string expr)]
         (make-validator
           (fn [value]
             (let [[ok result] (protect (expr value))]
               (if (and ok result)
                 nil
                 {:error :validation :expected desc :got (type-of value)})))
           desc)))
      ((validator? expr)
       # Already a compiled validator — pass through unchanged.
       expr)
      ((struct? expr)
       # Struct shape: compile each declared key's validator recursively.
       # Extra keys in the value are allowed (open-world).
       # Missing keys pass nil to the sub-validator (nil will typically fail).
       (let* [shape-keys (keys expr)
              compiled-shape (let [s @{}]
                                (each k in shape-keys
                                  (put s k (compile-validator (get expr k))))
                                (freeze s))
              desc (let [parts @[]]
                      (each k in shape-keys
                         (push parts (append (append (string k) " ")
                                             (get (get compiled-shape k) :describe))))
                       (append (append "{" (string/join parts ", ")) "}"))]
         (make-validator
           (fn [value]
             (if (not (struct? value))
               {:error :validation
                :expected desc
                :got (type-of value)}
               (let [failures @[]]
                 (each k in shape-keys
                   (let* [sub-v (get compiled-shape k)
                          result ((get sub-v :check) (get value k))]
                     (when (not (nil? result))
                       (push failures {:key k :failure result}))))
                 (if (> (length failures) 0)
                   {:error :validation :fields (freeze failures)}
                   nil))))
           desc)))
      (true
       (error {:error :type-error
               :reason :unsupported-type
               :got (type-of expr)
               :message "unsupported expression type"})))))

# ============================================================================
# check — multiple predicate functions (all must pass, short-circuits)
# ============================================================================

(def check
  (fn [& preds]
    "Build a validator from one or more predicate functions.
   All predicates must return truthy for the value to pass.
   Short-circuits on the first falsy result.
   Usage: (check integer? odd?)"
    (when (= (length preds) 0)
      (error {:error :arity-error :reason :too-few-args :minimum 1 :message "requires at least one predicate"}))
    (let* [descs (map string preds)
           desc (string/join descs " & ")]
      (make-validator
        (fn [value]
          (letrec [loop (fn [i]
                    (if (>= i (length preds))
                      nil
                      (let [pred (get preds i)]
                        (if (pred value)
                          (loop (+ i 1))
                          {:error :validation
                           :expected (get descs i)
                           :got (type-of value)}))))]
            (loop 0)))
        desc))))

# ============================================================================
# v/and — all sub-validators must pass (accumulates ALL failures)
# ============================================================================

(def v/and
  (fn [& exprs]
    "Build a validator that runs all sub-validators and collects all failures.
   Does not short-circuit. Returns nil if all pass, aggregate failure if any fail.
   Usage: (v/and integer? (fn [x] (> x 0)))"
    (when (= (length exprs) 0)
      (error {:error :arity-error :reason :too-few-args :minimum 1 :message "requires at least one expression"}))
    (let* [validators (map compile-validator exprs)
           desc (string/join (map (fn [v] (get v :describe)) validators) " & ")]
      (make-validator
        (fn [value]
          (let [failures @[]]
            (each v in validators
              (let [result ((get v :check) value)]
                (when (not (nil? result))
                  (push failures result))))
            (if (> (length failures) 0)
              {:error :validation :all (freeze failures)}
              nil)))
        desc))))

# ============================================================================
# v/or — first passing sub-validator wins (short-circuits)
# ============================================================================

(def v/or
  (fn [& exprs]
    "Build a validator that passes if any sub-validator passes.
   Short-circuits on the first pass. If all fail, returns aggregate failure.
   Usage: (v/or integer? string?)"
    (when (= (length exprs) 0)
      (error {:error :arity-error :reason :too-few-args :minimum 1 :message "requires at least one expression"}))
    (let* [validators (map compile-validator exprs)
           desc (string/join (map (fn [v] (get v :describe)) validators) " | ")]
      (make-validator
        (fn [value]
          (let [failures @[]]
            (letrec [loop (fn [i]
                      (if (>= i (length validators))
                        {:error :validation
                         :any (freeze failures)
                         :expected desc}
                        (let [result ((get (get validators i) :check) value)]
                          (if (nil? result)
                            nil
                            (begin
                              (push failures result)
                              (loop (+ i 1)))))))]
              (loop 0))))
        desc))))

# ============================================================================
# v/oneof — value must equal one of the given literals
# ============================================================================

(def v/oneof
  (fn [& values]
    "Build a validator that passes if value equals any of the given literals.
   Uses = for comparison.
   Usage: (v/oneof :a :b :c)"
    (when (= (length values) 0)
      (error {:error :arity-error :reason :too-few-args :minimum 1 :message "requires at least one value"}))
    (let* [parts (map string values)
           desc (append "one of: " (string/join parts ", "))]
      (make-validator
        (fn [value]
          (letrec [loop (fn [i]
                    (if (>= i (length values))
                      {:error :validation :expected desc :got value}
                      (if (= value (get values i))
                        nil
                        (loop (+ i 1)))))]
            (loop 0)))
        desc))))

# ============================================================================
# v/optional — nil passes; otherwise delegates to sub-validator
# ============================================================================

(def v/optional
  (fn [expr]
    "Build a validator that passes for nil, or delegates to sub-validator.
   Usage: (v/optional integer?)"
    (let* [sub (compile-validator expr)
           desc (append (append "optional(" (get sub :describe)) ")")]
      (make-validator
        (fn [value]
          (if (nil? value)
            nil
            ((get sub :check) value)))
        desc))))

# ============================================================================
# v/arrayof — every array element must pass the sub-validator
# ============================================================================

(def v/arrayof
  (fn [expr]
    "Build a validator that checks every element of an array.
   Collects all element failures (does not short-circuit).
   Returns {:error :validation :all [{:index N :failure F} ...]} on failure.
   Usage: (v/arrayof integer?)"
    (let* [sub (compile-validator expr)
           desc (append (append "arrayof(" (get sub :describe)) ")")]
      (make-validator
        (fn [value]
          (if (not (array? value))
            {:error :validation :expected desc :got (type-of value)}
            (let [failures @[]
                  n (length value)]
              (letrec [loop (fn [i]
                        (when (< i n)
                           (let [result ((get sub :check) (get value i))]
                             (when (not (nil? result))
                               (push failures {:index i :failure result})))
                          (loop (+ i 1))))]
                (loop 0))
              (if (> (length failures) 0)
                {:error :validation :all (freeze failures)}
                nil))))
        desc))))

# ============================================================================
# v/mapof — every struct key/value pair must pass their validators
# ============================================================================

(def v/mapof
  (fn [key-expr val-expr]
    "Build a validator that checks every key-value pair in a struct.
   Collects all failures. Each failure entry is:
     {:kind :key   :key k :failure F}  — key itself failed validation
     {:kind :value :key k :failure F}  — value at key failed validation
   Returns {:error :validation :all [...]} on failure.
   Usage: (v/mapof keyword? integer?)"
    (let* [key-v (compile-validator key-expr)
            val-v (compile-validator val-expr)
            desc (append (append (append (append "mapof("
                          (get key-v :describe))
                          ", ")
                          (get val-v :describe))
                          ")")]
      (make-validator
        (fn [value]
          (if (not (struct? value))
            {:error :validation :expected desc :got (type-of value)}
            (let [failures @[]]
              (each [k v] in (pairs value)
                (let [key-result ((get key-v :check) k)]
                  (when (not (nil? key-result))
                    (push failures {:kind :key :key k :failure key-result})))
                (let [val-result ((get val-v :check) v)]
                  (when (not (nil? val-result))
                    (push failures {:kind :value :key k :failure val-result}))))
              (if (> (length failures) 0)
                {:error :validation :all (freeze failures)}
                nil))))
        desc))))

# ============================================================================
# validate — thin wrapper over :check
# ============================================================================

(def validate
  (fn [validator value]
    "Run validator against value. Returns nil on success, failure struct on failure."
    ((get validator :check) value)))

# ============================================================================
# explain — format a failure struct as human-readable string
# ============================================================================

(defn explain-failure [failure]
  "Recursively format a failure struct as a human-readable string.
   Dispatches on which keys are present in the failure struct.
   For :fields → struct shape failure.
   For :any    → v/or failure.
   For :all with :index entries → v/arrayof failure.
   For :all with :kind entries  → v/mapof failure.
   For :all with plain entries  → v/and failure.
   For :expected/:got           → leaf failure."
  (cond
    ((has? failure :fields)
     # Struct shape failure
     (let [parts @["struct validation failed:"]]
       (each entry in (get failure :fields)
         (push parts
           (append (append (append "  " (string (get entry :key))) " — ")
                   (explain-failure (get entry :failure)))))
       (string/join parts "\n")))
    ((has? failure :any)
     # v/or failure
     (let [parts @[(append (append "none matched (" (get failure :expected)) "):")]]
       (each sub in (get failure :any)
         (push parts (append "  - " (explain-failure sub))))
       (string/join parts "\n")))
    ((has? failure :all)
     # v/and, v/arrayof, or v/mapof — distinguish by inspecting first entry
     (let [entries (get failure :all)]
       (if (= (length entries) 0)
         "validation failed: (no details)"
         (let [first-entry (get entries 0)]
           (cond
             ((has? first-entry :index)
              # v/arrayof
               (let [parts @["array validation failed:"]]
                 (each entry in entries
                   (push parts
                     (append (append (append "  index " (string (get entry :index))) ": ")
                             (explain-failure (get entry :failure)))))
                 (string/join parts "\n")))
             ((has? first-entry :kind)
              # v/mapof
               (let [parts @["map validation failed:"]]
                 (each entry in entries
                   (push parts
                     (append (append (append (append (append "  " (string (get entry :kind))) " at ")
                                             (string (get entry :key))) ": ")
                             (explain-failure (get entry :failure)))))
                 (string/join parts "\n")))
             (true
              # v/and
              (let [parts @["all of:"]]
                (each sub in entries
                  (push parts (append "  - " (explain-failure sub))))
                (string/join parts "\n"))))))))
    ((and (has? failure :expected) (has? failure :got))
     # Leaf failure
     (append (append (append "expected " (string (get failure :expected))) ", got ")
             (string (get failure :got))))
    (true
     # Unknown failure shape — best effort
     (string failure))))

(def explain
  (fn [validator value]
    "Run validator against value. Returns nil if valid, or a human-readable
     string describing the failure if invalid."
    (let [failure (validate validator value)]
      (if (nil? failure)
        nil
        (explain-failure failure)))))

# ============================================================================
# contract — wrap a function with argument and return-value validation
# ============================================================================

(def contract
  (fn [f arg-exprs ret-expr & rest-args]
    "Wrap function f with argument and return-value validation.
     - arg-exprs : array of validator expressions, one per argument
     - ret-expr  : validator expression for return value, or nil to skip
     - name      : optional string for error messages (default: (string f))
     Returns a new closure that:
     1. Checks argument count against (length arg-exprs).
     2. Validates each argument. On failure: signals
        {:error :contract-error :blame :caller :function NAME :arg INDEX :failure F}.
     3. Calls the original function.
     4. If ret-expr is non-nil, validates return value. On failure: signals
        {:error :contract-error :blame :function :function NAME :failure F}.
     5. Returns result."
    (let* [fname (if (empty? rest-args) (string f) (get rest-args 0))
           compiled-args (map compile-validator arg-exprs)
           compiled-ret (if (nil? ret-expr) nil (compile-validator ret-expr))
           n-expected (length arg-exprs)]
      (fn [& args]
        # Step 1: arity check
        (let [n-got (length args)]
          (when (not (= n-got n-expected))
            (error {:error    :contract-error
                    :blame    :caller
                    :function fname
                    :expected n-expected
                    :got      n-got})))
        # Step 2: validate each argument
        (letrec [check-args (fn [i]
                    (when (< i n-expected)
                      (let* [v (get compiled-args i)
                             failure ((get v :check) (get args i))]
                        (when (not (nil? failure))
                          (error {:error    :contract-error
                                  :blame    :caller
                                  :function fname
                                  :arg      i
                                  :failure  failure})))
                      (check-args (+ i 1))))]
          (check-args 0))
        # Step 3: call the original function
        (let [result (f ;args)]
          # Step 4: validate return value
          (when (not (nil? compiled-ret))
            (let [ret-failure ((get compiled-ret :check) result)]
              (when (not (nil? ret-failure))
                (error {:error    :contract-error
                        :blame    :function
                        :function fname
                        :failure  ret-failure}))))
          # Step 5: return result
          result)))))

# ============================================================================
# Module export closure (Chunk 2 — complete)
# ============================================================================

(fn []
  {:compile-validator compile-validator
   :validate          validate
   :explain           explain
   :contract          contract
   :check             check
   :v/and             v/and
   :v/or              v/or
   :v/oneof           v/oneof
   :v/optional        v/optional
   :v/arrayof         v/arrayof
   :v/mapof           v/mapof})
