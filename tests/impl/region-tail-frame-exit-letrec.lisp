(elle/epoch 13)
# audited: 2026-09-29
# A letrec closure, forward cell or member left past a frame-replacing tail call is released once, before or by it.
# docs/impl/region/relocate.md
# docs/impl/region/letrec.md
#
# region-tail-frame-exit.lisp moves a release the lowerer emits past a
# frame-replacing `TailCall` to just before it. This file drives that
# relocation over what a `letrec` (or a `def`-bound closure) leaves in the dead
# block: a sibling's compiled forward cell, a self-recursive closure the tail
# call's ARGUMENT consumed, the closure region under a branch tail, and the
# letrec member the body tail-calls, whose release the new activation takes
# over instead.
#
# This file is a LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each subject. The soundness complement is
# region-tail-frame-exit-uaf.lisp; the per-op rates are the oracle's
# `tail-frame-exit-*` rows (tests/impl/probe/branch.lisp).

(def window 2000)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(defn tail-sink ()
  0)
(defn tail-sink2 ()
  1)
(defn top-sub (x)
  (%sub x 1))

# forward cells ────────────────────────────────────────────────────────────────

# (a) a sibling's compiled FORWARD CELL. A one-way sibling capture is not a
# cycle — `go` calls `helper` and `helper` does not call back — so no SCC forms and
# the closure-cycle merge never sees the cell; per-region RC is what reclaims both.
# The cell's own release is the binding-scope `DecrefRegion` the lowerer emits after
# the letrec body, which this body's frame-replacing tail call leaves dead. Its
# count argument cannot come from a holder binding, because a binding names the
# closure region its cell points AT, never the cell's own — so the cell rides its
# binding's verdict, its holders being that binding's holders one indirection out.
# Stranding the cell strands the closure with it: the cell's reference is what
# holds that closure's region off zero.
(defn fwd-cell-plain (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (helper m))]
    (go n)))

# (b) the same shape with a SELF-RECURSIVE capturer, a closed control that bounds
# what the projection is responsible for. Here the ownership forest's capture adopt
# claims the cell into `go`'s closure region and suppresses its own decref, so `go`'s
# stranded-self deferral reclaims the pair and the relocation never has to reach the
# cell at all. It must stay bounded either way.
(defn fwd-cell (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (if (%lt m 1) :done (go (helper m))))]
    (go n)))

# (c) the RETURN face: `go` is handed back, so `helper` leaves with it — by the
# return facet alone, which the frame-held admission allows. What keeps the cell
# alive across the relocated release is the counted `closure ⊇ cell` edge `go`'s env
# took, a `needs_capture` binding being captured THROUGH its cell; the cell's own
# region must carry its binding's verdict here or it strands the closure it holds.
(defn fwd-cell-ret (n)
  (letrec [helper (fn (x)
                    (when (%not (%int? x)) (error :x))
                    (%sub x 1))
           go (fn (m)
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) go (go (helper m))))]
    (go n)))

# (d) the same, returning the SIBLING rather than the capturer: the cell's own
# content is what leaves, so the projection must carry the verdict either way round.
(defn fwd-cell-ret-sib (n)
  (letrec [helper (fn (x)
                    (when (%not (%int? x)) (error :x))
                    (%sub x 1))
           go (fn (m)
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) helper (go (helper m))))]
    (go n)))

# (e) the SIBLING captures the self-recursive member, and the letrec body
# tail-calls the sibling. One `TailCall` carries BOTH deferred channels: the
# merged arena's `deferred_release_slot` (`go`'s closure, its env, and the forward
# cell the single-closure self-edge admission collapsed into it) and the sibling's
# own `defer_callee_release`. They name different regions and each drops a
# different reference the frame owns, so the runtime runs both — and reading them
# as alternatives reclaims nothing at all, because the sibling's counted
# `closure ⊇ cell` edge holds the arena off zero until the sibling's own region
# goes.
(defn fwd-cell-sib (n)
  (letrec [go (fn (m) (if (%lt m 1) :done (go (%sub m 1))))
           outer (fn (m) (go m))]
    (outer n)))

# what an operand names ────────────────────────────────────────────────────────

