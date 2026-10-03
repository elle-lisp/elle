(elle/epoch 12)
# audited: 2026-09-21
# ── The spend check at io/submit (#1072) ───────────────────────────────
#
# `:deny` gates the primitive that MINTS authority. It did not gate the
# authority a fiber already holds: an io-request is an ordinary value, and
# the submission API declared `:error` alone, so a fiber that received a
# request by any route spent it in full. #1072: a fiber denying
# |:io :exec :fs| ran /bin/sh because someone handed it a spawn request.
#
# The fix derives the required bits from the request's own operation and
# tests them at io/submit against the submitting fiber. A spawn request
# needs |:io :exec|, so a fiber withholding either bit cannot submit one
# however it obtained it. The check is on the submitter; a scheduler that
# submits on another fiber's behalf spends its own authority (authority.md).

# ── A denied fiber cannot spend a spawn request handed to it ───────────

# Counterfactual: without the spend check this fiber runs /bin/sh and the
# file appears on disk (the #1072 repro).
(with-temp-dir dir
               (let [proof (path/join dir "proof")]
                 # Mint the request in a fiber that is allowed to. Nothing has run: the
                 # minter is parked on the request, the scheduler has never seen it.
                 (let [minter (fiber/new (fn []
                         (subprocess/exec "/bin/sh"
                         ["-c" (string "echo escaped > " proof)]))
                       |:error :io :exec|)]
                   (let [req (fiber/resume minter)]
                     (assert (io-request? req)
                             "the minter yielded a real io-request")
                     (assert (not (path/exists? proof)) "nothing has run yet")
                     # Hand it to a fiber denying everything the operation needs. Its mask
                     # names those bits so the denial parks catchably rather than ending the
                     # fiber uncaught (caps.md — the mask controls what the parent catches).
                     (let [f (fiber/new (fn [r]
                                          (let [b (io/backend :async)]
                                            (io/submit b r)
                                            (length (io/wait b 3000))))
                                        |:error :io :exec :fs|
                                        :deny |:io :exec :fs|)]
                       (fiber/resume f req)
                       (assert (= (fiber/status f) :paused)
                               "the denied io/submit suspends rather than running")
                       (let [v (fiber/value f)]
                         (assert (= :capability-denied (get v :error))
                                 "the denial names a capability denial")
                         (assert (= "io/submit" (get v :primitive))
                                 "the denial names io/submit as the primitive")
                         (assert ((get v :denied) :exec)
                                 "the denial names :exec, derived from the spawn request")))
                     # Nothing was submitted, so nothing is in flight to drain: the denied
                     # submit parked before the backend ever saw the request.
                     (assert (not (path/exists? proof))
                             "the denied fiber spawned nothing")))))

# ── The check derives from the operation, not from the denial set ──────

# The requirement is the request's own operation, so a denial of a bit the
# operation does not need must not block the submit. A fiber denying :exec
# submits a sleep request (needs |:io| only) and is not denied: it proceeds
# to a real I/O park, never a capability denial.
(let [minter (fiber/new (fn [] (ev/sleep 0.01)) |:error :io|)]
  (let [req (fiber/resume minter)]
    (assert (io-request? req) "sleep yielded a request")
    (let [f (fiber/new (fn [r]
                         (let [b (io/backend :async)]
                           (io/submit b r))) |:error :io :exec| :deny |:exec|)]
      (fiber/resume f req)
      (let [v (fiber/value f)]
        (assert (not (and (= :struct (type-of v))
                          (= :capability-denied (get v :error))))
                "denying :exec does not block a sleep request, which needs only :io")))))

(println "caps-io-submit: OK")
