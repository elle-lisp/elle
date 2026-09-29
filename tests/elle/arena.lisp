(elle/epoch 13)
# audited: 2026-09-28
# arena/stats, arena/count, arena/allocs and the id gauges report what the one heap holds.
#
# A test that inspects the region instructions in bytecode lives in Rust.
#
# docs/impl/region/diagnostics.md

# ── arena/stats ─────────────────────────────────────────────────────

(let [result (arena/stats)]
  (assert (struct? result) "arena/stats returns struct"))

(let [result (vm/query "arena/stats" nil)]
  (assert (struct? result) "vm/query arena/stats returns struct"))

(let* [s (arena/stats)]
  (assert (int? (get s :object-count)) "arena/stats :object-count is int")
  (assert (>= (get s :object-count) 0)
          "arena/stats :object-count is non-negative")
  (assert (int? (get s :peak-count)) "arena/stats :peak-count is int")
  (assert (>= (get s :peak-count) 0) "arena/stats :peak-count is non-negative")
  (assert (int? (get s :allocated-bytes)) "arena/stats :allocated-bytes is int")
  (assert (>= (get s :allocated-bytes) 0)
          "arena/stats :allocated-bytes is non-negative")
  (assert (nil? (get s :capacity)) "arena/stats has no :capacity field"))

# The scope fields read 0, and the allocator is always the region allocator.
(let* [s (arena/stats)]
  (assert (= (get s :scope-depth) 0) "arena/stats :scope-depth is 0")
  (assert (= (get s :scope-enter-count) 0) "arena/stats :scope-enter-count is 0")
  (assert (= (get s :scope-dtor-count) 0) "arena/stats :scope-dtor-count is 0")
  (assert (= (get s :active-allocator) :region)
          "arena/stats :active-allocator is :region"))

(let* [s (arena/stats)]
  (assert (nil? (get s :object-limit))
          "arena/stats :object-limit is nil with no limit set"))

# A limit far above what the file allocates, so setting it changes nothing
# the later cases measure.
(let* [_ (arena/set-object-limit 9999999)
       s (arena/stats)
       limit (get s :object-limit)
       _ (arena/set-object-limit nil)]
  (assert (= limit 9999999) "arena/stats :object-limit reflects set limit")
  (assert (nil? (get (arena/stats) :object-limit))
          ":object-limit is nil after removing limit"))

(let [[ok? err] (protect ((fn [] (apply arena/stats [1 2]))))]
  (assert (not ok?) "arena/stats rejects 2 arguments")
  (assert (= (get err :error) :arity-error) "arena/stats rejects 2 arguments"))

(let [[ok? err] (protect ((fn [] (arena/stats 42))))]
  (assert (not ok?) "arena/stats rejects non-fiber argument")
  (assert (= (get err :error) :type-error)
          "arena/stats rejects non-fiber argument"))

# With a fiber argument, arena/stats reports the same heap: every fiber of a VM
# shares it.
(let* [f (fiber/new (fn () 42) 1)
       _ (fiber/resume f)
       s (arena/stats f)]
  (assert (struct? s) "arena/stats with fiber arg returns struct")
  (assert (int? (get s :object-count)) "fiber stats :object-count is int")
  (assert (int? (get s :peak-count)) "fiber stats :peak-count is int")
  (assert (int? (get s :allocated-bytes)) "fiber stats :allocated-bytes is int")
  (assert (nil? (get s :capacity)) "fiber stats has no :capacity field"))

(assert (nil? (vm/primitive-meta "arena/fiber-stats"))
        "no primitive is named arena/fiber-stats")
(assert (nil? (vm/primitive-meta "arena/scope-stats"))
        "no primitive is named arena/scope-stats")

# ── arena/count ─────────────────────────────────────────────────────

(let [result (arena/count)]
  (assert (int? result) "arena/count returns int")
  (assert (> result 0) "arena/count is positive after init"))

# The trap: an unused binding is freed at its init. `items` is read after the
# second sample, so its region stays alive across the measurement.
(let* [before (arena/count)
       items (list 1 2 3 4 5)
       after (arena/count)]
  (assert (= (> after before) true) "arena count increases after allocation")
  (assert (= (length items) 5) "items list survives measurement"))

# arena/count reads the heap directly, so a sample allocates nothing.
(let* [a (arena/count)
       b (arena/count)]
  (assert (= (- b a) 0) "arena/count has zero overhead"))

# The same trap as above: `p` is read after the second sample.
(let* [before-count (get (arena/stats) :object-count)
       p (pair 1 2)
       after-count (get (arena/stats) :object-count)]
  (assert (> after-count before-count)
          ":object-count increases after allocation")
  (assert (= (first p) 1) "pair survives measurement"))

# The gauges take no scope argument. `apply` passes the compile-time arity
# check, so the runtime arity error is what the case asserts.
(let [[ok? err] (protect ((fn [] (apply arena/count [:global]))))]
  (assert (not ok?) "arena/count rejects :global")
  (assert (= (get err :error) :arity-error) "arena/count rejects :global"))
(let [[ok? err] (protect ((fn [] (apply arena/count [:fiber]))))]
  (assert (not ok?) "arena/count rejects :fiber")
  (assert (= (get err :error) :arity-error) "arena/count rejects :fiber"))
(let [[ok? err] (protect ((fn [] (apply arena/bytes [:global]))))]
  (assert (not ok?) "arena/bytes rejects :global")
  (assert (= (get err :error) :arity-error) "arena/bytes rejects :global"))
(let [[ok? err] (protect ((fn [] (apply arena/bytes [:fiber]))))]
  (assert (not ok?) "arena/bytes rejects :fiber")
  (assert (= (get err :error) :arity-error) "arena/bytes rejects :fiber"))
