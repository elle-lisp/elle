(elle/epoch 14)
# audited: 2026-10-06
# A wrapper call over two proven numbers dissolves to its %-op, and a call over
# unproven operands keeps the wrapper's guards.
#
# Each wrapper is spelled two ways. `dissolved` tests both operands for numbers
# without raising, so the grafted wrapper's own guards fold away and only the op
# is left. `grafted` proves nothing, so the graft keeps the wrapper's guards.
# Which code the compiler emits is the subject here, so the test reads the
# disassembly. What each wrapper answers is a language claim, in
# tests/lang/operators.lisp.
# docs/impl/graft.md

(defn disasm-text [f]
  "The disassembly of f's bytecode, as one string."
  (string/join (fn/disasm f) " "))

# A two-operand function that proves both operands numbers, and raises nothing.
(defmacro dissolved (op)
  `(fn [a b] (if (and (number? a) (number? b)) (,op a b) :not-numbers)))

# A two-operand function that proves nothing about its operands.
(defmacro grafted (op)
  `(fn [a b] (,op a b)))

# Every guard a grafted wrapper brings raises through an Emit. `=` has no guard
# to keep, so it is not here; its number test is pinned in
# hir::typeinfer::tests::graft.
#
# The counter-factual: while a wrapper call stayed a call, its guards lived in
# the wrapper's own body, so the unproven spelling held no Emit either.
(def wrappers
  [[:+ (dissolved +) (grafted +)] [:- (dissolved -) (grafted -)]
   [:* (dissolved *) (grafted *)] [:< (dissolved <) (grafted <)]
   [:> (dissolved >) (grafted >)] [:<= (dissolved <=) (grafted <=)]
   [:>= (dissolved >=) (grafted >=)]])

(each [name proven unproven] wrappers
  (assert (not (string/contains? (disasm-text proven) "Emit"))
          (string name " over two proven numbers keeps no guard"))
  (assert (string/contains? (disasm-text unproven) "Emit")
          (string name " over unproven operands keeps its guards")))
