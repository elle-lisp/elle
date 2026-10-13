(elle/epoch 15)
# audited: 2026-10-13
# ffi/write checks a scalar against its type as ffi/call checks an argument, and a refused write leaves the memory alone.
# docs/ffi.md
#
# The counter-factual narrows silently: 256 written as :u8 stores 0, -1 written
# as :u32 stores 4294967295, and a string written as :i32 is a type-error
# rather than the ffi-error ffi/call raises for the same value.

(def cell (ffi/malloc 8))

(defn refused? [type value]
  "True when ffi/write refuses value as type with an ffi-error, and the eight
   bytes under the cell are unchanged."
  (ffi/write cell :u64 0x0102030405060708)
  (let [[ok? err] (protect (ffi/write cell type value))]
    (and (not ok?) (= (get err :error) :ffi-error)
         (= (ffi/read cell :u64) 0x0102030405060708))))

(defn round-trip [type value]
  (ffi/write cell type value)
  (ffi/read cell type))

## ── Integers out of range ─────────────────────────────────────────

(assert (refused? :u8 256) ":u8 refuses 256")
(assert (refused? :u8 -1) ":u8 refuses -1")
(assert (refused? :uchar 256) ":uchar refuses 256")
(assert (refused? :i8 128) ":i8 refuses 128")
(assert (refused? :i8 -129) ":i8 refuses -129")
(assert (refused? :char 128) ":char refuses 128")
(assert (refused? :u16 65536) ":u16 refuses 65536")
(assert (refused? :i16 32768) ":i16 refuses 32768")
(assert (refused? :short -32769) ":short refuses -32769")
(assert (refused? :ushort -1) ":ushort refuses -1")
(assert (refused? :u32 -1) ":u32 refuses -1")
(assert (refused? :u32 4294967296) ":u32 refuses 2^32")
(assert (refused? :i32 2147483648) ":i32 refuses 2^31")
(assert (refused? :int -2147483649) ":int refuses -2^31 - 1")
(assert (refused? :uint -1) ":uint refuses -1")

## ── Values of the wrong type ──────────────────────────────────────

(assert (refused? :i32 "seven") ":i32 refuses a string")
(assert (refused? :u64 1.5) ":u64 refuses a float")
(assert (refused? :double "seven") ":double refuses a string")
(assert (refused? :ptr 42) ":ptr refuses an integer")

## ── The bounds themselves ─────────────────────────────────────────

(assert (= (round-trip :u8 255) 255) ":u8 holds 255")
(assert (= (round-trip :i8 -128) -128) ":i8 holds -128")
(assert (= (round-trip :i8 127) 127) ":i8 holds 127")
(assert (= (round-trip :u16 65535) 65535) ":u16 holds 65535")
(assert (= (round-trip :i16 -32768) -32768) ":i16 holds -32768")
(assert (= (round-trip :u32 4294967295) 4294967295) ":u32 holds 2^32 - 1")
(assert (= (round-trip :i32 -2147483648) -2147483648) ":i32 holds -2^31")
(assert (= (round-trip :float 2) 2.0) ":float accepts an integer")
(assert (= (round-trip :ptr nil) nil) ":ptr holds NULL")

## ── A struct field takes the same check ───────────────────────────

(let [st (ffi/struct [:u8 :u8])
      [ok? err] (protect (ffi/write cell st [1 256]))]
  (assert (not ok?) "a :u8 field refuses 256")
  (assert (= (get err :error) :ffi-error) "the field's refusal is an ffi-error"))

(ffi/free cell)
