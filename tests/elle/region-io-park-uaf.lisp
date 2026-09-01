(elle/epoch 12)
# An io park's `IoRequest` is released by the install that displaces it — the
# soundness face (docs/impl/region/owner.md § "Park/unpark symmetry").
#
# The leak face is region-io-park.lisp: an aborted or refused io park stranded
# its request, because the release that answers for a runtime-built payload ran
# on the resume path alone. Closing it makes `fiber/refuse` and `fiber/abort`
# each run a decref that fired on no path before, so each is a fresh chance to
# free the request under a reader that still holds it.
#
# The trap: the mediator reads the request BEFORE it ends the park — that is what
# reading `(fiber/value f)` on a `SIG_IO` signal is for — and may still hold it
# afterwards. `fiber/value` is pass-through, so a binding of the request carries a
# counted reference of its own; the release must consume the allocation's
# reference and no other. Every witness below therefore DEREFERENCES the request
# after the displacing install. A bare status check passes over a freed request
# and would have missed this.
#
# The second trap is the bits collision. A fiber denied `:io` parks under
# `SIG_IO`, the very bit that would say "this park's payload is an `IoRequest`",
# so bits alone hand one park to both readings and one reference is owed, not
# two. The io arm asks for an `IoRequest` instead, which a denial's struct never
# is, so the two name disjoint payloads. Ordering the two calls cannot stand in
# for that: the ledger record lives on the fiber that was DENIED, and the
# `protect` witnesses below install on a fiber that merely RELAYS the park, where
# there is no record to ask first. Both faces are here, bare and through
# `protect`, and each frees the payload under the mediator's read if the io arm
# claims what the record owns.
#
# The third trap is the ORDER of the two arms. The io arm decides by reading the
# parked value, and the record's release can be the payload's last, so the io arm
# runs first. Only the witness that holds NOTHING can catch that — a bound payload
# keeps the region alive across either order.
#
# The counter-factual is the release running twice, or running on a value the
# body allocated: the region frees while a holder still names it, and the read
# past the install faults at the deref under `--trace=guardfree`.

# ── the parked subject ───────────────────────────────────────────────────────
# A timer nothing will fire, under a mask that brings the park back to this
# driver as data.
(defn io-body ()
  (let [r (ev/sleep 10000)]
    (length "done")))
(defn mk ()
  (fiber/new io-body |:io :error|))

# ── (a) the request held across an abort, read after ─────────────────────────
(defn w-hold-abort (n)
  (let [f (mk)]
    (fiber/resume f)
    (let [req (fiber/value f)]
      (fiber/abort f (string "stop" n))
      (assert (= (type-of req) :io-request)
              "held request lost its type across the abort")
      (length (string req)))))

# ── (b) the request held across a refusal, read after ────────────────────────
(defn w-hold-refuse (n)
  (let [f (mk)]
    (fiber/resume f)
    (let [req (fiber/value f)]
      (fiber/refuse f (string "no" n))
      (assert (= (type-of req) :io-request)
              "held request lost its type across the refusal")
      (length (string req)))))

# ── (c) the request outlives the fiber, in a container ───────────────────────
# Nothing on the stack holds it once the driver moves on; the holder that must
# still find it alive is the array read back out.
(def @requests @[])
(defn w-stored (n)
  (let [f (mk)]
    (fiber/resume f)
    (push requests (fiber/value f))
    (fiber/abort f n)
    (length (string (get requests (%sub (length requests) 1))))))

# ── (d) the refusal chain — the body catches and parks again ─────────────────
# Each refusal ends the park it answers, and the driver reads the PREVIOUS
# request after the next release has run.
(defn twice-body ()
  (let [[ok1? e1] (protect (ev/sleep 10000))
        [ok2? e2] (protect (ev/sleep 10000))]
    (list ok1? ok2?)))
(defn w-refuse-twice (n)
  (let [f (fiber/new twice-body |:io :error|)]
    (fiber/resume f)
    (let [first-r (fiber/value f)]
      (fiber/refuse f :first)
      (assert (= (type-of first-r) :io-request)
              "first request freed by the refusal that displaced it")
      (let [second-r (fiber/value f)]
        (fiber/refuse f :second)
        (assert (= (type-of first-r) :io-request)
                "first request freed by the second refusal")
        (length (string second-r))))))

# ── (e) the park inside a `protect` ──────────────────────────────────────────
# `protect` runs the body in an inner fiber, so the request parks THERE and the
# outer fiber awaits it through a `FiberResume` frame. The request the mediator
# reads is the inner park's, and it must survive the install on this route too.
(defn protect-body ()
  (let [[ok? e] (protect (ev/sleep 10000))]
    (if ok? 1 0)))
(defn w-protect-abort (n)
  (let [f (fiber/new protect-body |:io :error|)]
    (fiber/resume f)
    (let [req (fiber/value f)]
      (fiber/abort f n)
      (assert (= (type-of req) :io-request)
              "inner park's request freed by the abort install")
      (length (string req)))))
(defn w-protect-refuse (n)
  (let [f (fiber/new protect-body |:io :error|)]
    (fiber/resume f)
    (let [req (fiber/value f)]
      (fiber/refuse f n)
      (assert (= (type-of req) :io-request)
              "inner park's request freed by the refusal install")
      (length (string req)))))

# ── (f) the bits collision, at the two installs the io arm newly reaches ─────
# A fiber denied `:io` parks under `SIG_IO` with a payload the DENIAL path built,
# not a request. The record owns that payload's release and the io arm has no
# claim on it. Claim it anyway and the struct frees while the mediator holds it —
# the field read below is what faults.
(defn io-denied-body ()
  (println "never runs — the fiber is denied :io"))
