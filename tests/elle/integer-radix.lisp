## Tests for (integer str radix) — radix-aware string-to-integer conversion

# ── Basic radix parsing ──────────────────────────────────────────────

(assert (= (integer "ff" 16) 255) "hex ff")
(assert (= (integer "FF" 16) 255) "hex FF (uppercase)")
(assert (= (integer "0" 16) 0) "hex zero")
(assert (= (integer "1a" 16) 26) "hex 1a")
(assert (= (integer "deadbeef" 16) 3735928559) "hex deadbeef")

(assert (= (integer "1010" 2) 10) "binary 1010")
(assert (= (integer "11111111" 2) 255) "binary 11111111")
(assert (= (integer "0" 2) 0) "binary zero")

(assert (= (integer "755" 8) 493) "octal 755")
(assert (= (integer "77" 8) 63) "octal 77")

(assert (= (integer "42" 10) 42) "explicit base 10")
(assert (= (integer "z" 36) 35) "base 36 z")
(assert (= (integer "10" 36) 36) "base 36 10")

# ── Original behavior unchanged ──────────────────────────────────────

(assert (= (integer "42") 42) "no-radix decimal")
(assert (= (integer "-7") -7) "no-radix negative")
(assert (= (integer 3.14) 3) "float truncation")
(assert (= (integer 42) 42) "int passthrough")
(let [([ok? _] (protect (integer :keyword)))]
  (assert (not ok?) "keyword to integer signals error"))

# ── Error cases ──────────────────────────────────────────────────────

(def [ok? _] (protect (integer "gg" 16)))
(assert (not ok?) "invalid hex digit signals error")

(def [ok? _] (protect (integer "42" 1)))
(assert (not ok?) "radix < 2 signals error")

(def [ok? _] (protect (integer "42" 37)))
(assert (not ok?) "radix > 36 signals error")

(println "all integer-radix tests passed")
