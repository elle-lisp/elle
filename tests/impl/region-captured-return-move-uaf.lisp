(elle/epoch 15)
# audited: 2026-10-21
# A return of a captured upvalue hands the caller its own owning reference, never the closure env's.
#
# docs/impl/region/mechanism.md
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
# balancing it. A sequence of consumptions would drain the rc to 0 while the
# capture still references the struct, and the next read would be a
# use-after-free. See `lower_return` in src/lir/lower/expr/boundary.rs and the
# return frontier in src/hir/region/infer/escape.rs.
#
# WHY CROSS-UNIT. `import-file` puts the module in its own compilation unit, so
# the accessor has no statically resolved call site and the region solver cannot
# inline it (`try_inline_call`). The call is opaque, and the caller's release
# rests on the callee's return mint alone — the `(import "std/http")` dispatch
# shape.
#
# WHY MULTIPLE CALLS. Without the mint, a single consumption would only drop rc
# 2 -> 1 (the module's capture cell plus this read both hold it). The struct
# would stay live and a plain read would see intact bytes (a false green).
# Draining past the capture's own reference is what would free the live value.
#
# MANIFESTATION. Under `--trace=guardfree` a freed struct would page-fault at
# the final read. In plain mode the junk allocations would recycle the freed
# region, so the read would deref-panic (region-generation assert) or return
# garbage the assert rejects.

(def cfg {:tag :live})

(let [m ((import-file "../modules/captured-return.lisp") :cfg cfg)]
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
