(elle/epoch 12)
# Soundness complement of region-break-transfer.lisp: a value carried out of a
# `block` by `break` must SURVIVE every later read of the block's result.
#
# `break` transfers its value to the block (docs/impl/region/mechanism.md §
# "`break` transfers its value; it does not consume it"). The transfer moves the
# broken value's release OUT of the block body — where the break jumps over it —
# and onto the node that consumes the BLOCK's value. Two ways that can go wrong
# and both fault here: pinning at the block's own exit label frees the value
# under whatever the block's result flows into, and failing to carry the broken
# value's regions out of the block leaves the binding that names the result
# holding none, so the binding-chain `decref_point` extension never sees them
# and every read below touches freed pages — SIGSEGV under `--trace=guardfree`.
#
# Every witness reads the broken value's HEAP contents after the block, through
# a chain long enough that an over-early free faults rather than reading stale
# but still-mapped bytes. A fresh subject per iteration keeps region ids
# churning so a freed region is recycled under the reader.

# ── witnesses: the block's result is read AFTER the block ─────────────────────

# (a) the plain shape: a fresh struct broken out, its field read afterwards.
(defn w-bare (i)
  (let [r (block (break {:a (string "v" i) :b i}))]
    (length (get r :a))))

# (b) the value is `let`-bound inside the block first, so its binding's own
# `decref_point` sits inside the body the break jumps out of.
(defn w-let (i)
  (let [r (block (let [x (list (string "p" i) (string "q" i))]
                   (break x)))]
    (length (first r))))

# (c) the block's result is stored into a container that outlives the frame —
# the escape route, which must incref against the transferred region. The sink
# is module-level so the store's own lifetime is unambiguous: what is under test
# is the broken value surviving into it, not the container's demise.
(def @sink @[])
(defn w-store (i)
  (let [r (block (break (string "s" i)))]
    (push sink r)
    (length (get sink (%sub (length sink) 1)))))

# (d) a branch in break position: either arm becomes the block's value.
(defn w-branch (i)
  (let [r (block (break (if (%gt i 0)
                          (list (string "a" i))
                          (list (string "b" i)))))]
    (length (first r))))

# (e) out of a `while` — the implicit `:while` block, whose exit label is
# outside the loop.
(defn w-while (i)
  (let [r (block (while true (break (string "w" i))))]
    (length r)))

# (f) out of a NESTED block to the outer one: the value crosses two exit labels.
(defn w-nested (i)
  (let [r (block :outer
            (block :inner
              (break :outer (list (string "n" i) i))))]
    (length (first r))))

# (g) the block's result is returned through a further call — the value leaves
# the frame that broke it.
(defn take-len (v)
  (length v))
(defn w-forward (i)
  (take-len (block (break (string "f" i)))))

# (h) the `forever`/`break` idiom, with the block in the FUNCTION's tail
# position and the break buried in the loop body — so the value the break
# carries is the function's return value, and the callee's release and the
# caller's must not both consume the one owning reference it holds. The broken
# binding is reassigned per iteration and its value is a slice ALIASING the
# argument, so an over-early release faults on the caller's read of a page the
# argument's region already returned. This is `lib/http.lisp`'s
# `sse-drain-buffered-lines`, whose regression takes the whole SSE suite down.
(defn w-tail-loop-break (i)
  (def @rest (string "p" i "\nq" i "\nz" i))
  (block :drain
    (forever
      (let [nl (string/find rest "\n")]
        (when (nil? nl) (break :drain rest))
        (assign rest (slice rest (inc nl)))))))
(defn w-tail-loop (i)
  (let [r (w-tail-loop-break i)]
    (length r)))

# ── controls: the same reads with no break — correct now (harness sanity) ─────
(defn c-block (i)
  (let [r (block (string "c" i))]
    (length r)))
(defn c-let (i)
  (let [r (let [x (string "d" i)]
            x)]
    (length r)))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var f 0)
(var g 0)
(var h 0)
(var k 0)
(var m 0)
(while (%lt i 3000)
  (assign a (w-bare i))
  (assign b (w-let i))
  (assign c (w-store i))
  (assign d (w-branch i))
  (assign e (w-while i))
  (assign f (w-nested i))
  (assign g (w-forward i))
  (assign m (w-tail-loop i))
  (assign h (c-block i))
  (assign k (c-let i))
  (assign i (%add i 1)))

# Controls: no break involved, correct now.
(assert (%gt h 0) "control: plain block result mis-read (harness broken)")
(assert (%gt k 0) "control: plain let result mis-read (harness broken)")

# Witnesses: the broken value must survive its consuming read.
(assert (%gt a 0) "break-carried struct field freed under the post-block read")
(assert (%gt b 0) "break-carried let-bound list freed under the post-block read")
(assert (%gt c 0)
        "break-carried value freed after being stored into a container")
(assert (%gt d 0) "break-carried branch value freed under the post-block read")
(assert (%gt e 0) "break-out-of-while value freed under the post-block read")
(assert (%gt f 0) "nested-block break value freed under the post-block read")
(assert (%gt g 0) "break-carried value freed under a forwarding call")
(assert (%gt m 0)
        "tail-block break out of a loop returned a value with no owning \
         reference — freed under the caller's read")

(println "region-break-transfer-uaf: ok")
