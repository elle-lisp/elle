;; FFI demo: calling C from Elle
;;
;; Exercises every layer of the FFI: library loading, symbol lookup,
;; signatures, direct calls, ffi/defbind macro, memory management,
;; struct marshalling, variadic calls, and callbacks.

;; Load the current process (includes libc/libm)
(def libc (ffi/native nil))

;; === Basic function binding ===
(ffi/defbind abs libc "abs" :int [:int])
(ffi/defbind sqrt libc "sqrt" :double [:double])
(ffi/defbind strlen libc "strlen" :size [:string])

(print "abs(-42) =" (abs -42))
(print "sqrt(2)  =" (sqrt 2.0))
(print "strlen   =" (strlen "hello world"))

;; === Memory management ===
(def buf (ffi/malloc 64))
(ffi/write buf :double 3.14159)
(print "read back:" (ffi/read buf :double))
(ffi/free buf)

;; === Structs ===
(def point-type (ffi/struct [:double :double]))
(def p (ffi/malloc (ffi/size point-type)))
(ffi/write p point-type [1.5 2.5])
(print "struct:   " (ffi/read p point-type))
(ffi/free p)

;; === Variadic (snprintf) ===
(def snprintf-ptr (ffi/lookup libc "snprintf"))
(def snprintf-sig (ffi/signature :int [:ptr :size :string :int] 3))
(def out (ffi/malloc 128))
(ffi/call snprintf-ptr snprintf-sig out 128 "the answer is %d" 42)
(print "snprintf: " (ffi/string out))
(ffi/free out)

;; === Callbacks (qsort) ===
(def qsort-ptr (ffi/lookup libc "qsort"))
(def qsort-sig (ffi/signature :void [:ptr :size :size :ptr]))
(def cmp-sig (ffi/signature :int [:ptr :ptr]))

(def arr (ffi/malloc 20))
(ffi/write arr (ffi/array :i32 5) [5 3 1 4 2])

(def cmp (ffi/callback cmp-sig
  (fn (a b) (- (ffi/read a :i32) (ffi/read b :i32)))))

(ffi/call qsort-ptr qsort-sig arr 5 4 cmp)
(print "sorted:   " (ffi/read arr (ffi/array :i32 5)))

(ffi/callback-free cmp)
(ffi/free arr)
