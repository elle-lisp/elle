(elle/epoch 12)
# audited: 2026-09-29
# A string a helper accumulates in a capture cell and returns stays live across the caller's read.
# docs/impl/region/bindings.md
#
# The sidecar arms guardfree and keeps the JIT off, so an over-free faults at
# the caller's read.
#
# WHAT IT PINS — the STRING sibling of region-fold-closure-arg-uaf.lisp (that one
# exercises a threaded/cell-held CLOSURE's lifetime; this one a cell-held
# STRING's). A helper builds a string by reassigning a `@`-capture cell in a
# loop (`(assign out (string out …))`) and RETURNS `out`; the caller reads the
# returned value one form later (here `(string "dial-" slug)`). Green pins that
# the loop-reassigned capture cell's returned region stays live across the
# caller's read.
#
# THE SHAPE'S INGREDIENTS (the lifetime needs each one):
#   * the accumulation is a LOOP over the capture cell (the loop-carried
#     reassignment is what exercises the return lifetime);
#   * `safe-uri` is a FUNCTION whose result a CALLER consumes (the activation
#     boundary matters — the identical body inlined at top level takes a
#     different route);
#   * the returned string is READ after return (built into another string / put
#     in a struct).
#
# ORIGIN — this is mu's `_safe-uri` (lib/cont/repo.lisp) and its twin `_slug`
# (lib/cont/config.lisp): a URI→ref-safe slug accumulated in `@out @""`, then
# consumed into a branch name (the dial-owner-git, repo, and adopt-config
# suites). Distinct from the struct write-path (`struct_put_with_rebind`,
# pinned by region-struct-mut-put-heap-key-uaf.lisp); this is the capture-cell string
# return path.

(defn safe-uri [uri]
  # Mirrors mu lib/cont/repo.lisp `_safe-uri`: slug the URI into a `@`-cell.
  (let [@out @""]
    (each i in (range (length uri))
      (let [c (slice uri i (+ i 1))]
        (assign out (string out c))))
    out))

(defn build-branch [uri]
  # Mirrors dial-owner-setup: the accumulated slug is READ one form after
  # `safe-uri` returns — the deref that faults on the freed page.
  (let [slug (safe-uri uri)]
    (string "dial-" slug)))

# Discard the result on purpose — comparing/printing it changes the allocation
# sequence and can hide an over-free. This is a UAF guard, not a value test; the
# reps keep it sensitive to a lifetime that only breaks under region-id churn.
(defn drive [reps]
  (def @c 0)
  (while (< c reps)
    (build-branch "a-b-c")
    (assign c (+ c 1))))

(drive 200)
(println "region-capture-cell-string-accum-uaf: ok")
