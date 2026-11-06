(elle/epoch 15)
# audited: 2026-11-05
# A closure called after a loop gets its arguments in order when the loop body splices a call and then runs an and.
# docs/functions.md
#
# The trap: the spliced call leaves a dead copy of its source on the operand
# stack, and the and's two paths reach its merge with and without that copy.
# On the wrong build, one stray pop per pass falls below the frame, and the
# call after the loop takes its second argument as its first: [3 3], with no
# error. The damage depends on what sits under the frame, so each form below
# is a whole function, and the first form is the program as it was found.

(defn scan-after-splice [n]
  (let [@out nil
        @i 0]
    (defn add [kind word]
      (assign out [kind word]))
    (defn scan []
      (while (< i n)
        (apply + [])
        (and (< i n) (if (= 1 2) true false))
        (assign i (inc i)))
      (add :kind i))
    (scan)
    out))

(assert (= (scan-after-splice 3) [:kind 3])
        "add receives :kind as its first argument")
(assert (= (scan-after-splice 0) [:kind 0])
        "a loop that never runs leaves the arguments in order")

# The and's first operand is the counter itself, not a comparison.
(defn scan-after-and-on-counter [n]
  (let [@out nil
        @i 0]
    (defn add [kind word]
      (assign out [kind word]))
    (defn scan []
      (while (< i n)
        (apply + [])
        (and i (if (= 1 2) true false))
        (assign i (inc i)))
      (add :kind i))
    (scan)
    out))

(assert (= (scan-after-and-on-counter 3) [:kind 3])
        "an and on the counter leaves the arguments in order")

# The and's second operand is a match, and the spliced call has an argument.
(defn scan-after-match [n]
  (let [@out nil
        @i 0]
    (defn add [kind word]
      (assign out [kind word]))
    (defn scan []
      (while (< i n)
        (apply + [1])
        (and (< i n)
             (match 1
               2 true
               _ false))
        (assign i (inc i)))
      (add :kind i))
    (scan)
    out))

(assert (= (scan-after-match 3) [:kind 3])
        "an and over a match leaves the arguments in order")
