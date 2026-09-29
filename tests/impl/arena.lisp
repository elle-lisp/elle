(elle/epoch 13)
# audited: 2026-09-29
# The arena gauges: arena/stats, arena/count, arena/allocs, the object limit and the physical-id counters.
# docs/impl/region/diagnostics.md
#
# The gauges are this implementation's extensions. tests/lang/fiber-heap-values.lisp
# holds what a program sees of the same heap: values that cross a fiber stay
# readable.

# ── arena/stats (struct form) ───────────────────────────────────────

(let [result (arena/stats)]
  (assert (struct? result) "arena/stats returns struct"))

(let* [stats (arena/stats)
       count (get stats :object-count)
       bytes (get stats :allocated-bytes)
       peak (get stats :peak-count)]
  (assert (>= count 0) "arena/stats :object-count is non-negative")
  (assert (>= bytes 0) "arena/stats :allocated-bytes is non-negative")
  (assert (>= peak 0) "arena/stats :peak-count is non-negative"))

(let [result (vm/query "arena/stats" nil)]
  (assert (struct? result) "vm/query arena/stats returns struct"))

# ── arena/count (int form) ──────────────────────────────────────────

(let [result (arena/count)]
  (assert (int? result) "arena/count returns int")
  (assert (> result 0) "arena/count is positive after init"))

# `items` is bound and read after the second arena/count, so its region stays
# alive across the measurement: a binding nothing reads is freed at its init.
(let* [before (arena/count)
       items (list 1 2 3 4 5)
       after (arena/count)]
  (assert (= (> after before) true) "arena count increases after allocation")
  (assert (= (length items) 5) "items list survives measurement"))

# arena/count reads the heap directly, with no SIG_QUERY, so it allocates
# nothing.
(let* [a (arena/count)
       b (arena/count)]
  (assert (= (- b a) 0) "arena/count has zero overhead"))

# After a full VM startup (stdlib loaded), arena/count on root must be > 0.
(assert (> (arena/count) 0)
        "root fiber arena/count is positive after stdlib load")

# ── arena/allocs (primitive) ────────────────────────────────────────

(let [result (rest (arena/allocs (fn () nil)))]
  (assert (= result 0) "nil thunk allocates 0 net objects"))

(let [result (rest (arena/allocs (fn () (pair 1 2))))]
  (assert (= result 1) "pair allocates 1 object"))

(let [result (first (arena/allocs (fn () (+ 40 2))))]
  (assert (= result 42) "arena/allocs preserves return value"))

(let [result (rest (arena/allocs (fn () (list 1 2 3 4 5))))]
  (assert (= result 5) "list of 5 allocates 5 pair cells"))

# ── arena/allocs measuring a thunk that drives fibers ───────────────
# A thunk handed to arena/allocs may itself call fiber/resume. User code always
# runs inside a fiber (the async scheduler), so that resume does not run inline:
# it returns SIG_SWITCH for the driving trampoline. arena/allocs drives that
# trampoline to completion, so the thunk finishes and a (result . net) pair
# comes back, not the resumed child's bare value.

# The thunk's tail IS the resume; its result is the child's return value (42).
(let [m (arena/allocs (fn [] (fiber/resume (fiber/new (fn [] 42) |:yield|))))]
  (assert (= (first m) 42)
          "arena/allocs returns the thunk result when the thunk resumes a fiber")
  (assert (int? (rest m))
          "arena/allocs net-allocs is an int when the thunk resumes a fiber"))

# The thunk runs PAST the internal resume to its real tail (:done), rather than
# stopping at the resume and surfacing the child's value.
(let [m (arena/allocs (fn []
                        (fiber/resume (fiber/new (fn [] 1) |:yield|))
                        :done))]
  (assert (= (first m) :done)
          "arena/allocs thunk continues past an internal fiber/resume"))

# A loop spawning and resuming fibers: the measurement completes and reports a
# non-negative net count.
(let [m (arena/allocs (fn []
                        (each i in (range 5)
                          (let [f (fiber/new (fn [] i) |:yield|)]
                            (fiber/resume f)))))]
  (assert (int? (rest m)) "arena/allocs measures a loop of fiber spawn+resume")
  (assert (>= (rest m) 0)
          "arena/allocs net-allocs is non-negative for a fiber loop"))

# ── One heap per instance ───────────────────────────────────────────
# Every fiber of an instance allocates on the instance's one heap, so a child's
# allocations show in arena/count read from inside the child.
(let* [f (fiber/new (fn ()
                      (let* [before (arena/count)
                             items (list 1 2 3 4 5)
                             after (arena/count)]
                        (assert (= (length items) 5) "items survives")
                        (- after before))) 1)]
  (let [allocs (fiber/resume f)]
    (assert (= allocs 5) "child sees exactly 5 allocations from list")))

