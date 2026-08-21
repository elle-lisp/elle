(elle/epoch 12)
# A local mutual-recursion clique (`ev`/`od`) one of whose members is RETURNED must
# reclaim its merged arena soundly — the returned handle stays live and re-enterable
# after the arena's deferred release runs.
#
# The closure-cycle merge collapses the `ev`/`od` SCC and their forward cells onto one
# arena. A returned member puts that arena on the return frontier, and the merge admits
# it because the merge's release is a DECREF, not a free: the returned member lives IN
# the arena, so the mint that funds the caller raises the arena's own count. What the
# admission then needs is ORDER, and the one structural fact that supplies it is that
# the letrec BODY hands the value over itself — every tail exit of it leaves the frame
# (docs/impl/region/letrec.md § The frontier gate). The three ways it can do that are
# each driven below, because each mints through a different instruction:
#
#   - a MEMBER tail call — the release rides the member deferral, which
#     `trampoline_loop` runs at the recursion's normal completion, after the mint;
#   - a NON-member tail call — a closure callee replaces the frame and takes the same
#     late deferral through `deferred_release_slot`, while a NATIVE callee keeps the
#     frame and falls through to the binding-scope drop, which the lowerer emits at the
#     `Letrec` node and therefore after the mint the tail call emits at the call site;
#   - a bare member VALUE — the letrec being the frame's tail, the frame's own `Return`
#     sits inside the letrec body, so it mints there, again before that drop.
#
# The soundness hazard this pins: if the arena's release ran BEFORE the mint — or if the
# deferral dropped the last reference rather than the frame's — the caller would hold a
# closure whose env lives in a freed arena. Every returned handle here is therefore
# RE-ENTERED after the release, with heap churn in between so a prematurely freed page
# is recycled and the stale env read is loud: a generation panic on the plain VM, a
# SIGSEGV under `--trace=guardfree`. Each clique's non-base arm returns the member too,
# so a corrupt read also shows as a wrong `fn?`/call result.
#
# A body that hands its value to an ENCLOSING consumer instead — the letrec bound OUT of
# the frame's tail position — reaches the same ordering from the other side: the release
# follows the handed-out member to the point the last-use rule already computed for it
# (docs/impl/region/letrec.md § "Drop site — following a handed-out member").
#
# Covers: each admitted body shape driven per loop iteration (the arena minted and freed
# hundreds of times); re-entry of the returned handle across allocation churn; a mixed
# body whose arms return DIFFERENT members; a branchy body whose two arms each tail-call
# a non-member; and three placements of the bound-out reading — the member handed on to
# the caller, called in place and never handed further, and reached through the frame's
# own tail call. Pinned under the UAF oracle by `region_letrec_return_cycle_uaf`
# (tests/integration/elle_scripts.rs).

# The admitted shape: the letrec body tail-calls the MEMBER `ev`, and both base cases
# hand back the member `ev` itself.
## `ev` is used in value position (returned), which disables call-site param joins, so
## a local diverging guard proves the %lt/%sub operands.
(defn ret-member [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (ev n)))

# Mixed members returned: `ev`'s base case hands back `od` while `od`'s hands back
# `ev`, so which member the caller receives depends on the recursion depth's parity.
# Both live in the same arena, so both are covered by the one deferral.
(defn ret-either [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) od (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (ev n)))

# A NON-member tail call whose callee resolves to a CLOSURE: the frame is replaced, so
# the binding-scope drop is dead code and the arena's release rides
# `TailCall::deferred_release_slot`, run at the recursion's completion after `ident`'s
# own `Return` mint.
(defn ident [x]
  x)
(defn ret-foreign [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (ident (ev n))))

# The same non-member tail call resolving to a NATIVE — the other half of that channel,
# and the one with no deferral at all. `first` pushes no frame, so control falls through
# to the binding-scope `DecrefRegion`; the reference that funds the caller is minted
# just before it, by the post-`TailCall` retain over the native's result. That result IS
# the member here (the pair's head), so the mint and the drop name the same arena and
# their order is the whole soundness argument.
(defn ret-native [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (first (%pair (ev n) nil))))

# A bare member VALUE tail: no tail call strands anything, and the frame's own `Return`
# — which functionalization places inside the letrec body, the letrec being this frame's
# tail — is what mints before the binding-scope drop. Branching on the value keeps that
# reading honest: each arm gets its OWN `Return`, so the admission must see both.
(defn ret-value [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (if (< n 0) od ev)))

# A BRANCHY body: two arms, each a non-member tail call, so two arena adopt sites are
# recorded against one merged root and exactly one may fire per call. Mutually exclusive
# arms are what make that a single release rather than two.
(defn ret-arms [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (if (< n 2) (ident (ev n)) (ident (od n)))))

