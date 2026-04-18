(elle/epoch 8)
## jit-type-predicates — verify IsArray, IsStruct, IsSet compile via JIT

## Functions using type predicates — should JIT-compile without rejection
(defn check-array (x) (array? x))
(defn check-struct (x) (struct? x))
(defn check-set (x) (set? x))

## Call 15 times each to exceed JIT threshold
(defn repeat (n f x)
  (if (<= n 0) true
    (begin (f x) (repeat (- n 1) f x))))

(repeat 15 check-array [1 2 3])
(repeat 15 check-struct {:a 1})
(repeat 15 check-set |1 2 3|)

## Verify correct results
(assert (check-array [1 2 3]) "array? true for array")
(assert (not (check-array 42)) "array? false for int")
(assert (check-struct {:a 1}) "struct? true for struct")
(assert (not (check-struct [1])) "struct? false for array")
(assert (check-set |1 2|) "set? true for set")
(assert (not (check-set {:a 1})) "set? false for struct")

## No IsArray/IsStruct/IsSet rejections
## Scan the rejection list and assert no reason mentions these instructions
(defn has-rejection-for? (reasons instr-name)
  (if (= reasons ())
    false
    (if (string/contains? (first reasons) instr-name)
      true
      (has-rejection-for? (rest reasons) instr-name))))

(def @reasons (map (fn (r) (get r :reason)) (jit/rejections)))
(assert (not (has-rejection-for? reasons "IsArray")) "IsArray not rejected")
(assert (not (has-rejection-for? reasons "IsStruct")) "IsStruct not rejected")
(assert (not (has-rejection-for? reasons "IsSet")) "IsSet not rejected")
