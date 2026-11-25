(elle/epoch 19)
# audited: 2026-11-25
# each over an @array its body pushes onto visits the positions the array had at the start.
# docs/loops.md

## The trap: the scheduler pushes onto an @array it walks with each. The
## counter-factual counts from minus the length, so a push shifts every later
## read: this loop sees 1 3 9, skips 2, and the skipped work never ends.
(let [queue @[1 2 3]
      seen @[]]
  (each x in queue
    (when (= x 1) (push queue 9))
    (push seen x))
  (assert (= (freeze seen) [1 2 3])
          "each: an @array's positions are the ones it had at the start"))
