(elle/epoch 18)
# audited: 2026-11-20
## The shrinker: steps that each propose simpler bits at one draw, and the sweeps that run them until none keeps a case.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
##
## A step is a function of one draw of the case the shrinker holds. It runs
## its candidates in order, and answers the first case that fails where the
## held case fails and is simpler, or nil.

(fn [&named bits source case]
  (defn first-some [xs f]
    "The first value of (F x) over XS that is not nil, or nil."
    (var found nil)
    (each x in xs
      (when (nil? found) (assign found (f x))))
    found)

  (defn deletions [d]
    "The spans of draw D the shrinker tries to delete, as [start end]: every
     bit the draw read, then the bits it rejected."
    (if (< (d :start) (d :accepted))
      [[(d :start) (d :end)] [(d :start) (d :accepted)]]
      [[(d :start) (d :end)]]))

  (defn shrink [g prop failure limit]
    "A case that fails where FAILURE fails, with `:minimal` true when it is no
     simpler: no deletion of one draw's bits or of the bits it rejected, no
     deletion of a run of elements of a repeated draw with its length written
     that many less, no swap of two neighboring elements, and no lowering of
     one draw's choice by 1 or to 0, gives a simpler case that fails there.
     Each step keeps a candidate that fails there and whose bits are simpler.
     Past LIMIT candidates the shrinker stops, and `:minimal` is false."
    (var best failure)
    (var ran 0)
    (var stopped false)
    (defn attempt [candidate]
      "The case the bits CANDIDATE make when CANDIDATE is simpler than BEST's,
       and the case fails where BEST fails and is simpler, or nil. Once LIMIT
       candidates ran, nil, and the shrinker stops."
      (cond
        (not (bits:simpler? candidate (best :bits))) nil
        (>= ran limit)
          (begin
            (assign stopped true)
            nil)
        true
          (let [c (case:run-case g prop (source:replay candidate))]
            (assign ran (+ ran 1))
            (when (and (= (c :failed-in) (best :failed-in))
                       (bits:simpler? (c :bits) (best :bits)))
              c))))
    (defn lowered [d c]
      "BEST's bits with C written over the accepted bits of draw D."
      (bits:spliced (best :bits) (d :accepted) (d :end)
                    (bits:encode c (- (d :end) (d :accepted)))))
    (defn run-deleted [d from k]
      "BEST's bits without the K elements of D's array from element FROM on,
       and with D's choice, which chose the array's length, written K less."
      (let [elements (d :elements)]
        (bits:spliced (lowered d (- (d :choice) k)) (get (get elements from) 0)
                      (get (get elements (+ from k -1)) 1) [])))
    (defn swapped [d j]
      "BEST's bits with elements J and J + 1 of D's array each in the other's
       place, or nil when the swap gives no simpler bits."
      (let [all (best :bits)
            [start middle] (get (d :elements) j)
            end (get (get (d :elements) (+ j 1)) 1)
            before (slice all start end)
            after (concat (slice all middle end) (slice all start middle))]
        (when (= -1 (compare after before)) (bits:spliced all start end after))))

    # ── the steps ───────────────────────────────────────────────────────
    (defn delete-runs [d]
      "Delete a run of the elements of D's array, the longest first. D's
       choice chose the length, so a run no longer than it leaves the array
       the elements it must hold."
      (when (d :elements)
        (first-some (range (d :choice) 0 -1)
                    (fn [k]
                      (first-some (range (+ (- (length (d :elements)) k) 1))
                                  (fn [from] (attempt (run-deleted d from k))))))))
    (defn delete-spans [d]
      "Delete the bits of D, then the bits D rejected."
      (first-some (deletions d)
                  (fn [[start end]]
                    (attempt (bits:spliced (best :bits) start end [])))))
    (defn swap-elements [d]
      "Swap two neighboring elements of D's array, the first pair first."
      (when (d :elements)
        (first-some (range (- (length (d :elements)) 1))
                    (fn [j]
                      (let [candidate (swapped d j)]
                        (when candidate (attempt candidate)))))))
    (defn lower [d]
      "Lower D's choice to 0, or else search between 0 and the choice for the
       smallest choice that fails there."
      (let [v (d :choice)]
        (when (> v 0)
          (or (attempt (lowered d 0))
              (let [@lo 0
                    @hi v
                    @found nil]
                (while (> (- hi lo) 1)
                  (let [mid (+ lo (bit/shr (- hi lo) 1))
                        c (attempt (lowered d mid))]
                    (if c
                      (begin
                        (assign hi mid)
                        (assign found c))
                      (assign lo mid))))
                found)))))

    # A sweep runs its steps at each draw in turn. After a kept case, the
    # first sweep tries the same draw again, and the second moves on, because
    # its search already found the smallest choice there.
    (def sweeps
      [[true [delete-runs delete-spans swap-elements]] [false [lower]]])
    (defn sweep [again steps]
      "Run STEPS at each draw of BEST, and keep the first case one answers.
       Whether any step kept a case."
      (var improved false)
      (var i 0)
      (while (and (not stopped) (< i (source:draw-count (best :record))))
        (let [d (source:draw-at (best :record) i)
              kept (first-some steps (fn [step] (step d)))]
          (when kept
            (assign best kept)
            (assign improved true))
          (unless (and kept again) (assign i (+ i 1)))))
      improved)
    (var improved true)
    (while (and improved (not stopped))
      (assign improved false)
      (each [again steps] in sweeps
        (when (and (not stopped) (sweep again steps)) (assign improved true))))
    (put best :minimal (not stopped)))

  {:shrink shrink})
