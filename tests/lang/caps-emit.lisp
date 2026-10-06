(elle/epoch 15)
# audited: 2026-10-06
# A literal emit of a bit the fiber withholds raises nothing, and parks the fiber on a denial.
# docs/signals/emit.md
# docs/signals/capabilities.md
#
# Counter-factual: the `Emit` instruction never read the withheld set, so each
# emit below raised its signal as if nothing were denied. The parent then read
# the emitted payload where a denial belongs, and a mask audited only a child
# that chose to cooperate.

(signal :caps-emit/beat)

(defn denial? [v]
  (and (struct? v) (= :capability-denied (get v :error))))

# ── The payload ─────────────────────────────────────────────────────────

(let [f (fiber/new (fn [] (emit :caps-emit/beat 7)) |:caps-emit/beat :error|
                   :deny |:caps-emit/beat|)
      d (fiber/resume f)]
  (assert (fiber/denied? f) "the fiber parks on a denial")
  (assert (denial? d) "the parent reads the denial, not the payload 7")
  (assert (= |:caps-emit/beat| (get d :denied)) "the denial names the bit")
  (assert (= "fiber/emit" (get d :primitive))
          "the literal form names the primitive the dynamic form calls")
  (assert (= fiber/emit (get d :func)) "and holds that primitive")
  (assert (= [|:caps-emit/beat| 7] (get d :args))
          "the arguments are the signal as a set, and the payload"))

# The dynamic form gives the same payload, its own first argument aside.
(def which :caps-emit/beat)
(let [literal (fiber/new (fn [] (emit :caps-emit/beat 1))
                         |:caps-emit/beat :error| :deny |:caps-emit/beat|)
      dynamic (fiber/new (fn [] (emit which 1)) |:caps-emit/beat :error|
                         :deny |:caps-emit/beat|)
      l (fiber/resume literal)
      d (fiber/resume dynamic)]
  (assert (= (get l :denied) (get d :denied)) "both forms deny the same bits")
  (assert (= (get l :func) (get d :func)) "and name the same primitive")
  (assert (= [:caps-emit/beat 1] (get d :args))
          "the dynamic form keeps the keyword it was given"))

# Only the withheld bits are denied. The emit raises nothing at all, so the
# :yield it also names never reaches the parent either.
(let [f (fiber/new (fn [] (emit |:caps-emit/beat :yield| 3))
                   |:caps-emit/beat :yield :error| :deny |:caps-emit/beat|)
      d (fiber/resume f)]
  (assert (denial? d) "a set that names a withheld bit is denied")
  (assert (= |:caps-emit/beat| (get d :denied))
          "the denial names that bit alone")
  (assert (= [|:caps-emit/beat :yield| 3] (get d :args))
          "the arguments keep the whole set"))

# A bit the fiber holds is raised as before.
(let [f (fiber/new (fn [] (emit :caps-emit/beat 7)) |:caps-emit/beat :error|
                   :deny |:fs|)]
  (assert (= 7 (fiber/resume f)) "an emit of a held bit raises its payload"))

# ── The resume answers the emit ─────────────────────────────────────────

(let [f (fiber/new (fn [] (+ 1 (emit :caps-emit/beat 7)))
                   |:caps-emit/beat :error| :deny |:caps-emit/beat|)]
  (fiber/resume f)
  (assert (= 11 (fiber/resume f 10)) "the resume value is the emit's result")
  (assert (= :dead (fiber/status f))))

# The parent grants the denial by raising the signal itself.
(let [child (fiber/new (fn [] (+ 1 (emit :caps-emit/beat 7)))
                       |:caps-emit/beat :error| :deny |:caps-emit/beat|)
      relay (fiber/new (fn []
                         (let [d (fiber/resume child)]
                           (fiber/resume child
                           (apply (get d :func) (get d :args)))))
                       |:caps-emit/beat|)]
  (assert (= 7 (fiber/resume relay)) "the relay raised the child's signal")
  (assert (= 11 (fiber/resume relay 10))
          "and its answer reached the child's emit"))

# ── yield and error ─────────────────────────────────────────────────────

(let [f (fiber/new (fn []
                     (yield 1)
                     :after) |:yield :error| :deny |:yield|)
      d (fiber/resume f)]
  (assert (denial? d) "a fiber denied :yield cannot yield")
  (assert (= |:yield| (get d :denied)))
  (assert (= [|:yield| 1] (get d :args)))
  (assert (= :after (fiber/resume f)) "the resume continues past the yield"))

(let [f (fiber/new (fn [] (error {:error :mine})) |:error| :deny |:error|)
      d (fiber/resume f)]
  (assert (denial? d) "a fiber denied :error cannot raise an error")
  (assert (= :paused (fiber/status f)) "the denial parks the fiber")
  (assert (= [|:error| {:error :mine}] (get d :args)))
  (assert (= 5 (fiber/resume f 5)) "the resume answers the error expression"))

# ── A denial deeper than the fiber body ─────────────────────────────────

(defn beat-twice [x]
  (+ (emit :caps-emit/beat x) (emit :caps-emit/beat x)))
(defn via [x]
  (+ 100 (beat-twice x)))
(let [f (fiber/new (fn [] (via 1)) |:caps-emit/beat :error|
                   :deny |:caps-emit/beat|)]
  (assert (denial? (fiber/resume f)) "the first emit in a callee is denied")
  (assert (denial? (fiber/resume f 2)) "and so is the second")
  (assert (= 105 (fiber/resume f 3)) "each resume answers its own emit"))

(defn beat-tail [x]
  (emit :caps-emit/beat x))
(let [f (fiber/new (fn [] (beat-tail 4)) |:caps-emit/beat :error|
                   :deny |:caps-emit/beat|)]
  (assert (denial? (fiber/resume f)) "an emit in tail position is denied")
  (assert (= 9 (fiber/resume f 9)) "and the resume is the body's result"))

(defn fail-deep []
  (error {:error :deep}))
(let [f (fiber/new (fn [] (+ 1 (fail-deep))) |:error| :deny |:error|)]
  (assert (denial? (fiber/resume f)) "an error in a callee is denied")
  (assert (= 3 (fiber/resume f 2)) "the callee's caller runs on"))

# ── Routing ─────────────────────────────────────────────────────────────

# A mask that does not name the denied bit lets the denial pass the parent.
(let [outer (fiber/new (fn []
                         (let [inner (fiber/new (fn [] (emit :caps-emit/beat 1))
                               |:error| :deny |:caps-emit/beat|)]
                           (fiber/resume inner))) |:caps-emit/beat|)]
  (assert (denial? (fiber/resume outer)) "the denial reaches the fiber above"))

# A fiber made inside a sandbox keeps the sandbox's denial.
(let [sandbox (fiber/new (fn []
                           (fiber/new (fn [] (emit :caps-emit/beat 1))
                                      |:caps-emit/beat :error|)) |:error|
                         :deny |:caps-emit/beat|)
      made (fiber/resume sandbox)]
  (assert (denial? (fiber/resume made)) "the made fiber cannot raise the bit"))

# ── Refusal ─────────────────────────────────────────────────────────────

(let [f (fiber/new (fn []
                     (let [[ok? e] (protect (emit :caps-emit/beat 1))]
                       [ok? e])) |:caps-emit/beat :error|
                   :deny |:caps-emit/beat|)]
  (fiber/resume f)
  (assert (fiber/denied? f))
  (fiber/refuse f :no)
  (assert (= [false :no] (fiber/value f))
          "the refusal raises at the emit, where protect catches it"))

(println "caps-emit: OK")
