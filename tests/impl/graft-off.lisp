(elle/epoch 14)
# audited: 2026-10-06
# Under graft = false a call stays a call, to a function the file defines and to
# a stdlib wrapper, while the stdlib keeps the grafts its boot made.
#
# The counter-factual: under the graft, caller's (h 1 2) becomes an AddInt over
# two constants and plus's (+ a b) becomes the wrapper's guards around an Add, so
# neither body holds a TailCall. Had the setting reached the boot, not= would
# call not instead of holding the Not that not's body reaches.
# docs/impl/graft.md

(defn disasm-text [f]
  "The disassembly of f's bytecode, as one string."
  (string/join (fn/disasm f) " "))

(defn h [a b]
  "A function the file defines."
  (+ a b))

(defn caller []
  "A call to h."
  (h 1 2))

(defn plus [a b]
  "A call to the + wrapper."
  (+ a b))

(assert (string/contains? (disasm-text caller) "TailCall")
        "the call to h stays a call")
(assert (not (string/contains? (disasm-text caller) "AddInt"))
        "h's body is not in caller")
(assert (string/contains? (disasm-text plus) "TailCall")
        "the call to + stays a call")
(assert (not (string/contains? (disasm-text plus) "Emit"))
        "the wrapper's guards are not in plus")
(assert (string/contains? (disasm-text not=) "Not")
        "the stdlib keeps the grafts its boot made")
(assert (= (caller) 3) "the call computes what its body computes")
(assert (= (plus 1 2) 3) "the call to + computes the sum")
