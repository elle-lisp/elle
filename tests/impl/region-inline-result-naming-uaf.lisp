(elle/epoch 12)
# Soundness complement of region-inline-result-naming.lisp: naming an inlined
# call's result by the CALL's own region must not free anything early.
#
# The caller holds no region of the callee's (docs/impl/region/mechanism.md § "A
# call's result is named by the call's own region"), so the result carries exactly
# ONE caller-side release: value-routed off the call's own region, placed at the
# result binding's last use. That is the direction that can over-free. What the
# callee hands back is not always freshly its own — an argument returned unchanged,
# an element read out of an argument, a value it stored somewhere first — and for
# those the caller's naming offers no second holding to keep the region standing.
# A counted edge has to, or the one release reaches zero under a live reader.
#
# Every witness below is such a hand-off, and each names the edge that has to keep
# the region up: the callee's return mint, a container's store funnel, a closure
# env's capture incref, a fiber's park retain, the branch's per-path frontier. If
# the one release drops the region to zero anyway, the read below faults. Fresh
# allocation per round keeps region ids churning, so a freed region is recycled
# under the reader rather than read as stale but still-mapped bytes.

(def rounds 400)

# ── witnesses: what an inlined callee hands back is not always its own ──

# (a) the callee returns its ARGUMENT unchanged. The caller's result does not name
# the argument's region, so the callee's return mint is the only thing standing
# between the argument's own release at the call and the read of the result.
(defn w-arg-passthrough (n)
  (var i 0)
  (var acc 0)
  (while (< i n)
    (let [id (fn (z) z)]
      (let [xs (list (string "p" i) (string "q" i))]
        (let [v (id xs)]
          (assign acc (+ acc (length v))))))
    (assign i (%add i 1)))
  acc)

# (b) the callee picks one of two arguments per PATH. Escape marks the whole
# result returnable as soon as one path hands a value over, so the release must
# be right on both.
(defn w-arg-branch (n)
  (var i 0)
  (var acc 0)
  (while (< i n)
    (let [pick (fn (a b) (if (even? i) a b))]
      (let [xs (list (string "a" i))
            ys (list (string "b" i) (string "c" i))]
        (let [v (pick xs ys)]
          (assign acc (+ acc (length v))))))
    (assign i (%add i 1)))
  acc)

# (c) the callee READS an element out of its argument. The element lives inside
# the container's region, so the container is what must still be standing when
# the caller reads the result.
(defn w-element-readout (n)
  (var i 0)
  (var acc 0)
  (while (< i n)
    (let [hd (fn (c) (first c))]
      (let [xs (list (string "h" i) (string "t" i))]
        (let [v (hd xs)]
          (assign acc (+ acc (length v))))))
    (assign i (%add i 1)))
  acc)

# (d) the result is STORED into a module-level container that outlives the frame.
# The store funnel's incref is the surviving edge; the caller's one release must
# leave it standing.
(def sink @[nil])
(defn w-store-escape (n)
  (var i 0)
  (while (< i n)
    (let [mk (fn (z) (list (string "s" z) (string "u" z)))]
      (put sink 0 (mk i)))
    (assign i (%add i 1)))
  (length (get sink 0)))

# (e) the result is CAPTURED by a closure called after the loop. The capture
# incref is the surviving edge.
(defn w-capture (n)
  (var held nil)
  (var i 0)
  (while (< i n)
    (let [mk (fn (z) (list (string "c" z) (string "d" z)))]
      (let [v (mk i)]
        (assign held (fn () (length v)))))
    (assign i (%add i 1)))
  (held))

# (f) the result crosses the FIBER frontier. The park retain is the surviving
# edge, and it is taken after the caller's release would have run.
(defn w-yield (n)
  (let [f (fiber/new (fn ()
                       (var i 0)
                       (while (< i n)
                         (let [mk (fn (z) (list (string "y" z)))]
                           (yield (mk i)))
                         (assign i (%add i 1)))
                       :done) |:yield|)]
    (var seen 0)
    (var v (fiber/resume f))
    (while (= (fiber/status f) :paused)
      (assign seen (+ seen (length v)))
      (assign v (fiber/resume f)))
    seen))

# (g) the result feeds the NEXT round's call as an argument, so one round's result
# region is the next round's argument. Length is held at two so the churn is in the
# regions, not the data.
(defn w-fed-forward (n)
  (let [grow (fn (c) (concat c (list :x)))
        shrink (fn (c) (rest c))]
    (var v (list :seed :x))
    (var i 0)
    (while (< i n)
      (assign v (shrink (grow v)))
      (assign i (%add i 1)))
    (length v)))

# (h) the branch shape the naming leak strands, with the result READ after the
# merge: the release sits in the arm that allocates, so the read past the merge is
# what a premature one faults on.
(defn w-branch-read-after (n)
  (var i 0)
  (var acc 0)
  (while (< i n)
    (let [mk (fn (z) (list (string "m" z) (string "n" z)))]
      (let [v (if (even? i) (mk i) (list (string "e" i)))]
        (assign acc (+ acc (length v)))))
    (assign i (%add i 1)))
  acc)

# (i) a self-recursive local walk whose base case allocates the result the caller
# reads — the shape whose release sits in the base arm, one activation below the
# frame that reads it.
(defn w-self-rec (n)
  (var i 0)
  (var acc 0)
  (while (< i n)
    (letrec [go (fn (m)
                  (when (%not (%int? m)) (error :m-not-int))
                  (if (%lt m 1)
                    (list (string "g" i) (string "h" i))
                    (go (%sub m 1))))]
      (let [v (go 2)]
        (assign acc (+ acc (length v)))))
    (assign i (%add i 1)))
  acc)

(println "region-inline-result-naming-uaf: arg-passthrough "
         (w-arg-passthrough rounds))
(println "  arg-branch " (w-arg-branch rounds))
(println "  element-readout " (w-element-readout rounds))
(println "  store-escape " (w-store-escape rounds))
(println "  capture " (w-capture rounds))
(println "  yield " (w-yield rounds))
(println "  fed-forward " (w-fed-forward rounds))
(println "  branch-read-after " (w-branch-read-after rounds))
(println "  self-rec " (w-self-rec rounds))

(assert (= (w-arg-passthrough 4) 8) "argument handed back unchanged was lost")
(assert (= (w-element-readout 4) 8) "element read out of an argument was lost")
(assert (= (w-store-escape 4) 2) "stored result was lost")
(assert (= (w-capture 4) 2) "captured result was lost")
(assert (= (w-fed-forward 4) 2) "fed-forward result was lost")
(assert (= (w-self-rec 4) 8) "self-recursive base-case result was lost")

(println "region-inline-result-naming-uaf: ok")
