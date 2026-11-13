# The traffic counters

<!-- audited: 2026-11-13 -->

How often a heap's counts move, which is what the region system costs a program
as it runs.

`arena/increfs` counts every increment the store runs, an inert one on an owned
region included. `arena/decrefs` counts every release the program runs, and
leaves out the free cascade's. Each starts at 0 when the heap is made, never
goes down, and is Immediate, so a call moves neither.

Every leak and soundness instrument ([diagnostics.md](diagnostics.md)) reads a
redundant pair as correct: one count raised and dropped again leaks nothing and
frees nothing early. These counters are what sees one. Both tiers account alike,
so a shape's count per call is the same number on each, and a test asserts it
exactly ([handoff.md](handoff.md)). A read out of a collection takes one count,
and its release gives it back:

```lisp
(def words [(string "a" "b")])

(defn word-size []
  (def w (words 0))
  (length w))

(defn traffic []
  (def increfs (arena/increfs))
  (def decrefs (arena/decrefs))
  (word-size)
  [(- (arena/increfs) increfs) (- (arena/decrefs) decrefs)])

(assert (= (traffic) [1 1]) "the read takes one count, and its release gives it back")
```

The readings sit inside a function, because the top level releases what it
holds at its last use there. `words` is last used by the `(word-size)`
statement, and an assertion's message is a string built and released.
