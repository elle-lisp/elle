(elle/epoch 12)
# tests/elle/region-captured-return-move-uaf.lisp
#
# Regression guard for the `http.lisp` returned-captured-value over-free
# (hand-off.md "Open bug 1"): a return of a captured upvalue must hand the
# caller its own owning reference, never the closure env's.
#
# THE SHAPE. A closure-as-module captures a struct at init; an accessor returns
# that captured struct; the module's methods consume it
# (tests/modules/captured-return.lisp, mirroring lib/http.lisp's
# `require-compress`). `fetch` merely reads a captured upvalue — the closure env
# owns that reference, the callee produces none of its own. Every `Return` mints
# a fresh owning reference (`lower_return`'s `IncrefValueRegion`), so the
# caller's `DecrefValueRegion` at the call site releases the mint and leaves the
# env's capture-ref intact. Were a return to a captured upvalue to skip that
# mint, each consumption would drop the captured struct's rc with nothing
# balancing it: a sequence of consumptions drains the rc to 0 while the capture
# still references the struct, and the next read is a use-after-free. See
# src/lir/lower/expr.rs `lower_return` and src/hir/regions/analyze.rs (the
# return-as-escape post-pass).
#
# WHY CROSS-UNIT. The bug fires only when the accessor has no statically-
# resolved call site in its compilation unit: a direct named call lets the
# region solver inline (`try_inline_call`) and re-walk the accessor in the
# caller's context, recognising the result as captured and emitting no decref.
# `import-file` puts the module in its own unit, defeating that — exactly the
# `(import "std/http")` dispatch of the real bug.
#
# WHY MULTIPLE CALLS. A single consumption only drops rc 2 -> 1 (the module's
# capture cell plus this read both hold it), so the struct stays live and a
# plain read sees intact bytes (a false green). Draining past the capture's own
# reference is what frees the live value.
#
# MANIFESTATION. Under `--trace=guardfree` the freed struct page-faults at the
# final read; in plain mode the junk allocations recycle the freed region so
# the read deref-panics (region-generation assert) or returns garbage the
# assert rejects.

(def cfg {:tag :live})

(let [m ((import-file "tests/modules/captured-return.lisp") :cfg cfg)]
  (m:a)
  (m:b)
  (m:c)
  (m:a)  # Recycle the freed region so a premature free reads garbage in plain mode.
  (def junk @[])
  (each i in (range 0 64)
    (push junk {:a i :b i :c i}))  # The captured struct must still be intact: its tag is :live.
  (assert (= (m:a) :live)
          (string "captured struct freed under move return: " (m:a))))

(print :ok)
