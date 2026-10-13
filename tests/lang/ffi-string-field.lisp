(elle/epoch 15)
# audited: 2026-10-13
# ffi/write and a callback's return type refuse a type that holds :string, since nothing would own the C string.
# docs/ffi.md
#
# The counter-factual writes the field: ffi/write stores a pointer to a C
# string it frees before returning, and a callback returning a struct with a
# :string field hands C a pointer that is freed before C reads it. Each probe
# here asks for the refusal, and that the memory under it stays as it was.

(def slot (ffi/malloc 32))
(def thirty-two-bytes (ffi/array :u8 32))

(defn refused-write? [type value]
  "True when ffi/write refuses value as type with an ffi-error, and the
   thirty-two bytes under the slot are unchanged."
  (ffi/write slot thirty-two-bytes (bytes ;(map (fn [i] i) (range 32))))
  (let [before (ffi/read slot thirty-two-bytes)
        [ok? err] (protect (ffi/write slot type value))]
    (and (not ok?) (= (get err :error) :ffi-error)
         (= (ffi/read slot thirty-two-bytes) before))))

(defn refused-callback? [ret]
  (let [[ok? err] (protect (ffi/callback (ffi/signature ret []) (fn [] nil)))]
    (and (not ok?) (= (get err :error) :ffi-error))))

## ── ffi/write ─────────────────────────────────────────────────────

(assert (refused-write? (ffi/struct [:i32 :string]) [1 "one"])
        "a struct with a :string field")
(assert (refused-write? (ffi/struct [:i32 :string]) [1 nil])
        "a :string field holding nil: the type is refused, not the value")
(assert (refused-write? (ffi/struct [:i32 (ffi/struct [:u8 :string])])
                        [1 [2 "two"]]) "a :string field in a nested struct")
(assert (refused-write? (ffi/array :string 2) ["a" "b"]) "an array of :string")
(assert (refused-write? :string "one") "a bare :string")

## ── ffi/callback ──────────────────────────────────────────────────

(assert (refused-callback? :string) "a callback cannot return :string")
(assert (refused-callback? (ffi/struct [:i32 :string]))
        "a callback cannot return a struct with a :string field")

## ── What stays allowed ────────────────────────────────────────────

(let [by-pointer (ffi/struct [:i32 :ptr])]
  (ffi/write slot by-pointer [1 nil])
  (assert (= (ffi/read slot by-pointer) [1 nil])
          "a :ptr field takes the place of a :string field"))

(let [cb (ffi/callback (ffi/signature :void [:string]) (fn [s] nil))]
  (assert (ptr? cb) "a callback may take a :string argument")
  (ffi/callback-free cb))

(let [cb (ffi/callback (ffi/signature :ptr []) (fn [] nil))]
  (assert (ptr? cb) "a callback may return :ptr")
  (ffi/callback-free cb))

(ffi/free slot)
