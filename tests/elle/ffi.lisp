(elle/epoch 9)

## FFI integration tests
## Tests the full pipeline: Elle source → compiler → VM → libffi → C

## ── ffi/with-stack ────────────────────────────────────────────────────

(ffi/with-stack [[p :int 42]]
                (assert (= (ffi/read p :int) 42) "ffi/with-stack typed scalar"))

(ffi/with-stack [[buf 16]] (ffi/write buf :int 99)
                (assert (= (ffi/read buf :int) 99) "ffi/with-stack raw buffer"))

(ffi/with-stack [[a :int 10] [b :int 20]]
                (assert (= (+ (ffi/read a :int) (ffi/read b :int)) 30)
                        "ffi/with-stack multiple"))

## ── ffi/pin ───────────────────────────────────────────────────────────

(let* [ptr (ffi/pin (bytes 72 101 108))]
  (assert (= (ffi/read ptr :u8) 72) "ffi/pin first byte")
  (ffi/free ptr))

(let* [ptr (ffi/pin "Hi")]
  (assert (= (ffi/read ptr :u8) 72) "ffi/pin string")
  (ffi/free ptr))

## ── Type introspection ──────────────────────────────────────────────

(assert (= (ffi/size :i32) 4) "ffi/size :i32")
(assert (= (ffi/size :double) 8) "ffi/size :double")
(assert (= (ffi/size :ptr) 8) "ffi/size :ptr")
(assert (= (ffi/size :void) nil) "ffi/size :void")
(assert (= (ffi/align :double) 8) "ffi/align :double")

## ── Signature creation ──────────────────────────────────────────────

(assert (not (nil? (ffi/signature :int @[:int]))) "ffi/signature creation")
(assert (not (nil? (ffi/signature :void @[]))) "ffi/signature void no args")
(let [[ok? _] (protect ((fn () (ffi/signature :bad @[:int]))))]
  (assert (not ok?) "ffi/signature bad type"))

## ── Memory management ───────────────────────────────────────────────

(def ptr (ffi/malloc 64))
(ffi/free ptr)
(assert (= :ok :ok) "ffi/malloc and ffi/free")

(def ptr (ffi/malloc 4))
(ffi/write ptr :i32 42)
(def val (ffi/read ptr :i32))
(ffi/free ptr)
(assert (= val 42) "ffi/read write roundtrip i32")

(def ptr (ffi/malloc 8))
(ffi/write ptr :double 1.234)
(def val (ffi/read ptr :double))
(ffi/free ptr)
(assert (= val 1.234) "ffi/read write double")

(let [[ok? _] (protect ((fn () (ffi/read nil :i32))))]
  (assert (not ok?) "ffi/read null error"))
(let [[ok? _] (protect ((fn () (ffi/malloc -1))))]
  (assert (not ok?) "ffi/malloc negative error"))

(let [[ok? _] (protect ((fn ()
                          (let [ptr (ffi/malloc 8)]
                            (ffi/free ptr)
                            (ffi/free ptr)))))]
  (assert (not ok?) "ffi/double free error"))

(let [[ok? _] (protect ((fn ()
                          (let [ptr (ffi/malloc 8)]
                            (ffi/write ptr :int 42)
                            (ffi/free ptr)
                            (ffi/read ptr :int)))))]
  (assert (not ok?) "ffi/use after free read error"))

(let [[ok? _] (protect ((fn ()
                          (let [ptr (ffi/malloc 8)]
                            (ffi/free ptr)
                            (ffi/write ptr :int 99)))))]
  (assert (not ok?) "ffi/use after free write error"))

(def ptr (ffi/malloc 8))
(ffi/write ptr :int 42)
(def v (ffi/read ptr :int))
(ffi/free ptr)
(assert (= v 42) "ffi/managed pointer normal use")

(def ptr (ffi/malloc 8))
(def r (ptr? ptr))
(ffi/free ptr)
(assert r "ffi/pointer predicate managed")

## ── Library loading and calling ─────────────────────────────────────

(def libc (ffi/native nil))
(def abs-ptr (ffi/lookup libc "abs"))
(def abs-sig (ffi/signature :int @[:int]))
(assert (= (ffi/call abs-ptr abs-sig -42) 42) "ffi/call abs")

