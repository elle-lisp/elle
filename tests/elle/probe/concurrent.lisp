(elle/epoch 12)
# audited: 2026-09-08
# The direct-loop rows whose drive crosses a fiber: closures, protect and defer, the park families, the emit and error deliveries.
#
# docs/impl/region/diagnostics.md
(def suite-concurrent
  [# non-yielding fiber / closure / protect loops
   ["closure-while"
    (fn [j]
      (let [f (fn [] j)]
        (f))) 0]
   ["fiber-while"
    (fn [j]
      (let [f (fiber/new (fn [] j) 1)]
        (fiber/resume f))) 0]
   ["concat-while" (fn [j] (concat "x" (number->string j))) 0]
   ["protect-while"
    (fn [j]
      (let [[ok v] (protect ((fn [] j)))]
        v)) 0]  # `defer` on its ordinary SUCCESS path — the twin of `protect-while`
   # above. Same inner fiber, same resume; the whole difference is the trailing
   # `if`, which reads the fiber with `fiber/value` in the arm taken here and with
   # `fiber/propagate` in the arm that is not. Declaring `fiber/propagate` `Mixed`
   # seeds that fiber on escape's store facet, the branch-arm release window
   # declines, and the branch's only release stays in the untaken arm — 2 regions
   # and 3 objects stranded per evaluation, so a loop whose body is wrapped in
   # `defer` grows without bound (docs/impl/region/effects.md § `Opaque`, "The
   # child-chain WIRING is `Opaque` too"). `protect-while` has no such arm and reads
   # 0 whatever the declaration says, which is what makes it the pair-control here
   # and not the gauge. Both are CLOSED controls (undeclared, like
   # `rest-array-copy`), so a regression to open trips the completeness gate loudly
   # instead of being absorbed under F2.
   ["defer-while"
    (fn [j]
      (defer
        (length [1 2])
        ((fn [] j)))) 0]  # The other arm, and the control.
   # `defer-error` raises in the body, so the arm it drives is the PROPAGATE arm —
   # the one that held the branch's only release under `Mixed`, and so the one that
   # correctly stays closed under `defer-while`'s counterfactual. It is what tells a
   # real fix from a moved strand: a change that only re-anchored the release onto
   # the other arm closes `defer-while` and opens this. The arm also leaves by
   # SIG_PROPAGATE rather than falling through, so a release the window replicates
   # into it must survive a signal exit to read 0 here.
   ["defer-error"
    (fn [j]
      (protect (defer
                 (length [1 2])
                 (error j)))) 0]
   ["one-shot"
    (fn [j]
      (let [f (fiber/new (fn [] j) 1)]
        (fiber/resume f))) 0]
   ["alloc-return"
    (fn [j]
      (let [f (fiber/new (fn [] (string "v-" j)) 1)]
        (fiber/resume f))) 0]
   ["fiber-nested"
    (fn [j]
      (let [f (fiber/new (fn []
                           (let [g (fiber/new (fn [] j) 1)]
                             (fiber/resume g))) 1)]
        (fiber/resume f))) 0]
   ["multi-resume"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield 1)
                           (yield 2)
                           3) |:yield|)]
        (fiber/resume f)
        (fiber/resume f)
        (fiber/resume f))) 0]
   ["protect-call"
    (fn [j]
      (let [[ok v] (protect (+ 1 2))]
        v)) 0]
   ["yield-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield {:x j})
                           99) |:yield|)]
        (fiber/resume f))) 0]
   ["never-resumed"
    (fn [j]
      (let [f (fiber/new (fn [] {:x j}) |:yield|)]
        f)) 0]
   ["denied-discard"
    (fn [j]
      (let [f (fiber/new (fn [] (println "blocked")) |:error :io| :deny |:io|)]
        (fiber/resume f)
        (get (fiber/value f) :error))) 0]
   # The adopt-park family and its controls — the `ap-*` defns above hold the
   # attribution set. CLOSED controls on both dimensions (see the ledger note
   # beside `declare-root :f2`); the region pins live in `@dual-read`.
   ["adopt-park-drop"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-adopting-body j)) |:yield|)]
        (fiber/resume f))) 0]
   ["adopt-park-abort"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-adopting-body j)) |:yield|)]
        (fiber/resume f)
        (protect (fiber/abort f "boom")))) 0]
   ["adopt-park-cancel"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-adopting-body j)) |:yield|)]
        (fiber/resume f)
        (fiber/cancel f :dead))) 0]
   ["adopt-complete"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-adopting-body j)) |:yield|)]
        (fiber/resume f)
        (fiber/resume f))) 0]
   ["adopt-nopark"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-nopark-body j)) 1)]
        (fiber/resume f))) 0]
   ["plain-park-drop"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-plain-body j)) |:yield|)]
        (fiber/resume f))) 0]
   ["adopt-before-park-drop"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-before-body j)) |:yield|)]
        (fiber/resume f))) 0]
   ["adopt-before-park-cancel"
    (fn [j]
      (let [f (fiber/new (fn [] (ap-before-body j)) |:yield|)]
        (fiber/resume f)
        (fiber/cancel f :dead))) 0]  # A parked fiber hard-killed by `fiber/cancel` reclaims fully: the kill
   # frees everything the fiber owns (owner nodes, the parked signal's park
   # escape retain), and no carrier retain pins the fiber region
   # (docs/impl/region/owner.md § "Park/unpark symmetry").
   ["cancel-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield j)
                           9) |:yield|)]
        (fiber/resume f)
        (fiber/cancel f :dead)
        (fiber/status f))) 0]  # `fiber/abort` of a PARKED fiber. `fiber/abort` is a
   # native tail call here, and its fiber argument is a captured upvalue — a BORROWED
   # tail argument, for which the frame mints a fresh owning reference so the callee
   # has one to release. The abort leaves by SIG_ABORT, which reaches neither consumer
   # of that retain (a frame-replacing closure callee's owned-param release, or the
   # post-`TailCall` fall-through the native's normal completion runs), so the signal
   # exit consumes it itself (docs/impl/region/mechanism.md § "What the fall-through
   # owes, a signal exit owes too"). The stranded reference was the fiber's own, which
   # pinned the body closure and the parked frame's payload behind it. A CLOSED control
   # now, beside `denied-discard`, whose residual was the args array the denied
   # `(println …)` built for its own spliced call.
   ["abort-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield j)
                           9) |:yield|)]
        (fiber/resume f)
        (protect (fiber/abort f "boom")))) 0]  # An abandoned park through the DYNAMIC
   # emit path: a first argument the compiler cannot read as a keyword set falls
   # through to the `emit` primitive, so the park is an ordinary call rather than the
   # `Emit` terminator and the body reference the discharge stands in for comes from
   # the call rather than from `lower_emit` (docs/impl/region/owner.md § "What yields
   # is the emit OPERATION, not the `Emit` node"). Each gauges the reference's ARITY,
   # not its presence — withholding it over-frees, which no leak gauge sees and
   # `tests/elle/region-dynamic-emit-borrow-uaf.lisp` reports. The four must stay
   # together: the two witnesses differ only in POSITION, which decides where the
   # reference comes from — a non-tail park mints one at the payload argument, a tail
   # park already has the borrowed-argument retain and the suspending exit leaves it
   # standing (docs/impl/region/mechanism.md § "What the fall-through owes, a signal
   # exit owes too") — and each has a control that removes one ingredient:
   # `emit-lit-discard` takes the literal path with the same borrow, and
   # `emit-dyn-fresh` takes the dynamic path with a payload the body allocates, where
   # nothing is owed and a mint would strand one per park. CLOSED controls
   # (undeclared, like `rest-array-copy`), so a regression to open trips the
   # completeness gate loudly rather than being absorbed under F2.
   ["emit-dyn-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-sig emit-subject)
                           9) |:yield|)]
        (fiber/resume f))) 0]
   ["emit-dyn-tail"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-sig emit-subject)) |:yield|)]
        (fiber/resume f))) 0]
   ["emit-lit-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit :yield emit-subject)
                           9) |:yield|)]
        (fiber/resume f))) 0]
   ["emit-dyn-fresh"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-sig (string "v" j))
                           9) |:yield|)]
        (fiber/resume f))) 0]  # The same operation raising a TERMINAL signal, where
   # the reference the tail call holds answers to a different consumer: the payload's
   # DELIVERY, released by whoever catches the signal. The exit consumes its
   # borrowed-argument retains — the block that would have consumed them is abandoned,
   # and an `:error` fiber's restart replays it — so it mints the delivery and records
   # it, the pair `handle_emit` performs on the literal path
   # (docs/impl/region/mechanism.md § "What the fall-through owes, a signal exit owes
   # too"). CLOSED controls (undeclared, like `rest-array-copy`), so a regression to
   # open trips the completeness gate loudly rather than being absorbed under F2.
   # Each reads the mint's ARITY: withholding it over-frees, which no leak gauge sees
   # and tests/elle/region-dynamic-emit-terminal-uaf.lisp reports. The six must stay
   # together, because only the gaps between them separate the mint from the record.
   # `emit-dyn-error-fresh` allocates its payload in the body, so the frame's own
   # reference is what the record reclaims and a mint per reference reads 1;
   # `emit-dyn-error-repeat` names one region through BOTH arguments, so the frame
   # holds a moved reference and a retain and the walk must run the release the
   # exemption used to skip; `emit-dyn-error-restart` resumes the raised fiber, so the
   # replayed block releases the frame's reference where the discharge otherwise
   # would. `emit-dyn-error-discard` and `emit-lit-tail-error` remove one ingredient
   # each — the tail position, and the primitive itself.
   ["emit-dyn-tail-error"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-error-sig emit-subject)) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-error-sig emit-subject)
                           9) |:error|)]
        (fiber/resume f))) 0]
   ["emit-lit-tail-error"
    (fn [j]
      (let [f (fiber/new (fn [] (emit :error emit-subject)) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-fresh"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-error-sig (string "v" j))) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-repeat"
    (fn [j]
      (let [f (fiber/new (fn []
                           (let [t (set :error)]
                             (emit t t))) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-restart"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-error-sig (string "v" j))) |:error|)]
        (fiber/resume f)
        (fiber/resume f))) 0]  # The same raise OFF TAIL POSITION, where the site
   # takes the retain instead of the call's argument convention and the exit leaves
   # it standing for the continuation past the call (docs/impl/region/owner.md
   # § "What yields is the emit OPERATION, not the `Emit` node"). CLOSED controls
   # (undeclared, like `rest-array-copy`). What each reads is where that retain's one
   # consumer is: `emit-dyn-error-discard` above resumes once, so no replay arrives
   # and the frames' own release table is the only route to it — the face that goes
   # open by one region per op if the site's stash stops recording there, or if the
   # raise stops recording its mint. `emit-dyn-stmt-error-restart` resumes twice, so
   # the replay runs that release instead, and `emit-dyn-stmt-error-fresh` allocates
   # its payload in the body, where the site mints nothing and the body's own release
   # is what the two routes carry. All three read the mint's ARITY: withholding it
   # over-frees, which no leak gauge sees and
   # tests/elle/region-dynamic-emit-statement-uaf.lisp reports.
   ["emit-dyn-stmt-error-restart"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-error-sig emit-subject)
                           9) |:error|)]
        (fiber/resume f)
        (fiber/resume f))) 0]
   ["emit-dyn-stmt-error-fresh"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-error-sig (string "v" j))
                           9) |:error|)]
        (fiber/resume f)
        (fiber/resume f))) 0]  # An emit-raised error's payload keeps
   # every frame-owed release: `(error v)` mints the payload's delivery itself (the
   # `EmitEscape` retain the resumer's release of the resume result consumes), so the
   # raise records the mint and the abandoned-frame walk and the parked frame's
   # discharge stop exempting the payload's region (docs/impl/region/mechanism.md
   # § "An abandoned frame runs the releases it still owes"). CLOSED controls
   # (undeclared, like `rest-array-copy`), so a regression to open trips the
   # completeness gate loudly rather than being absorbed under F2; the soundness
   # complement is tests/elle/region-error-payload-uaf.lisp. The faces are distinct
   # consumers of the recorded mint and must stay together: `error-payload` raises
   # in the try's own body frame, which is PARKED for the restarts system, so its
   # release runs at the free-path discharge; `error-payload-helper` raises in a
   # called frame, which is WALKED at the error exit; `error-payload-param` hands
   # the payload down as an owned parameter, so the tail-replaced parked frame owes
   # it through the prologue-recorded slot; `error-payload-struct` raises a struct whose message
   # string is a second region, so both of the frame's tables are owed.
   # `error-payload-native` is the pair-control for all four: a native raise
   # installs its payload unretained, so the frame-funded exemption stays — the gap
   # isolates the recorded mint from the walk and discharge themselves.
   # `error-payload-helper` calls its raiser as a STATEMENT: a bare call in the
   # try body is a frame-replacing tail call, which lands in the parked frame like
   # `error-payload` — only the non-tail call leaves a callee frame for the walk.
   # Its pin is CROSS-TIER at 0: both tiers walk the abandoned frame, the compiled
   # one off the tables its prologue materialized and the locals it spills at the
   # exit, so the rate agrees under --jit=off and --jit=eager. The compiled face
   # has its own gauge in tests/elle/region-jit-error-unwind.lisp.
   ["error-payload"
    (fn [j]
      (try
        (error (string "x" j))
        (catch e nil))) 0]
   ["error-payload-helper"
    (fn [j]
      (try
        (begin
          (ep-raiser j)
          nil)
        (catch e nil))) 0]
   ["error-payload-param"
    (fn [j]
      (try
        (ep-raise-param (string "x" j))
        (catch e nil))) 0]
   ["error-payload-struct"
    (fn [j]
      (try
        (error {:error :e :message (string "m" j)})
        (catch e nil))) 0]
   ["error-payload-native"
    (fn [j]
      (try
        (get j :k)
        (catch e nil))) 0]  # The two fiber-crossing DELIVERIES, both CLOSED
   # controls (undeclared, like `rest-array-copy`) so a regression to open trips
   # the completeness gate loudly rather than being absorbed under F2. Each
   # gauges a mint's ARITY rather than its presence: withhold the mint and the
   # crossing over-frees, which is a soundness failure no leak gauge can see and
   # `--trace=guardfree` reports (`region_primitive_resume_uaf`,
   # `region_fiber_propagate_uaf` in tests/integration/elle_scripts.rs); mint
   # more than one and the surplus is a reference no release answers, which is
   # what these rates catch.
   #
   # `primitive-resume-*` are the three shapes a parked primitive's resume value
   # reaches — bound by the body, returned from tail position, and held across a
   # further park — beside `emit-resume-literal`, the control whose resume block
   # mints in bytecode and so is correct with no delivery mint at all. A parked
   # CAPABILITY DENIAL is the fourth park of this shape and has no probe here:
   # `denied-discard` drives that shape under F2, and it never resumes the denied
   # fiber, so no rate here would see the delivery at all. Its soundness face is
   # the `w-denied` witness under guardfree.
   ["primitive-resume-bind" (fn [j] (pr-bind j)) 0]
   ["primitive-resume-tail" (fn [j] (pr-tail j)) 0]
   ["primitive-resume-keep" (fn [j] (pr-keep j)) 0]
   ["emit-resume-literal" (fn [j] (pr-literal j)) 0]
   # `propagate-*` read the same mint across propagate DEPTH: the three must stay
   # together, because a surplus delivery reference strands one region per park
   # and only the depth gap tells that apart from the raise's own cost.
   # `propagate-none` is that raise with no propagate in it, and it is a scalar 0
   # rather than the baseline a differential would subtract — the raising body's
   # own reference to the payload it allocated is released by the abandoned
   # frame's release-table walk (§ `error-payload*` above), so there is nothing
   # left for a depth difference to hide behind.
   ["propagate-none" (fn [j] (pg-none j)) 0]
   ["propagate-one" (fn [j] (pg-one j)) 0]
   ["propagate-three" (fn [j] (pg-three j)) 0]
   # The captured-`def` env cell and its `let` twin, CLOSED controls for the env
   # ROUTE. Undeclared, like `rest-array-copy`.
   ["env-cell-def-capture" (fn [j] (ec-def-capture)) 0]
   ["env-cell-let-twin" (fn [j] (ec-let-twin)) 0]
   # The closure-as-module pair, CLOSED controls for the declined move. The
   # `Immediate` init is what splits the binding's two regions; the heap init is
   # the same module with both admitted together.
   ["module-cell-read-window" (fn [j] (mod-cell-immediate)) 0]
   ["module-cell-heap-init" (fn [j] (mod-cell-heap)) 0]])

(run-direct-loop suite-concurrent)
