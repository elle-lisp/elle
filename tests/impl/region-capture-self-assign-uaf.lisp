(elle/epoch 15)
# audited: 2026-10-30
# A closure that assigns a captured binding the value it already holds leaves that value's region live.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree and keeps the JIT off, so an over-free faults at
# the stale deref.
#
# The binding is captured, so the assign stores through its capture cell
# (`capture_store_with_rebind`). The cell holds the last count of the value's
# region. A store that releases the displaced value before it counts the new
# one takes that count to zero and frees the region, then counts the value it
# is storing into a freed region. Every read of the binding after the closure
# returns is then a use-after-free.
#
# Each shape below faults on its own when the store releases first. A file
# runs as one process, so before the fix the first shape is the one that
# faults. The shapes sit at file scope, where the cell holds the region's last
# count; inside a function the init's own reference outlives the closure call.

## 1. A `let` binding.
(let [@s {:a 1}]
  ((fn [] (assign s s)))
  (assert (= s {:a 1}) "let: the value survives the self-assign"))

## 2. A file-scope `def`.
(def @d {:a 1})
((fn [] (assign d d)))
(assert (= d {:a 1}) "def: the value survives the self-assign")

## 3. A second binding of the same value holds no reference of its own.
(let [@s {:a 1}
      keep s]
  ((fn [] (assign s s)))
  (assert (= s {:a 1}) "alias: the binding survives the self-assign"))

## 4. A field read after the self-assign.
(let [@s {:a 1}]
  ((fn [] (assign s s)))
  (assert (= (s :a) 1) "field: the read finds the field"))

## 5. A step function that answers its state unchanged, as an immediate-mode
## frame loop does when nothing happened.
(defn step [state event]
  state)
(defn drive [f]
  (f))
(let [@state {:selected nil}]
  (drive (fn [] (assign state (step state {:clicks ||}))))
  (assert (= state {:selected nil}) "step: the state survives the frame"))

## 6. A cell filled from a call result inside one closure, then stored back by
## another.
(defn fresh [n]
  {:a n})
(let [@s nil]
  ((fn [] (assign s (fresh 1))))
  ((fn [] (assign s s)))
  (assert (= s {:a 1}) "fill: the value survives the self-assign"))

## 7. The same self-assign, repeated.
(let [@s {:a 1}
      again (fn [] (assign s s))]
  (repeat 3 (again))
  (assert (= s {:a 1}) "repeat: the value survives every self-assign"))
