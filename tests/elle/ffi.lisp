(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## FFI integration tests
## Tests the full pipeline: Elle source → compiler → VM → libffi → C

## ── Type introspection ──────────────────────────────────────────────

(assert-eq (ffi/size :i32) 4 "ffi/size :i32")
(assert-eq (ffi/size :double) 8 "ffi/size :double")
(assert-eq (ffi/size :ptr) 8 "ffi/size :ptr")
(assert-eq (ffi/size :void) nil "ffi/size :void")
(assert-eq (ffi/align :double) 8 "ffi/align :double")

## ── Signature creation ──────────────────────────────────────────────

(assert-not-nil (ffi/signature :int @[:int]) "ffi/signature creation")
(assert-not-nil (ffi/signature :void @[]) "ffi/signature void no args")
(assert-err (fn () (ffi/signature :bad @[:int])) "ffi/signature bad type")

## ── Memory management ───────────────────────────────────────────────

(def ptr (ffi/malloc 64))
(ffi/free ptr)
(assert-eq :ok :ok "ffi/malloc and ffi/free")

(def ptr (ffi/malloc 4))
(ffi/write ptr :i32 42)
(def val (ffi/read ptr :i32))
(ffi/free ptr)
(assert-eq val 42 "ffi/read write roundtrip i32")

(def ptr (ffi/malloc 8))
(ffi/write ptr :double 1.234)
(def val (ffi/read ptr :double))
(ffi/free ptr)
(assert-eq val 1.234 "ffi/read write double")

(assert-err (fn () (ffi/read nil :i32)) "ffi/read null error")
(assert-err (fn () (ffi/malloc -1)) "ffi/malloc negative error")

(assert-err (fn () (let ((ptr (ffi/malloc 8)))
                     (ffi/free ptr)
                     (ffi/free ptr)))
            "ffi/double free error")

(assert-err (fn () (let ((ptr (ffi/malloc 8)))
                     (ffi/write ptr :int 42)
                     (ffi/free ptr)
                     (ffi/read ptr :int)))
            "ffi/use after free read error")

(assert-err (fn () (let ((ptr (ffi/malloc 8)))
                     (ffi/free ptr)
                     (ffi/write ptr :int 99)))
            "ffi/use after free write error")

(def ptr (ffi/malloc 8))
(ffi/write ptr :int 42)
(def v (ffi/read ptr :int))
(ffi/free ptr)
(assert-eq v 42 "ffi/managed pointer normal use")

(def ptr (ffi/malloc 8))
(def r (pointer? ptr))
(ffi/free ptr)
(assert-true r "ffi/pointer predicate managed")

## ── Library loading and calling ─────────────────────────────────────

(def libc (ffi/native nil))
(def abs-ptr (ffi/lookup libc "abs"))
(def abs-sig (ffi/signature :int @[:int]))
(assert-eq (ffi/call abs-ptr abs-sig -42) 42 "ffi/call abs")

(def libc (ffi/native nil))
(def strlen-ptr (ffi/lookup libc "strlen"))
(def strlen-sig (ffi/signature :size @[:string]))
(assert-eq (ffi/call strlen-ptr strlen-sig "hello") 5 "ffi/call strlen")

(def libm (ffi/native nil))
(def sqrt-ptr (ffi/lookup libm "sqrt"))
(def sqrt-sig (ffi/signature :double @[:double]))
(def result (ffi/call sqrt-ptr sqrt-sig 4.0))
(assert-eq result 2.0 "ffi/call sqrt")

(def self (ffi/native nil))
(def strlen-ptr (ffi/lookup self "strlen"))
(def strlen-sig (ffi/signature :size @[:string]))
(assert-eq (ffi/call strlen-ptr strlen-sig "world") 5 "ffi/native self strlen")

(def self (ffi/native nil))
(def abs-ptr (ffi/lookup self "abs"))
(def abs-sig (ffi/signature :int @[:int]))
(assert-eq (ffi/call abs-ptr abs-sig -99) 99 "ffi/native self abs")

## ── Error handling ──────────────────────────────────────────────────

(assert-err (fn () (ffi/native "/nonexistent/lib.so")) "ffi/native missing library")

(assert-err (fn () (def sig (ffi/signature :void @[]))
                   (ffi/call nil sig))
            "ffi/call nil pointer")

(assert-err (fn () (def sig (ffi/signature :int @[:int]))
                   (def ptr (ffi/malloc 1))
                   (ffi/call ptr sig))
            "ffi/call wrong arg count")

## ── Variadic functions ─────────────────────────────────────────────

(def self (ffi/native nil))
(def snprintf-ptr (ffi/lookup self "snprintf"))
(def buf (ffi/malloc 64))
(def sig (ffi/signature :int @[:ptr :size :string :int] 3))
(def written (ffi/call snprintf-ptr sig buf 64 "num: %d" 42))
(def result-str (ffi/string buf))
(ffi/free buf)
(assert-eq result-str "num: 42" "ffi/call snprintf")

(assert-not-nil (ffi/signature :int @[:ptr :size :string :int] 3) "ffi/variadic signature creation")
(assert-err (fn () (ffi/signature :int @[:int] 5)) "ffi/variadic fixed args out of range")

## ── ffi/string ─────────────────────────────────────────────────────

(assert-eq (ffi/string nil) nil "ffi/string nil")

## ── ffi/struct + struct marshalling ────────────────────────────────

(assert-not-nil (ffi/struct @[:i32 :double :ptr]) "ffi/struct creation")

(assert-eq (ffi/size (ffi/struct @[:i32 :double])) 16 "ffi/struct size")
(assert-eq (ffi/align (ffi/struct @[:i8 :double])) 8 "ffi/struct align")

(def st (ffi/struct @[:i32 :double]))
(def buf (ffi/malloc (ffi/size st)))
(ffi/write buf st @[42 3.14])
(def vals (ffi/read buf st))
(ffi/free buf)
(assert-eq (get vals 0) 42 "ffi/struct read write roundtrip field 0")
(assert-eq (get vals 1) 3.14 "ffi/struct read write roundtrip field 1")

(def inner (ffi/struct @[:i8 :i32]))
(def outer (ffi/struct @[:i64 inner]))
(def buf (ffi/malloc (ffi/size outer)))
(ffi/write buf outer @[999 @[7 42]])
(def vals (ffi/read buf outer))
(ffi/free buf)
(assert-eq (get vals 0) 999 "ffi/struct nested read write outer")
(def inner-vals (get vals 1))
(assert-eq (get inner-vals 0) 7 "ffi/struct nested read write inner 0")
(assert-eq (get inner-vals 1) 42 "ffi/struct nested read write inner 1")

(assert-not-nil (ffi/array :i32 10) "ffi/array creation")
(assert-eq (ffi/size (ffi/array :i32 10)) 40 "ffi/array size")

(def at (ffi/array :i32 3))
(def buf (ffi/malloc (ffi/size at)))
(ffi/write buf at @[10 20 30])
(def vals (ffi/read buf at))
(ffi/free buf)
(assert-eq (get vals 0) 10 "ffi/array read write roundtrip 0")
(assert-eq (get vals 1) 20 "ffi/array read write roundtrip 1")
(assert-eq (get vals 2) 30 "ffi/array read write roundtrip 2")

(assert-err (fn () (def st (ffi/struct @[:i32 :double]))
                   (def buf (ffi/malloc (ffi/size st)))
                   (ffi/write buf st @[42])
                   (ffi/free buf))
            "ffi/struct wrong field count")

(assert-err (fn () (ffi/struct @[])) "ffi/struct empty rejected")
(assert-err (fn () (ffi/array :i32 0)) "ffi/array zero rejected")

(def st (ffi/struct @[:i32 :double]))
(assert-not-nil (ffi/signature st @[:ptr]) "ffi/signature with struct type")

(def st (ffi/struct @[:i32 :double]))
(assert-not-nil (ffi/signature :void @[st]) "ffi/signature with struct arg")

(def st (ffi/struct @[:i8 :u8 :i16 :u16 :i32 :u32 :i64 :u64 :float :double]))
(def buf (ffi/malloc (ffi/size st)))
(ffi/write buf st @[-1 255 -1000 60000 -100000 3000000000 -999999999 999999999 1.5 2.5])
(def vals (ffi/read buf st))
(ffi/free buf)
(assert-eq (get vals 0) -1 "ffi/struct all numeric types i8")
(assert-eq (get vals 1) 255 "ffi/struct all numeric types u8")
(assert-eq (get vals 2) -1000 "ffi/struct all numeric types i16")
(assert-eq (get vals 3) 60000 "ffi/struct all numeric types u16")
(assert-eq (get vals 4) -100000 "ffi/struct all numeric types i32")
(assert-eq (get vals 5) 3000000000 "ffi/struct all numeric types u32")
(assert-eq (get vals 6) -999999999 "ffi/struct all numeric types i64")
(assert-eq (get vals 7) 999999999 "ffi/struct all numeric types u64")
(assert-eq (get vals 8) 1.5 "ffi/struct all numeric types float")
(assert-eq (get vals 9) 2.5 "ffi/struct all numeric types double")

## ── Callback creation ───────────────────────────────────────────────

(def sig (ffi/signature :int @[:ptr :ptr]))
(def cb (ffi/callback sig (fn (a b) 0)))
(def is-ptr (not (nil? cb)))
(ffi/callback-free cb)
(assert-true is-ptr "ffi/callback creation")

(assert-eq (ffi/callback-free nil) nil "ffi/callback free nil")

(assert-err (fn () (def sig (ffi/signature :int @[:ptr :ptr]))
                   (ffi/callback sig 42))
            "ffi/callback wrong type")

(assert-err (fn () (def sig (ffi/signature :int @[:ptr :ptr]))
                   (ffi/callback sig (fn (a) 0)))
            "ffi/callback arity mismatch")

(assert-err (fn () (def sig (ffi/signature :int @[:ptr :int] 1))
                   (ffi/callback sig (fn (a b) 0)))
            "ffi/callback variadic rejected")

(assert-err (fn () (ffi/callback-free (ffi/malloc 8))) "ffi/callback free unknown ptr")

## ── Callback with qsort ────────────────────────────────────────────

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar (ffi/callback compar-sig
  (fn (a b)
    (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 20))
(ffi/write arr (ffi/array :i32 5) @[5 3 1 4 2])
(ffi/call qsort-ptr qsort-sig arr 5 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 5)))
(ffi/free arr)
(ffi/callback-free compar)
(assert-eq (get sorted 0) 1 "ffi/callback qsort 0")
(assert-eq (get sorted 1) 2 "ffi/callback qsort 1")
(assert-eq (get sorted 2) 3 "ffi/callback qsort 2")
(assert-eq (get sorted 3) 4 "ffi/callback qsort 3")
(assert-eq (get sorted 4) 5 "ffi/callback qsort 4")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar (ffi/callback compar-sig
  (fn (a b)
    (- (ffi/read b :i32) (ffi/read a :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 20))
(ffi/write arr (ffi/array :i32 5) @[10 30 20 50 40])
(ffi/call qsort-ptr qsort-sig arr 5 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 5)))
(ffi/free arr)
(ffi/callback-free compar)
(assert-eq (get sorted 0) 50 "ffi/callback qsort descending 0")
(assert-eq (get sorted 1) 40 "ffi/callback qsort descending 1")
(assert-eq (get sorted 2) 30 "ffi/callback qsort descending 2")
(assert-eq (get sorted 3) 20 "ffi/callback qsort descending 3")
(assert-eq (get sorted 4) 10 "ffi/callback qsort descending 4")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar (ffi/callback compar-sig
  (fn (a b)
    (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 12))
(ffi/write arr (ffi/array :i32 3) @[1 2 3])
(ffi/call qsort-ptr qsort-sig arr 3 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 3)))
(ffi/free arr)
(ffi/callback-free compar)
(assert-eq (get sorted 0) 1 "ffi/callback qsort already sorted 0")
(assert-eq (get sorted 1) 2 "ffi/callback qsort already sorted 1")
(assert-eq (get sorted 2) 3 "ffi/callback qsort already sorted 2")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar (ffi/callback compar-sig
  (fn (a b)
    (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 4))
(ffi/write arr (ffi/array :i32 1) @[42])
(ffi/call qsort-ptr qsort-sig arr 1 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 1)))
(ffi/free arr)
(ffi/callback-free compar)
(assert-eq (get sorted 0) 42 "ffi/callback qsort single element")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def compar (ffi/callback compar-sig
  (fn (a b)
    (- (ffi/read a :i32) (ffi/read b :i32)))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 8))
