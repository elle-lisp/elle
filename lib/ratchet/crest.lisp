(elle/epoch 15)
# audited: 2026-10-21
## lib/ratchet/crest.lisp — the crest: the slope of the object high-water mark
## in the input size of one call, for what a call holds to its return.
## docs/ratchet.md
##
## Loaded by lib/ratchet.lisp with the instrument's own settle!, so a crest
## prints and is kept like every other reading:
##   (def cr ((import "std/ratchet/crest") settle!))
## The guide is lib/ratchet.md.

(fn [settle!]
  # ── the mark ──────────────────────────────────────────────────────
  # The baseline is arena/peak read after the reset, not the reset's own
  # answer, which is the previous mark. What the instrument itself mints
  # inside the window is the same at every size, and cancels in the
  # difference the crest reads.
  (defn mark-of [shape n]
    "The objects (SHAPE n) held live at its peak, above the count it began
     at."
    (when (%not (%int? n)) (error :n-not-int))
    (arena/reset-peak)
    (let [base (arena/peak)]
      (shape n)
      (let [top (arena/peak)]
        (when (%not (%int? base)) (error :mark-not-integer))
        (when (%not (%int? top)) (error :mark-not-integer))
        (%sub top base))))

  (defn slope [shape n]
    "The mark's difference between 2N and N, over N: objects per element of
     the input, after one uncounted run of (SHAPE N)."
    (when (%not (%int? n)) (error :n-not-int))
    (shape n)
    (let [at-n (mark-of shape n)
          at-2n (mark-of shape (%mul 2 n))]
      (/ (float (%sub at-2n at-n)) (float n))))

  # ── the proof ─────────────────────────────────────────────────────
  # A list of n pairs holds every pair until the call ends, so its mark
  # climbs one per element: the shape a live crest must read above its
  # floor, driven once per instrument ahead of the first crest.
  (defn climb [n]
    (when (%not (%int? n)) (error :n-not-int))
    (def @xs ())
    (def @i 0)
    (while (%lt i n)
      (assign xs (pair i xs))
      (assign i (%add i 1)))
    (length xs))

  (def @proven false)

  (defn prove! []
    "Report the crest's own live-growth row, `crest gauge (live-growth)`,
     once: a growth floor at 0.5 on the :crest axis."
    (when (not proven)
      (assign proven true)
      (settle! {:subject "crest gauge (live-growth)"
                :axis :crest
                :value (slope climb 100)
                :half 0
                :unit "objects/element"
                :class :growth
                :floor 0.5})))

  # ── the reading ───────────────────────────────────────────────────
  (defn crest [subject shape &named @n]
    "The slope of the object high-water mark in the input size of (SHAPE n):
     the mark above its reset baseline at N and at twice N, their difference
     over N, after one uncounted run. A mark is a count, so the half-width is
     0. One reading, printed and answered."
    (default n 100)
    (when (%not (%int? n)) (error :n-not-int))
    (prove!)
    (settle! {:subject subject
              :axis :crest
              :value (slope shape n)
              :half 0
              :unit "objects/element"}))

  {:crest crest})