# ── A constant per-iteration cost ───────────────────────────────────
# Macro expansion cost per iteration is stable across different N once the
# transformer cache is warm. The first expansion per macro compiles the
# transformer closure, which must survive to be cached; later expansions use
# the cached closure and cost less. The warm-up below fills the cache, so both
# measurements read only the warm path.
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
                 (+ x 1)))  # warm-up: compile transformer closures
       _ (eval '(defn temp (x)
                 (+ x 1)))  # warm-up: compile transformer closures
       _ (eval '(defn temp (x)
                 (+ x 1)))  # warm-up: ensure cache is fully populated
       p50 (measure 50)
       p100 (measure 100)]
  (assert (or (= p50 p100) (<= p100 p50))
          (concat "per-iter allocation cost is constant after cache warm-up: p50="
                  (number->string p50) " p100=" (number->string p100))))

# ── The gauges take no scope argument ───────────────────────────────
# `apply` bypasses the compile-time arity check, so the runtime arity error is
# what each case reads.
(let [[ok? err] (protect ((fn [] (apply arena/count [:global]))))]
  (assert (not ok?) "arena/count rejects a :global argument")
  (assert (= (get err :error) :arity-error)
          "arena/count rejects a :global argument"))
(let [[ok? err] (protect ((fn [] (apply arena/count [:fiber]))))]
  (assert (not ok?) "arena/count rejects a :fiber argument")
  (assert (= (get err :error) :arity-error)
          "arena/count rejects a :fiber argument"))
(let [[ok? err] (protect ((fn [] (apply arena/bytes [:global]))))]
  (assert (not ok?) "arena/bytes rejects a :global argument")
  (assert (= (get err :error) :arity-error)
          "arena/bytes rejects a :global argument"))
(let [[ok? err] (protect ((fn [] (apply arena/bytes [:fiber]))))]
  (assert (not ok?) "arena/bytes rejects a :fiber argument")
  (assert (= (get err :error) :arity-error)
          "arena/bytes rejects a :fiber argument"))
(let [[ok? err] (protect ((fn [] (apply arena/peak [:global]))))]
  (assert (not ok?) "arena/peak rejects a :global argument")
  (assert (= (get err :error) :arity-error)
          "arena/peak rejects a :global argument"))
(let [[ok? err] (protect ((fn [] (apply arena/reset-peak [:global]))))]
  (assert (not ok?) "arena/reset-peak rejects a :global argument")
  (assert (= (get err :error) :arity-error)
          "arena/reset-peak rejects a :global argument"))
(let [[ok? err] (protect ((fn [] (apply arena/object-limit [:global]))))]
  (assert (not ok?) "arena/object-limit rejects a :global argument")
  (assert (= (get err :error) :arity-error)
          "arena/object-limit rejects a :global argument"))
(let [[ok? err] (protect ((fn [] (apply arena/set-object-limit [100 :global]))))]
  (assert (not ok?) "arena/set-object-limit rejects a second :global argument")
  (assert (= (get err :error) :arity-error)
          "arena/set-object-limit rejects a second :global argument"))

# ── arena/stats: the fields ─────────────────────────────────────────
# `build_stats` (src/vm/signal/query.rs) answers :scope-depth 0,
# :active-allocator :region and both scope counters 0 in every state: this
# implementation has no user-level allocation scope for them to report.
(let* [s (arena/stats)]
  (assert (struct? s) "arena/stats returns struct")
  (assert (int? (get s :object-count)) "arena/stats :object-count is int")
  (assert (int? (get s :peak-count)) "arena/stats :peak-count is int")
  (assert (int? (get s :allocated-bytes)) "arena/stats :allocated-bytes is int")
  (assert (int? (get s :scope-depth)) "arena/stats :scope-depth is int")
  (assert (= :region (get s :active-allocator))
          "arena/stats :active-allocator is :region")
  (assert (int? (get s :scope-enter-count))
          "arena/stats :scope-enter-count is int")
  (assert (int? (get s :scope-dtor-count))
          "arena/stats :scope-dtor-count is int"))

(let* [s (arena/stats)]
  (assert (nil? (get s :capacity)) "arena/stats has no :capacity field"))

(let* [s (arena/stats)]
  (assert (= (get s :scope-depth) 0) "arena/stats :scope-depth is 0 at root"))