(defn w-denied-abort (n)
  (let [f (fiber/new io-denied-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/abort f n)
      (assert (= (get p :error) :capability-denied)
              "an :io denial's payload was released twice at the abort")
      (length (get p :primitive)))))
(defn w-denied-refuse (n)
  (let [f (fiber/new io-denied-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/refuse f n)
      (assert (= (get p :error) :capability-denied)
              "an :io denial's payload was released twice at the refusal")
      (length (get p :primitive)))))

# ── (g) the collision on a fiber that RELAYS the park ────────────────────────
# `protect` runs the denied call in an inner fiber, so the ledger record is
# written THERE while the outer fiber's slot holds the same payload under the
# same `SIG_IO`. The install reaches the outer fiber and finds no record, so
# asking the record first cannot make the two readings exclusive here — only the
# io arm's own reading can. Reading the payload's type is what keeps this witness
# standing; reading the bit frees the struct under the mediator.
(defn denied-protect-body ()
  (let [[ok? e] (protect (ev/sleep 1))]
    (if ok? 1 0)))
(defn w-denied-protect-abort (n)
  (let [f (fiber/new denied-protect-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/abort f n)
      (assert (= (get p :error) :capability-denied)
              "a relayed :io denial's payload was released twice at the abort")
      (length (get p :primitive)))))
(defn w-denied-protect-refuse (n)
  (let [f (fiber/new denied-protect-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/refuse f n)
      (assert (= (get p :error) :capability-denied)
              "a relayed :io denial's payload was released twice at the refusal")
      (length (get p :primitive)))))

# ── (h) the collision with NOTHING holding the payload ───────────────────────
# The two readings run one after the other at every install, and the denial arm's
# decref can be the payload's last — nobody here reads `(fiber/value f)`, so no
# counted reference stands behind it. The io arm reaches its verdict by READING
# the parked value, so it must run first; run it after and it dereferences a value
# the denial arm just freed. The witnesses above cannot see this: binding the
# payload is what keeps the region alive across the install, so every held face
# passes either way. What faults here is the runtime's own read, so completing IS
# the assertion.
(defn w-denied-unheld-resume (n)
  (let [f (fiber/new io-denied-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (fiber/resume f n)
    (fiber/status f)))
(defn w-denied-unheld-abort (n)
  (let [f (fiber/new io-denied-body |:error :io| :deny |:io|)]
    (fiber/resume f)
    (fiber/abort f n)
    (fiber/status f)))

# ── controls — a body-allocated park payload, ended the same two ways ────────
# Its body owns a reference of its own, so the install owes nothing. A release
# reaching this park would free the payload under these very reads.
(defn emit-body (n)
  (let [r (emit :yield {:tag (string "e" n)})]
    5))
(defn c-emit-abort (n)
  (let [f (fiber/new (fn () (emit-body n)) |:yield :error|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/abort f :done)
      (length (get p :tag)))))
(defn c-emit-refuse (n)
  (let [f (fiber/new (fn () (emit-body n)) |:yield :error|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/refuse f :no)
      (length (get p :tag)))))

# ── drive: a fresh park per iteration keeps region ids churning, so a recycled
# id detonates on its generation stamp rather than reading stale bytes.
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
  (var q 0)
  (var r 0)
  (var s :none)
  (var t :none)
  (while (%lt i reps)
    (assign a (w-hold-abort i))
    (assign b (w-hold-refuse i))
    (assign c (w-stored i))
    (assign d (w-refuse-twice i))
    (assign e (w-protect-abort i))
    (assign f (w-protect-refuse i))
    (assign g (w-denied-abort i))
    (assign h (w-denied-refuse i))
    (assign q (w-denied-protect-abort i))
    (assign r (w-denied-protect-refuse i))
    (assign s (w-denied-unheld-resume i))
    (assign t (w-denied-unheld-abort i))
    (assign k (c-emit-abort i))
    (assign m (c-emit-refuse i))
    (assign requests @[])
    (assign i (%add i 1)))
  (list a b c d e f g h k m q r s t))

(let [r (drive 200)]
  (assert (> (get r 0) 0) "request freed under a held read past the abort")
  (assert (> (get r 1) 0) "request freed under a held read past the refusal")
  (assert (> (get r 2) 0) "stored request freed under the container read")
  (assert (> (get r 3) 0) "request freed across a refusal chain")
  (assert (> (get r 4) 0) "inner park's request freed by the abort install")
  (assert (> (get r 5) 0) "inner park's request freed by the refusal install")
  (assert (> (get r 6) 0) ":io denial's payload released twice at the abort")
  (assert (> (get r 7) 0) ":io denial's payload released twice at the refusal")
  (assert (> (get r 8) 0) "control: emit payload freed by the abort install")
  (assert (> (get r 9) 0) "control: emit payload freed by the refusal install")
  (assert (> (get r 10) 0)
          "relayed :io denial's payload released twice at the abort")
  (assert (> (get r 11) 0)
          "relayed :io denial's payload released twice at the refusal")
  # Answering the denied `println` lets the body run on to its next `:io` call,
  # which is denied in turn — so a resumed `:io`-denied fiber parks again.
  (assert (= (get r 12) :paused)
          "an unheld :io denial did not run on after its resume answered it")
  (assert (= (get r 13) :error)
          "an unheld :io denial did not end :error under its abort"))

(println "region-io-park-uaf: ok")
