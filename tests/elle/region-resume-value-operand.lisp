(elle/epoch 13)
# audited: 2026-09-28
# A resume value an operand consumes is released like a bound one: named by the ANF lift, freed with its holder.
#
# docs/impl/anf.md
# docs/impl/region/park.md
#
# The resume value crosses from the resumer uncounted, so `lower_emit` mints a
# reference for the frame, and the frame releases it through a slot. A `let`
# names the slot; an operand position has the ANF lift name it. Unnamed, the mint
# strands one region per resume that delivers a heap value, and every value
# assertion still passes. So the gauge below is the counter-factual, and the
# witnesses after it are the other side: the value must outlive the resumer's
# release and a further park, and it must survive in the array the body returns.

(def window 1000)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# subjects ─────────────────────────────────────────────────────────────────────
# Each drives one fiber to its first park, then resumes it with a fresh heap
# value and drops it.

(defn drive [body mask]
  (let [f (fiber/new body mask)]
    (fiber/resume f)
    (fiber/resume f @[:v])))

# The array-literal element.
(defn element []
  (drive (fn [] [:got (yield 1)]) |:yield|))

# The struct-literal value.
(defn struct-value []
  (drive (fn [] {:got (yield 1)}) |:yield|))

# A call argument whose result carries nothing of the resume value.
(defn call-arg []
  (drive (fn [] (length (yield 1))) |:yield|))

# A non-last `begin` slot, which discards the resume value.
(defn discarded []
  (drive (fn []
           (begin
             (yield 1)
             :done)) |:yield|))

# A raise a restart resumes. The raise is an `Emit` too.
(defn raise-element []
  (drive (fn [] [:got (error :stop)]) |:error|))

# controls ─────────────────────────────────────────────────────────────────────
# The same bodies with the resume value bound first, and the element shape with
# an immediate resume value. All three are bounded without the name, so a red
# subject above is the operand position and not the surrounding drive.

(defn bound-element []
  (drive (fn []
           (let [v (yield 1)]
             [:got v])) |:yield|))
(defn bound-raise []
  (drive (fn []
           (let [v (error :stop)]
             [:got v])) |:error|))
(defn immediate-element []
  (let [f (fiber/new (fn [] [:got (yield 1)]) |:yield|)]
    (fiber/resume f)
    (fiber/resume f 5)))

(def element-d (measure element 100 window))
(def struct-d (measure struct-value 100 window))
(def call-arg-d (measure call-arg 100 window))
(def discarded-d (measure discarded 100 window))
(def raise-d (measure raise-element 100 window))
(def bound-element-d (measure bound-element 100 window))
(def bound-raise-d (measure bound-raise 100 window))
(def immediate-d (measure immediate-element 100 window))

(println "region-resume-value-operand deltas over " window " iters:")
(println "  element " element-d "  struct " struct-d "  call arg " call-arg-d
         "  discarded " discarded-d "  raise " raise-d)
(println "  controls: bound " bound-element-d "  bound raise " bound-raise-d
         "  immediate " immediate-d)

# A strand is one region per resume, so it reads ≥ window. 100 is slack for the
# one-time intercept.
(defn bounded? [d label]
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? bound-element-d "control: bound element")
(bounded? bound-raise-d "control: bound raise")
(bounded? immediate-d "control: immediate element")
(bounded? element-d "resume value as an array-literal element")
(bounded? struct-d "resume value as a struct-literal value")
(bounded? call-arg-d "resume value as a call argument")
(bounded? discarded-d "resume value discarded by begin")
(bounded? raise-d "restarted raise as an array-literal element")

# witnesses ────────────────────────────────────────────────────────────────────
# Each resume value is a fresh array built by a call whose result the resumer
# releases once `fiber/resume` returns, so the frame's own reference is all that
# holds it. A fresh value per iteration keeps region ids churning, so a freed and
# recycled region reads wrong or faults under guardfree.

(defn fresh [n]
  (array "a" (string "b" n)))

# The element outlives a further park: the second yield parks with the first
# resume value held for the array not yet built.
(defn held-across-park [n]
  (let [f (fiber/new (fn [] [(yield 0) (yield 1)]) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (fresh n))
    (fresh (%add n 1))
    (fiber/resume f :x)))

# The same, as call arguments.
(defn args-across-park [n]
  (let [f (fiber/new (fn [] (array (yield 0) (yield 1))) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (fresh n))
    (fresh (%add n 1))
    (fiber/resume f :x)))

# The element outlives the fiber that built its array.
(defn outlives-fiber [n]
  (let [f (fiber/new (fn [] [:got (yield 1)]) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (fresh n))))

# A restarted raise's element outlives the fiber too.
(defn raise-outlives-fiber [n]
  (let [f (fiber/new (fn [] [:got (error :stop)]) |:error|)]
    (fiber/resume f)
    (fiber/resume f (fresh n))))

(var k 0)
(while (%lt k 200)
  (let [r (held-across-park k)]
    (assert (= (get (get r 0) 1) (string "b" k)) "element held across a park")
    (assert (= (get r 1) :x) "second element"))
  (let [r (args-across-park k)]
    (assert (= (get (get r 0) 1) (string "b" k))
            "call argument held across a park"))
  (let [r (outlives-fiber k)]
    (fresh k)
    (assert (= (get (get r 1) 1) (string "b" k)) "element outlives its fiber"))
  (let [r (raise-outlives-fiber k)]
    (fresh k)
    (assert (= (get (get r 1) 1) (string "b" k))
            "restarted raise's element outlives its fiber"))
  (assign k (%add k 1)))

# Value preservation for every subject.
(assert (= (get (element) 0) :got) "element result")
(assert (= (get (get (element) 1) 0) :v) "element value")
(assert (= (get (get (struct-value) :got) 0) :v) "struct value")
(assert (= (call-arg) 1) "call-arg result")
(assert (= (discarded) :done) "discarded result")
(assert (= (get (get (raise-element) 1) 0) :v) "raise element value")

(println "region-resume-value-operand: ok")