# (f) the exemption reads an operand's VALUE, not its syntax. Here the letrec
# body's tail call names `go` nowhere — its ARGUMENT is a call to `go`, so what the
# callee is handed is that call's RESULT, and `go`'s own closure region was read
# and finished with before the tail call was made. Its release sits at the letrec's
# scope end, past the frame-replacing `TailCall`, and the relocation is what carries
# it back: a self-recursive member is the tail callee's own region only when the
# body tail-calls IT (docs/impl/selfrec.md). Two faces of the same reading: a
# sibling callee and a top-level callee.
(defn arg-call-selfrec (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (helper (go n))))
(defn arg-call-toplevel (n)
  (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (top-sub (go n))))

# (g) the same reading's over-free face, in the leak direction: the operand's
# value-producing leaf IS an allocation, so its region stays exempt. A fresh lambda
# handed to the tail call is the callee's owned parameter, and the closure region
# the argument's `%pair` builds is the moved value itself — hoisting either would
# drop the reference the callee now owns. Bounded here and correct-valued in the
# uaf complement are the two halves of one claim.
(defn call-thunk (g)
  (g))
(defn lambda-arg (n)
  (call-thunk (fn () n)))
(defn aggregate-arg (n)
  (let [xs (list n n)]
    (top-sub (length (%pair xs nil)))))

# a letrec closure under a branch tail ─────────────────────────────────────────

# (h) the same closure region where the letrec BODY's tail is a BRANCH. Every
# arm leaves through its own frame-replacing callee, so the scope-end release is
# emitted at a merge no path arrives at and the relocation replicates it ahead of
# each arm's `TailCall`. A replica counts once only where the run nil-stamps the
# slot it read, and this region's default route is by region id — so the release
# takes the value route of the slot the `letrec` binder recorded for it
# (docs/impl/region/replicate.md). Every arm is driven, and each must release once.
(defn arm-selfrec (n t)
  (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (go n)
    (if t (tail-sink) (tail-sink2))))
(defn arm-selfrec-cond (n t)
  (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (go n)
    (cond
      (%eq t 0) (tail-sink)
      (%eq t 1) (tail-sink2)
      (tail-sink))))

# (h2) the recursion runs INSIDE the arm, so the arm both consumes the closure
# and leaves through a callee. The replica still belongs ahead of that call: the
# closure region is named by neither the callee nor an argument, the callee's
# result being what the arm hands on.
(defn arm-selfrec-inner (n t)
  (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (if t
      (let [r (go n)]
        (top-sub r))
      (tail-sink2))))

# (h3) the partial face: the else arm falls through to the merge and takes the
# release emitted there, while the then arm runs its replica. The nil-stamp is
# what makes the two copies act once, so both arms are driven.
(defn arm-selfrec-partial (n t)
  (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (go n)
    (if t (tail-sink) 5)))

# the def binder ───────────────────────────────────────────────────────────────

# (i) the `def` face of the same bodies. A `def` has no scope NODE, so the
# analysis leaves its closure region's demise where the binding chain put it — the
# binding's last use — and a use as a CALLEE resolves through `last_use` to the node
# that CONSUMES it. So the release is emitted where that call has returned and the
# recursion has completed, needing no relocation at all; only when the consuming
# call is itself the frame-replacing tail call is it dead, and there the deferral
# supplies it (`def-tail`, the closed control). What keeps the live rows off the
# `MakeClosure` itself is that a `def` evaluates to what it bound, so the
# unused-binding narrowing floors the demise at the `def` rather than at its
# initializer (docs/impl/region/anchors.md).
(defn def-arg-call (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (top-sub (go n)))
(defn def-nontail (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (%add (go n) 0))
(defn def-stmt (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (go n)
  0)
(defn def-tail (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (go n))

# the letrec member the body tail-calls ────────────────────────────────────────

# (j) `helper` is captured by its sibling, so it is allocated per call rather than
# seeded as a constant, and its uses span the whole letrec — which puts its demise
# at the letrec's scope end rather than at the call node. Its own region is exempt
# from the relocation by design (moving that release ahead of the call would free
# the closure the call is about to enter), and the exemption's reason IS that the
# new activation takes the release over — so the deferral has to reach a release
# placed at the letrec's scope end, not only one demising at the CALL node. Neither
# of the other two channels fits the shape: a one-way sibling capture is neither
# self-recursion (`stranded_self_bindings`) nor an SCC (`stranded_cycle_bindings`).
#
# The forward CELL is not the callee's own region, so it relocates as any holder
# does and its cascade drops the `cell ⊇ closure` edge ahead of the call; what the
# deferral drops afterwards is the frame's own slot reference.
(defn callee-letrec-member (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (helper m))]
    (helper (go n))))

# (j2) the same member where nothing calls the capturer, so the tail call's
# ARGUMENT names no member at all: the strand is a property of the callee's own
# release placement, not of what the argument evaluated.
(defn callee-member-plain (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (helper m))]
    (helper n)))

