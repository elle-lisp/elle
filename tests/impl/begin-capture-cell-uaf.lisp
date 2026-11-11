#!/usr/bin/env elle
(elle/epoch 15)

# audited: 2026-11-09
# A closure keeps the top-level binding it captured after a later top-level form redefines the name.
# docs/bindings.md
#
# Form 1 defines x and a closure over it, and form 2 redefines x by a
# destructure. A top-level begin's forms are the file's own, so form 2 binds a
# new x, and the closure keeps the first. The counter-factual: the two forms
# once shared one binding, and the capture cell form 1 made for it was freed
# at form 1's end, so form 2's struct reused the slot and the next access to x
# read through a dangling CaptureCell into that struct.

(begin
  (def x 100)
  (defn add-to-x (& nums)
    (+ x (first nums)))
  (assert (= (add-to-x 42) 142) "form 1: variadic closure capture"))

(begin
  (def {:x x :y y :z z} {:x 1 :y 2 :z 3})
  (assert (= (+ x (+ y z)) 6)
          "form 2: a destructure redefines x in a sibling form")
  (assert (= (add-to-x 42) 142) "form 2: the closure keeps the first x"))