(ffi/write arr (ffi/array :i32 2) @[2 1])
(ffi/call qsort-ptr qsort-sig arr 2 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 2)))
(ffi/free arr)
(ffi/callback-free compar)
(assert-eq (get sorted 0) 1 "ffi/callback qsort two elements 0")
(assert-eq (get sorted 1) 2 "ffi/callback qsort two elements 1")

(def libc (ffi/native nil))
(def qsort-ptr (ffi/lookup libc "qsort"))
(def compar-sig (ffi/signature :int @[:ptr :ptr]))
(def direction 1)
(def compar (ffi/callback compar-sig
  (fn (a b)
    (* direction (- (ffi/read a :i32) (ffi/read b :i32))))))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def arr (ffi/malloc 12))
(ffi/write arr (ffi/array :i32 3) @[3 1 2])
(ffi/call qsort-ptr qsort-sig arr 3 4 compar)
(def sorted (ffi/read arr (ffi/array :i32 3)))
(ffi/free arr)
(ffi/callback-free compar)
(assert-eq (get sorted 0) 1 "ffi/callback with closure capture 0")
(assert-eq (get sorted 1) 2 "ffi/callback with closure capture 1")
(assert-eq (get sorted 2) 3 "ffi/callback with closure capture 2")

## ── ffi/defbind macro ────────────────────────────────────────────

