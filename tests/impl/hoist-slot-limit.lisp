(elle/epoch 15)
# audited: 2026-11-16
# A unit whose functions read more distinct constants than a function has local slots fails to compile, with an error that names the limit.
# docs/impl/hoist.md
#
# The hoist binds each distinct constant a function reads in one let at the
# root of the unit, so the entry function holds one local slot per constant.
# A slot index is 16 bits wide.
#
# The counter-factual: the unit compiled, and every slot index past 65535
# wrapped around to name a slot another binding holds.
#
# Each function calls its parameter, which no graft can replace, so each
# string stays a constant the function reads. A call stores nothing, so the
# compile takes about a second where a push of each string takes twenty.

(defn unit [fns per]
  "Source text: fns functions, each passing per distinct strings to its argument."
  (def buf @"")
  (var f 0)
  (while (< f fns)
    (push buf (string "(defn f" f " [g]"))
    (var i 0)
    (while (< i per)
      (push buf (string " (g \"s" f "_" i "\")"))
      (assign i (+ i 1)))
    (push buf " nil)\n")
    (assign f (+ f 1)))
  (freeze buf))

(defn compiled [src]
  "[ok? err] for a compile of src as one unit."
  (protect (compile/whole-module src "<hoist-slot-limit>")))

(let [[ok? _] (compiled (unit 10 100))]
  (assert ok? "a unit with 1000 distinct constants compiles"))

(let [[ok? err] (compiled (unit 660 100))
      message (if ok? "" (string (get err :message)))]
  (assert (not ok?) "a unit with 66000 distinct constants does not compile")
  (assert (string/find message "65535") "the error names the limit")
  (assert (string/find message "local slots") "and what it limits"))

(println "hoist-slot-limit: ok")
