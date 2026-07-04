(elle/epoch 12)
# tests/elle/region-io-effect-pass.lisp — runtime guard for the io / fiber
# region-effect pass (docs/impl/region-effects.md "Native region effects").
#
# These primitives YIELD (SIG_YIELD | SIG_IO), so the declaration oracle in
# `dispatch_native_call` is EXEMPT on their result — an over-claim is silent,
# not a panic. The solver-side guard (no Mixed hard-edge; Fresh ⇒
# fresh_result_regions) lives in src/hir/regions/tests/effects.rs
# `io_yield_pass_tightenings_drop_the_mixed_hard_edge`. THIS file is the
# RUNTIME half: it asserts the resumed value's region kind — the invariant each
# declaration depends on — so a future change to a completion path that made an
# `Immediate` op return a heap value (or a `Fresh` op return an immediate) goes
# RED here, flagging the declaration as newly unsound.
#
# `arena/region-of` is 0 for an immediate (int / nil / keyword) and a real
# (>= 2) heap region otherwise. So:
#   Immediate ⇒ region-of result = 0
#   Fresh     ⇒ region-of (heap) result ≠ 0
#   Opaque    ⇒ heap result (≠ 0); region identity unconstrained, type pinned
# The region COUNT cannot be used (the ambient io-yield leak swamps it); region KIND can.

(def path "/dev/shm/elle-region-io-effect-pass")

(defn heap? [x]
  (not (= (arena/region-of x) 0)))
(defn immediate? [x]
  (= (arena/region-of x) 0))

# ── 1. Write port: open (Fresh) → flush/tell/seek/close (Immediate) ──────────
(let [p (port/open path :write)]
  (assert (port? p) "1a: port/open returns a port")
  (assert (heap? p)
          "1b: port/open is Fresh — the port is a heap value (region ≠ 0)")
  (let [n (port/write p "hello world")]
    (assert (immediate? n) "1c: port/write yields an immediate byte count"))
  (let [f (port/flush p)]
    (assert (nil? f) "1d: port/flush yields nil")
    (assert (immediate? f) "1e: port/flush is Immediate (region 0)"))
  (let [t (port/tell p)]
    (assert (int? t) "1f: port/tell yields an int position")
    (assert (immediate? t) "1g: port/tell is Immediate (region 0)"))
  (let [s (port/seek p 0)]
    (assert (int? s) "1h: port/seek yields an int position")
    (assert (immediate? s) "1i: port/seek is Immediate (region 0)"))
  (let [c (port/close p)]
    (assert (nil? c) "1j: port/close yields nil")
    (assert (immediate? c) "1k: port/close is Immediate (region 0)")))

# ── 2. Read port: read (Fresh) and read-all (Opaque) are heap results ────────
(let [p (port/open path :read)]
  (defer
    (port/close p)
    (let [d (port/read p 5)]
      (assert (string? d) "2a: port/read returns a string on a text port")
      (assert (= d "hello") "2b: port/read content")
      (assert (heap? d)
              "2c: port/read is Fresh — the buffer is a heap value (region ≠ 0)"))
    (let [rest (port/read-all p)]
      (assert (string? rest) "2d: port/read-all returns a string")
      (assert (= rest " world") "2e: port/read-all content")
      (assert (heap? rest)
              "2f: port/read-all is Opaque — a heap result (minted on the origin heap)"))))

# ── 3. ev/sleep (Immediate, nil) ─────────────────────────────────────────────
(let [r (ev/sleep 0)]
  (assert (nil? r) "3a: ev/sleep yields nil")
  (assert (immediate? r) "3b: ev/sleep is Immediate (region 0)"))

# ── 4. subprocess/wait (Immediate, int exit code) ────────────────────────────
(let [proc (subprocess/exec "true" @[])]
  (let [code (subprocess/wait proc)]
    (assert (int? code) "4a: subprocess/wait yields an int exit code")
    (assert (= code 0) "4b: `true` exits 0")
    (assert (immediate? code) "4c: subprocess/wait is Immediate (region 0)")))

# ── 5. sys/resolve (Opaque, array of strings on the origin heap) ─────────────
(let [ips (sys/resolve "localhost")]
  (assert (array? ips) "5a: sys/resolve returns an array")
  (assert (> (length ips) 0) "5b: localhost resolves to at least one IP")
  (assert (heap? ips) "5c: sys/resolve is Opaque — a heap array (origin heap)"))

(println "region-io-effect-pass: ok")
