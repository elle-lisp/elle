#!/usr/bin/env elle

# FFI — calling C from Elle
#
# Demonstrates:
#   Library loading   — ffi/native to open libc/libm
#   Symbol binding    — ffi/defbind for typed wrappers
#   Direct calls      — abs, sqrt, strlen via bound functions
#   Memory management — ffi/malloc, ffi/write, ffi/read, ffi/free
#   Struct marshalling — ffi/struct for composite types
#   Variadic calls    — snprintf via ffi/call with varargs
#   Callbacks         — qsort with Elle comparison function

(import-file "./examples/assertions.lisp")


# Load the current process (includes libc/libm)
(def libc (ffi/native nil))


# Basic function binding
(ffi/defbind abs libc "abs" :int @[:int])
(ffi/defbind sqrt libc "sqrt" :double @[:double])
(ffi/defbind strlen libc "strlen" :size @[:string])

(display "  abs(-42) = ")
(print (abs -42))
(assert-eq (abs -42) 42 "abs(-42) should be 42")

(display "  sqrt(2)  = ")
(print (sqrt 2.0))
(assert-eq (sqrt 2.0) 1.4142135623730951 "sqrt(2.0)")

(display "  strlen   = ")
(print (strlen "hello world"))
(assert-eq (strlen "hello world") 11 "strlen of 'hello world'")


# Memory management
(def buf (ffi/malloc 64))
(ffi/write buf :double 3.14159)
(def read-back (ffi/read buf :double))
(display "  read back: ")
(print read-back)
(assert-eq read-back 3.14159 "ffi/write then ffi/read :double")
(ffi/free buf)


# Structs
(def point-type (ffi/struct @[:double :double]))
(def p (ffi/malloc (ffi/size point-type)))
(ffi/write p point-type @[1.5 2.5])
(def point-val (ffi/read p point-type))
(display "  struct:    ")
(print point-val)
(assert-eq point-val @[1.5 2.5] "ffi/struct read-back")
(ffi/free p)


# Variadic (snprintf)
(def snprintf-ptr (ffi/lookup libc "snprintf"))
(def snprintf-sig (ffi/signature :int @[:ptr :size :string :int] 3))
(def out (ffi/malloc 128))
(ffi/call snprintf-ptr snprintf-sig out 128 "the answer is %d" 42)
(def formatted (ffi/string out))
(display "  snprintf:  ")
(print formatted)
(assert-string-eq formatted "the answer is 42" "snprintf formatting")
(ffi/free out)


# Callbacks (qsort)
(def qsort-ptr (ffi/lookup libc "qsort"))
(def qsort-sig (ffi/signature :void @[:ptr :size :size :ptr]))
(def cmp-sig (ffi/signature :int @[:ptr :ptr]))

(def arr (ffi/malloc 20))
(ffi/write arr (ffi/array :i32 5) @[5 3 1 4 2])

(def cmp (ffi/callback cmp-sig
  (fn [a b] (- (ffi/read a :i32) (ffi/read b :i32)))))

(ffi/call qsort-ptr qsort-sig arr 5 4 cmp)
(def sorted (ffi/read arr (ffi/array :i32 5)))
(display "  sorted:    ")
(print sorted)
(assert-eq sorted @[1 2 3 4 5] "qsort should sort ascending")

(ffi/callback-free cmp)
(ffi/free arr)

(print "")
(print "all ffi passed.")
