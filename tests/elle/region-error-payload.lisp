(elle/epoch 12)
# An emit-raised error's payload keeps every frame-owed release
# (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
# still owes" — "What the signal carries is not abandoned — unless the raise
# minted its delivery").
#
# `(error v)` is `(emit :error v)`, and the emit mints the payload's delivery
# reference itself (the `EmitEscape` retain the resumer's release of the resume
# result consumes). So where the raise chain OWNS the payload — it allocated
# it, or received it as an owned parameter — the frame's own release is
# genuinely owed, and exempting the payload's region from the abandoned-frame
# walk and the parked frame's discharge strands one region per
# raised-and-caught error. A native raise is the opposite: it installs the
# payload unretained, so the frame's left-standing reference IS the delivery
# and the exemption is exactly right.
#
# This file is the LEAK gauge — an `arena/region-count` delta over a fixed
# window, bounded for every subject. The soundness complement is
# region-error-payload-uaf.lisp.

(def window 500)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-count) before))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) the raising frame allocates the payload: its release sits past the emit,
# which never returns.
(defn direct []
  (try
    (error (string "x" 1))
    (catch e nil))
  nil)

# (b) a helper allocates and raises: the owning frame is WALKED at the error
# exit rather than parked, so this face exercises the walk's reading.
(defn raiser []
  (error (string "x" 1)))
(defn via-helper []
  (try
    (raiser)
    (catch e nil))
  nil)

# (c) the payload arrives as an owned parameter: the raising frame's
# owned-param release is the one owed.
(defn raise-param [v]
  (error v))
(defn via-param []
  (try
    (raise-param (string "x" 1))
    (catch e nil))
  nil)

# (d) a struct-literal payload: the struct and its message string are the
# raise chain's own allocations.
(defn via-struct []
  (try
    (error {:error :e :message (string "m" 1)})
    (catch e nil))
  nil)

# controls ─────────────────────────────────────────────────────────────────────

(def prebuilt {:error :e :message "m"})

# (e) a BORROWED payload: the raise chain owns no reference, so nothing is
# owed and nothing may be released. Bounded before this mechanism and after
# it; a regression here is a release the frame never had.
(defn borrowed []
  (try
    (error prebuilt)
    (catch e nil))
  nil)

# (f) a NATIVE raise: the payload is the native's fresh error struct, funded
# by its own birth reference. The frame-funded exemption stays.
(defn native []
  (try
    (get 5 :k)
    (catch e nil))
  nil)

# (g) the restarts system: an `:error` fiber resumed again replays the parked
# frame's own releases, so the discharge never runs and the ledger must close
# on the replay path too.
(defn restarted []
  (let [f (fiber/new (fn [] (error (string "x" 1))) |:error|)]
    (fiber/resume f)
    (fiber/resume f 7)
    (fiber/value f))
  nil)

# measurement ──────────────────────────────────────────────────────────────────

(def d-direct (measure direct 20 window))
(def d-helper (measure via-helper 20 window))
(def d-param (measure via-param 20 window))
(def d-struct (measure via-struct 20 window))
(def d-borrowed (measure borrowed 20 window))
(def d-native (measure native 20 window))
(def d-restarted (measure restarted 20 window))

(println "region-error-payload over " window " iters (region deltas):")
(println "  direct    " d-direct)
(println "  helper    " d-helper)
(println "  param     " d-param)
(println "  struct    " d-struct)
(println "  borrowed  " d-borrowed " (control)")
(println "  native    " d-native " (control)")
(println "  restarted " d-restarted " (control)")

(assert (%lt d-borrowed 50)
        (concat "control: a borrowed payload owes nothing, delta="
                (number->string d-borrowed)))
(assert (%lt d-native 50)
        (concat "control: a native raise funds its own delivery, delta="
                (number->string d-native)))
(assert (%lt d-restarted 50)
        (concat "control: a restarted error fiber replays its own releases, "
                "delta=" (number->string d-restarted)))

(assert (%lt d-direct 50)
        (concat "the raising frame's payload release is owed, delta="
                (number->string d-direct)))
(assert (%lt d-helper 50)
        (concat "a walked helper frame's payload release is owed, delta="
                (number->string d-helper)))
(assert (%lt d-param 50)
        (concat "an owned parameter's payload release is owed, delta="
                (number->string d-param)))
(assert (%lt d-struct 50)
        (concat "a struct payload's allocations are owed, delta="
                (number->string d-struct)))
(println "region-error-payload: ok")
