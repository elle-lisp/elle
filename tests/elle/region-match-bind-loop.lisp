(elle/epoch 12)
# Every binder records its scope (docs/impl/region/mechanism.md § "Every binder
# records its scope").
#
# A `Var` read inside a loop is extended to the loop node when the binding it
# names is bound OUTSIDE that loop — the body re-reads it every iteration, so its
# region must outlive the loop. The premise is a containment test over the
# binding's recorded SCOPE NODE, and an unrecorded binder has no scope node: an
# absent scope reads as "bound outside" and hoists the release past a loop whose
# body re-allocates the value every iteration.
#
# A `match` arm's pattern binding is the case that bites, because it carries a
# region it did not allocate. `{:type :a :v v}` binds `v` by an uncounted read out
# of the scrutinee (rules.md Rule 4's borrowing node), so `v` resolves to the
# SCRUTINEE's region — and a read of `v` in the arm body therefore decides where a
# whole fresh scrutinee is released. Hoisted out of the loop, one release covers
# N iterations, and N−1 scrutinees — with every object each holds — are held to
# fiber teardown.
#
# Each subject runs its own loop INLINE: the strand is per iteration of the loop
# the match sits in, so a thunk called per iteration would reclaim at the call
# boundary and hide it. The gauge is `arena/count` across one call, which must be
# bounded — the leak is at least one whole scrutinee per iteration.
#
# This file is the LEAK gauge; the soundness complement is
# region-match-bind-loop-uaf.lisp and the per-op rate is the `struct-match` probe
# in tests/elle/oracle.lisp.

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

# (a) the canonical shape: a struct scrutinee whose bound field the taken arm
# reads. The read is what extends the scrutinee's release, so the scrutinee — one
# whole region per iteration — is what a hoist strands.
(defn s-taken (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match {:type :a :v i}
      {:type :a :v v} v
      _ 0)
    (assign i (%add i 1))))

# (b) the read is in an arm the scrutinee never matches. The extension is
# structural, not path-sensitive, so an arm no execution takes strands the
# scrutinee exactly as the taken one does.
(defn s-untaken (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match {:type :a :v i}
      {:type :b :v v} v
      {:type :a} 1
      _ 0)
    (assign i (%add i 1))))

# (c) the scrutinee holds HEAP fields, so the strand is the struct plus everything
# its free cascade would reclaim — the cost scales with the scrutinee's object
# graph, not with the projection.
(defn s-heap-field (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match {:type :a :v (string "s" i) :w (string "w" i)}
      {:type :a :v v} v
      _ 0)
    (assign i (%add i 1))))

# (d) the projection is used but not returned: the arm's value is a literal, so
# it is the READ of the bound name, not the arm's result, that places the release.
(defn s-used-not-returned (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match {:type :a :v i}
      {:type :a :v v} (begin
                        (%int? v)
                        1)
      _ 0)
    (assign i (%add i 1))))

# (e) a list scrutinee under an array pattern — the class is the pattern binding,
# not the struct.
(defn s-list (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match (list :a i)
      [x y] y
      _ 0)
    (assign i (%add i 1))))

# (f) the read is in the arm's GUARD rather than its body. A guard runs under the
# same arm scope, so it must place the release the same way.
(defn s-guard (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match {:type :a :v i}
      {:type :a :v v} when
      (< v 0) 1
      _ 0)
    (assign i (%add i 1))))

# (g) the match sits in an INNER loop: the scope node is inside both, so the
# release must stay in the inner body rather than being hoisted to either loop.
(defn s-nested (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (var k 0)
    (while (%lt k 4)
      (match {:type :a :v k}
        {:type :a :v v} v
        _ 0)
      (assign k (%add k 1)))
    (assign i (%add i 1))))

# controls ─────────────────────────────────────────────────────────────────────
# Already-bounded shapes: nothing reads a bound name, so no read extends the
# scrutinee and the strand cannot form. A red subject above is the scope
# defaulting, not the loop or the match around it.

(defn c-no-binding (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match {:type :a :v i}
      {:type :a} 1
      _ 0)
    (assign i (%add i 1))))

(defn c-bound-unused (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    (match {:type :a :v i}
      {:type :a :v v} 1
      _ 0)
    (assign i (%add i 1))))

(defn c-bare-scrutinee (n)
  (when (%not (%int? n)) (error :n-not-int))
  (var i 0)
  (while (%lt i n)
    {:type :a :v i}
    (assign i (%add i 1))))

(def taken-d (gauge s-taken window))
(def untaken-d (gauge s-untaken window))
(def heap-field-d (gauge s-heap-field window))
(def used-not-returned-d (gauge s-used-not-returned window))
(def list-d (gauge s-list window))
(def guard-d (gauge s-guard window))
(def nested-d (gauge s-nested window))
(def no-binding-d (gauge c-no-binding window))
(def bound-unused-d (gauge c-bound-unused window))
(def bare-scrutinee-d (gauge c-bare-scrutinee window))

(println "region-match-bind-loop deltas over " window " iters:")
(println "  taken " taken-d "  untaken " untaken-d "  heap-field " heap-field-d)
(println "  used-not-returned " used-not-returned-d "  list " list-d "  guard "
         guard-d)
(println "  nested " nested-d)
(println "  controls: no-binding " no-binding-d "  bound-unused " bound-unused-d
         "  bare " bare-scrutinee-d)

# Every leak in this class is at least one whole scrutinee per iteration, so a
# surviving strand reads >= 2000 over the window. 100 is slack for the one-time
# intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? no-binding-d "control: pattern binding nothing")
(bounded? bound-unused-d "control: bound name never read")
(bounded? bare-scrutinee-d "control: the scrutinee alone, discarded")

(bounded? taken-d "scrutinee stranded by the taken arm's read of its projection")
(bounded? untaken-d "scrutinee stranded by a read in an arm never taken")
(bounded? heap-field-d "scrutinee object graph stranded by a projection read")
(bounded? used-not-returned-d
          "scrutinee stranded by a read that is not the arm's value")
(bounded? list-d "list scrutinee stranded by an array pattern's projection read")
(bounded? guard-d "scrutinee stranded by a read in the arm's guard")
(bounded? nested-d "scrutinee stranded out of the inner loop")

# Value preservation: placing the release must not change what runs.
(assert (= (match {:type :a :v 7}
             {:type :a :v v} v
             _ 0) 7) "taken arm result lost")
(assert (= (match {:type :a :v 7}
             {:type :b :v v} v
             {:type :a} 1
             _ 0) 1) "untaken-arm shape reached the wrong arm")
(assert (= (match [:a 7]
             [x y] y
             _ 0) 7) "array pattern result lost")

(println "region-match-bind-loop: ok")
