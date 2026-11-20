(elle/epoch 16)
# audited: 2026-11-17
# A def nothing reads keeps its value live wherever its body hands that value on.
# docs/impl/region/anchors.md
#
# The leak half is region-define-init-release.lisp, and this file is the
# over-free half. A `def` evaluates to what it bound, so its initializer's
# demise must not be placed at the initializer.
#
# The binding is UNREAD in every row below, which is exactly the condition the
# last-use narrowing keys on — "nothing names this value, so it dies where it was
# made". For every other binder that is true: the form's value is its body, so an
# unread init really is dead at the init. For a `def` it is not. A `def` that
# ends a body is that body's value, so the value flows on through the body, and
# a demise at the initializer frees it under whatever the body was handed to.
# Each row is a different way the value leaves the body, and the read on the far
# side is what faults.
#
# Every row reads the value's HEAP contents through a chain long enough that an
# over-early free faults rather than reading stale but still-mapped bytes, with a
# fresh subject per iteration so a freed region is recycled under the reader.

# (a) a block that ends in the `def` is an ARGUMENT: its value is handed to a
# callee that reads through it.
(defn takes-list (v)
  (length (first v)))
(defn def-as-arg (i)
  (takes-list (block (def x (list (string "a" i) i)))))

# (b) the `def` is the function's whole body, so its value is RETURNED and the
# caller reads it after the defining frame is gone.
(defn def-returned (i)
  (def x (list (string "b" i) i)))
(defn def-returned-read (i)
  (length (first (def-returned i))))

# (c) a block that ends in the `def` is a `let` init, so the value is read back
# through the OTHER name. The demise rides that binding's chain and must not be
# undercut.
(defn def-as-let-init (i)
  (let [y (block (def x (list (string "c" i) i)))]
    (length (first y))))

# (d) the `def` is the last form of a block with a form before it, so the value
# reaches its consumer past the block's earlier forms.
(defn def-in-block (i)
  (takes-list (block 0
                (def x (list (string "d" i) i)))))

# (e) a block that ends in the `def` sits in a branch ARM, so the value reaches
# the consumer through a merge, and the sibling arm supplies a different one.
(defn def-in-arm (i t)
  (takes-list (if t
                (block (def x (list (string "e" i) i)))
                (list "z" 0))))

# (f) the value of a block that ends in the `def` is STORED into a container
# that outlives the frame, and read back out afterwards. The store's incref is
# what a demise at the initializer would have to leave standing.
(def @sink @[])
(defn def-stored (i)
  (push sink (block (def x (list (string "f" i) i))))
  (length (first (get sink 0))))

# (g) a block that ends in the `def` sits inside a closure's body, so the value
# is consumed in the closure's own activation rather than the frame that built
# the closure.
(defn def-captured (i)
  (let [g (fn () (length (first (block (def x (list (string "g" i) i))))))]
    (g)))

# (h) a block that ends in the `def` sits inside a fiber's body, so the value is
# consumed in the fiber's own frame, ahead of the yield that parks it.
(defn def-yielded (i)
  (let [fb (fiber/new (fn ()
                        (emit :yield (takes-list
                                       (block (def x (list (string "h" i) i))))))
                      |:yield|)]
    (fiber/resume fb)))

# ── control: the same read where the binding IS named ────────────────────────
# The narrowing does not fire here (the binding has a use), so this row must pass
# whatever the `def` face does — a harness check, not a subject.
(defn def-read-by-name (i)
  (def x (list (string "n" i) i))
  (length (first x)))

(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var f 0)
(var g 0)
(var h 0)
(var n 0)
(while (%lt i 1500)
  (assign a (def-as-arg i))
  (assign b (def-returned-read i))
  (assign c (def-as-let-init i))
  (assign d (def-in-block i))
  (assign e (def-in-arm i true))
  (assign f (def-stored i))
  (assign g (def-captured i))
  (assign h (def-yielded i))
  (assign n (def-read-by-name i))
  # The sink is a module-level container by design (row f stores into it); drain
  # it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt n 0) "control: the named-binding read mis-read (harness broken)")

(assert (%gt a 0) "a `def`'s value freed under the callee it was handed to")
(assert (%gt b 0) "a returned `def`'s value freed under the caller's read")
(assert (%gt c 0) "a `def`'s value freed under the name a `let` bound it to")
(assert (%gt d 0) "a `def`'s value freed under the block that propagated it")
(assert (%gt e 0) "a `def`'s value freed under the arm that produced it")
(assert (%gt f 0) "a `def`'s value freed after being stored into a container")
(assert (%gt g 0) "a `def`'s value freed under the closure that captured it")
(assert (> h 0) "a `def`'s value freed under a parked frame's resume")
(assert (= (def-in-arm 0 false) 1) "the sibling arm's value lost")

(println "region-define-init-release-uaf: ok")