# (j3) the member holds a HEAP capture of its own, so the region the deferral
# frees is the root of a cascade rather than a lone closure: `s` reaches the tail
# callee only through `helper`'s environment, and the counted edge the funnel took
# there falls away with the closure region at the callee's completion.
(defn callee-member-capture (s)
  (letrec [helper (fn (x) (%add x (length s)))
           go (fn (m) (helper m))]
    (helper 1)))

(def fwd-cell-d (measure (fn () (fwd-cell 3)) 200 window))
(def fwd-cell-plain-d (measure (fn () (fwd-cell-plain 3)) 200 window))
(def fwd-cell-ret-d (measure (fn () (fwd-cell-ret 3)) 200 window))
(def fwd-cell-ret-sib-d (measure (fn () (fwd-cell-ret-sib 3)) 200 window))
(def fwd-cell-sib-d (measure (fn () (fwd-cell-sib 3)) 200 window))
(def arg-call-selfrec-d (measure (fn () (arg-call-selfrec 3)) 200 window))
(def arg-call-toplevel-d (measure (fn () (arg-call-toplevel 3)) 200 window))
(def lambda-arg-d (measure (fn () (lambda-arg 3)) 200 window))
(def aggregate-arg-d (measure (fn () (aggregate-arg 3)) 200 window))
(def arm-selfrec-t-d (measure (fn () (arm-selfrec 3 true)) 200 window))
(def arm-selfrec-f-d (measure (fn () (arm-selfrec 3 false)) 200 window))
(def arm-selfrec-cond-0-d (measure (fn () (arm-selfrec-cond 3 0)) 200 window))
(def arm-selfrec-cond-2-d (measure (fn () (arm-selfrec-cond 3 2)) 200 window))
(def arm-selfrec-inner-t-d
  (measure (fn () (arm-selfrec-inner 3 true)) 200 window))
(def arm-selfrec-inner-f-d
  (measure (fn () (arm-selfrec-inner 3 false)) 200 window))
(def arm-selfrec-partial-t-d
  (measure (fn () (arm-selfrec-partial 3 true)) 200 window))
(def arm-selfrec-partial-f-d
  (measure (fn () (arm-selfrec-partial 3 false)) 200 window))
(def def-arg-call-d (measure (fn () (def-arg-call 3)) 200 window))
(def def-nontail-d (measure (fn () (def-nontail 3)) 200 window))
(def def-stmt-d (measure (fn () (def-stmt 3)) 200 window))
(def def-tail-d (measure (fn () (def-tail 3)) 200 window))
(def callee-letrec-member-d
  (measure (fn () (callee-letrec-member 3)) 200 window))
(def callee-member-plain-d (measure (fn () (callee-member-plain 3)) 200 window))
(def member-src [1 2 3])
(def callee-member-capture-d
  (measure (fn () (callee-member-capture member-src)) 200 window))

(println "region-tail-frame-exit-letrec deltas over " window " iters:")
(println "  fwd cells: plain " fwd-cell-plain-d "  selfrec-control " fwd-cell-d
         "  returned " fwd-cell-ret-d "/" fwd-cell-ret-sib-d
         "  sibling-captures-member " fwd-cell-sib-d)
(println "  operand value: selfrec " arg-call-selfrec-d "  toplevel "
         arg-call-toplevel-d "  lambda " lambda-arg-d "  aggregate "
         aggregate-arg-d)
(println "  letrec closure under a branch tail: if " arm-selfrec-t-d "/"
         arm-selfrec-f-d "  cond " arm-selfrec-cond-0-d "/" arm-selfrec-cond-2-d
         "  inner " arm-selfrec-inner-t-d "/" arm-selfrec-inner-f-d "  partial "
         arm-selfrec-partial-t-d "/" arm-selfrec-partial-f-d)
(println "  def binder: arg-call " def-arg-call-d "  nontail " def-nontail-d
         "  stmt " def-stmt-d "  tail " def-tail-d)
