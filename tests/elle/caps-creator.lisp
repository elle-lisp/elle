(elle/epoch 13)
# audited: 2026-09-29
# ── A fiber takes its creator's withheld set ──────────────────────────
#
# `fiber/new` gives a fiber the withheld set of the fiber that creates it, and
# each resume adds the resumer's (docs/signals/capabilities.md).
# This file pins the creator's half with no scheduler involved: a fiber made
# inside a sandbox and resumed from outside it.
#
# Counterfactual: with the withheld set flowing only at resume, the fiber takes
# the empty set of the unrestricted fiber that resumes it. It then reads the
# file its creator could not, and `(fiber/caps made)` lists :fs.

(defn make-inside [deny body]
  "A fiber that code denied `deny` creates, handed back unresumed."
  (fiber/resume (fiber/new (fn [] (fiber/new body |:fs :error|)) |:error|
                           :deny deny)))

# ── The denial rides the fiber out of the sandbox ─────────────────────

(let [made (make-inside |:fs| (fn [] (fiber/caps)))]
  (assert (not (contains? (fiber/caps made) :fs))
          "a fiber made in the sandbox lacks :fs before anything resumes it")
  (let [caps (fiber/resume made)]
    (assert (not (contains? caps :fs))
            "and still lacks :fs when an unrestricted fiber resumes it")
    (assert (contains? caps :io) "it keeps what nobody withheld")))

(with-temp-dir dir
               (let [target (path/join dir "made")
                     made (make-inside |:fs| (fn [] (file/write target "x")))]
                 (fiber/resume made)
                 (assert (= (fiber/status made) :paused)
                         "the escaped fiber's write parks on a denial")
                 (assert (= (get (fiber/value made) :error) :capability-denied)
                         "the payload is a capability denial")
                 (assert (= (get (fiber/value made) :primitive) "file/write")
                         "naming the write the fiber attempted")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

# ── The creator's set joins the fiber's own :deny ─────────────────────

(let [made (fiber/resume (fiber/new (fn []
                                      (fiber/new (fn [] (fiber/caps)) |:error|
                                      :deny |:ffi|)) |:error| :deny |:fs|))
      caps (fiber/resume made)]
  (assert (not (contains? caps :fs)) "the creator's :fs denial holds")
  (assert (not (contains? caps :ffi)) "beside the fiber's own :ffi denial"))

# ── Two levels of creation compose ────────────────────────────────────

# The middle fiber is made inside the sandbox and makes the innermost one, so
# the innermost fiber inherits through a creator that itself inherited.
(let [middle (make-inside |:fs|
                          (fn [] (fiber/new (fn [] (fiber/caps)) |:error|)))
      inner (fiber/resume middle)]
  (assert (not (contains? (fiber/resume inner) :fs))
          "a fiber made by a fiber made in the sandbox lacks :fs"))

# ── An unrestricted creator withholds nothing ─────────────────────────

(let [made (make-inside || (fn [] (fiber/caps)))]
  (assert (contains? (fiber/resume made) :fs)
          "a fiber whose creator denies nothing holds :fs"))

(println "caps-creator: OK")