(def libc (ffi/native nil))
(ffi/defbind abs libc "abs" :int @[:int])
(assert-eq (abs -42) 42 "ffi/defbind abs")

(def libc (ffi/native nil))
(ffi/defbind sqrt libc "sqrt" :double @[:double])
(assert-eq (sqrt 144.0) 12.0 "ffi/defbind sqrt")

(def libc (ffi/native nil))
(ffi/defbind strlen libc "strlen" :size @[:string])
(assert-eq (strlen "hello") 5 "ffi/defbind strlen")

(def libc (ffi/native nil))
(ffi/defbind abs libc "abs" :int @[:int])
(ffi/defbind strlen libc "strlen" :size @[:string])
(def result @[(abs -99) (strlen "world")])
(assert-eq (get result 0) 99 "ffi/defbind multiple 0")
(assert-eq (get result 1) 5 "ffi/defbind multiple 1")

(def libc (ffi/native nil))
(ffi/defbind getpid libc "getpid" :int @[])
(def pid (getpid))
(assert-true (> pid 0) "ffi/defbind zero args")

## ── ffi/signature and ffi/defbind with immutable array arg-types ─

# Regression test for issue #560: ffi/signature must accept immutable arrays.

(def libc (ffi/native nil))
(ffi/defbind abs libc "abs" :int [:int])
(assert-eq (abs -42) 42 "ffi/defbind immutable array arg-types")

(def libc (ffi/native nil))
(def ptr (ffi/lookup libc "abs"))
(def sig (ffi/signature :int [:int]))
(assert-eq (ffi/call ptr sig -7) 7 "ffi/signature with immutable array")

(def libc (ffi/native nil))
(ffi/defbind getpid libc "getpid" :int [])
(assert-true (> (getpid) 0) "ffi/defbind empty immutable array")
