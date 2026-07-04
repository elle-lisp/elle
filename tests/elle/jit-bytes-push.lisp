(elle/epoch 12)
## jit-bytes-push — verify IntrBytesPush compiles via JIT
##
## Exercises both branches of %bytes-push under JIT:
##   1. Mutable @bytes: appends one byte in place, returns the same collection.
##   2. Immutable bytes: allocates a new bytes value with the byte appended.
##
## Counterfactual check: before the JIT implementation exists,
## (push-mut-hot @b 1) inside the hot loop will compile to a JIT panic
## ("not yet implemented: IntrBytesPush in JIT") on the 16th call. The
## worker dies silently, no rejection is recorded, but the cache stays
## empty so `(jit? f)` returns false — that is the real counterfactual gate.

## Helper: call f n times with two args, return last result.
(defn repeat2 (n f x y)
  (if (<= n 0)
    true
    (begin
      (f x y)
      (repeat2 (- n 1) f x y))))

## Helper: scan rejection list for instruction name.
(defn has-rejection? (name)
  (defn scan (rs)
    (if (= rs ())
      false
      (if (string/contains? (get (first rs) :reason) name) true (scan (rest rs)))))
  (scan (jit/rejections)))

## ===== Mutable @bytes path =====
## Use %bytes-push on an @bytes; it must mutate in place and return the
## same collection. The function below is called >15 times so the JIT
## compiles it; if IntrBytesPush is unimplemented the JIT panics.
(defn push-mut-hot (buf b)
  (%bytes-push buf b))

(def @hot-buf (@bytes))
(repeat2 30 push-mut-hot hot-buf 65)  # 'A'

(assert (= (length hot-buf) 30)
        "mutable @bytes-push: in-place mutation accumulates length")
(assert (= (get hot-buf 0) 65) "mutable @bytes-push: first byte is 'A'")
(assert (= (get hot-buf 29) 65) "mutable @bytes-push: last byte is 'A'")

## Identity-preservation: push must return the same @bytes we passed in.
(defn push-returns-same? (buf b)
  (%identical? (%bytes-push buf b) buf))

(def @id-buf (@bytes 1))
(repeat2 20 push-returns-same? id-buf 2)
(assert (push-returns-same? id-buf 3)
        "mutable @bytes-push: returns the same collection (identity)")

## Truncation to u8 (matches VM behaviour: `value.as_int() as u8`).
(def @trunc (@bytes))
(repeat2 20 push-mut-hot trunc 257)  # 257 & 0xff == 1
(assert (= (get trunc 0) 1) "mutable @bytes-push: integer truncates to u8")

## ===== Immutable bytes path =====
## %bytes-push on an immutable bytes value allocates a new bytes with the
## byte appended; the original is untouched.
(defn push-imm-hot (b byte)
  (%bytes-push b byte))

(def orig (bytes 1 2 3))
(repeat2 30 push-imm-hot orig 4)

(def result (push-imm-hot orig 4))
(assert (= (length result) 4)
        "immutable bytes-push: result has one more byte than original")
(assert (= (get result 3) 4)
        "immutable bytes-push: appended byte is at the new tail")
(assert (= (length orig) 3) "immutable bytes-push: original bytes is unchanged")

## ===== JIT actually compiled the hot functions =====
## `jit/rejections` drains pending JIT compilations as a side effect; calling
## it first guarantees the worker has finished (success, rejection, or panic)
## before we ask `(jit? f)`.
## These compilation checks are only meaningful when a JIT policy is
## active. Under --jit=off and --checked-intrinsics (which forces JIT
## off) nothing compiles, so (jit? f) is always false; the behavioral
## %bytes-push assertions above already cover those modes.
(when (not (= (vm/config :jit) :off))
  (jit/rejections)
  (assert (not (has-rejection? "IntrBytesPush"))
          "IntrBytesPush is JIT-supported (not in rejections list)")
  (assert (jit? push-mut-hot)
          "push-mut-hot was JIT-compiled (mutable @bytes-push path)")
  (assert (jit? push-returns-same?)
          "push-returns-same? was JIT-compiled (identity check path)")
  (assert (jit? push-imm-hot)
          "push-imm-hot was JIT-compiled (immutable bytes-push path)"))
