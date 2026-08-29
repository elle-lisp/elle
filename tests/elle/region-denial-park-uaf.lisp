(elle/epoch 12)
# A payload the RUNTIME built is released by the install that displaces it —
# the soundness face (docs/impl/region/owner.md § "Park/unpark symmetry").
#
# The leak face is region-denial-park.lisp: a mediated denial stranded its
# payload because no continuation of the body releases a value the body never
# named. Closing it makes `fiber/resume`, `fiber/refuse` and `fiber/abort` each
# run a release that fired on no path before, so each is a fresh chance to free
# the payload under a reader that still holds it.
#
# The trap: the parent reads the payload BEFORE it displaces it — that is what
# mediation is — and may still hold it afterwards. `fiber/value` is
# pass-through, so a binding of the payload carries a counted reference of its
# own; the release must consume the allocation's reference and no other. Every
# witness below therefore reads a HEAP field (`:primitive`, `:args`) AFTER the
# displacing install. A bare status or arity check passes over a freed payload
# and would have missed this.
#
# The counter-factual is the release running twice, or running on a value the
# body allocated: the payload's region frees while the mediator still holds it,
# and the field read faults at the deref under `--trace=guardfree`.

# ── the mediated subject ─────────────────────────────────────────────────────
(defn denied-body ()
  (let [r (file/read "/no/such/path")]
    (length r)))
(defn mk ()
  (fiber/new denied-body |:fs :error| :deny |:fs|))

# ── (a) the payload held across a resume, read after ─────────────────────────
(defn w-hold-resume (n)
  (let [f (mk)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f (string "answer" n))
      (assert (= (get p :error) :capability-denied)
              "held payload lost its tag across the resume")
      (length (get p :primitive)))))

# ── (b) the resume value read OUT of the payload ─────────────────────────────
# The delivered value lives in the payload's own region, so the release must not
# treat the two as one — the body binds it and reads it past the suspend.
(defn args-body ()
  (let [r (file/read "/no/such/path")]
    (length (get r 0))))
(defn w-alias-resume (n)
  (let [f (fiber/new args-body |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f (get p :args))
      (assert (%gt (length (get p :args)) 0)
              "aliased payload lost its :args across the resume")
      (fiber/value f))))

# ── (c) the payload outlives the fiber, in a container ───────────────────────
# Nothing on the stack holds it once the driver moves on; the holder that must
# still find it alive is the array read back out.
(def @denials @[])
(defn w-stored (n)
  (let [f (mk)]
    (fiber/resume f)
    (push denials (fiber/value f))
    (fiber/resume f n)
    (length (get (get denials (%sub (length denials) 1)) :primitive))))

# ── (d) the refusal face — the child catches and traps again ─────────────────
# `caps-refuse.lisp` in region form: each refusal displaces the park it answers,
# and the parent reads the NEXT denial's payload after the previous release ran.
(defn twice-body ()
  (let [[ok1? e1] (protect (file/read "/no/such/one"))
        [ok2? e2] (protect (file/read "/no/such/two"))]
    (list ok1? ok2?)))
