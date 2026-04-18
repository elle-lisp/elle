(elle/epoch 8)
## Tests for (parse-int str radix) — radix-aware string-to-integer conversion

# ── Basic radix parsing ──────────────────────────────────────────────

(assert (= (parse-int "ff" 16) 255) "hex ff")
(assert (= (parse-int "FF" 16) 255) "hex FF (uppercase)")
(assert (= (parse-int "0" 16) 0) "hex zero")
(assert (= (parse-int "1a" 16) 26) "hex 1a")
(assert (= (parse-int "deadbeef" 16) 3735928559) "hex deadbeef")

(assert (= (parse-int "1010" 2) 10) "binary 1010")
(assert (= (parse-int "11111111" 2) 255) "binary 11111111")
(assert (= (parse-int "0" 2) 0) "binary zero")

(assert (= (parse-int "755" 8) 493) "octal 755")
(assert (= (parse-int "77" 8) 63) "octal 77")

(assert (= (parse-int "42" 10) 42) "explicit base 10")
(assert (= (parse-int "z" 36) 35) "base 36 z")
(assert (= (parse-int "10" 36) 36) "base 36 10")

# ── Original behavior unchanged ──────────────────────────────────────

(assert (= (parse-int "42") 42) "no-radix decimal")
(assert (= (parse-int "-7") -7) "no-radix negative")
(assert (= (integer 3.14) 3) "float truncation")
(assert (= (integer 42) 42) "int passthrough")
(let [[ok? _] (protect (parse-int :keyword))]
  (assert (not ok?) "keyword to parse-int signals error"))

# ── Error cases ──────────────────────────────────────────────────────

(def [ok? _] (protect (parse-int "gg" 16)))
(assert (not ok?) "invalid hex digit signals error")

(def [ok? _] (protect (parse-int "42" 1)))
(assert (not ok?) "radix < 2 signals error")

(def [ok? _] (protect (parse-int "42" 37)))
(assert (not ok?) "radix > 36 signals error")

(println "all integer-radix tests passed")
