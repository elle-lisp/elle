(elle/epoch 12)
# audited: 2026-09-21
# ── The capability gate on the WASM host (#1188) ───────────────────────
#
# Every dispatch tier gates a native call on the calling fiber's withheld
# set. The WASM host reaches a native by four paths — `rt_call`,
# `rt_prepare_tail_call`, the `call_primitive` import, and the tiered
# linker's own `rt_call` — and each asks before it runs the primitive.
#
# Counterfactual: under `--wasm=full` with no gate the denied fiber RUNS
# the primitive and reaches :dead carrying its answer, so each assertion
# below reads that answer where the denial belongs.
#
# The trap this file was written around: a fiber body that reaches the
# primitive THROUGH a prelude function denies correctly even with the host
# ungated, because a prelude closure is not compiled into the module and
# runs on the host VM, whose own gate then fires. So every case below
# names a native the body calls directly. Routing through `println` hides
# the defect completely.
#
# The file runs on every tier, which is the point: the denial a program
# sees must not depend on which tier compiled its body.

# ── A call-position native is denied ───────────────────────────────────

# `do` keeps the call mid-activation, so this is the ordinary call path.
(let [f (fiber/new (fn []
                     (do
                       (path/exists? "should-be-blocked")
                       1)) |:error :fs| :deny |:fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "a denied native parks the fiber rather than running")
  (let [v (fiber/value f)]
    (assert (= :capability-denied (get v :error))
            "the payload names a capability denial")
    (assert ((get v :denied) :fs) "the payload names the denied bit")
    (assert (= "path/exists?" (get v :primitive))
            "the payload names the primitive the fiber called")))

# ── A tail-position native is denied ───────────────────────────────────

# The tail path is a host path of its own and reaches its own native, so a
# body whose last form IS the denied call must park too.
(let [f (fiber/new (fn [] (path/exists? "should-be-blocked")) |:error :fs|
                   :deny |:fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "a denied tail-position native parks the fiber")
  (let [v (fiber/value f)]
    (assert (= :capability-denied (get v :error))
            "the tail path builds the same denial payload")
    (assert (= "path/exists?" (get v :primitive)) "and names the same primitive")))

# ── A capability the fiber holds is not denied ─────────────────────────

# The gate tests the withheld set, not the primitive's declared bits: a
# fiber that withholds nothing runs the same call to completion.
(let [f (fiber/new (fn []
                     (do
                       (path/exists? "no-such-path")
                       :ran)) |:error :fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead)
          "a fiber withholding nothing runs its native to completion")
  (assert (= :ran (fiber/value f)) "and returns the body's own value"))

# ── A bit the primitive does not need is not denied ────────────────────

# Denying an unrelated capability must leave the call alone, so the gate
# cannot be a blanket refusal of every native on a restricted fiber.
(let [f (fiber/new (fn []
                     (do
                       (path/exists? "no-such-path")
                       :ran)) |:error :fs :exec| :deny |:exec|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead)
          "denying :exec does not block a call that needs only :fs")
  (assert (= :ran (fiber/value f)) "and the body returns its own value"))

# ── A denial parks whatever bits it carries, :error included ───────────

# Mediation is built on this: the worked example in capabilities.md denies
# :error, catches the denial, and resumes the fiber with the result of the
# call it refused. A tier that classified the denial by its bits would not
# park an :error denial at all, because :error is the one bit that does not
# suspend on its own.
(let [f (fiber/new (fn []
                     (do
                       (length "hello")
                       1)) |:error| :deny |:error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "an :error denial parks the fiber like any other")
  (let [v (fiber/value f)]
    (assert (= :capability-denied (get v :error))
            "the :error denial carries the same payload")
    (assert (= "length" (get v :primitive))
            "and names the primitive the fiber called")))

# ── The same, in tail position ─────────────────────────────────────────

# A tail denial is carried differently: no frame is built where the call
# was, and the driver it unwinds to parks one. The fiber must still come
# to rest :paused holding the payload.
(let [f (fiber/new (fn [] (length "hello")) |:error| :deny |:error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "a tail-position :error denial comes to rest :paused")
  (assert (= :capability-denied (get (fiber/value f) :error))
          "and carries the denial payload"))

# ── The argument-derived requirement is asked here too ─────────────────

# This branch's seam: `io/submit` declares `:error` alone and derives the
# rest from the request's operation. A tier gating on declared bits alone
# submits the spawn, so this pins the derivation on the host path and not
# only the denial machinery.
(let [minter (fiber/new (fn [] (subprocess/exec "/bin/sh" ["-c" "true"]))
                        |:error :io :exec|)]
  (let [req (fiber/resume minter)]
    (assert (io-request? req) "the minter yielded a real io-request")
    (let [f (fiber/new (fn [r] (io/submit (io/backend :async) r))
                       |:error :io :exec| :deny |:exec|)]
      (fiber/resume f req)
      (assert (= (fiber/status f) :paused)
              "the denied io/submit parks rather than submitting")
      (let [v (fiber/value f)]
        (assert (= :capability-denied (get v :error))
                "the submit denial names a capability denial")
        (assert ((get v :denied) :exec)
                "the denial names :exec, derived from the spawn request")))))

(println "caps-wasm-host: OK")