(def libc (ffi/native nil))
(def strlen-ptr (ffi/lookup libc "strlen"))
(def strlen-sig (ffi/signature :size @[:string]))
(assert (= (ffi/call strlen-ptr strlen-sig "hello") 5) "ffi/call strlen")

(def libm (ffi/native nil))
(def sqrt-ptr (ffi/lookup libm "sqrt"))
(def sqrt-sig (ffi/signature :double @[:double]))
(def result (ffi/call sqrt-ptr sqrt-sig 4.0))
(assert (= result 2.0) "ffi/call sqrt")

(def self (ffi/native nil))
(def strlen-ptr (ffi/lookup self "strlen"))
(def strlen-sig (ffi/signature :size @[:string]))
(assert (= (ffi/call strlen-ptr strlen-sig "world") 5) "ffi/native self strlen")

(def self (ffi/native nil))
(def abs-ptr (ffi/lookup self "abs"))
(def abs-sig (ffi/signature :int @[:int]))
(assert (= (ffi/call abs-ptr abs-sig -99) 99) "ffi/native self abs")

## ── Error handling ──────────────────────────────────────────────────

(let [[ok? _] (protect ((fn () (ffi/native "/nonexistent/lib.so"))))]
  (assert (not ok?) "ffi/native missing library"))

(let [[ok? _] (protect ((fn ()
                          (def sig (ffi/signature :void @[]))
                          (ffi/call nil sig))))]
  (assert (not ok?) "ffi/call nil pointer"))

(let [[ok? _] (protect ((fn ()
                          (def sig (ffi/signature :int @[:int]))
                          (def ptr (ffi/malloc 1))
                          (ffi/call ptr sig))))]
  (assert (not ok?) "ffi/call wrong arg count"))

## ── Variadic functions ─────────────────────────────────────────────

(def self (ffi/native nil))
(def snprintf-ptr (ffi/lookup self "snprintf"))
(def buf (ffi/malloc 64))
(def sig (ffi/signature :int @[:ptr :size :string :int] 3))
(def written (ffi/call snprintf-ptr sig buf 64 "num: %d" 42))
(def result-str (ffi/string buf))
(ffi/free buf)
(assert (= result-str "num: 42") "ffi/call snprintf")

(assert (not (nil? (ffi/signature :int @[:ptr :size :string :int] 3)))
        "ffi/variadic signature creation")
(let [[ok? _] (protect ((fn () (ffi/signature :int @[:int] 5))))]
  (assert (not ok?) "ffi/variadic fixed args out of range"))

## ── ffi/string ─────────────────────────────────────────────────────

(assert (= (ffi/string nil) nil) "ffi/string nil")

## ── ffi/struct + struct marshalling ────────────────────────────────

(assert (not (nil? (ffi/struct @[:i32 :double :ptr]))) "ffi/struct creation")

(assert (= (ffi/size (ffi/struct @[:i32 :double])) 16) "ffi/struct size")
(assert (= (ffi/align (ffi/struct @[:i8 :double])) 8) "ffi/struct align")

(def st (ffi/struct @[:i32 :double]))
(def buf (ffi/malloc (ffi/size st)))
(ffi/write buf st @[42 3.14])
(def vals (ffi/read buf st))
(ffi/free buf)
(assert (= (get vals 0) 42) "ffi/struct read write roundtrip field 0")
(assert (= (get vals 1) 3.14) "ffi/struct read write roundtrip field 1")

(def inner (ffi/struct @[:i8 :i32]))
(def outer (ffi/struct @[:i64 inner]))
(def buf (ffi/malloc (ffi/size outer)))
(ffi/write buf outer @[999 @[7 42]])
(def vals (ffi/read buf outer))
(ffi/free buf)
(assert (= (get vals 0) 999) "ffi/struct nested read write outer")
(def inner-vals (get vals 1))
(assert (= (get inner-vals 0) 7) "ffi/struct nested read write inner 0")
(assert (= (get inner-vals 1) 42) "ffi/struct nested read write inner 1")

(assert (not (nil? (ffi/array :i32 10))) "ffi/array creation")
(assert (= (ffi/size (ffi/array :i32 10)) 40) "ffi/array size")

(def at (ffi/array :i32 3))
(def buf (ffi/malloc (ffi/size at)))
(ffi/write buf at @[10 20 30])
(def vals (ffi/read buf at))
(ffi/free buf)
(assert (= (get vals 0) 10) "ffi/array read write roundtrip 0")
(assert (= (get vals 1) 20) "ffi/array read write roundtrip 1")
(assert (= (get vals 2) 30) "ffi/array read write roundtrip 2")

