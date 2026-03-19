# elle-random

A pseudo-random number generator plugin for Elle, wrapping the Rust `rand` crate (0.9).

## Migration Notice

**Version 1.0.0 (Chunk 3):** The underlying PRNG changed from `fastrand` (WyRand) to `rand` 0.9 (ChaCha12). Sequences generated from the same seed will differ from previous versions. **The API is unchanged.** All existing code continues to work; only the output values differ.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_random.so` (or `target/release/libelle_random.so`).

## Usage

```lisp
(import-file "path/to/libelle_random.so")

;; Deterministic sequences
(random/seed 42)              ;; seed the main PRNG
(random/int)                  ;; random integer (full range)
(random/int 100)              ;; 0..100
(random/int 10 20)            ;; 10..20
(random/float)                ;; 0.0..1.0
(random/bool)                 ;; true or false
(random/bytes 16)             ;; 16 random bytes
(random/shuffle @[1 2 3 4 5]) ;; shuffled array
(random/choice @[1 2 3 4 5])  ;; random element

;; Distributions
(random/normal)               ;; Gaussian, mean=0.0, stddev=1.0
(random/normal 100.0 15.0)    ;; mean=100.0, stddev=15.0
(random/exponential)          ;; exponential, lambda=1.0
(random/exponential 0.5)      ;; lambda=0.5

;; Weighted selection
(random/weighted [1 2 3] [0.1 0.3 0.6])  ;; weighted random choice

;; Cryptographically secure random
(random/csprng-bytes 32)      ;; 32 crypto-secure random bytes
(random/csprng-seed (bytes 0 1 2 ...))  ;; seed CSPRNG with 32-byte key

;; Sampling without replacement
(random/sample [1 2 3 4 5] 3) ;; 3 distinct elements
```

## Primitives

### Existing Primitives (7)

| Name | Args | Returns | Signal |
|------|------|---------|--------|
| `random/seed` | seed (integer) | nil | errors |
| `random/int` | [max] or [min, max] | integer | errors |
| `random/float` | — | float (0..1) | silent |
| `random/bool` | — | boolean | silent |
| `random/bytes` | length | bytes | errors |
| `random/shuffle` | array or @array | new shuffled array | errors |
| `random/choice` | array or @array | random element | errors |

### New Primitives (6)

| Name | Args | Returns | Signal |
|------|------|---------|--------|
| `random/normal` | [mean] [stddev] | float | errors |
| `random/exponential` | [lambda] | float | errors |
| `random/weighted` | items, weights | value | errors |
| `random/csprng-bytes` | length | bytes | errors |
| `random/csprng-seed` | seed | nil | errors |
| `random/sample` | collection, n | array | errors |

---

## Primitive Documentation

### `random/seed`

**Signature:** `(random/seed seed-int)`

**Returns:** `nil`

**Signal:** errors

**Purpose:** Seed the main PRNG with a 64-bit integer. Subsequent calls to `random/int`, `random/float`, `random/bool`, `random/bytes`, `random/shuffle`, and `random/choice` will produce a deterministic sequence.

**Examples:**
```lisp
(random/seed 42)
(random/int 100)  ;; deterministic
(random/int 100)  ;; same sequence as before
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not an integer | `type-error` | `"random/seed: expected integer, got {type}"` |

---

### `random/int`

**Signature:** `(random/int)` or `(random/int max)` or `(random/int min max)`

**Returns:** integer

**Signal:** errors

**Purpose:** Generate a random integer. With no arguments, returns a full-range integer. With one argument, returns `[0, max)`. With two arguments, returns `[min, max)`.

**Examples:**
```lisp
(random/int)           ;; any i64
(random/int 100)       ;; 0..99
(random/int 10 20)     ;; 10..19
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not an integer | `type-error` | `"random/int: expected integer, got {type}"` |
| `max <= 0` | `range-error` | `"random/int: max must be positive"` |
| `min >= max` | `range-error` | `"random/int: min must be less than max"` |

---

### `random/float`

**Signature:** `(random/float)`

**Returns:** float in `[0.0, 1.0)`

**Signal:** silent

**Purpose:** Generate a random float uniformly distributed in `[0.0, 1.0)`.

**Examples:**
```lisp
(random/float)  ;; e.g., 0.42857...
```

---

### `random/bool`

**Signature:** `(random/bool)`

**Returns:** boolean (`true` or `false`)

**Signal:** silent

**Purpose:** Generate a random boolean with 50% probability of each value.

**Examples:**
```lisp
(random/bool)  ;; true or false
```

---

### `random/bytes`

**Signature:** `(random/bytes length)`

**Returns:** bytes of the specified length

**Signal:** errors

**Purpose:** Generate `length` random bytes.

**Examples:**
```lisp
(random/bytes 16)  ;; 16 random bytes
(random/bytes 32)  ;; 32 random bytes
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not an integer | `type-error` | `"random/bytes: expected integer, got {type}"` |
| `length < 0` | `range-error` | `"random/bytes: length must be non-negative"` |

