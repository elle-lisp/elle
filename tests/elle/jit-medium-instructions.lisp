(elle/epoch 9)
## jit-medium-instructions — verify JIT support for 7 medium instructions

## Helper: scan rejection list for instruction name
(defn has-rejection? (name)
  (defn scan (rs)
    (if (= rs ())
      false
      (if (string/contains? (get (first rs) :reason) name)
        true
        (scan (rest rs)))))
  (scan (jit/rejections)))

## Helper: call f n times with no args
(defn repeat0 (n f)
  (if (<= n 0) true
    (begin (f) (repeat0 (- n 1) f))))

## Helper: call f n times with one arg
(defn repeat1 (n f x)
  (if (<= n 0) true
    (begin (f x) (repeat1 (- n 1) f x))))

## ===== ArrayMutLen =====
## Array length check in a match pattern emits ArrayMutLen.
(defn array-len-2? (arr)
  (match arr
    [a b] true
    _ false))

(repeat1 15 array-len-2? [1 2])

(assert (array-len-2? [1 2]) "array-len-2? true for 2-elem array")
(assert (not (array-len-2? [1])) "array-len-2? false for 1-elem array")
(assert (not (array-len-2? [1 2 3])) "array-len-2? false for 3-elem array")
(assert (not (has-rejection? "ArrayMutLen")) "ArrayMutLen not rejected")

## ===== CarOrNil / CdrOrNil =====
## Functions with &rest params and match on them trigger CarOrNil/CdrOrNil
## in the non-strict destructuring path.
(defn second-arg (x & rest)
  (match rest
    (a & _) a
    _ nil))

(repeat1 15 (fn (x) (second-arg x 99)) 1)

(assert (= (second-arg 1 2 3) 2) "second-arg returns second arg")
(assert (= (second-arg 1) nil) "second-arg nil when no second arg")
(assert (not (has-rejection? "CarOrNil")) "CarOrNil not rejected")
(assert (not (has-rejection? "CdrOrNil")) "CdrOrNil not rejected")

## ===== ArrayMutRefOrNil =====
## Array index matching with optional elements uses ArrayMutRefOrNil.
(defn arr-second (arr)
  (match arr
    [_ b] b
    _ nil))

(repeat1 15 arr-second [10 20])

(assert (= (arr-second [10 20]) 20) "arr-second returns element at index 1")
(assert (= (arr-second [10]) nil) "arr-second nil on short array")
(assert (not (has-rejection? "ArrayMutRefOrNil")) "ArrayMutRefOrNil not rejected")

## ===== ArrayMutPush / ArrayMutExtend =====
## The splice operator ; emits ArrayMutPush (scalar) and ArrayMutExtend (array).
(defn splice-arrays (a b)
  [;a ;b])

(defn splice-mixed (x)
  [1 ;[2 3] x])

(repeat1 15 (fn (_) (splice-arrays [1 2] [3 4])) nil)
(repeat1 15 splice-mixed 5)

(assert (= (splice-arrays [1 2] [3 4]) [1 2 3 4]) "splice-arrays extends arrays")
(assert (= (splice-mixed 5) [1 2 3 5]) "splice-mixed with scalar")
(assert (not (has-rejection? "ArrayMutPush")) "ArrayMutPush not rejected")
(assert (not (has-rejection? "ArrayMutExtend")) "ArrayMutExtend not rejected")

## ===== PushParamFrame =====
## (parameterize ...) emits PushParamFrame + PopParamFrame.
(def @p (make-parameter 0))

(defn with-p (val)
  (parameterize ((p val))
    (p)))

(repeat1 15 with-p 42)

(assert (= (with-p 99) 99) "parameterize sets parameter value")
(assert (= (p) 0) "parameter restored after parameterize")
(assert (not (has-rejection? "PushParamFrame")) "PushParamFrame not rejected")
