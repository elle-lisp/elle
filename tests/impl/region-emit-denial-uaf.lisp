(elle/epoch 15)
# audited: 2026-10-06
# A denied emit's payload and the value it holds survive every install that displaces the park.
# docs/impl/region/park.md
#
# The leak face is region-emit-denial.lisp. Each install that displaces a denied
# emit runs a release, so each is a chance to free the denial struct, or the
# emitted value inside its `:args`, under a reader that still holds it.
#
# The trap: the mediator reads the payload BEFORE it displaces it, and may hold
# it afterwards. Every witness below reads a HEAP field after the displacing
# install, and the body reads the emitted value past the resume. A status or
# arity check passes over a freed payload, and would have missed the release
# running twice. The sidecar arms guardfree, so a freed read faults at its deref.

(signal :region-emit-denial-uaf/beat)

(defn denial? [v]
  (and (struct? v) (= :capability-denied (get v :error))))

# The body keeps the value it emitted and reads it after the resume.
(defn denied-body (n)
  (let [mine (string "mine" n)
        r (emit :region-emit-denial-uaf/beat mine)]
    (+ (length mine) (length r))))
(defn mk (n)
  (fiber/new (fn () (denied-body n)) |:region-emit-denial-uaf/beat :error|
             :deny |:region-emit-denial-uaf/beat|))

# ── (a) the payload held across a resume, read after ─────────────────────────
(defn w-hold-resume (n)
  (let [f (mk n)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (assert (denial? p) "the emit was not denied")
      (let [r (fiber/resume f (string "answer" n))]
        (assert (= (get p :primitive) "fiber/emit")
                "held payload lost its name across the resume")
        (+ r (length (get (get p :args) 1)))))))

# ── (b) the resume value read OUT of the payload ─────────────────────────────
# The emitted value goes back as the answer, so the body reads its own value
# through the denial's `:args` past the suspend.
(defn w-alias-resume (n)
  (let [f (mk n)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (let [r (fiber/resume f (get (get p :args) 1))]
        (assert (%gt (length (get (get p :args) 1)) 0)
                "aliased payload lost its :args across the resume")
        r))))

# ── (c) the payload outlives the fiber, in a container ───────────────────────
(def @denials @[])
(defn w-stored (n)
  (let [f (mk n)]
    (fiber/resume f)
    (push denials (fiber/value f))
    (fiber/resume f "x")
    (length (get (get (get denials (%sub (length denials) 1)) :args) 1))))

# ── (d) refusal and abort ────────────────────────────────────────────────────
(defn w-refuse (n)
  (let [f (mk n)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/refuse f :denied)
      (assert (= (get p :error) :capability-denied)
              "payload freed by the refusal that displaced it")
      (length (get (get p :args) 1)))))
(defn w-abort (n)
  (let [f (mk n)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/abort f :done)
      (assert (= (get p :error) :capability-denied)
              "payload freed by the abort that displaced it")
      (length (get (get p :args) 1)))))

# ── (e) the denial inside a `protect` ────────────────────────────────────────
(defn protect-body (n)
  (let [mine (string "mine" n)
        [ok? e] (protect (emit :region-emit-denial-uaf/beat mine))]
    (if ok? (+ (length mine) (length e)) (length mine))))
(defn w-protect-resume (n)
  (let [f (fiber/new (fn () (protect-body n))
                     |:region-emit-denial-uaf/beat :error|
                     :deny |:region-emit-denial-uaf/beat|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (let [r (fiber/resume f (string "c" n))]
        (assert (= (get p :primitive) "fiber/emit")
                "payload freed by the resume that displaced the inner park")
        (+ r (length (get (get p :args) 1)))))))

# ── (f) a denied error emit, answered ────────────────────────────────────────
# A fiber denied :error cannot call a primitive that declares it, and most do:
# `string`, `length`, `get`, a struct literal's `struct`. So the body builds its
# value and its result with array literals alone, and the parent reads both.
(defn error-body (n)
  (let [mine @[n]
        r (error mine)]
    @[mine r]))
(defn w-error-resume (n)
  (let [f (fiber/new (fn () (error-body n)) |:error| :deny |:error|)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (assert (denial? p) "the error emit was not denied")
      (let [r (fiber/resume f (string "answer" n))]
        (assert (= n (get (get (get p :args) 1) 0))
                "the denied error's payload freed by the resume")
        (+ (length (get r 0)) (length (get r 1)))))))

# ── (g) a denied error emit left parked, its fiber dropped or killed ─────────
# The discharge owes the denial struct its allocation's reference and no more,
# though the park is under a terminal bit. The parent still holds the struct,
# and through its `:args` the value the body emitted.
(defn w-error-dropped (n)
  (let [p (let [f (fiber/new (fn () (error-body n)) |:error| :deny |:error|)]
            (fiber/resume f))]
    (assert (denial? p) "the error emit was not denied")
    (length (get (get p :args) 1))))
(defn w-error-cancel (n)
  (let [f (fiber/new (fn () (error-body n)) |:error| :deny |:error|)
        p (fiber/resume f)]
    (fiber/cancel f :gone)
    (assert (denial? p) "payload freed by the cancel that killed its fiber")
    (length (get (get p :args) 1))))

# ── drive: a fresh payload per iteration keeps region ids churning ───────────
(defn drive (reps)
  (var i 0)
  (var out @[])
  (while (%lt i reps)
    (assign
      out
      @[(w-hold-resume i) (w-alias-resume i) (w-stored i) (w-refuse i)
        (w-abort i) (w-protect-resume i) (w-error-resume i) (w-error-dropped i)
        (w-error-cancel i)])
    (assign denials @[])
    (assign i (%add i 1)))
  out)

(let [r (drive 300)]
  (assert (> (get r 0) 0) "payload freed under a held read past the resume")
  (assert (> (get r 1) 0) "the emitted value freed under the body's read")
  (assert (> (get r 2) 0) "stored payload freed under the container read")
  (assert (> (get r 3) 0) "payload freed under the read past the refusal")
  (assert (> (get r 4) 0) "payload freed under the read past the abort")
  (assert (> (get r 5) 0) "inner park's payload freed by the resume install")
  (assert (> (get r 6) 0) "denied error's body freed past the resume")
  (assert (> (get r 7) 0) "denied error's payload freed with its dropped fiber")
  (assert (> (get r 8) 0) "denied error's payload freed by the cancel"))

(println "region-emit-denial-uaf: ok")