---

### `random/shuffle`

**Signature:** `(random/shuffle array)`

**Returns:** new shuffled array (same mutability as input)

**Signal:** errors

**Purpose:** Return a shuffled copy of the input array. If the input is immutable, the result is immutable. If the input is mutable (`@array`), the result is mutable.

**Examples:**
```lisp
(random/shuffle [1 2 3 4 5])   ;; e.g., [3 1 5 2 4]
(random/shuffle @[1 2 3 4 5])  ;; e.g., @[3 1 5 2 4]
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not an array | `type-error` | `"random/shuffle: expected array, got {type}"` |

---

### `random/choice`

**Signature:** `(random/choice array)`

**Returns:** random element from the array

**Signal:** errors

**Purpose:** Return a uniformly random element from the input array.

**Examples:**
```lisp
(random/choice [1 2 3 4 5])  ;; e.g., 3
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not an array | `type-error` | `"random/choice: expected array, got {type}"` |
| Array is empty | `range-error` | `"random/choice: array is empty"` |

---

### `random/normal`

**Signature:** `(random/normal)` or `(random/normal mean)` or `(random/normal mean stddev)`

**Returns:** float (Gaussian-distributed)

**Signal:** errors

**Purpose:** Generate a random float from a normal (Gaussian) distribution. Defaults: `mean=0.0`, `stddev=1.0`. Implementation: Box-Muller transform.

**Examples:**
```lisp
(random/normal)           ;; standard normal (mean=0, stddev=1)
(random/normal 100.0)     ;; mean=100, stddev=1
(random/normal 100.0 15.0) ;; mean=100, stddev=15
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not numeric | `type-error` | `"random/normal: expected number, got {type}"` |
| `stddev <= 0` | `range-error` | `"random/normal: stddev must be positive"` |

---

### `random/exponential`

**Signature:** `(random/exponential)` or `(random/exponential lambda)`

**Returns:** float (exponentially-distributed, always positive)

**Signal:** errors

**Purpose:** Generate a random float from an exponential distribution with rate parameter `lambda`. Default: `lambda=1.0`. Implementation: inverse CDF (`-ln(1 - U) / lambda`).

**Examples:**
```lisp
(random/exponential)    ;; lambda=1.0
(random/exponential 0.5) ;; lambda=0.5 (slower decay)
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not numeric | `type-error` | `"random/exponential: expected number, got {type}"` |
| `lambda <= 0` | `range-error` | `"random/exponential: lambda must be positive"` |

---

### `random/weighted`

**Signature:** `(random/weighted items weights)`

**Returns:** random element from `items` (weighted by `weights`)

**Signal:** errors

**Purpose:** Select a random element from `items` according to the probability distribution defined by `weights`. Both arguments must be arrays of the same length. Weights are positive numbers (integers or floats); they are normalized internally.

**Examples:**
```lisp
(random/weighted [1 2 3] [0.1 0.3 0.6])  ;; 3 is most likely
(random/weighted ["a" "b" "c"] [1 1 1])  ;; uniform
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `items` is not an array | `type-error` | `"random/weighted: items must be an array"` |
| `weights` is not an array | `type-error` | `"random/weighted: weights must be an array"` |
| Length mismatch | `range-error` | `"random/weighted: items and weights must have the same length"` |
| `weights` is empty | `range-error` | `"random/weighted: weights array is empty"` |
| Any weight is negative | `range-error` | `"random/weighted: weights must be non-negative"` |
| All weights are zero | `range-error` | `"random/weighted: at least one weight must be positive"` |
| Weight is not numeric | `type-error` | `"random/weighted: weight must be numeric, got {type}"` |

---

### `random/csprng-bytes`

**Signature:** `(random/csprng-bytes length)`

**Returns:** bytes of the specified length (cryptographically secure)

**Signal:** errors

**Purpose:** Generate `length` cryptographically secure random bytes using a separate ChaCha20 CSPRNG (independent from the main PRNG). Suitable for cryptographic keys, nonces, and other security-sensitive applications.

**Examples:**
```lisp
(random/csprng-bytes 32)  ;; 32 crypto-secure random bytes
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not an integer | `type-error` | `"random/csprng-bytes: expected integer, got {type}"` |
| `length < 0` | `range-error` | `"random/csprng-bytes: length must be non-negative"` |

