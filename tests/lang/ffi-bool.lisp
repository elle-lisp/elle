(elle/epoch 15)
# audited: 2026-10-13
# :bool is a one-byte C bool, and :bool32 is a boolean held in a 32-bit integer.
# docs/ffi.md
#
# The trap: a C function that returns a bool defines only the low byte of the
# return register. A compiler can leave other bits set above that byte, so a
# false return can sit in a register that reads 0x1D0F00 as an int. A callback
# declared to return :u32 makes that register on demand, with no C fixture.
#
# The counter-factual reads :bool as an int. Then the 0x1D0F00 register reads
# true, a nonzero byte beside a zero :bool reads true, and a :bool write
# overwrites the three bytes after it. Every memory probe builds its bytes one
# at a time, so its answer does not depend on the byte order.

(def four-bytes (ffi/array :u8 4))

(defn returning [bits]
  (ffi/callback (ffi/signature :u32 []) (fn () bits)))

(defn returned-as [type bits]
  (let* [f (returning bits)
         r (ffi/call f (ffi/signature type []))]
    (ffi/callback-free f)
    r))

(defn argument-as [type bits]
  (let* [seen @[]
         record (fn (b) (push seen b))
         f (ffi/callback (ffi/signature :void [type]) record)]
    (ffi/call f (ffi/signature :void [:u32]) bits)
    (ffi/callback-free f)
    (get seen 0)))

(defn read-over [type b0 b1 b2 b3]
  (let* [p (ffi/malloc 4)]
    (ffi/write p four-bytes (bytes b0 b1 b2 b3))
    (let [v (ffi/read p type)]
      (ffi/free p)
      v)))

(defn written-over-ones [type value]
  (let* [p (ffi/malloc 4)]
    (ffi/write p four-bytes (bytes 255 255 255 255))
    (ffi/write p type value)
    (let [v (ffi/read p four-bytes)]
      (ffi/free p)
      v)))

## ── Size and layout ───────────────────────────────────────────────

(assert (= (ffi/size :bool) 1) ":bool is one byte")
(assert (= (ffi/align :bool) 1) ":bool aligns to one byte")
(assert (= (ffi/size (ffi/struct [:u8 :bool :u8])) 3)
        "a :bool field packs between two bytes")
(assert (= (ffi/size :bool32) 4) ":bool32 is four bytes")
(assert (= (ffi/align :bool32) 4) ":bool32 aligns to four bytes")
(assert (= (ffi/size (ffi/struct [:u8 :bool32])) 8)
        "a :bool32 field aligns to four bytes")

## ── Return values ─────────────────────────────────────────────────

(assert (= (returned-as :bool 0x1D0F00) false)
        ":bool return reads only the low byte")
(assert (= (returned-as :bool 0x1D0F01) true) ":bool return of a set byte")
(assert (= (returned-as :bool 2) true) ":bool return of any nonzero byte")
(assert (= (returned-as :bool32 0x1D0F00) true)
        ":bool32 return reads all four bytes")
(assert (= (returned-as :bool32 0) false) ":bool32 return of zero")

## ── Callback arguments ────────────────────────────────────────────

(assert (= (argument-as :bool 0x100) false)
        ":bool callback argument reads only the low byte")
(assert (= (argument-as :bool 0x101) true)
        ":bool callback argument of a set byte")
(assert (= (argument-as :bool32 0x100) true)
        ":bool32 callback argument reads all four bytes")
(assert (= (argument-as :bool32 0) false) ":bool32 callback argument of zero")

## ── Memory ────────────────────────────────────────────────────────

(assert (= (read-over :bool 0 255 255 255) false)
        "ffi/read :bool ignores the bytes after it")
(assert (= (read-over :bool 1 0 0 0) true) "ffi/read :bool of one")
(assert (= (read-over :bool 2 0 0 0) true) "ffi/read :bool of any nonzero byte")
(assert (= (read-over :bool32 0 0 0 1) true)
        "ffi/read :bool32 of a nonzero high byte")
(assert (= (read-over :bool32 1 0 0 0) true)
        "ffi/read :bool32 of a nonzero low byte")
(assert (= (read-over :bool32 0 0 0 0) false) "ffi/read :bool32 of zero")

(assert (= (written-over-ones :bool true) (bytes 1 255 255 255))
        "ffi/write :bool true writes one byte")
(assert (= (written-over-ones :bool false) (bytes 0 255 255 255))
        "ffi/write :bool false writes one byte")
(assert (= (written-over-ones :bool32 false) (bytes 0 0 0 0))
        "ffi/write :bool32 false writes four bytes")

(let* [p (ffi/malloc 4)]
  (ffi/write p :bool32 true)
  (assert (= (ffi/read p :u32) 1) "ffi/write :bool32 true writes the integer 1")
  (ffi/free p))

## ── Struct fields ─────────────────────────────────────────────────

(let* [st (ffi/struct [:u8 :bool :u8])
       p (ffi/malloc (ffi/size st))]
  (ffi/write p st [7 true 9])
  (assert (= (ffi/read p (ffi/array :u8 3)) (bytes 7 1 9))
          "a :bool field is the byte between its neighbors")
  (assert (= (ffi/read p st) [7 true 9]) "a :bool field reads back")
  (ffi/free p))