(let* [stats (arena/stats)
       enters (get stats :scope-enter-count)
       dtors (get stats :scope-dtor-count)]
  (assert (>= enters 0) "root fiber scope-enter-count is non-negative")
  (assert (>= dtors 0) "root fiber scope-dtor-count is non-negative"))

# A child fiber's stats read the same fields.
(let* [f (fiber/new (fn ()
                      (let* [_ (arena/stats)]
                        (arena/stats))) 1)
       stats (fiber/resume f)
       enters (get stats :scope-enter-count)
       dtors-run (get stats :scope-dtor-count)]
  (assert (>= enters 0) "new fiber :scope-enter-count is non-negative")
  (assert (>= dtors-run 0) "new fiber :scope-dtor-count is non-negative"))

(let* [s (arena/stats)
       stats-bytes (get s :allocated-bytes)
       direct-bytes (arena/bytes)]
  (assert (>= stats-bytes 0) "arena/stats :allocated-bytes is non-negative")
  (assert (>= direct-bytes 0) "arena/bytes is non-negative"))

# `p` is bound and read after the second arena/stats, so its region stays alive
# across the measurement.
(let* [before-s (arena/stats)
       before-count (get before-s :object-count)
       p (pair 1 2)  # allocates one Cons
       after-s (arena/stats)
       after-count (get after-s :object-count)]
  (assert (> after-count before-count)
          ":object-count increases after allocation")
  (assert (= (first p) 1) "pair survives measurement"))

# ── arena/stats: the object limit ───────────────────────────────────
# Each case sets a limit far above what the file allocates, then clears it.
(let* [s (arena/stats)]
  (assert (nil? (get s :object-limit))
          "arena/stats :object-limit is nil with no limit set"))

(let* [_ (arena/set-object-limit 9999999)
       s (arena/stats)
       limit (get s :object-limit)
       _ (arena/set-object-limit nil)]
  (assert (= limit 9999999) "arena/stats :object-limit reflects set limit"))

(let* [_ (arena/set-object-limit 9999999)
       _ (arena/set-object-limit nil)
       s (arena/stats)]
  (assert (nil? (get s :object-limit))
          ":object-limit is nil after removing limit"))

# ── arena/stats: its arguments ──────────────────────────────────────

(let [[ok? err] (protect ((fn [] (apply arena/stats [1 2]))))]
  (assert (not ok?) "arena/stats rejects 2 arguments")
  (assert (= (get err :error) :arity-error) "arena/stats rejects 2 arguments"))

(let [[ok? err] (protect ((fn [] (arena/stats 42))))]
  (assert (not ok?) "arena/stats rejects non-fiber argument")
  (assert (= (get err :error) :type-error)
          "arena/stats rejects non-fiber argument"))

# With a fiber argument, arena/stats answers the same fields for that fiber.
(let* [f (fiber/new (fn () 42) 1)
       _ (fiber/resume f)
       s (arena/stats f)]
  (assert (struct? s) "arena/stats with fiber arg returns struct")
  (assert (int? (get s :object-count)) "fiber stats :object-count is int")
  (assert (int? (get s :peak-count)) "fiber stats :peak-count is int")
  (assert (int? (get s :allocated-bytes)) "fiber stats :allocated-bytes is int")
  (assert (nil? (get s :capacity)) "fiber stats has no :capacity field"))

# arena/stats is the one stats primitive: no separate fiber or scope form.
(assert (nil? (vm/primitive-meta "arena/fiber-stats"))
        "there is no arena/fiber-stats primitive")
(assert (nil? (vm/primitive-meta "arena/scope-stats"))
        "there is no arena/scope-stats primitive")

# ── the id dimension: arena/region-ids and arena/region-table ───────
# The primitive surface only. What the gauges are FOR — the bound on physical id
# issuance per call — is measured by the oracle's `id-*` rows
# (tests/impl/probe/store.lisp; docs/impl/region/model.md).
#
# Both are high-water marks, so neither ever goes down: a burst of allocation and
# release leaves each where it was or higher. That is the property a delta
# measurement rests on — it lets a reading be subtracted from a later one — and
# nothing else about the pair is a caller's business here.
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

# Both are Immediate, so sampling them allocates nothing and cannot perturb the
# measurement a caller is taking around them.
(let* [a (arena/region-ids)
       b (arena/region-ids)]
  (assert (= a b) "arena/region-ids allocates nothing"))
(let* [a (arena/region-table)
       b (arena/region-table)]
  (assert (= a b) "arena/region-table allocates nothing"))