# The letrec bound OUT of the frame's tail position: its body falls out to a bare member
# value, so `c` names the member's region directly — an uncounted read — and the value
# reaches `f`'s caller through `c`, past the letrec. Pinning the arena's release at the
# binding scope would free it under `c`; the merge instead follows the value out, adopting
# the release point the last-use rule already computed for the handed-out member — here
# the enclosing `Return`, whose mint precedes that node's own releases
# (docs/impl/region/letrec.md § "Drop site — following a handed-out member"). The hazard
# this drives is that ordering: get it wrong and the caller holds a closure whose env
# lives in a freed arena.
(defn ret-bound [n]
  (let [c (letrec [ev (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (od (%sub m 1))))
                   od (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (ev (%sub m 1))))]
            ev)]
    (ident n)
    c))

# The other half of that reading: the handed-out member is CALLED in place and never
# leaves `f`, so there is no mint at all and the adopted release point is the member's
# ordinary last use. The arena must survive every re-entry of `c` and still come back —
# the call below runs the whole recursion through the merged arena after the letrec node
# the release used to sit on.
(defn bound-called [n]
  (let [c (letrec [ev (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (od (%sub m 1))))
                   od (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (ev (%sub m 1))))]
            ev)]
    (ident n)
    (assert (fn? (c 3)) "bound-called: re-entering the bound-out member")
    (assert (fn? (c 0))
            "bound-called: the base case through the bound-out member")
    :ok))

# The handed-out member reached through the frame's own TAIL CALL. `c` is the callee in
# tail position, so the release point adopted for it sits past a frame-replacing
# `TailCall` — the one placement where the arena's release may not run at all. That
# direction is safe by construction: a release that does not run over-keeps, where the
# opposite error would free the arena under the callee, which IS a member of it. Driven
# here so the recursion actually runs through the arena after the letrec node.
(defn bound-tail [n]
  (let [c (letrec [ev (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (od (%sub m 1))))
                   od (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (ev (%sub m 1))))]
            ev)]
    (ident n)
    (c 3)))

# Re-enter a returned handle repeatedly, churning fresh heap between calls so a
# prematurely freed arena page is recycled under the closure's env.
(defn drive [label mk]
  (def @churn @[])
  (def @i 0)
  (while (%lt i 80)
    (let [f (mk i)]
      (assert (fn? f)
              (string label ": returned value must be a closure at i=" i))
      # Churn allocates between the release and the re-entry below.
      (push churn (string label "-" i))
      # Re-entry AFTER the deferred release ran: reads the closure's env out of the
      # arena the recursion completed in.
      (let [g (f 3)]
        (assert (fn? g)
                (string label
                        ": re-entering the returned member must yield a member "
                        "at i=" i " (arena freed early?)")))
      (assert (fn? (f 0))
              (string label ": the base case must still return a member at i=" i)))
    (assign i (%add i 1)))
  (assert (= (length churn) 80) (string label ": churn count")))

(drive "ret-member" ret-member)
(drive "ret-either" ret-either)
(drive "ret-foreign" ret-foreign)
(drive "ret-native" ret-native)
(drive "ret-value" ret-value)
(drive "ret-arms" ret-arms)
(drive "ret-bound" ret-bound)
(drive "bound-tail" bound-tail)

# The called-in-place shape returns a keyword, not a closure, so it drives its own loop —
# the re-entry assertions live inside `bound-called` itself, where the arena is still the
# executing frame's.
(def @churn2 @[])
(def @m 0)
(while (%lt m 80)
  (assert (= (bound-called m) :ok)
          (string "bound-called: must run to completion at i=" m))
  (push churn2 (string "bc-" m))
  (assign m (%add m 1)))
(assert (= (length churn2) 80) "bound-called: churn count")

# Hold several returned handles live SIMULTANEOUSLY, then re-enter each. Each call mints
# its own arena, so this pins that one call's deferral never touches another's — and
# that a handle held across many later mint/free cycles is still sound.
(def @held @[])
(def @j 0)
(while (%lt j 40)
  (push held (ret-member 3))
  (assign j (%add j 1)))
# Re-enter every held handle after 40 further mint/free cycles have churned the pool.
(def @k 0)
(while (%lt k 40)
  (let [f (get held k)]
    (assert (fn? f) (string "held: handle " k " must survive later arenas"))
    (assert (fn? (f 2)) (string "held: handle " k " must still be re-enterable")))
  (assign k (%add k 1)))

(println "region-letrec-return-cycle-uaf: ok")
