(elle/epoch 11)
## jit-string-push — verify IntrStringPush compiles via JIT
##
## Exercises both branches of %string-push under JIT:
##   1. Mutable @string: appends bytes in place, returns the same collection.
##   2. Immutable string: allocates a new concatenated string.
##
## Counterfactual check: before the JIT implementation exists,
## (push-mut-hot @"hi" "!") inside the hot loop will compile to a JIT panic
## ("not yet implemented: IntrStringPush in JIT") on the 16th call.

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

## ===== Mutable @string path =====
## Use %string-push on an @string; it must mutate in place and return the
## same collection. The function below is called >15 times so the JIT
## compiles it; if IntrStringPush is unimplemented the JIT panics.
(defn push-mut-hot (buf s)
  (%string-push buf s))

(def @hot-buf @"")
(repeat2 30 push-mut-hot hot-buf "ab")

(assert (= (length hot-buf) 60)
        "mutable @string-push: in-place mutation accumulates length")
(assert (= (freeze hot-buf)
           "abababababababababababababababababababababababababababababab")
        "mutable @string-push: bytes are appended correctly")

## Identity-preservation: push must return the same @string we passed in.
(defn push-returns-same? (buf s)
  (%identical? (%string-push buf s) buf))

(def @id-buf @"x")
(repeat2 20 push-returns-same? id-buf "y")
(assert (push-returns-same? id-buf "z")
        "mutable @string-push: returns the same collection (identity)")

## ===== Immutable string path =====
## %string-push on an immutable string allocates a new string with the
## concatenation; the original is untouched.
(defn push-imm-hot (s suffix)
  (%string-push s suffix))

(def orig "hello")
(repeat2 30 push-imm-hot orig "!")

(def result (push-imm-hot orig "!"))
(assert (= result "hello!")
        "immutable string-push: returns concatenated new string")
(assert (= orig "hello") "immutable string-push: original string is unchanged")

## ===== Multibyte / UTF-8 =====
(def @mb @"café")
(repeat2 5 push-mut-hot mb "é")
(assert (= (freeze mb) "caféééééé")
        "mutable @string-push: multibyte content appends correctly")

## ===== JIT actually compiled the hot functions =====
## `jit/rejections` drains pending JIT compilations as a side effect; calling
## it first guarantees the worker has finished (success, rejection, or panic)
## before we ask `(jit? f)`. If IntrStringPush is unimplemented, the worker
## panics — no rejection is recorded, but the cache stays empty, so
## `(jit? f)` returns false. This is the real counterfactual gate.
## These compilation checks are only meaningful when a JIT policy is
## active. Under --jit=off and --checked-intrinsics (which forces JIT
## off) nothing compiles, so (jit? f) is always false; the behavioral
## %string-push assertions above already cover those modes.
(when (not (= (vm/config :jit) :off))
  (jit/rejections)
  (assert (not (has-rejection? "IntrStringPush"))
          "IntrStringPush is JIT-supported (not in rejections list)")
  (assert (jit? push-mut-hot)
          "push-mut-hot was JIT-compiled (mutable @string-push path)")
  (assert (jit? push-returns-same?)
          "push-returns-same? was JIT-compiled (identity check path)")
  (assert (jit? push-imm-hot)
          "push-imm-hot was JIT-compiled (immutable string-push path)"))
