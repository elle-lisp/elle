(elle/epoch 18)
# audited: 2026-11-20
## The recording source behind `draw`: it answers each draw from bits or from a decider, and records every bit and draw.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
## docs/stdlib/check/replay.md
##
## A source is a struct. `:draw` answers a draw, and `:record` returns what
## its draws recorded so far as a struct: `:bits`, every bit the draws read or
## wrote, and one array per field, with an entry per draw. `:bounds` holds
## each draw's bound, `:choices` its choice, `:starts` where its bits start,
## `:accepted` where its accepted bits start, and `:ends` where they end.
## `:elements` holds the `[start end]` of each element's bits for the draw
## that chose the length of a repeated draw, and nil for any other draw.

(fn [&named bits errors]
  (def limit (bit/shl 1 62))

  (defn check-bound [n]
    "Raise unless N is a bound a draw takes: an integer from 1 to 2^62."
    (unless (and (int? n) (<= 1 n) (<= n limit))
      (errors:argument-error (string "draw: expected an integer from 1 to 2^62, got "
                                     n))))

  (defn check-edges [n edges]
    "Raise unless EDGES is nil or an array of choices from 0 to N less one."
    (unless (or (nil? edges)
                (and (array? edges)
                     (all? (fn [c] (and (int? c) (<= 0 c) (< c n))) edges)))
      (errors:argument-error (string "draw: expected edges to be an array of choices "
                                     "from 0 to " (- n 1) ", got " edges))))

  (defn recording [next-bit &opt decide]
    "A source whose draws read bits from NEXT-BIT, a function of the index of
     the bit, and record every bit and every draw. A draw reads the bits of
     its bound less one, and reads as many again while they name the bound or
     more. Given DECIDE, a function of a draw's bound and edges that answers a
     choice or nil, a draw DECIDE answers writes the bits of that choice."
    (def out @[])
    (def columns
      {:bounds @[]
       :choices @[]
       :starts @[]
       :accepted @[]
       :ends @[]
       :elements @[]})
    (defn next []
      "Read the next bit, and record it."
      (let [b (next-bit (length out))]
        (push out b)
        b))
    (defn choose [n edges]
      "A choice from 0 to N less one, recorded as a draw."
      (check-bound n)
      (check-edges n edges)
      (let [k (bits:width n)
            start (length out)
            decided (when decide (decide n edges))]
        (var at start)
        (var c decided)
        (if (nil? decided)
          (begin
            (assign c (bits:read next k))
            (while (>= c n)
              (assign at (length out))
              (assign c (bits:read next k))))
          (bits:write out decided k))
        (push columns:bounds n)
        (push columns:choices c)
        (push columns:starts start)
        (push columns:accepted at)
        (push columns:ends (length out))
        (push columns:elements nil)
        c))
    (defn draw [n &opt lo hi]
      "A choice from 0 to N less one, where LO, when given, holds the draw's
       edges. Given HI too, an array of LO to HI values of the generator N:
       one draw chooses the length, and records the bits of each element."
      (if (nil? hi)
        (choose n lo)
        (begin
          (unless (and (int? lo) (int? hi) (<= 0 lo) (<= lo hi))
            (errors:argument-error (string "draw: expected 0 <= lo <= hi, got "
                                     lo " and " hi)))
          (let [at (length columns:choices)
                size (+ lo (choose (+ (- hi lo) 1) nil))
                elements @[]
                values @[]]
            (repeat size
              (let [start (length out)]
                (push values (n draw))
                (push elements [start (length out)])))
            (put columns:elements at (freeze elements))
            (freeze values)))))
    (defn record []
      "What the draws recorded so far."
      {:bits (freeze out)
       :bounds (freeze columns:bounds)
       :choices (freeze columns:choices)
       :starts (freeze columns:starts)
       :accepted (freeze columns:accepted)
       :ends (freeze columns:ends)
       :elements (freeze columns:elements)})
    {:draw draw :record record})

  (defn replay [sequence]
    "A source that reads the bits of SEQUENCE in order, then 0."
    (recording (fn [i] (if (< i (length sequence)) (get sequence i) 0))))

  (defn replay-choices [choices]
    "A source whose draws answer the choices of CHOICES in order, each
     written in the bits of its draw's width. A choice at or past its draw's
     bound answers 0, and so does each draw past the end of CHOICES."
    (var i 0)
    (recording (fn [_] 0)
               (fn [n _]
                 (let [c (if (< i (length choices)) (get choices i) 0)]
                   (assign i (+ i 1))
                   (if (< c n) c 0)))))

  (defn draw-count [record]
    "How many draws RECORD holds."
    (length (record :choices)))

  (defn draw-at [record i]
    "Draw I of RECORD, as a struct of its fields: :bound, :choice, :start,
     :accepted, :end and :elements."
    {:bound (get (record :bounds) i)
     :choice (get (record :choices) i)
     :start (get (record :starts) i)
     :accepted (get (record :accepted) i)
     :end (get (record :ends) i)
     :elements (get (record :elements) i)})

  {:recording recording
   :replay replay
   :replay-choices replay-choices
   :draw-count draw-count
   :draw-at draw-at})
