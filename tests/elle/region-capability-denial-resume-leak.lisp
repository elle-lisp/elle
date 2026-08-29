(elle/epoch 12)
# Counterfactual for the mediated-denial park's stranded payload region.
#
# A fiber that calls a withheld primitive parks the `{:error :capability-denied
# …}` payload the VM builds in its place, and the parent mediates: it reads the
# payload, decides, and resumes the fiber with a stand-in result. That park has
# no body reference — the denied primitive never ran, so the replayed frame's
# result release targets the parent's resume value — and the resume owes the
# payload's region one decref (docs/impl/region/owner.md § "Park/unpark
# symmetry", "A park with no body reference owes one release at the resume").
#
# Baseline stranded two regions per mediated call, growing without bound in any
# loop that mediates. The counter-factual is the region count over a fixed loop:
# a correct runtime keeps it flat, and the discard face — a denial the parent
# never resumes past — was already flat, so the growth belongs to the resume.

# A mediated denial in CALL position: `let` keeps the denied call
# mid-activation, so the interpreter denies through `handle_capability_denial`.
(defn mediate-call []
  (let [f (fiber/new (fn []
                       (let [r (file/read "/no/such/path")]
                         5)) |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (fiber/resume f 7)))

# The same denial in TAIL position, through `handle_capability_denial_tail` (and
# its JIT twin `jit_capability_denial` once the loop makes the body hot).
(defn read-blocked []
  (file/read "/no/such/path"))

(defn mediate-tail []
  (let [f (fiber/new (fn [] (read-blocked)) |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (fiber/resume f 7)))

# The control: the parent reads the payload and drops the fiber without
# resuming past the denial. `release_discarded_signal` runs the same single
# decref there, so this face was never the leak.
(defn discard-denial []
  (let [f (fiber/new (fn []
                       (let [r (file/read "/no/such/path")]
                         5)) |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (get (fiber/value f) :error)))

# The second control: an ordinary park resumed the same way. Its payload IS
# body-owned, so nothing here may release it — this is the shape the fix must
# leave alone.
(defn mediate-yield []
  (let [f (fiber/new (fn []
                       (let [r (emit :yield {:a 1})]
                         5)) |:yield|)]
    (fiber/resume f)
    (fiber/resume f 7)))

(defn region-growth [f n]
  # Warm first: the first iterations mint the shape's constant regions (code,
  # closure templates), which are not per-op growth.
  (each i (range 0 50)
    (f))
  (let [before (arena/region-count)]
    (each i (range 0 n)
      (f))
    (%sub (arena/region-count) before)))

(let [d (region-growth mediate-call 400)]
  (assert (%lt d 40)
          (string "mediated capability denial (call position) strands regions: "
                  "400 mediations grew the region count by " d
                  " (must stay bounded — the resume owes the payload one decref)")))

(let [d (region-growth mediate-tail 400)]
  (assert (%lt d 40)
          (string "mediated capability denial (tail position) strands regions: "
                  "400 mediations grew the region count by " d
                  " (must stay bounded — the resume owes the payload one decref)")))

(let [d (region-growth discard-denial 400)]
  (assert (%lt d 40)
          (string "unmediated capability denial strands regions: 400 denials "
                  "grew the region count by " d)))

(let [d (region-growth mediate-yield 400)]
  (assert (%lt d 40)
          (string "resumed yield park strands regions: 400 parks grew the "
                  "region count by " d)))

# ── The over-free face ─────────────────────────────────────────────────
# The resume's decref answers for the payload's own leftover reference, never
# for a holder's. A parent that binds the payload before resuming still holds a
# counted reference of it, so every field must read back intact after the
# resume — a decref that took the holder's reference instead frees the struct
# under this read.
(each i (range 0 200)
  (let [f (fiber/new (fn []
                       (let [r (file/read "/no/such/path")]
                         5)) |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f 7)
      # Churn between the resume and the read so a prematurely-freed payload
      # region is recycled into a different heap object before it is derefed.
      (def junk (@string))
      (%string-push junk "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")
      (assert (= :capability-denied (get p :error))
              "a payload the parent still holds survives the resume")
      (assert ((get p :denied) :fs)
              "the held payload's :denied set survives the resume")
      (assert (= "file/read" (get p :primitive))
              "the held payload's :primitive string survives the resume"))))

# ── Semantics ──────────────────────────────────────────────────────────
# The release must not disturb what mediation returns: the fiber continues past
# the denied call with the parent's stand-in value as that call's result.
(let [f (fiber/new (fn []
                     (let [r (file/read "/no/such/path")]
                       (+ r 1))) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "a denied fiber parks")
  (assert (= (fiber/resume f 41) 42)
          "the mediating resume value stands in for the denied call's result")
  (assert (= (fiber/status f) :dead) "the mediated fiber runs to completion"))

(println "region-capability-denial-resume-leak: OK")
