(elle/epoch 12)
# Soundness complement of region-boundary-park.lisp
# (docs/impl/region/owner.md § "A boundary ends a park with no reader and no
# install, so it owes both references"). Run under `--trace=guardfree` by the
# subprocess pin `region_boundary_park_uaf` in tests/integration/elle_scripts.rs.
#
# A `squelch`/`attune` boundary now releases two references a park was left
# with: the delivery retain no reader consumed, and — for a payload the runtime
# built — the one its allocation left. Both are decrefs that never ran before,
# and the fiber SURVIVES the boundary, so what has to be whole afterwards is
# everything that still names the payload or shares its region.
#
# Six faces.
#
# 1. The payload the BODY allocated. Its own reference belongs to the abandoned
#    frame's release table; only the delivery retain is the boundary's, so the
#    value must still be readable through every holder that outlives the park.
# 2. A payload the body STORED somewhere longer-lived before parking. The store
#    took a counted reference, so the boundary's decrefs must not reach it.
# 3. A payload the body BORROWED rather than allocated — a module-level binding
#    it emits. The compiler mints the body's reference for exactly this shape,
#    and an over-release here frees the binding under the whole program.
# 4. REPETITION over the io machinery. The scheduler and its backend are reused
#    across boundaries, so a region drained by one over-release surfaces within
#    a few iterations; each subject runs in a loop and reads afterwards.
# 5. A park some other host ended. `compile/run-on` rejects a suspension, and
#    the ledger record outlives it; a boundary later in the same fiber must not
#    release a park that is already over.
# 6. The park that was NOT abandoned. An ordinary io call after every boundary
#    proves the install path still owns its own release — the boundary must not
#    have taken a reference the resume was going to need.
#
# Every read below happens AFTER the boundary ran, so an over-release faults at
# the deref (guardfree) or trips the generation check.

# ── 1. a body-allocated emit payload ─────────────────────────────────────────
# The squelched body allocates the payload and keeps its own binding live past
# the emit. The boundary drops the delivery retain; the frame's release table
# drops the body's. A third decref would free `s` under the binding.

(def emit-body
  (squelch (fn []
             (let [s (string "payload-" 1)]
               (begin
                 (emit :yield s)
                 (length s)))) :yield))

(defn emit-across [tag]
  (let [held (string "held-" tag)]
    (let [[ok e] (protect (emit-body))]
      [(length held) ok (get e :error)])))

(var i 0)
(while (< i 40)
  (let [r (emit-across i)]
    (assert (< 0 (get r 0)) "the catching frame's own value must be whole")
    (assert (= (get r 1) false) "the boundary must raise")
    (assert (= (get r 2) :signal-violation)
            "the violation must be readable after the boundary's releases"))
  (assign i (+ i 1)))

# ── 2. a payload stored somewhere longer-lived ───────────────────────────────
# `sink` outlives every call and holds a counted reference to the very value the
# body then emits. The boundary's decrefs answer for the park's references, not
# for the store's.

(def sink @[])

(def store-emit-body
  (squelch (fn []
             (let [s (string "kept-" 1)]
               (begin
                 (push sink s)
                 (emit :yield s)
                 (length s)))) :yield))

(assign i 0)
(while (< i 40)
  (protect (store-emit-body))
  (assign i (+ i 1)))

(assert (= (length sink) 40) "every stored payload must have reached the sink")
(var n 0)
(while (< n (length sink))
  (assert (= (type-of (get sink n)) :string)
          "a payload the sink holds must outlive the boundary")
  (assign n (+ n 1)))

# ── 3. a BORROWED payload: a module-level binding the body emits ─────────────
# The body allocates nothing — it emits a value the module owns — so the only
# reference the boundary may take is the one the compiler minted for the park.
# Reading `shared` after every boundary is where an over-release surfaces.

(def shared (string "shared-" 1))

(def borrow-emit-body (squelch (fn [] (emit :yield shared)) :yield))

(assign i 0)
(while (< i 40)
  (begin
    (protect (borrow-emit-body))
    (assert (= shared "shared-1")
            "a borrowed payload must survive the boundary that ended its park"))
  (assign i (+ i 1)))

# ── 4. io parks, repeatedly ──────────────────────────────────────────────────
# The request is the runtime's value, so BOTH its references are the boundary's.
# A drained region shows up in the io machinery behind the next call, which is
# why the writes below run after each boundary rather than only at the end.

(def io-body (attune |:error| (fn [] (println ""))))
(def sleep-body (attune |:error| (fn [] (ev/sleep 0))))

(assign i 0)
(while (< i 40)
  (begin
    (protect (io-body))
    (protect (sleep-body))
    (assert (= (type-of (string "probe-" i)) :string)
            "allocation must go on working after the boundary's releases"))
  (assign i (+ i 1)))

# ── 5. a park some other host ended is no longer the boundary's to claim ─────
# `compile/run-on :jit` cannot host a suspension, so it rejects the park and the
# ledger record outlives it. The boundary below ends no park at all (a squelched
# raise), so what keeps it off that stale record is the identity gate: the exit
# names the park it is ending, and a record that does not name it is skipped.
# Anything released here would be the stale record's.

(def escaped (string "escaped-" 1))
(defn yielder []
  (yield escaped))
(protect (compile/run-on :jit yielder))

(def raise-body (squelch (fn [] (error "boom")) :error))

(assign i 0)
(while (< i 40)
  (begin
    (protect (raise-body))
    (assert (= escaped "escaped-1")
            "a boundary must not claim a park its host already abandoned"))
  (assign i (+ i 1)))

# ── 6. the install path still owns its own release ───────────────────────────
# An ordinary io call after all of the above: its park ends at a resume, whose
# install releases the request itself. If the boundary had taken a reference the
# install needed, the shared io machinery would already be gone.

(println "region-boundary-park-uaf: io still works")

# And a fresh fiber round-trip, for the emit side of the same question.
(def coro (fiber/new (fn [] (yield (string "round" 1))) |:yield|))
(fiber/resume coro nil)
(assert (= (type-of (fiber/value coro)) :string)
        "a delivered park must still reach its resumer intact")

(println "region-boundary-park-uaf: ok")
