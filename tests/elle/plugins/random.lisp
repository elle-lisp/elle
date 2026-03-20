(elle/epoch 1)

## Random plugin integration tests

(def [ok? plugin] (protect (import-file "target/release/libelle_random.so")))
(when (not ok?)
  (display "SKIP: random plugin not built\n")
  (exit 0))

(def seed-fn        (get plugin :seed))
(def int-fn         (get plugin :int))
(def float-fn       (get plugin :float))
(def bool-fn        (get plugin :bool))
(def bytes-fn       (get plugin :bytes))
(def shuffle-fn     (get plugin :shuffle))
(def choice-fn      (get plugin :choice))
(def normal-fn      (get plugin :normal))
(def exponential-fn (get plugin :exponential))
(def weighted-fn    (get plugin :weighted))
(def csprng-bytes-fn (get plugin :csprng-bytes))
(def csprng-seed-fn (get plugin :csprng-seed))
(def sample-fn      (get plugin :sample))

# ── random/seed + random/int determinism ───────────────────────────

## Seeding produces deterministic sequences
(seed-fn 42)
(def seq1 [(int-fn 1000) (int-fn 1000) (int-fn 1000) (int-fn 1000) (int-fn 1000)])
(seed-fn 42)
(def seq2 [(int-fn 1000) (int-fn 1000) (int-fn 1000) (int-fn 1000) (int-fn 1000)])
(assert (= seq1 seq2) "random/seed: same seed produces same int sequence")

# ── random/float ───────────────────────────────────────────────────

## float returns value in [0, 1)
(let ((f (float-fn)))
  (assert (and (>= f 0.0) (< f 1.0)) "random/float in [0, 1)"))

## float returns a float (smoke test over multiple samples)
(assert (float? (float-fn)) "random/float returns float")

# ── random/bool ────────────────────────────────────────────────────

## bool returns a boolean
(assert (bool? (bool-fn)) "random/bool returns boolean")

# ── random/bytes ───────────────────────────────────────────────────

## bytes returns a bytes value of the correct length
(let ((b (bytes-fn 16)))
  (assert (bytes? b) "random/bytes returns bytes")
  (assert (= (length b) 16) "random/bytes returns correct length"))

## bytes with length 0 returns empty bytes
(assert (= (length (bytes-fn 0)) 0) "random/bytes 0 returns empty bytes")

# ── random/normal ──────────────────────────────────────────────────

## normal with no args returns a float
(assert (float? (normal-fn)) "random/normal returns float")

## normal with mean and stddev returns a float
(assert (float? (normal-fn 5.0 2.0)) "random/normal with args returns float")

## normal rejects stddev <= 0
(let (([ok? err] (protect ((fn () (normal-fn 0.0 0.0)))))) (assert (not ok?) "random/normal: stddev=0 is range-error") (assert (= (get err :error) :range-error) "random/normal: stddev=0 is range-error"))

(let (([ok? err] (protect ((fn () (normal-fn 0.0 -1.0)))))) (assert (not ok?) "random/normal: negative stddev is range-error") (assert (= (get err :error) :range-error) "random/normal: negative stddev is range-error"))

# ── random/exponential ─────────────────────────────────────────────

## exponential returns a positive float
(let ((v (exponential-fn)))
  (assert (float? v) "random/exponential returns float")
  (assert (> v 0.0) "random/exponential returns positive value"))

## exponential with lambda returns positive float
(assert (> (exponential-fn 2.0) 0.0) "random/exponential with lambda=2.0 is positive")

## exponential rejects lambda <= 0
(let (([ok? err] (protect ((fn () (exponential-fn 0.0)))))) (assert (not ok?) "random/exponential: lambda=0 is range-error") (assert (= (get err :error) :range-error) "random/exponential: lambda=0 is range-error"))

# ── random/weighted ────────────────────────────────────────────────

## weighted returns one of the provided items
(let ((items ["a" "b" "c"])
      (weights [1.0 2.0 3.0]))
  (let ((chosen (weighted-fn items weights)))
    (assert (or (= chosen "a") (= chosen "b") (= chosen "c")) "random/weighted returns element from items")))

## weighted rejects mismatched lengths
(let (([ok? err] (protect ((fn () (weighted-fn ["a" "b"] [1.0])))))) (assert (not ok?) "random/weighted: mismatched lengths is range-error") (assert (= (get err :error) :range-error) "random/weighted: mismatched lengths is range-error"))

## weighted rejects non-positive weights
(let (([ok? err] (protect ((fn () (weighted-fn ["a"] [0.0])))))) (assert (not ok?) "random/weighted: zero weight is range-error") (assert (= (get err :error) :range-error) "random/weighted: zero weight is range-error"))

# ── random/csprng-bytes ────────────────────────────────────────────

## csprng-bytes returns bytes of the requested length
(let ((b (csprng-bytes-fn 16)))
  (assert (bytes? b) "random/csprng-bytes returns bytes")
  (assert (= (length b) 16) "random/csprng-bytes returns correct length"))

## csprng-bytes rejects negative length
(let (([ok? err] (protect ((fn () (csprng-bytes-fn -1)))))) (assert (not ok?) "random/csprng-bytes: negative length is range-error") (assert (= (get err :error) :range-error) "random/csprng-bytes: negative length is range-error"))

# ── random/csprng-seed determinism ─────────────────────────────────

## CSPRNG is deterministic after seeding with same 32-byte seed
(def seed32 (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31))
(csprng-seed-fn seed32)
(def csprng-seq1 [(csprng-bytes-fn 8) (csprng-bytes-fn 8)])
(csprng-seed-fn seed32)
(def csprng-seq2 [(csprng-bytes-fn 8) (csprng-bytes-fn 8)])
(assert (= csprng-seq1 csprng-seq2) "random/csprng-seed: same seed produces same byte sequence")

## csprng-seed rejects non-bytes
(let (([ok? err] (protect ((fn () (csprng-seed-fn "not-bytes")))))) (assert (not ok?) "random/csprng-seed: non-bytes is type-error") (assert (= (get err :error) :type-error) "random/csprng-seed: non-bytes is type-error"))

## csprng-seed rejects wrong length
(let (([ok? err] (protect ((fn () (csprng-seed-fn (bytes 0 1 2 3))))))) (assert (not ok?) "random/csprng-seed: wrong length is range-error") (assert (= (get err :error) :range-error) "random/csprng-seed: wrong length is range-error"))

# ── random/sample ──────────────────────────────────────────────────

## sample returns exactly n elements
(let ((s (sample-fn [1 2 3 4 5] 3)))
  (assert (= (length s) 3) "random/sample returns n elements"))

## sample with n=0 returns empty array
(assert (= (length (sample-fn [1 2 3] 0)) 0) "random/sample n=0 returns empty")

## sample with n=length returns all elements (as array)
(assert (= (length (sample-fn [1 2 3] 3)) 3) "random/sample n=len returns all")

## sample rejects n > length
(let (([ok? err] (protect ((fn () (sample-fn [1 2 3] 4)))))) (assert (not ok?) "random/sample: n > length is range-error") (assert (= (get err :error) :range-error) "random/sample: n > length is range-error"))

## sample rejects negative n
(let (([ok? err] (protect ((fn () (sample-fn [1 2] -1)))))) (assert (not ok?) "random/sample: negative n is range-error") (assert (= (get err :error) :range-error) "random/sample: negative n is range-error"))