(let [[ok? _] (protect ((fn ()
                          (def st (ffi/struct @[:i32 :double]))
                          (def buf (ffi/malloc (ffi/size st)))
                          (ffi/write buf st @[42])
                          (ffi/free buf))))]
  (assert (not ok?) "ffi/struct wrong field count"))

(let [[ok? _] (protect ((fn () (ffi/struct @[]))))]
  (assert (not ok?) "ffi/struct empty rejected"))
(let [[ok? _] (protect ((fn () (ffi/array :i32 0))))]
  (assert (not ok?) "ffi/array zero rejected"))

(def st (ffi/struct @[:i32 :double]))
(assert (not (nil? (ffi/signature st @[:ptr]))) "ffi/signature with struct type")

(def st (ffi/struct @[:i32 :double]))
(assert (not (nil? (ffi/signature :void @[st]))) "ffi/signature with struct arg")

(def st (ffi/struct @[:i8 :u8 :i16 :u16 :i32 :u32 :i64 :u64 :float :double]))
(def buf (ffi/malloc (ffi/size st)))
(ffi/write buf st
           @[-1 255 -1000 60000 -100000 3000000000 -999999999 999999999 1.5 2.5])
(def vals (ffi/read buf st))
(ffi/free buf)
(assert (= (get vals 0) -1) "ffi/struct all numeric types i8")
(assert (= (get vals 1) 255) "ffi/struct all numeric types u8")
(assert (= (get vals 2) -1000) "ffi/struct all numeric types i16")
(assert (= (get vals 3) 60000) "ffi/struct all numeric types u16")
(assert (= (get vals 4) -100000) "ffi/struct all numeric types i32")
(assert (= (get vals 5) 3000000000) "ffi/struct all numeric types u32")
(assert (= (get vals 6) -999999999) "ffi/struct all numeric types i64")
(assert (= (get vals 7) 999999999) "ffi/struct all numeric types u64")
(assert (= (get vals 8) 1.5) "ffi/struct all numeric types float")
(assert (= (get vals 9) 2.5) "ffi/struct all numeric types double")

## ── Callback creation ───────────────────────────────────────────────

(def sig (ffi/signature :int @[:ptr :ptr]))
(def cb (ffi/callback sig (fn (a b) 0)))
(def is-ptr (not (nil? cb)))
(ffi/callback-free cb)
(assert is-ptr "ffi/callback creation")

(assert (= (ffi/callback-free nil) nil) "ffi/callback free nil")

(let [[ok? _] (protect ((fn ()
                          (def sig (ffi/signature :int @[:ptr :ptr]))
                          (ffi/callback sig 42))))]
  (assert (not ok?) "ffi/callback wrong type"))

(let [[ok? _] (protect ((fn ()
                          (def sig (ffi/signature :int @[:ptr :ptr]))
                          (ffi/callback sig (fn (a) 0)))))]
  (assert (not ok?) "ffi/callback arity mismatch"))

(let [[ok? _] (protect ((fn ()
                          (def sig (ffi/signature :int @[:ptr :int] 1))
                          (ffi/callback sig (fn (a b) 0)))))]
  (assert (not ok?) "ffi/callback variadic rejected"))

(let [[ok? _] (protect ((fn () (ffi/callback-free (ffi/malloc 8)))))]
  (assert (not ok?) "ffi/callback free unknown ptr"))