(defn w-refuse-twice (n)
  (let [f (fiber/new twice-body |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (let [first-p (fiber/value f)]
      (fiber/refuse f :first)
      (assert (= (get (get first-p :args) 0) "/no/such/one")
              "first denial's payload freed by the refusal that displaced it")
      (let [second-p (fiber/value f)]
        (assert (= (get (get second-p :args) 0) "/no/such/two")
                "second denial named the wrong path")
        (fiber/refuse f :second)
        (assert (= (get first-p :primitive) "file/read")
                "first payload freed by the second refusal")
        (length (get second-p :primitive))))))

# ── (e) the denial inside a `protect` ────────────────────────────────────────
# `protect` runs the body in an inner fiber, so the denial parks THERE and the
# mediator's install reaches that fiber directly through a `FiberResume` frame,
# never through `fiber/resume`. The payload the parent reads is the inner park's,
# and it must survive the install that displaces it on this route too.
(defn protect-body ()
  (let [[ok? e] (protect (file/read "/no/such/path"))]
    (if ok? 1 0)))
(defn w-protect-resume (n)
  (let [f (fiber/new protect-body |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f (string "c" n))
      (assert (= (get (get p :args) 0) "/no/such/path")
              "payload freed by the resume that displaced the inner park")
      (length (get p :primitive)))))
(defn w-protect-refuse (n)
  (let [f (fiber/new protect-body |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/refuse f :denied)
      (assert (= (get p :error) :capability-denied)
              "payload freed by the refusal that displaced the inner park")
      (length (get p :primitive)))))

# ── (f) the bits collision — the shape a second release detonates on ─────────
# A fiber denied `:io` parks under `SIG_IO`, the very bit `release_parked_signal`
# reads to recognize a yielding io op's request. So this park answers to BOTH
# routes at a resume, and exactly one reference is owed: run them both and the
# payload's region is freed while the mediator still holds it. That is the whole
# reason the record is asked FIRST and the io arm skipped when it claims the park.
(defn io-denied-body ()
  (println "never runs — the fiber is denied :io"))
(defn w-io-resume (n)
  (let [f (fiber/new io-denied-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f n)
      (assert (= (get p :error) :capability-denied)
              "an :io denial's payload was released twice at the resume")
      (length (get p :primitive)))))
(defn w-io-refuse (n)
  (let [f (fiber/new io-denied-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/refuse f :denied)
      (assert (= (get p :error) :capability-denied)
              "an :io denial's payload was released twice at the refusal")
      (length (get p :primitive)))))

# ── (g) the abort face ───────────────────────────────────────────────────────
(defn w-abort (n)
  (let [f (mk)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/abort f :done)
      (assert (= (get p :error) :capability-denied)
              "payload freed by the abort that displaced it")
      (length (get p :primitive)))))

# ── controls — a body-allocated park payload, displaced the same three ways ──
# Its body owns a reference of its own, so the install owes nothing. A release
# added here would free the payload under these very reads.
(defn emit-body (n)
  (let [r (emit :yield {:tag (string "e" n)})]
    5))
(defn c-emit-resume (n)
  (let [f (fiber/new (fn () (emit-body n)) |:yield :error|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f 7)
      (length (get p :tag)))))
(defn c-emit-refuse (n)
  (let [f (fiber/new (fn () (emit-body n)) |:yield :error|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/refuse f :no)
      (length (get p :tag)))))

# ── drive: a fresh payload per iteration keeps region ids churning, so a ─────
# recycled id detonates on its generation stamp rather than reading stale bytes.
(defn drive (reps)
  (var i 0)
  (var a 0)
  (var b 0)
  (var c 0)
  (var d 0)
  (var e 0)
  (var f 0)
  (var g 0)
  (var h 0)
  (var k 0)
  (var m 0)
  (var n 0)
  (while (%lt i reps)
    (assign a (w-hold-resume i))
    (assign b (w-alias-resume i))
    (assign c (w-stored i))
    (assign d (w-refuse-twice i))
    (assign e (w-protect-resume i))
    (assign f (w-protect-refuse i))
    (assign g (w-io-resume i))
    (assign h (w-io-refuse i))
    (assign k (w-abort i))
    (assign m (c-emit-resume i))
    (assign n (c-emit-refuse i))
    (assign denials @[])
    (assign i (%add i 1)))
  (list a b c d e f g h k m n))

(let [r (drive 300)]
  (assert (> (get r 0) 0) "payload freed under a held read past the resume")
  (assert (> (get r 1) 0) "aliased resume value freed under the body's read")
  (assert (> (get r 2) 0) "stored payload freed under the container read")
  (assert (> (get r 3) 0) "payload freed across a refusal chain")
  (assert (> (get r 4) 0) "inner park's payload freed by the resume install")
  (assert (> (get r 5) 0) "inner park's payload freed by the refusal install")
  (assert (> (get r 6) 0) ":io denial's payload released twice at the resume")
  (assert (> (get r 7) 0) ":io denial's payload released twice at the refusal")
  (assert (> (get r 8) 0) "payload freed under the read past the abort")
  (assert (> (get r 9) 0) "control: emit payload freed by the resume install")
  (assert (> (get r 10) 0) "control: emit payload freed by the refusal install"))

(println "region-denial-park-uaf: ok")
