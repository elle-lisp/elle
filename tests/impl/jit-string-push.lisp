(elle/epoch 12)
## audited: 2026-09-29
## IntrStringPush compiles under the JIT, on the mutable and the immutable path.
## docs/impl/jit.md
##
## The two branches of %string-push: a mutable @string appends bytes in place
## and returns the same collection; an immutable string allocates a new,
## concatenated one.
##
## The file runs under the build's own policy, under the eager profile, and
## with every function compiled on the VM thread through
## tests/impl/jit-string-push-syncjit.lisp.
##
## The counter-factual: with IntrStringPush unimplemented, the hot function
## never compiles and no rejection names it. The value assertions pass either
## way; only `(jit? f)` sees the difference.

## Helper: call f n times with two args.
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
## %string-push on an @string must mutate it in place and return the same
## collection.
##
## `%string-push` is a call-position intrinsic, so its container operand must be
## a statically-proven string (the contract table's `StringPush` row; STRING or
## MUTABLE_STRING — docs/intrinsics.md). `buf` arrives as an
## unguarded parameter — `push-mut-hot` is applied indirectly through `repeat2`,
## so no call site proves its type. A `%string?` guard cannot narrow it: it proves
## the UNION {STRING, MUTABLE_STRING}, which the flat type lattice cannot name, so
## it pins no single point (`typeinfer::guard`). The authoritative narrowing is a
## `match (type-of …)` keyword arm — inside `:@string`, `buf` IS a mutable string;
## inside `:string`, an immutable one — each a single lattice point that discharges
## the contract. The stdlib `push` wrapper narrows its container the same way.
(defn push-mut-hot (buf s)
  (match (type-of buf)
    :string (%string-push buf s)
    :@string (%string-push buf s)
    _ (error {:error :type-error :message "push-mut-hot: string required"})))

(def @hot-buf @"")
(repeat2 30 push-mut-hot hot-buf "ab")

(assert (= (length hot-buf) 60)
        "mutable @string-push: in-place mutation accumulates length")
(assert (= (freeze hot-buf)
           "abababababababababababababababababababababababababababababab")
        "mutable @string-push: bytes are appended correctly")

## Identity-preservation: push must return the same @string we passed in.
## Same `match (type-of …)` narrowing as above — `buf` is an indirectly-applied
## parameter, narrowed to a single string point per arm before the intrinsic.
(defn push-returns-same? (buf s)
  (match (type-of buf)
    :string (%identical? (%string-push buf s) buf)
    :@string (%identical? (%string-push buf s) buf)
    _ (error {:error :type-error :message "push-returns-same?: string required"})))

(def @id-buf @"x")
(repeat2 20 push-returns-same? id-buf "y")
(assert (push-returns-same? id-buf "z")
        "mutable @string-push: returns the same collection (identity)")

## ===== Immutable string path =====
## %string-push on an immutable string allocates a new string with the
## concatenation; the original is untouched. Same `match (type-of …)` narrowing —
## the `:string` arm proves the immutable point, so the immutable-copy path of the
## intrinsic stays exercised under JIT.
(defn push-imm-hot (s suffix)
  (match (type-of s)
    :string (%string-push s suffix)
    :@string (%string-push s suffix)
    _ (error {:error :type-error :message "push-imm-hot: string required"})))

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
## before we ask `(jit? f)`.
## The compilation checks need a JIT. In a build without one `(vm/config :jit)`
## reads nil and nothing compiles, and the value assertions above are the test.
(when (vm/config :jit)
  (jit/rejections)
  (assert (not (has-rejection? "IntrStringPush"))
          "IntrStringPush is JIT-supported (not in rejections list)")
  (assert (jit? push-mut-hot)
          "push-mut-hot was JIT-compiled (mutable @string-push path)")
  (assert (jit? push-returns-same?)
          "push-returns-same? was JIT-compiled (identity check path)")
  (assert (jit? push-imm-hot)
          "push-imm-hot was JIT-compiled (immutable string-push path)"))