## ── Callback with qsort ────────────────────────────────────────────

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar
  (ffi/callback compar-sig (fn (a b) (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 20))
(ffi/write arr (ffi/array :i32 5) @[5 3 1 4 2])
(ffi/call qsort-ptr qsort-sig arr 5 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 5)))
(ffi/free arr)
(ffi/callback-free compar)
(assert (= (get sorted 0) 1) "ffi/callback qsort 0")
(assert (= (get sorted 1) 2) "ffi/callback qsort 1")
(assert (= (get sorted 2) 3) "ffi/callback qsort 2")
(assert (= (get sorted 3) 4) "ffi/callback qsort 3")
(assert (= (get sorted 4) 5) "ffi/callback qsort 4")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar
  (ffi/callback compar-sig (fn (a b) (- (ffi/read b :i32) (ffi/read a :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 20))
(ffi/write arr (ffi/array :i32 5) @[10 30 20 50 40])
(ffi/call qsort-ptr qsort-sig arr 5 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 5)))
(ffi/free arr)
(ffi/callback-free compar)
(assert (= (get sorted 0) 50) "ffi/callback qsort descending 0")
(assert (= (get sorted 1) 40) "ffi/callback qsort descending 1")
(assert (= (get sorted 2) 30) "ffi/callback qsort descending 2")
(assert (= (get sorted 3) 20) "ffi/callback qsort descending 3")
(assert (= (get sorted 4) 10) "ffi/callback qsort descending 4")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar
  (ffi/callback compar-sig (fn (a b) (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 12))
(ffi/write arr (ffi/array :i32 3) @[1 2 3])
(ffi/call qsort-ptr qsort-sig arr 3 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 3)))
(ffi/free arr)
(ffi/callback-free compar)
(assert (= (get sorted 0) 1) "ffi/callback qsort already sorted 0")
(assert (= (get sorted 1) 2) "ffi/callback qsort already sorted 1")
(assert (= (get sorted 2) 3) "ffi/callback qsort already sorted 2")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar
  (ffi/callback compar-sig (fn (a b) (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 4))
(ffi/write arr (ffi/array :i32 1) @[42])
(ffi/call qsort-ptr qsort-sig arr 1 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 1)))
(ffi/free arr)
(ffi/callback-free compar)
(assert (= (get sorted 0) 42) "ffi/callback qsort single element")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar
  (ffi/callback compar-sig (fn (a b) (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 8))
(ffi/write arr (ffi/array :i32 2) @[2 1])
(ffi/call qsort-ptr qsort-sig arr 2 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 2)))
(ffi/free arr)
(ffi/callback-free compar)
(assert (= (get sorted 0) 1) "ffi/callback qsort two elements 0")
(assert (= (get sorted 1) 2) "ffi/callback qsort two elements 1")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def direction 1)
(def compar
  (ffi/callback compar-sig
                (fn (a b) (* direction (- (ffi/read a :i32) (ffi/read b :i32))))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 12))
(ffi/write arr (ffi/array :i32 3) @[3 1 2])
(ffi/call qsort-ptr qsort-sig arr 3 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 3)))
(ffi/free arr)
(ffi/callback-free compar)
(assert (= (get sorted 0) 1) "ffi/callback with closure capture 0")
(assert (= (get sorted 1) 2) "ffi/callback with closure capture 1")
(assert (= (get sorted 2) 3) "ffi/callback with closure capture 2")

## ── ffi/defbind macro ────────────────────────────────────────────

(def libc (ffi/native nil))
(ffi/defbind abs libc "abs" :int @[:int])
(assert (= (abs -42) 42) "ffi/defbind abs")

(def libc (ffi/native nil))
(ffi/defbind sqrt libc "sqrt" :double @[:double])
(assert (= (sqrt 144.0) 12.0) "ffi/defbind sqrt")

(def libc (ffi/native nil))
(ffi/defbind strlen libc "strlen" :size @[:string])
(assert (= (strlen "hello") 5) "ffi/defbind strlen")

(def libc (ffi/native nil))
(ffi/defbind abs libc "abs" :int @[:int])
(ffi/defbind strlen libc "strlen" :size @[:string])
(def result @[(abs -99) (strlen "world")])
(assert (= (get result 0) 99) "ffi/defbind multiple 0")
(assert (= (get result 1) 5) "ffi/defbind multiple 1")

(def libc (ffi/native nil))
(ffi/defbind getpid libc "getpid" :int @[])
(def pid (getpid))
(assert (> pid 0) "ffi/defbind zero args")

## ── ffi/signature and ffi/defbind with immutable array arg-types ─

# Regression test for issue #560: ffi/signature must accept immutable arrays.

(def libc (ffi/native nil))
(ffi/defbind abs libc "abs" :int [:int])
(assert (= (abs -42) 42) "ffi/defbind immutable array arg-types")

(def libc (ffi/native nil))
(def ptr (ffi/lookup libc "abs"))
(def sig (ffi/signature :int [:int]))
(assert (= (ffi/call ptr sig -7) 7) "ffi/signature with immutable array")

