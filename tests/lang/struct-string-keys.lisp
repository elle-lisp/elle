(elle/epoch 12)
# String and array keys in structs.
#
# A struct key holds no Rust-heap memory: a string or array key is a value in
# the struct's own region (docs/impl/values.md § "Struct keys"). This file
# pins what stays true of a key whose bytes moved into the region — lookup by
# content, survival past the region the key was built in, the ranking that
# decides key order, and the crossing into a worker thread — so a change to
# the key representation cannot quietly alter any of them.

# ── lookup is by content, not by identity ─────────────────────────
# The probe string and the stored key are two separate allocations. `concat`
# builds a fresh string, so an identity comparison would miss.

(let [s (struct "name" 1)]
  (assert (= (get s (concat "na" "me")) 1) "string key found by content")
  (assert (has? s "name") "has? finds a string key")
  (assert (= (get s "other") nil) "absent string key reads nil"))

(let [s (struct [1 2] :found)]
  (assert (= (get s [1 2]) :found) "array key found by content")
  (assert (= (get s [1 3]) nil) "unequal array key reads nil"))

# ── a stored key outlives the region it was built in ──────────────
# `keyed` builds its key string inside its own call, which is released when the
# call returns. A key that aliased that string instead of copying it would read
# freed pages here; the loop runs far past the JIT threshold so both tiers see
# the same reuse.

(defn keyed []
  (struct (concat "k" "7") 7))

(var last 0)
(var i 0)
(while (%lt i 500)
  (assign last (get (keyed) "k7"))
  (assign i (%add i 1)))
(assert (= last 7) "a struct key survives the region its string was built in")

# ── put and del carry string keys ─────────────────────────────────

(let* [s (struct "a" 1)
       s2 (put s "b" 2)
       s3 (del s2 "a")]
  (assert (= (get s2 "a") 1) "put preserves the original string key")
  (assert (= (get s2 "b") 2) "put adds a string key")
  (assert (not (has? s3 "a")) "del removes a string key")
  (assert (= (get s3 "b") 2) "del preserves the other string key"))

# ── keys rank by key order, not by value order ────────────────────
# Keys sort nil, bool, int, symbol, string, keyword, empty list, array, heap.
# Value ordering ranks keywords BEFORE strings, so a struct that mixed the two
# would iterate differently if key order ever delegated to it.

(let [s (struct 42 :int-key "b" :string-key :d :keyword-key)]
  (assert (= (keys s) (quote (42 "b" :d)))
          "keys iterate int, then string, then keyword"))

# ── mutable structs take string keys too ──────────────────────────

(let [m @{}]
  (put m "k" 1)
  (assert (= (get m "k") 1) "@struct stores a string key")
  (put m "k" 2)
  (assert (= (get m "k") 2) "@struct overwrites through a string key")
  (del m "k")
  (assert (not (has? m "k")) "@struct removes a string key"))

# ── a string-keyed struct crosses a thread boundary ───────────────
# The worker builds the struct on its own heap and the value is rebuilt on
# this one. A key that carried a raw pointer could not make the crossing.

(let [s (sys/join (sys/spawn (fn [] (struct "k" 1 "j" 2))))]
  (assert (= (get s "k") 1) "a string-keyed struct returns from a worker")
  (assert (= (get s "j") 2) "every string key survives the return"))

# ── equality and hashing see through the representation ───────────

(assert (= (struct "a" 1 "b" 2) (struct "b" 2 "a" 1))
        "string-keyed struct equality ignores insertion order")
(assert (has? (set (struct "a" 1)) (struct "a" 1))
        "a string-keyed struct hashes as a set element")

(println "struct-string-keys: all tests passed")
