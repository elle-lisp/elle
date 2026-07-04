(elle/epoch 12)
# Counterfactual: closure-env commingling on the TAIL-call path.
#
# A call in TAIL position is handled by `tail_call_inner` (src/vm/call.rs), NOT
# `call_inner`. The two build the callee's environment differently:
#   - `call_inner` mints a fresh env region per call
#     (`new_runtime_region_for_call_slot`) — that region then leaks whole (see
#     region-env-leak.lisp);
#   - `tail_call_inner` passes `region_id.get()` — the bytecode's STATIC region
#     slot — to `populate_env` DIRECTLY as a physical region id, unremapped.
#     Because a static slot is per-function (the same raw id every activation),
#     every tail call through the same TailCall site builds its env cells /
#     rest-arg conses into the SAME physical region. They commingle (Rule 6
#     violation) and that region is never freed, so its objects accumulate.
#
# The signature of this path is distinct and insidious: `arena/region-count`
# stays FLAT (one region, reused) while `arena/count` grows linearly. A
# region-count-only leak test sees nothing; only the object count exposes it.
# (The compiled-allocation analogue — a `%pair` built in a tail iteration — was
# fixed in 7f4c2066 via `runtime_region_for_alloc_slot` minting fresh; see
# region-tailloop-uniqueness.lisp. The closure-ENV path here is the remaining
# env-region commingling.)
#
# docs/impl/region-rules.md Rule 6 ("No commingling") + Rule 8 ("No leaks"). RED now,
# GREEN once the tail-call env path mints/owns per-execution regions like the
# Call-position path.

# Tail-position subjects: each function's body IS a single call (tail call).
(defn variadic (& xs)
  (length xs))

(defn counter (@x)
  (fn ()
    (assign x (%add x 1))
    x))

(defn plain (a b)
  (%add a b))

# wrap-* tail-call the subject; the callee's env is built by `tail_call_inner`.
(defn wrap-variadic (k)
  (variadic 1 2 3))

(defn wrap-counter (k)
  (counter k))

# Control: a tail call whose callee has NO env allocations (no rest arg, no
# captured-mutated params) commingles nothing — bounded NOW.
(defn wrap-plain (k)
  (plain 1 2))

# ── measurement ───────────────────────────────────────────────────
# Two scales (50 and 2000); object count is the witness, region count the
# foil (it stays flat through the commingling).

(defn wv-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (wrap-variadic 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn wv-reg [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (wrap-variadic 0)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn wc-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (wrap-counter i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn wp-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (wrap-plain 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(def p-obj (wp-obj 2000))
(def v-obj-50 (wv-obj 50))
(def v-obj-2k (wv-obj 2000))
(def v-reg-2k (wv-reg 2000))
(def c-obj-50 (wc-obj 50))
(def c-obj-2k (wc-obj 2000))

(println "region-tail-env-commingle:")
(println "  wrap-plain    (control) obj 2000=" p-obj)
(println "  wrap-variadic obj 50=" v-obj-50 " 2000=" v-obj-2k
         " (region delta 2000=" v-reg-2k ")")
(println "  wrap-counter  obj 50=" c-obj-50 " 2000=" c-obj-2k)

# Control: a tail call with no env allocations leaks nothing. Passes NOW.
(assert (%lt p-obj 50)
        (concat "control: plain tail call leaks objects, delta="
                (number->string p-obj)))

# Witness: the tail-called variadic's rest-arg conses commingle into one reused
# region and accumulate. RED (region-count stays flat — only object count sees it).
(assert (%lt v-obj-2k 100)
        (concat "tail-call variadic env commingles/leaks objects (region-count "
                "flat at " (number->string v-reg-2k) "): obj 50="
                (number->string v-obj-50) " 2000=" (number->string v-obj-2k)))

# Witness: the tail-called counter's captured-param lbox commingles/leaks.
(assert (%lt c-obj-2k 100)
        (concat "tail-call counter env commingles/leaks objects: obj 50="
                (number->string c-obj-50) " 2000=" (number->string c-obj-2k)))

(println "region-tail-env-commingle: ok")