(def libc (ffi/native nil))
(ffi/defbind getpid libc "getpid" :int [])
(assert (> (getpid) 0) "ffi/defbind empty immutable array")

## ── Pointer arithmetic ──────────────────────────────────────────────

# ptr/add basic: offset into a buffer and read/write at offset
(def buf (ffi/malloc 64))
(def p2 (ptr/add buf 16))
(ffi/write p2 :i32 99)
(assert (= (ffi/read p2 :i32) 99) "ptr/add offset read write")
(ffi/free buf)

# ptr/add negative offset
(def buf (ffi/malloc 64))
(def p2 (ptr/add buf 32))
(def p3 (ptr/add p2 -16))
(ffi/write p3 :i32 77)
(assert (= (ffi/read (ptr/add buf 16) :i32) 77) "ptr/add negative offset")
(ffi/free buf)

# ptr/add returns raw pointer (not managed — cannot double-free)
(def buf (ffi/malloc 64))
(def p2 (ptr/add buf 8))
(assert (ptr? p2) "ptr/add result is pointer")
(ffi/free buf)

# ptr/add error: null pointer
(let [[ok? _] (protect ((fn () (ptr/add nil 8))))]
  (assert (not ok?) "ptr/add null error"))

# ptr/add error: freed pointer
(let [[ok? _] (protect ((fn ()
                          (let [p (ffi/malloc 8)]
                            (ffi/free p)
                            (ptr/add p 4)))))]
  (assert (not ok?) "ptr/add freed pointer error"))

# ptr/add error: wrong type
(let [[ok? _] (protect ((fn () (ptr/add 42 8))))]
  (assert (not ok?) "ptr/add wrong type error"))

# ptr/add error: non-integer offset
(let [[ok? _] (protect ((fn ()
                          (def p (ffi/malloc 8))
                          (ptr/add p "hello"))))]
  (assert (not ok?) "ptr/add non-integer offset error"))

# ptr/diff basic
(def buf (ffi/malloc 64))
(def p2 (ptr/add buf 24))
(assert (= (ptr/diff p2 buf) 24) "ptr/diff positive")
(assert (= (ptr/diff buf p2) -24) "ptr/diff negative")
(ffi/free buf)

# ptr/to-int and ptr/from-int roundtrip
(def buf (ffi/malloc 64))
(def addr (ptr/to-int buf))
(assert (integer? addr) "ptr/to-int returns integer")
(assert (> addr 0) "ptr/to-int positive address")
(def p2 (ptr/from-int addr))
(assert (ptr? p2) "ptr/from-int returns pointer")
(ffi/write p2 :i32 123)
(assert (= (ffi/read buf :i32) 123) "ptr/from-int roundtrip")
(ffi/free buf)

# ptr/from-int zero returns nil
(assert (nil? (ptr/from-int 0)) "ptr/from-int zero is nil")

# ptr/from-int error: negative
## ptr/from-int accepts negative values (sentinel pointers like SQLITE_TRANSIENT)
(let [p (ptr/from-int -1)]
  (assert (ptr? p) "ptr/from-int negative returns pointer")
  (assert (= (ptr/to-int p) -1) "ptr/from-int negative roundtrips"))

# ptr/from-int error: wrong type
(let [[ok? _] (protect ((fn () (ptr/from-int "hello"))))]
  (assert (not ok?) "ptr/from-int wrong type error"))

# ptr/to-int error: null pointer
(let [[ok? _] (protect ((fn () (ptr/to-int nil))))]
  (assert (not ok?) "ptr/to-int null error"))

# ptr/to-int error: wrong type
(let [[ok? _] (protect ((fn () (ptr/to-int 42))))]
  (assert (not ok?) "ptr/to-int wrong type error"))

# ptr/add with managed pointer input produces raw pointer usable with ffi/read
(def buf (ffi/malloc 32))
(ffi/write buf :i32 111)
(def p2 (ptr/add buf 4))
(ffi/write p2 :i32 222)
(assert (= (ffi/read buf :i32) 111) "ptr/add managed input field 0")
(assert (= (ffi/read p2 :i32) 222) "ptr/add managed input field 1")
(ffi/free buf)

# Alignment check via ptr/to-int
(def buf (ffi/malloc 64))
(assert (= (mod (ptr/to-int buf) 8) 0) "malloc alignment check")
(ffi/free buf)
