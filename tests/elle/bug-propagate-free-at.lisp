(elle/epoch 12)
# Regression: propagating HIR forms (or, and, if, let body, begin tail)
# treated their children's last-use as the propagating node's own HirId
# rather than the OUTER consumer. For a call-result region, the lowerer
# emits ReleaseValueRegion at the alloc's last-use; releasing too early
# freed the slot before the outer consumer stored the value, and the
# slot's memory was then reused for a different HeapObject — the stale
# Value's tag bits no longer matched the object's discriminant, tripping
# the deref tag/object debug_assert.
#
# Originally surfaced as a UAF in tests/elle/telemetry.lisp at
# lib/telemetry.lisp:135: `(put aggs key @{:value v :time now :attrs (or attrs {})})`
# where the empty `{}` allocated as `or`'s second arm was released before
# the outer `@struct` (LStructMut) constructor consumed it.
#
# Fix: src/hir/liveness.rs::LastUseBuilder propagates parent_consumes
# and parent_id through Or/And/If branches/Cond/Match arms/
# Let-Letrec-Loop body/Begin tail/Block tail/Parameterize body.

# ── (or X (alloc)) inside a mutable struct constructor ──────────
(def attrs1 nil)
(def s1 @{:a (or attrs1 {})})
(assert (= (type-of (get s1 :a)) :struct)
        "(or nil {}) :a should be the empty immutable struct")
(assert (= (length (pairs (get s1 :a))) 0) "(or nil {}) :a should be empty")

# ── (and X (alloc)) — same pattern ──────────────────────────────
(def cond2 true)
(def s2 @{:b (and cond2 {:k 1})})
(assert (= (type-of (get s2 :b)) :struct)
        "(and true {:k 1}) :b should be a struct")
(assert (= (get (get s2 :b) :k) 1) "(and true {:k 1}) :b :k should be 1")

# ── (if cond (alloc) (alloc)) — both branches must propagate ────
(def s3-then @{:c (if true {:x 1} {:x 2})})
(def s3-else @{:c (if false {:x 1} {:x 2})})
(assert (= (get (get s3-then :c) :x) 1) "if-then branch propagates")
(assert (= (get (get s3-else :c) :x) 2) "if-else branch propagates")

# ── (let [...] body) — body propagates ──────────────────────────
(def s4
  @{:d (let [tmp 42]
         {:wrapped tmp})})
(assert (= (get (get s4 :d) :wrapped) 42) "let body propagates")

# ── (begin ... tail) — tail propagates ──────────────────────────
(def s5
  @{:e (begin
         nil
         {:tag :ok})})
(assert (= (get (get s5 :e) :tag) :ok) "begin tail propagates")

# ── nested propagation: `(or x (let [t 1] {:t t}))` ─────────────
(def attrs6 nil)
(def s6
  @{:f (or attrs6
           (let [t 1]
             {:t t}))})
(assert (= (get (get s6 :f) :t) 1) "nested or+let propagates")

# ── type-of stress: forces deref on the propagated value ────────
# Before the fix this is where the panic fires:
#   tag/object mismatch — use-after-free? value.tag=0xe object=@struct
(println (type-of (get s1 :a)))
# struct
(println (type-of (get s2 :b)))
# struct
(println (type-of (get s3-then :c)))
# struct
(println (type-of (get s4 :d)))
# struct
(println (type-of (get s5 :e)))
# struct
(println (type-of (get s6 :f)))
# struct

(println "  propagate-free-at: ok")
