(elle/epoch 11)
## tests/elle/prim-ffi.lisp
## FFI type system, memory operations, pointer arithmetic

## ── ffi/size ───────────────────────────────────────────────────────

(assert (= (ffi/size :i32) 4) "ffi/size :i32")
(assert (= (ffi/size :double) 8) "ffi/size :double")
(assert (nil? (ffi/size :void)) "ffi/size :void is nil")

(let [[ok? _] (protect ((fn [] (ffi/size :nonsense))))]
  (assert (not ok?) "ffi/size unknown type errors"))

## ── ffi/align ──────────────────────────────────────────────────────

(assert (= (ffi/align :double) 8) "ffi/align :double")

## ── ffi/signature ──────────────────────────────────────────────────

(let [sig (ffi/signature :double @[:double])]
  (assert sig "ffi/signature basic"))

(let [[ok? _] (protect ((fn [] (ffi/signature :bad @[]))))]
  (assert (not ok?) "ffi/signature unknown return type errors"))

# variadic signature
(let [sig (ffi/signature :int @[:ptr :string :int] 2)]
  (assert sig "ffi/signature variadic"))

(let [[ok? _] (protect ((fn [] (ffi/signature :int @[:int] 5))))]
  (assert (not ok?) "ffi/signature variadic out of range errors"))

## ── ffi/malloc and ffi/free ────────────────────────────────────────

(let [p (ffi/malloc 100)]
  (assert (ptr? p) "ffi/malloc returns pointer")
  (ffi/free p))

(ffi/free nil)
# freeing nil is ok

(let [[ok? _] (protect ((fn [] (ffi/malloc 0))))]
  (assert (not ok?) "ffi/malloc 0 errors"))

## ── ffi/read and ffi/write ─────────────────────────────────────────

(let [p (ffi/malloc 8)]
  (ffi/write p :i32 42)
  (assert (= (ffi/read p :i32) 42) "ffi read/write i32 roundtrip")
  (ffi/write p :double 1.234)
  (assert (= (ffi/read p :double) 1.234) "ffi read/write double roundtrip")
  (ffi/free p))

(let [[ok? _] (protect ((fn [] (ffi/read nil :i32))))]
  (assert (not ok?) "ffi/read null pointer errors"))

## ── ffi/struct ─────────────────────────────────────────────────────

(let [st (ffi/struct @[:i32 :i32])]
  (assert st "ffi/struct basic")
  (assert (= (ffi/size st) 8) "ffi/size struct"))

(let [[ok? _] (protect ((fn [] (ffi/struct @[]))))]
  (assert (not ok?) "ffi/struct empty errors"))

(let [[ok? _] (protect ((fn [] (ffi/struct @[:void]))))]
  (assert (not ok?) "ffi/struct void field errors"))

## ── ffi/array ──────────────────────────────────────────────────────

(let [arr (ffi/array :i32 10)]
  (assert arr "ffi/array basic"))

(let [[ok? _] (protect ((fn [] (ffi/array :i32 0))))]
  (assert (not ok?) "ffi/array zero count errors"))

(let [[ok? _] (protect ((fn [] (ffi/array :i32 -5))))]
  (assert (not ok?) "ffi/array negative count errors"))

## ── ffi/string ─────────────────────────────────────────────────────

(assert (nil? (ffi/string nil)) "ffi/string null is nil")

(let [[ok? _] (protect ((fn [] (ffi/string 42))))]
  (assert (not ok?) "ffi/string wrong type errors"))

## ── ptr/add ────────────────────────────────────────────────────────

(let [p (ffi/malloc 64)]
  (let [p2 (ptr/add p 16)]
    (assert (ptr? p2) "ptr/add returns pointer"))
  (ffi/free p))

(let [[ok? err] (protect ((fn [] (ptr/add nil 8))))]
  (assert (not ok?) "ptr/add null errors")
  (assert (= (get err :error) :argument-error) "ptr/add null: argument-error"))

(let [[ok? err] (protect ((fn [] (ptr/add 42 8))))]
  (assert (not ok?) "ptr/add wrong type errors")
  (assert (= (get err :error) :type-error) "ptr/add wrong type: type-error"))

## ── ptr/diff ───────────────────────────────────────────────────────

(let [p (ffi/malloc 64)]
  (let [p2 (ptr/add p 24)]
    (assert (= (ptr/diff p2 p) 24) "ptr/diff basic")
    (assert (= (ptr/diff p p2) -24) "ptr/diff negative"))
  (ffi/free p))

## ── ptr/to-int and ptr/from-int ────────────────────────────────────

(let [p (ffi/malloc 8)]
  (let [addr (ptr/to-int p)]
    (assert (integer? addr) "ptr/to-int returns integer")
    (let [p2 (ptr/from-int addr)]
      (assert (ptr? p2) "ptr/from-int returns pointer")))
  (ffi/free p))

(assert (nil? (ptr/from-int 0)) "ptr/from-int 0 is nil")

# negative pointer values are valid (e.g. SQLITE_TRANSIENT = -1)
(let [p (ptr/from-int -1)]
  (assert (ptr? p) "ptr/from-int -1 returns pointer"))

(let [[ok? _] (protect ((fn [] (ptr/to-int nil))))]
  (assert (not ok?) "ptr/to-int null errors"))

## ── struct read/write ──────────────────────────────────────────────

(let [st (ffi/struct @[:i32 :double])]
  (let [p (ffi/malloc (ffi/size st))]
    (ffi/write p st @[42 2.5])
    (let [result (ffi/read p st)]
      (assert (= (get result 0) 42) "struct read/write: i32 field")
      (assert (= (get result 1) 2.5) "struct read/write: double field"))
    (ffi/free p)))

(println "prim-ffi: all tests passed")
