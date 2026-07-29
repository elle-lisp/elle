(elle/epoch 12)
# A call's result is named by the call's own region
# (docs/impl/region/mechanism.md § "A call's result is named by the call's own
# region").
#
# The region walk INLINES a resolvable lambda callee's body so the intrinsics
# buried inside it record their cross-region edges at the call site. The regions
# that walk yields belong to the CALLEE's activation, and the caller's binding for
# the result must not adopt them: it holds the call's own region, exactly as it
# does for an opaque callee.
#
# Adopting them makes the caller a nominal holder of a region it never allocates,
# and the `decref_point` machinery reads the fiction as fact. The shape that bites
# is a BRANCH whose arms reach one callee: the arm that allocates the region and
# the arm that merely names it through the inline are mutually exclusive, the
# structurally-latest use wins, and the single release is emitted on the path that
# never mints the region — while the allocating path emits none at all. One whole
# region per iteration, plus everything its free cascade would reclaim, is then
# held to fiber teardown.
#
# Each subject runs its own loop INLINE, because a thunk called per iteration
# would reclaim at the call boundary and read 0 whether or not the strand is
# there. The gauge is `arena/count` across one call, which must be bounded.
#
# The per-op rate for the `(let [f (fn () (fn () j))] ((f)))` shape is the
# `nested-closure` probe in tests/elle/oracle.lisp.

(def window 2000)

(defn gauge (subject window)
  "Objects retained across one SUBJECT run of WINDOW iterations, after a warm run
   absorbs the one-time intercept."
  (subject 200)
  (def before (arena/count))
  (subject window)
  (%sub (arena/count) before))

# subjects ─────────────────────────────────────────────────────────────────────
# Each subject reaches its iteration count through a closure value, so the count
# arrives untyped; the allocation-free diverging guard is what proves the loop's
# `%lt` operand without perturbing the gauge.

# (a) the canonical shape: a self-recursive local closure whose BASE arm allocates
# the result. The recursive arm inlines the same body, so it names the base arm's
# own result region — the sibling that wins the structurally-latest use.
(defn s-self-rec (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (letrec [go (fn (m)
                  (when (%not (%int? m)) (error :m-not-int))
                  (if (%lt m 1) (list i i) (go (%sub m 1))))]
      (go 1))
    (assign i (%add i 1))))

# (b) no recursion needed: two mutually exclusive arms reaching ONE local closure
# is enough for both arms' result bindings to name that closure's result region.
(defn s-two-arms-one-callee (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (let [mk (fn (z) (list z z))]
      (if (%lt i 0) (mk i) (mk 7)))
    (assign i (%add i 1))))

# (c) the mixed shape: one arm allocates in place, the other reaches the closure,
# so the two arms' results are distinct regions and only one of them is named
# across the boundary. It strands all the same — the class is the naming, not a
# symmetry between the arms.
(defn s-mixed-arms (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (let [mk (fn (z) (list z z))]
      (if (%lt i 0) (list i i) (mk 7)))
    (assign i (%add i 1))))

# (d) a closure RETURNED from an inlined callee: the caller's temp names the
# region the callee's body minted for the inner lambda, so the closure and its env
# ride the same misplacement.
(defn s-nested-closure (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (let [f (fn () (fn () i))]
      ((f)))
    (assign i (%add i 1))))

# (e) the result carries a heap object graph, so the strand is the whole cascade
# the region's free would reclaim, not one cell.
(defn s-heap-result (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (let [mk (fn (z) (list (string "a" z) (string "b" z)))]
      (if (%lt i 0) (mk i) (mk 7)))
    (assign i (%add i 1))))

# controls ─────────────────────────────────────────────────────────────────────
# Already-bounded shapes: with no branch there is only one holder to place the
# release, and a top-level `defn` callee is not inlined at all. A red subject
# above is the result naming, not the loop or the closure around it.

(defn c-single-call (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (let [mk (fn (z) (list z z))]
      (mk i))
    (assign i (%add i 1))))

(defn mk-top (z)
  (list z z))

(defn c-top-level-callee (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (if (%lt i 0) (mk-top i) (mk-top 7))
    (assign i (%add i 1))))

(defn c-branch-no-call (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (if (%lt i 0) (list i i) (list 7 7))
    (assign i (%add i 1))))

(def self-rec-d (gauge s-self-rec window))
(def two-arms-d (gauge s-two-arms-one-callee window))
(def mixed-arms-d (gauge s-mixed-arms window))
(def nested-closure-d (gauge s-nested-closure window))
(def heap-result-d (gauge s-heap-result window))
(def single-call-d (gauge c-single-call window))
(def top-level-d (gauge c-top-level-callee window))
(def branch-no-call-d (gauge c-branch-no-call window))

(println "region-inline-result-naming deltas over " window " iters:")
(println "  self-rec " self-rec-d "  two-arms " two-arms-d "  mixed-arms "
         mixed-arms-d)
(println "  nested-closure " nested-closure-d "  heap-result " heap-result-d)
(println "  controls: single-call " single-call-d "  top-level " top-level-d
         "  branch-no-call " branch-no-call-d)

# Every leak in this class is at least one whole region per iteration, so a
# surviving strand reads >= 2000 over the window. 100 is slack for the one-time
# intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? single-call-d "control: one call, no branch")
(bounded? top-level-d "control: a top-level callee is not inlined")
(bounded? branch-no-call-d "control: both arms allocate in place")

(bounded? self-rec-d "base-arm result stranded by the recursive arm's inline")
(bounded? two-arms-d
          "arm result stranded by the sibling arm's call to one callee")
(bounded? mixed-arms-d
          "call-arm result stranded beside an allocating sibling arm")
(bounded? nested-closure-d "returned closure stranded by the caller's naming")
(bounded? heap-result-d "heap result graph stranded by the caller's naming")

# Value preservation: naming the result differently must not change what runs.
(assert (= (let [mk (fn (z) (list z z))]
             (first (if (%lt 1 0) (mk 1) (mk 7)))) 7) "two-arm call result lost")
(assert (= (letrec [go (fn (m)
                         (when (%not (%int? m)) (error :m-not-int))
                         (if (%lt m 1) (list 5 5) (go (%sub m 1))))]
             (first (go 1))) 5) "self-recursive base-case result lost")
(assert (= (let [f (fn () (fn () 9))]
             ((f))) 9) "nested closure result lost")

(println "region-inline-result-naming: ok")
