(elle/epoch 12)
# Good naming conventions — a zero-diagnostic lint corpus. Bindings are
# immutable so no mutability lint fires, and the trailing struct reads every
# one of them so no unused-binding lint fires. Only the names are the subject.

(def square 42)
(def my-variable 10)
(def add-two (fn (x y) (+ x y)))
(def number? (fn (x) (int? x)))
(def set-value! (fn (x v) v))
(def foo-bar-baz 123)

{:square square
 :my-variable my-variable
 :add-two add-two
 :number? number?
 :set-value! set-value!
 :foo-bar-baz foo-bar-baz}
