(elle/epoch 12)
# Soundness complement of tests/elle/region-jit-error-unwind.lisp: a COMPILED
# frame's error exit must release only what that frame owed
# (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
# still owes"). Run under `--trace=guardfree` by the subprocess pin
# `region_jit_error_unwind_uaf` in tests/integration/elle_scripts.rs.
#
# The compiled walk reads its value route off the locals the exit spilled and
# its slot route off the activation map the prologue pushed, then pops that map.
# What must survive is every reference the walk does not own: the delivery the
# catcher reads, a counted store's, a borrowed payload's owner, and — the
# reference the map pop answers for — the CALLER's own binding, live across a
# compiled callee's walked exit.
#
# Every read below happens after the walk ran, so an over-release faults at the
# deref under guardfree or trips the generation check.

# The raisers must be compiled for the walk under test to be the compiled one,
# and the compile is asynchronous, so the drive outlasts it. The cap only bounds
# that wait, and a policy that compiles nothing would pay it in full for no
# coverage — so the warm-up is skipped outright when the JIT is off, and the
# window below still gauges the interpreter walk.
(def jit-live? (not (= (vm/config :jit) :off)))
(def warm-cap 20000)
(def window 200)

(def shared {:error :shared :message "borrowed"})
(def kept @[])

# ── the raisers ───────────────────────────────────────────────────────────────

(defn ep-raise [j]
  (error (string "payload-" j)))

(defn raise-shared [j]
  (error shared))

# Stores into a container that outlives the frame, THEN raises: the store funnel
# counted the sink's reference, so the walk's release of the frame's own cannot
# take the value below it.
(defn stores-then-raises [j]
  (let [v (string "kept-" j)]
    (begin
      (push kept v)
      (error (string "boom-" j)))))

# ── the subjects ──────────────────────────────────────────────────────────────

# 1. The catcher reads the payload the compiled raiser allocated. The raise
# minted the delivery itself, so the frame's own reference IS released — and the
# delivery must be what is left.
(defn catch-reads [j]
  (try
    (begin
      (ep-raise j)
      0)
    (catch e (string/size-of e))))

# 2. The stored value must read whole after the raising frame is gone.
(defn store-survives [j]
  (try
    (begin
      (stores-then-raises j)
      nil)
    (catch e nil))
  (string/size-of (pop kept)))

# 3. The raise chain owns no reference of a module-level payload, so the walk
# must release nothing for it however many times it is raised.
(defn borrowed-survives [j]
  (try
    (begin
      (raise-shared j)
      0)
    (catch e (string/size-of (get e :message)))))

# 4. The caller's binding is live across the COMPILED callee's walked error
# exit. The callee's walk names only the callee's slots, and the map it reads is
# its own — which is also what its exit pops, so the caller's later releases
# resolve against the caller's map and not the callee's leftovers.
(defn holds-across-catch [j]
  (let [held (string "held-" j)]
    (begin
      (try
        (begin
          (ep-raise j)
          nil)
        (catch e nil))
      (string/size-of held))))

# ── the drive ─────────────────────────────────────────────────────────────────

# The RAISERS are the compiled frames: a body holding a `try` is not a JIT
# candidate, so each subject below wraps its raiser in an interpreted catcher and
# the walk under test is the raiser's own.
(defn all-hot? []
  (and (jit? ep-raise) (jit? raise-shared) (jit? stores-then-raises)))

(defn check [j]
  (assert (< 0 (catch-reads j))
          "the delivery the catcher reads must survive the compiled walk")
  (assert (< 0 (store-survives j))
          "a value the compiled frame stored outward must survive its walk")
  (assert (< 0 (borrowed-survives j))
          "a borrowed payload's owner must survive the compiled walk")
  (assert (< 0 (holds-across-catch j))
          "the caller's binding must survive a compiled callee's walked exit"))

(var i 0)
(while (and jit-live? (%lt i warm-cap) (not (all-hot?)))
  (check i)
  (assign i (%add i 1)))

(def hot (all-hot?))

(var k 0)
(while (%lt k window)
  (check k)
  (assign k (%add k 1)))

(assert (= (get shared :message) "borrowed")
        "the module binding must be whole after every raise")
(assert (= (length kept) 0)
        "each stored payload is read back, so the sink stays bounded")

(println "region-jit-error-unwind-uaf: ok (compiled " hot ")")
