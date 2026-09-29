(elle/epoch 13)
# audited: 2026-09-29
# A restart delivers into an error park, and owes what the raise site left unfunded: the soundness face.
# docs/impl/region/park.md
#
# A fiber stopped on an error parks just past the raising call. The restart
# value takes that call's result, and the continuation releases it like any
# result. An `error` raise is an `Emit`, whose continuation funds its own
# release of the value. Every other raise — a primitive, an instruction, a
# callee, a raise in a replayed frame, an injected refusal over a denial park,
# and a child's error that stops its parent at the parent's `fiber/resume` —
# produced no result, so the delivery must mint. Without it the continuation
# releases a reference the resumer still owns, and the value dies under every
# holder that outlives the restart.
#
# Each face restarts with a FRESH string, keeps it in the fiber's result, and
# compares it against an equal string built afterwards. Where the freed page
# was recycled the compare fails on any run; where it was not, the read is
# stale but mapped, so the sidecar arms guardfree, which unmaps the page and
# faults on it. The `emit` and abort-at-a-yield faces need no delivery mint and
# are the controls: the same program, one path already funded. The leak face
# is region-fiber-restart.lisp.

(defn raise-now []
  (+ 100 (error :boom)))
(defn yield-then-get []
  (yield 1)
  [:inner (get nil :x)])

(defn check [out s]
  (if (= (get out 1) s) 1 0))

# ── a primitive raises in the body ────────────────────────────────────
(defn face-primitive [i]
  (let [f (fiber/new (fn [] [:got (get nil :x)]) |:error|)]
    (fiber/resume f)
    (check (fiber/resume f (string "prim" i)) (string "prim" i))))

# ── an instruction raises in the body ─────────────────────────────────
(defn face-instruction [i]
  (let [f (fiber/new (fn [] [:got (first nil)]) |:error|)]
    (fiber/resume f)
    (check (fiber/resume f (string "instr" i)) (string "instr" i))))

# ── a primitive raises in tail position ───────────────────────────────
(defn face-tail [i]
  (let [f (fiber/new (fn [] (get nil :x)) |:error|)]
    (fiber/resume f)
    (if (= (fiber/resume f (string "tail" i)) (string "tail" i)) 1 0)))

# ── a callee raises on the fiber's first run ──────────────────────────
(defn face-callee [i]
  (let [f (fiber/new (fn [] [:got (raise-now)]) |:error|)]
    (fiber/resume f)
    (check (fiber/resume f (string "callee" i)) (string "callee" i))))

# ── a replayed frame raises ───────────────────────────────────────────
(defn face-replayed [i]
  (let [f (fiber/new (fn [] [:got (yield-then-get)]) |:yield :error|)]
    (fiber/resume f)
    (fiber/resume f)
    (check (get (fiber/resume f (string "replay" i)) 1) (string "replay" i))))

# ── a refusal over a denial park ──────────────────────────────────────
(defn face-refused [i]
  (let [f (fiber/new (fn [] [:got (file/read "/nonexistent-refused")])
                     |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (fiber/refuse f :no)
    (check (fiber/resume f (string "refused" i)) (string "refused" i))))

# ── a child's error stops its parent at the parent's fiber/resume ─────
(defn face-escaped [i]
  (let [p (fiber/new (fn []
                       (let [c (fiber/new (fn [] (error :boom)) |:yield|)]
                         [:parent (fiber/resume c)])) |:error|)]
    (fiber/resume p)
    (check (fiber/resume p (string "escaped" i)) (string "escaped" i))))

# ── controls: an emit park, funded by its continuation ────────────────
(defn control-emit [i]
  (let [f (fiber/new (fn [] [:got (error :boom)]) |:error|)]
    (fiber/resume f)
    (check (fiber/resume f (string "emit" i)) (string "emit" i))))

(defn control-abort [i]
  (let [f (fiber/new (fn [] [:got (yield 1)]) |:yield :error|)]
    (fiber/resume f)
    (fiber/abort f :stop)
    (check (fiber/resume f (string "abort" i)) (string "abort" i))))

# ── drive: a fresh subject each iteration; an early free faults on it ─

(def faces
  [control-emit control-abort face-primitive face-instruction face-tail
   face-callee face-replayed face-refused face-escaped])

(defn drive [reps]
  (let [counts @[0 0 0 0 0 0 0 0 0]]
    (var i 0)
    (while (%lt i reps)
      (var k 0)
      (while (%lt k 9)
        (put counts k (+ (get counts k) ((get faces k) i)))
        (assign k (%add k 1)))
      (assign i (%add i 1)))
    (freeze counts)))

(def reps 400)
(let [r (drive reps)]
  (assert (= (get r 0) reps)
          "control: an emit restart mis-read (harness broken)")
  (assert (= (get r 1) reps)
          "control: an abort restart mis-read (harness broken)")
  (assert (= (get r 2) reps)
          "primitive raise: restart value freed under the body")
  (assert (= (get r 3) reps)
          "instruction raise: restart value freed under the body")
  (assert (= (get r 4) reps) "tail raise: restart value freed under the result")
  (assert (= (get r 5) reps) "callee raise: restart value freed under the body")
  (assert (= (get r 6) reps)
          "replayed raise: restart value freed under the body")
  (assert (= (get r 7) reps) "refusal: restart value freed under the body")
  (assert (= (get r 8) reps)
          "escaped error: restart value freed under the parent"))

(println "region-fiber-restart-uaf: ok")
