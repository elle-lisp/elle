(elle/epoch 18)
# audited: 2026-11-24
# Tail-call rotation, letrec-local recursive closures, a returned self-recursive closure's region, and the scheduler round trip.
#
# docs/impl/region/diagnostics.md
# ── Tail-call rotation ────────────────────────────────────────────────
# The loop IS the recursion, so the run-block is the recursive call itself: one
# call with arg b performs b allocations via tail recursion. Tail-call rotation
# (not while-scope) is the mechanism that must reclaim them — so it gets its own
# driver. n varies the input so a body cannot constant-fold. All four recur fns
# reach the estimator as fn-values, so no visible call site proves `n` and
# call-site param joins do not fire; a local diverging guard proves each
# %sub operand instead (docs/intrinsics.md). Contrast lcl-self below, which is
# called directly and needs no guard. The guard never fires on the driver's int
# inputs and holds no heap arg, so the measured tails are undisturbed at 0/op.
(defn struct-recur [n]
  (when (%not (%int? n)) (error :struct-recur-nan))
  (if (= n 0)
    nil
    (begin
      {:x n}
      (struct-recur (%sub n 1)))))
(defn string-recur [n]
  (when (%not (%int? n)) (error :string-recur-nan))
  (if (= n 0)
    nil
    (begin
      (string "iter-" n)
      (string-recur (%sub n 1)))))
