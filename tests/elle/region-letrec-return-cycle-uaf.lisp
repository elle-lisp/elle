(elle/epoch 12)
# A local mutual-recursion clique (`ev`/`od`) one of whose members is RETURNED must
# reclaim its merged arena soundly — the returned handle stays live and re-enterable
# after the arena's deferred release runs.
#
# The closure-cycle merge collapses the `ev`/`od` SCC and their forward cells onto one
# arena. A returned member puts that arena on the return frontier, and the merge admits
# it because the merge's release is a DECREF, not a free: the returned member lives IN
# the arena, so the callee's `Return` mint raises the arena's own count, and the letrec
# body's tail is a call to the MEMBER `ev`, whose deferral runs at the recursion's
# NORMAL COMPLETION — after that mint. So the deferral drops only the frame's own
# reference while the caller's stands (docs/impl/region/letrec.md § The frontier gate).
#
# The soundness hazard this pins: if the arena's release ran BEFORE the mint — or if the
# deferral dropped the last reference rather than the frame's — the caller would hold a
# closure whose env lives in a freed arena. Every returned handle here is therefore
# RE-ENTERED after the release, with heap churn in between so a prematurely freed page
# is recycled and the stale env read is loud: a generation panic on the plain VM, a
# SIGSEGV under `--trace=guardfree`. Each clique's non-base arm returns the member too,
# so a corrupt read also shows as a wrong `fn?`/call result.
#
# Covers: the member-tail admission driven per loop iteration (the arena minted and
# freed hundreds of times); re-entry of the returned handle across allocation churn;
# a mixed body whose arms return DIFFERENT members; the refused residual (a non-member
# body tail), which stays Shared and must still run correctly. Pinned under the UAF
# oracle by `region_letrec_return_cycle_uaf` (tests/integration/elle_scripts.rs).

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

# The REFUSED residual: the same returned cycle whose body tail-calls a NON-member, so
# the merge keeps the Shared baseline (its native fall-through would be the live
# scope-exit drop, which runs before the mint). It leaks — the F4 probe
# `recur-local-mutual-ret-foreign` measures that — but it must still RUN correctly, and
# its returned handle must be re-enterable too.
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