(println "  letrec member callee: arg-call " callee-letrec-member-d "  plain "
         callee-member-plain-d "  capture " callee-member-capture-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads >=2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? fwd-cell-plain-d
          "a sibling's forward cell past a frame-replacing body")
(bounded? fwd-cell-d
          "control: the same cell where the capture adopt already claims it")
(bounded? fwd-cell-ret-d "the forward cell of a capturer the frame hands back")
(bounded? fwd-cell-ret-sib-d "the forward cell whose own content is handed back")
(bounded? fwd-cell-sib-d
          "the arena and the sibling callee stranded by one tail call")

(bounded? arg-call-selfrec-d
          "a self-recursive member the tail call's ARGUMENT calls")
(bounded? arg-call-toplevel-d "the same under a top-level tail callee")
(bounded? lambda-arg-d "a fresh lambda handed to the tail call as its argument")
(bounded? aggregate-arg-d "the aggregate an argument builds around a local")

(bounded? arm-selfrec-t-d
          "a letrec closure whose body's tail is a branch, then arm")
(bounded? arm-selfrec-f-d "the same branch's else arm")
(bounded? arm-selfrec-cond-0-d "the same under a cond clause body")
(bounded? arm-selfrec-cond-2-d "the same under a cond else body")
(bounded? arm-selfrec-inner-t-d
          "the arm that both runs the recursion and leaves through a callee")
(bounded? arm-selfrec-inner-f-d "the sibling of that arm")
(bounded? arm-selfrec-partial-t-d
          "the tail-calling arm of a partly falling-through branch tail")
(bounded? arm-selfrec-partial-f-d
          "the falling-through arm, which must not double-release")

(bounded? def-arg-call-d
          "a `def`-bound self-recursive closure the tail call's ARGUMENT calls")
(bounded? def-nontail-d "the same `def` under a non-tail consumer")
(bounded? def-stmt-d "the same `def` called for effect")
(bounded? def-tail-d "control: the `def` whose body tail-calls the binding")

(bounded? callee-letrec-member-d
          "the letrec member the body tail-calls, released at the letrec scope end")
(bounded? callee-member-plain-d
          "the same member where the argument names no member")
(bounded? callee-member-capture-d
          "the same member's own heap capture, reclaimed by its cascade")

# Value preservation: relocating a release must not change what runs.
(assert (= (fwd-cell 3) :done) "forward-cell walker result lost")
(assert (= (fwd-cell-sib 3) :done) "sibling-captures-member result lost")
(assert (= (fwd-cell-plain 3) 2) "plain forward-cell result lost")
(assert (= (def-arg-call 3) -1) "def-binder arg-call result lost")
(assert (= (def-nontail 3) 0) "def-binder nontail result lost")
(assert (= (def-stmt 3) 0) "def-binder statement result lost")
(assert (= (def-tail 3) 0) "def-binder tail result lost")
(assert (= (arg-call-selfrec 3) -1) "operand-value selfrec result lost")
(assert (= (arg-call-toplevel 3) -1) "operand-value toplevel result lost")
(assert (= (arm-selfrec 3 true) 0) "branch-tail letrec then result lost")
(assert (= (arm-selfrec 3 false) 1) "branch-tail letrec else result lost")
(assert (= (arm-selfrec-cond 3 2) 0) "branch-tail letrec cond else result lost")
(assert (= (arm-selfrec-inner 3 true) -1) "inner-recursion arm result lost")
(assert (= (arm-selfrec-inner 3 false) 1) "inner-recursion sibling arm lost")
(assert (= (arm-selfrec-partial 3 false) 5)
        "partial branch-tail fall-through result lost")
(assert (= (callee-letrec-member 3) 1) "callee-letrec-member result lost")
(assert (= (callee-member-plain 3) 2) "callee-member-plain result lost")
(assert (= (callee-member-capture member-src) 4)
        "callee-member-capture result lost")
(assert (= (lambda-arg 3) 3) "lambda argument result lost")
(assert (= (aggregate-arg 3) 0) "aggregate argument result lost")
# The returned capturer's base case hands back `go` itself, so driving it re-enters
# the recursion — every step derefs the cell to reach `helper`, after the defining
# frame is gone.
(assert (not (nil? ((fwd-cell-ret 3) 3)))
        "returned capturer must still be callable after its cell's release")
(assert (= ((fwd-cell-ret-sib 3) 9) 8)
        "returned sibling must still be callable after its cell's release")

(println "region-tail-frame-exit-letrec: ok")