(letrec [odd-recur (fn [n]
                     (when (%not (%int? n)) (error :odd-recur-nan))
                     (if (= n 0)
                       nil
                       (begin
                         {:odd n}
                         (even-recur (%sub n 1)))))
         even-recur (fn [n]
                      (when (%not (%int? n)) (error :even-recur-nan))
                      (if (= n 0)
                        nil
                        (begin
                          {:even n}
                          (odd-recur (%sub n 1)))))]
  (println "── folded suite: tail-call rotation ──")
  (r:drive "recur-struct" struct-recur)
  (r:drive "recur-string" string-recur)
  (r:drive "recur-mutual" even-recur)

  # ── Letrec-local recursive closures — self (cell-free) vs mutual (cycle) ──
  # A `letrec`-bound recursive closure NESTED in a function body — the UNIVERSAL shape:
  # every recursive local helper, every variadic operator (`+`/`<` build a
  # `(letrec [go …] …)` over their varargs). The `recur-*` probes above do NOT cover it:
  # each is bound at the top level, outside every function body.
  #
  # SELF-recursion (`recur-local-self`) is reclaimed (rate 0). A self-recursive `loop` is
  # CELL-FREE: its self-edge does not mark it captured, so there is no forward cell and
  # no cell↔closure cycle. Its self-reference resolves to the executing closure
  # (`LoadSelf` or a self-call), RC-identical to a top-level recursive `defn`
  # (docs/impl/selfrec.md). The
  # per-call closure region is stranded past the recursive `TailCall` and reclaimed by the
  # tail-call deferred release (lir/lower/control/call/defer.rs `tail_callee_defers_release`).
  # The HOF rows above (map/reduce/zip/…) ride this same cell-free mechanism — their `go`
  # helpers.
  #
  # MUTUAL recursion (`recur-local-mutual`) is reclaimed (rate 0). `ev`/`od` each capture
  # the OTHER, a genuine closure↔closure cell cycle. But an immutable lambda-initialized
  # letrec binding's forward cell is a compiled static-slot cell in every position. So the
  # closure-cycle merge collapses the SCC and its cells onto one arena in-lambda, as at
  # top level. The tail-call letrec body `(ev n)` strands the binding-scope drop; the
  # tail-call deferred release releases the merged arena once at the recursion's normal
  # completion (docs/impl/region/letrec.md).
  (defn lcl-self [n]
    (letrec [go (fn [m] (if (%lt m 1) :done (go (%sub m 1))))]
      (go n)))
  (defn lcl-mutual [n]
    (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1))))
             od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))]
      (ev n)))
  (println "── folded suite: letrec-local recursive closures ──")
  (r:rate "recur-local-self" (fn [j] (lcl-self 3)))
  (r:rate "recur-local-mutual" (fn [j] (lcl-mutual 3)))

  # NON-member body tail — the same ev/od cycle, but the letrec BODY ends in a tail call
  # to a NON-member. `(ev n)` above is a tail call to a MEMBER, whose stranded
  # binding-scope drop rides `stranded_cycle_bindings`. Here `(%add (ev n) 0)` is an
  # inline opcode whose operand is the call, and in `(+ (ev n) 0)` the stdlib redefines
  # `+` to a bytecode CLOSURE. Each ends in a frame-replacing tail call to a non-member.
  # That strands the merged arena's binding-scope drop as dead code.
  #
  # The release rides the explicit arena adopt (`TailCall::deferred_release_slots`,
  # `RegionInfo::cycle_tail_release`). A closure callee (`+`) adopts the arena at the
  # recursion's completion. A native callee (`%add`) never replaces the frame and falls
  # through to the live scope-exit drop. The two are exclusive per call, so exactly one
  # release fires however the callee resolves.
  #
  # Both reclaim (rate 0). A closure-cycle merge that refused a non-member-tail clique
  # would leave it Shared, leaking its whole arena ~4/op (docs/impl/region/letrec.md).
  # The base cases return 0/1 so `(%add (ev n) 0)` is well-typed.
  (defn lcl-mutual-native [n]
    (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
             od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
      (%add (ev n) 0)))
  (defn lcl-mutual-op [n]
    (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
             od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
      (+ (ev n) 0)))
  (r:rate "recur-local-mutual-native" (fn [j] (lcl-mutual-native 3)))
  (r:rate "recur-local-mutual-op" (fn [j] (lcl-mutual-op 3)))

  # RETURNED closure cycle — the return-funded merge admission (rate 0). The same ev/od SCC
  # as `recur-local-mutual` above, one base case apart: it returns the MEMBER `ev` instead
  # of a keyword, putting a member on the return frontier.
  #
  # The merge admits it anyway. The merge's release is a decref rather than a free, and
  # the returned member lives IN the merged arena, so the callee's `Return` mint raises
  # the arena's own count. And the letrec body's tail is a call to the MEMBER `ev`, whose
  # deferral runs at the recursion's normal completion, AFTER that mint. So the deferral
  # drops only the frame's reference while the caller's stands, and the discard at the
  # call site takes the arena to zero and subtree-drops the cycle
  # (docs/impl/region/letrec.md).
  #
  # The FIBER half of the frontier still refuses outright. A returned cycle whose letrec body does not hand the
  # value over itself keeps the Shared baseline, because its binding-scope drop would
  # fire before any mint. Refusing the whole return facet instead holds this cycle's four
  # regions — two closures, two forward cells — per call.
  (defn lcl-mutual-ret [n]
    # `ev` is returned (a value use), which disables call-site param joins, so a local
    # diverging guard proves the %lt/%sub operands.
    (letrec [ev (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) ev (od (%sub m 1))))
             od (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) ev (ev (%sub m 1))))]
      (ev n)))
  (r:rate "recur-local-mutual-ret" (fn [j] (lcl-mutual-ret 3)))

  # The same returned ev/od cycle, one body-tail apart: it tail-calls a NON-member
  # (`lcl-ident`) rather than the member `ev`. Neither the channel that carries the
  # arena's release nor whether the compiler can classify the callee enters the ordering
  # argument. A CLOSURE callee replaces the frame and takes the `deferred_release_slots`
  # deferral at the recursion's completion. A NATIVE callee keeps the frame and falls
  # through to the binding-scope drop the lowerer emits at the `Letrec` node. That drop
  # comes after the mint the tail call itself emits at the call site.
  #
  # Both are after the mint, so the merge admits the return facet here too (rate 0).
  # Refusing it held this cycle's four regions, two closures and two forward cells, per
  # call. A control at 0.
  (defn lcl-ident [x]
    x)
  (defn lcl-mutual-ret-foreign [n]
    (letrec [ev (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) ev (od (%sub m 1))))
             od (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) ev (ev (%sub m 1))))]
      (lcl-ident (ev n))))
  (r:rate "recur-local-mutual-ret-foreign" (fn [j] (lcl-mutual-ret-foreign 3)))

  # The third admitted body shape: a bare member VALUE tail, no tail call at all. The
  # letrec is this frame's tail, so functionalization puts the frame's `Return` INSIDE the
  # letrec body and it mints there, ahead of the binding-scope drop. A control at 0,
  # like `recur-local-mutual-ret-foreign` above.
  (defn lcl-mutual-ret-value [n]
    (letrec [ev (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) ev (od (%sub m 1))))
             od (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) ev (ev (%sub m 1))))]
      ev))
  (r:rate "recur-local-mutual-ret-value" (fn [j] (lcl-mutual-ret-value 3)))

  # The fourth body shape: the identical returned ev/od cycle, one BINDING out of tail
  # position. The letrec's value is bound to `c` and handed on by a later statement. So the
  # body falls out to a bare value with no `Return` and no tail call of its own. `c` names
  # the member's region directly, an uncounted read, and the mint that funds the caller is
  # an enclosing node away.
  #
  # Pinning the arena's release at the binding scope would fire it
  # at the letrec, under a live `c`. The merge instead follows the value out and adopts
  # the release point the last-use rule computed for the handed-out member. Here that is
  # the enclosing `Return`, whose mint precedes that node's own releases
  # (docs/impl/region/letrec.md).
  #
  # This is the boundary control for `recur-local-mutual-ret-value` above. The two differ
  # only in whether the letrec is the frame's tail, the fact that decides where the mint
  # lands, so the pair reads the two dispositions directly rather than by resemblance.
  # Refusing this shape held its four regions, two closures and two forward cells, per
  # call.
  (defn lcl-mutual-ret-bound [n]
    (let [c (letrec [ev (fn [m]
                          (when (%not (%int? m)) (error :m))
                          (if (%lt m 1) ev (od (%sub m 1))))
                     od (fn [m]
                          (when (%not (%int? m)) (error :m))
                          (if (%lt m 1) ev (ev (%sub m 1))))]
              ev)]
      (lcl-ident n)
      c))
  (r:rate "recur-local-mutual-ret-bound" (fn [j] (lcl-mutual-ret-bound 3)))

  # The FACTORY — the everyday shape the whole family serves: mutually recursive local
  # helpers over shared state, handed back in one struct. The letrec body's tail is a
  # struct literal over BOTH members, so it is a tail call on the `struct` native
  # carrying `ev` and `od` in BY-MOVE. That by-move refusal answers a callee which
  # REPLACES the frame, whose new activation's owned-parameter release would decref the
  # merged arena a second time against the deferred release. A native does neither: it
  # borrows its arguments and keeps the frame.
  #
  # So the binding-scope drop stays live and single, and the members the returned struct keeps are a cross-region reference into
  # the arena, RC-counted exactly as a foreign capture is (docs/impl/region/letrec.md).
  # Refusing it held five regions per call: the cycle's two closures and two forward
  # cells, plus the `t` the leaked cycle pinned. A control at 0.
  (defn lcl-mutual-factory [n]
    # Both members leave in the struct (a value use), which disables call-site param
    # joins, so a local diverging guard proves the %lt/%sub operands.
    (let [t @{}]
      (letrec [ev (fn [m]
                    (when (%not (%int? m)) (error :m))
                    (if (%lt m 1) t (od (%sub m 1))))
               od (fn [m]
                    (when (%not (%int? m)) (error :m))
                    (put t m true)
                    (ev (%sub m 1)))]
        {:a ev :b od})))
  (r:rate "recur-local-mutual-factory" (fn [j] (lcl-mutual-factory 3)))

  # ── Retained-closure reclamation (a RETURNED self-recursive closure's region) ──
  # `recur-local-self` above reads the LEAK rate of a self-recursive closure used as a
  # LOOP (0 — cell-free, reclaimed per call). These two RETAIN each returned closure in
  # a block-local @keep, so the question becomes whether the closure's own region
  # reclaims when @keep is freed at the block's return.
  #
  # `lcl-self-ret`'s `go` is cell-free and self-recursive, so the lowerer strands its
  # scope-end `DecrefRegion` past the letrec body's frame-replacing tail call and the
  # runtime deferred release is the region's ONLY channel. A returned closure keeps
  # that channel: the callee's `Return` mints the caller's reference before
  # `trampoline_loop` breaks and runs the deferred decref, so the caller's reference is
  # standing while the deferral drops the frame's own (docs/impl/selfrec.md). The CONTROL
  # `lcl-foreign-ret` is not self-recursive, so nothing strands its release in the
  # first place and the gap isolates the strand rather than the retain. Object growth,
  # not region growth, is the gauge (closure + env share one region). That the
  # self-recursive LOOP is cell-free is a distinct property: see
  # runtime::tests::ownership::jit::self_recursive_loop_is_cell_free. The soundness half,
  # that the returned handle is still live after the deferred release, is pinned under
  # the UAF oracle by tests/impl/region-selfrec-return-release.lisp.
  (defn lcl-self-ret [n]
    "Self-recursive local closure that RETURNS itself (so a retain pins its region)."
    # go is returned (value position), which disables call-site param joins, so a
    # local diverging guard proves the %lt/%sub operands (as in lcl-foreign-ret).
    (letrec [go (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) go (go (%sub m 1))))]
      (go n)))
  (defn lcl-foreign-ret [n]
    "Equal-arity cell-free control: captures the immediate n, not itself."
    # h is only returned (no in-file call sites), so a local diverging guard
    # proves the %lt operand.
    (let [h (fn [m]
              (when (%not (%int? m)) (error :m))
              (if (%lt m 1) n n))]
      h))
  (defn retain-block [mk]
    "Run-block: build (mk) b times into a block-local @keep so each pinned closure's
   region — and the cell it holds — stays live, exposing the per-call mint."
    (fn [b]
      (when (%not (%int? b)) (error :block-not-int))
      (def @keep @[])
      (def @j 0)
      (while (%lt j b)
        (push keep (mk))
        (assign j (%add j 1)))))
  (r:drive "recur-local-self-mint" (retain-block (fn [] (lcl-self-ret 3))))
  (r:drive "recur-local-foreign-mint" (retain-block (fn [] (lcl-foreign-ret 3))))

  # ── The same strand, handed across the FIBER frontier ──────────────────
  # `recur-local-self-yield` and `recur-local-self-send` are controls at 0 for
  # the fiber half of the stranded-self deferred release. Each hands its cell-free
  # self-recursive closure across a fiber frontier, emitted to the resumer or sent over
  # a channel, and then tail-calls it. So the scope-end `DecrefRegion` is dead past that
  # `TailCall` and the deferral is the region's only channel. The crossing is no reason
  # to withhold it. The emit's park retain into `fiber.signal`, which the resumer's
  # result release consumes, and `chan/send`'s send-site incref each count a reference
  # of their own. So the deferral drops the frame's alone (docs/impl/selfrec.md).
  #
  # `recur-local-self` above is the control, the same strand with no crossing, so the
  # gap between them isolates the crossing rather than the strand. The soundness half, that the delivered handle is
  # still live after the deferred release, is pinned under the UAF oracle by
  # tests/impl/region-selfrec-fiber-release.lisp.
  (defn lcl-self-yield [n]
    "Self-recursive local closure YIELDED to the resumer before the body tail-calls it."
    # go crosses the frontier (a value use), which disables call-site param joins, so a
    # local diverging guard proves the %lt/%sub operands (as in lcl-self-ret).
    (letrec [go (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) :done (go (%sub m 1))))]
      (yield go)
      (go n)))
  (def [lcl-snd lcl-rcv] (chan))
  (defn lcl-self-send [n]
    "The same closure SENT over a channel — the other fiber-frontier seed."
    (letrec [go (fn [m]
                  (when (%not (%int? m)) (error :m))
                  (if (%lt m 1) :done (go (%sub m 1))))]
      (chan/send lcl-snd go)
      (go n)))
  (r:rate "recur-local-self-yield"
          (fn [j]
            # Two resumes per op: the first runs to the yield, the second runs the
            # recursion, whose normal completion is where the deferral fires.
            (let [f (fiber/new (fn [] (lcl-self-yield 3)) |:yield|)]
              (fiber/resume f)
              (fiber/resume f))))
  (r:rate "recur-local-self-send"
          (fn [j]
            (lcl-self-send 3)
            (get (chan/recv lcl-rcv) 1)))

  # ── The scheduler frontier — a spawned fiber's round trip ─────────────
  # `ev/spawn` + `ev/join` is the shape structured-concurrency programs are
  # built from, and the one the h2 tests multiply: one session
  # answering 320 requests held ~1 GB of live heap on this round trip alone.
  #
  # What stranded, read off `--trace=rc` for one op: the fiber's own region, the closure
  # it was made from, and the `[ok? value]` pair the join delivered. Each was left at
  # rc=1, its birth reference never released. The frame that owned each one handed it to another
  # fiber on ONE path and reached its end on every other. `wake-select-waiters` takes the
  # completed fiber by tail-call move and resumes a select waiter with it. A program with
  # no select outstanding never takes that arm. The release the branch-arm window
  # would anchor at the merge was refused because the region crosses the fiber frontier —
  # a refusal the crossing's own count retires (docs/impl/region/window.md). A CLOSED
  # control, and the shape is gauged directly by
  # tests/impl/region-fiber-frontier-window.lisp.
  #
  # The SCHEDULER's half of the per-fiber cost is closed and stays closed:
  # a delivered join retires the completion records that would otherwise hold
  # every fiber a program ever spawned (docs/scheduler.md,
  # pinned by tests/impl/sched-completion-records.lisp).
  (r:rate "spawn-join" (fn [j] (ev/join (ev/spawn (fn [] 7))))))