---

### `random/csprng-seed`

**Signature:** `(random/csprng-seed seed)`

**Returns:** `nil`

**Signal:** errors

**Purpose:** Seed the CSPRNG with a 32-byte key for reproducible cryptographically-secure sequences. The `seed` argument must be exactly 32 bytes (either `bytes` or `@bytes`).

**Examples:**
```lisp
(random/csprng-seed (bytes 0 1 2 ... 31))  ;; 32 bytes
(random/csprng-bytes 16)  ;; deterministic from here
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument is not bytes | `type-error` | `"random/csprng-seed: expected bytes, got {type}"` |
| Seed is not exactly 32 bytes | `range-error` | `"random/csprng-seed: seed must be exactly 32 bytes, got {length}"` |

---

### `random/sample`

**Signature:** `(random/sample collection n)`

**Returns:** immutable array of `n` distinct elements

**Signal:** errors

**Purpose:** Draw `n` elements without replacement from a collection (array, list, or set). Returns an immutable array. Implementation: partial Fisher-Yates shuffle.

**Examples:**
```lisp
(random/sample [1 2 3 4 5] 3)  ;; e.g., [2 4 1]
(random/sample (list 10 20 30 40) 2)  ;; e.g., [30 10]
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `collection` is not a sequence | `type-error` | `"random/sample: collection must be an array or list, got {type}"` |
| `n` is not an integer | `type-error` | `"random/sample: n must be an integer, got {type}"` |
| `n < 0` | `range-error` | `"random/sample: n must be non-negative"` |
| `n > collection length` | `range-error` | `"random/sample: n ({n}) exceeds collection length ({length})"` |

---

## State Architecture

The plugin maintains two independent thread-local RNGs:

### Main PRNG (`RNG`)

- **Type:** `rand::rngs::StdRng` (ChaCha12-based)
- **Seeding:** `random/seed(int)` → `StdRng::seed_from_u64(u64)`
- **Initialization:** Seeded from OS entropy on first use
- **Used by:** `random/int`, `random/float`, `random/bool`, `random/bytes`, `random/shuffle`, `random/choice`, `random/normal`, `random/exponential`, `random/weighted`, `random/sample`

### CSPRNG (`CSPRNG`)

- **Type:** `rand_chacha::ChaCha20Rng` (ChaCha20, cryptographically secure)
- **Seeding:** `random/csprng-seed(bytes)` → `ChaCha20Rng::from_seed([u8; 32])`
- **Initialization:** Seeded from OS entropy on first use
- **Used by:** `random/csprng-bytes`

**Invariants:**

1. The two RNGs are completely independent. Seeding one does not affect the other.
2. Each RNG is thread-local; different threads have separate RNG states.
3. `random/seed` and `random/csprng-seed` are the only ways to control determinism.
4. All other primitives use the appropriate RNG based on their purpose (main PRNG for general use, CSPRNG for cryptographic use).

---

## Error Handling

All primitives that can fail use `Signal::errors()` and return an error struct on failure:

```lisp
(protect (random/int -5))
;; => [false {:kind :range-error :message "random/int: max must be positive"}]
```

Primitives that are infallible (`random/float`, `random/bool`) use `Signal::silent()` and never error.

---

## Determinism and Reproducibility

To reproduce a sequence:

```lisp
(random/seed 42)
(def seq1 [(random/int 100) (random/int 100) (random/int 100)])

(random/seed 42)
(def seq2 [(random/int 100) (random/int 100) (random/int 100)])

(assert-eq seq1 seq2)  ;; true
```

For cryptographic sequences:

```lisp
(random/csprng-seed (bytes 0 1 2 ... 31))
(def crypto1 (random/csprng-bytes 16))

(random/csprng-seed (bytes 0 1 2 ... 31))
(def crypto2 (random/csprng-bytes 16))

(assert-eq crypto1 crypto2)  ;; true
```
