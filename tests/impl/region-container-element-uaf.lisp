(elle/epoch 15)
# audited: 2026-11-17
# A name bound to an element of a var, or to a pattern over it, keeps that element after the var's next assign.
# docs/impl/region/reads.md
#
# The sidecar arms guardfree, so a read of a freed value faults where it reads.
#
# Each shape binds a name to an element of the value a var holds, through an
# opcode read or a pattern, then assigns the var, then reads the name. The
# assign drops what the var held, and the element with it, so the name needs a
# counted reference of its own. A pattern has no binder of its own, so the
# compiler binds its value to a name first and counts that read.
#
# The counter-factual: `%get`, `%first` and the patterns took no count. The
# element's only protection was the var's own reference, which the assign
# released, so the second pass read an element of a freed array. The native
# `first` takes a count of its own, so its row stayed clean throughout.

# (a) a %get of the loop's var.
(defn a-run [n]
  (def out @[])
  (var c (array @[0]))
  (var i 0)
  (while (< i n)
    (def t (%get c 0))
    (assign c (array @[i]))
    (push out (get t 0))
    (assign i (+ i 1)))
  (freeze out))

# (b) the %get sits in one arm of a branch.
(defn b-run [n]
  (def out @[])
  (var c (array @[0]))
  (var i 0)
  (while (< i n)
    (def t (if (< i 0) nil (%get c 0)))
    (assign c (array @[i]))
    (push out (get t 0))
    (assign i (+ i 1)))
  (freeze out))

# (c) a %first cursor over a list a call returns.
(defn c-list []
  (list @[0] @[1] @[2] @[3]))
(defn c-run []
  (def out @[])
  (var cur (c-list))
  (while (pair? cur)
    (def x (%first cur))
    (assign cur (%rest cur))
    (push out (get x 0)))
  (freeze out))

# (d) a def pattern over the loop's var.
(defn d-run [n]
  (def out @[])
  (var c (array @[0]))
  (var i 0)
  (while (< i n)
    (def [t] c)
    (assign c (array @[i]))
    (push out (get t 0))
    (assign i (+ i 1)))
  (freeze out))

# (e) a match on the loop's var, the assign inside the arm.
(defn e-run [n]
  (def out @[])
  (var c (array @[0]))
  (var i 0)
  (while (< i n)
    (match c
      [t]
        (begin
          (assign c (array @[i]))
          (push out (get t 0)))
      _ nil)
    (assign i (+ i 1)))
  (freeze out))

# (f) a let pattern over the loop's var.
(defn f-run [n]
  (def out @[])
  (var c (array @[0]))
  (var i 0)
  (while (< i n)
    (let [[t] c]
      (assign c (array @[i]))
      (push out (get t 0)))
    (assign i (+ i 1)))
  (freeze out))

# (g) a %get under each, where the var is no loop parameter.
(defn g-run []
  (def out @[])
  (var c (array @[0]))
  (each i in [0 1 2 3]
    (def t (%get c 0))
    (assign c (array @[i]))
    (push out (get t 0)))
  (freeze out))

# (h) a def pattern under each.
(defn h-run []
  (def out @[])
  (var c (array @[0]))
  (each i in [0 1 2 3]
    (def [t] c)
    (assign c (array @[i]))
    (push out (get t 0)))
  (freeze out))

# (i) a match under each.
(defn i-run []
  (def out @[])
  (var c (array @[0]))
  (each i in [0 1 2 3]
    (match c
      [t]
        (begin
          (assign c (array @[i]))
          (push out (get t 0)))
      _ nil))
  (freeze out))

# (j) the control: the native first takes a count of its own.
(defn j-run [n]
  (def out @[])
  (var c (array @[0]))
  (var i 0)
  (while (< i n)
    (def t (first c))
    (assign c (array @[i]))
    (push out (get t 0))
    (assign i (+ i 1)))
  (freeze out))

(assert (= (a-run 4) [0 0 1 2]) "a %get reader keeps its element")
(assert (= (b-run 4) [0 0 1 2]) "a %get in a branch arm keeps its element")
(assert (= (c-run) [0 1 2 3]) "a %first cursor keeps each element")
(assert (= (d-run 4) [0 0 1 2]) "a def pattern keeps its element")
(assert (= (e-run 4) [0 0 1 2]) "a match pattern keeps its element")
(assert (= (f-run 4) [0 0 1 2]) "a let pattern keeps its element")
(assert (= (g-run) [0 0 1 2]) "a %get under each keeps its element")
(assert (= (h-run) [0 0 1 2]) "a def pattern under each keeps its element")
(assert (= (i-run) [0 0 1 2]) "a match under each keeps its element")
(assert (= (j-run 4) [0 0 1 2]) "the native first control keeps its element")

(println "region-container-element-uaf: ok")