(let [[ok? err] (protect ((fn [] (apply arena/peak [:global]))))]
  (assert (not ok?) "arena/peak rejects :global")
  (assert (= (get err :error) :arity-error) "arena/peak rejects :global"))
(let [[ok? err] (protect ((fn [] (apply arena/reset-peak [:global]))))]
  (assert (not ok?) "arena/reset-peak rejects :global")
  (assert (= (get err :error) :arity-error) "arena/reset-peak rejects :global"))
(let [[ok? err] (protect ((fn [] (apply arena/object-limit [:global]))))]
  (assert (not ok?) "arena/object-limit rejects :global")
  (assert (= (get err :error) :arity-error) "arena/object-limit rejects :global"))
(let [[ok? err] (protect ((fn [] (apply arena/set-object-limit [100 :global]))))]
  (assert (not ok?) "arena/set-object-limit rejects a second argument")
  (assert (= (get err :error) :arity-error)
          "arena/set-object-limit rejects a second argument"))

# ── arena/allocs ────────────────────────────────────────────────────

(let [result (rest (arena/allocs (fn () nil)))]
  (assert (= result 0) "nil thunk allocates 0 net objects"))

(let [result (rest (arena/allocs (fn () (pair 1 2))))]
  (assert (= result 1) "pair allocates 1 object"))

(let [result (first (arena/allocs (fn () (+ 40 2))))]
  (assert (= result 42) "arena/allocs preserves return value"))

(let [result (rest (arena/allocs (fn () (list 1 2 3 4 5))))]
  (assert (= result 5) "list of 5 allocates 5 pair cells"))

# A thunk that resumes a fiber. User code runs inside a fiber, so the resume
# returns SIG_SWITCH for the driving trampoline instead of running inline. The
# counter-factual: an arena/allocs that stops at the switch answers the resumed
# child's bare value, not a (result . net) pair.
(let [m (arena/allocs (fn [] (fiber/resume (fiber/new (fn [] 42) |:yield|))))]
  (assert (= (first m) 42)
          "arena/allocs returns the thunk result when the thunk resumes a fiber")
  (assert (int? (rest m))
          "arena/allocs net-allocs is an int when the thunk resumes a fiber"))

(let [m (arena/allocs (fn []
                        (fiber/resume (fiber/new (fn [] 1) |:yield|))
                        :done))]
  (assert (= (first m) :done)
          "arena/allocs thunk continues past an internal fiber/resume"))

(let [m (arena/allocs (fn []
                        (each i in (range 5)
                          (let [f (fiber/new (fn [] i) |:yield|)]
                            (fiber/resume f)))))]
  (assert (int? (rest m)) "arena/allocs measures a loop of fiber spawn+resume")
  (assert (>= (rest m) 0)
          "arena/allocs net-allocs is non-negative for a fiber loop"))

# ── One heap for every fiber ────────────────────────────────────────

(let* [f (fiber/new (fn ()
                      (let* [before (arena/count)
                             items (list 1 2 3 4 5)
                             after (arena/count)]
                        (assert (= (length items) 5) "items survives")
                        (- after before))) 1)]
  (let [allocs (fiber/resume f)]
    (assert (= allocs 5) "child sees exactly 5 allocations from list")))

# ── A constant cost per expansion ───────────────────────────────────
# The first expansion of a macro compiles and caches its transformer, so three
# warm-up expansions run before either measurement.
(let* [measure (fn (n)
                 (let* [before (arena/count)]
                   (letrec [loop (fn (i)
                                   (when (< i n)
                                     (eval '(defn temp (x)
                                       (+ x 1)))
                                     (loop (+ i 1))))]
                     (loop 0))
                   (/ (- (arena/count) before) n)))
       _ (eval '(defn temp (x)
                 (+ x 1)))
       _ (eval '(defn temp (x)
                 (+ x 1)))
       _ (eval '(defn temp (x)
                 (+ x 1)))
       p50 (measure 50)
       p100 (measure 100)]
  (assert (or (= p50 p100) (<= p100 p50))
          (concat "per-iter allocation cost is constant after cache warm-up: p50="
                  (number->string p50) " p100=" (number->string p100))))

# ── The id dimension: arena/region-ids and arena/region-table ───────
# The primitive surface only. The bound on physical id issuance per call is
# measured by the `id-*` probes in tests/elle/probe/store.lisp.
#
# Both are high-water marks, so neither ever goes down: a burst of allocation and
# release leaves each where it was or higher. That is the property a delta
# measurement rests on — it lets a reading be subtracted from a later one.
(let* [ids-before (arena/region-ids)
       table-before (arena/region-table)]
  (assert (int? ids-before) "arena/region-ids returns int")
  (assert (int? table-before) "arena/region-table returns int")
  (assert (> ids-before 0) "arena/region-ids is positive after stdlib load")
  (assert (> table-before 0) "arena/region-table is positive after stdlib load")
  (var n 0)
  (while (< n 200)
    (pair n n)
    (assign n (+ n 1)))
  (assert (>= (arena/region-ids) ids-before)
          "arena/region-ids fell after a burst of allocation and release")
  (assert (>= (arena/region-table) table-before)
          "arena/region-table fell after a burst of allocation and release"))

# Both are immediates, so sampling them allocates nothing and cannot perturb the
# measurement a caller is taking around them.
(let* [a (arena/region-ids)
       b (arena/region-ids)]
  (assert (= a b) "arena/region-ids allocates nothing"))
(let* [a (arena/region-table)
       b (arena/region-table)]
  (assert (= a b) "arena/region-table allocates nothing"))
