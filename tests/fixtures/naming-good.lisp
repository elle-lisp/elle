(elle/epoch 12)
# Good naming conventions — a zero-diagnostic lint corpus. Bindings are
# immutable so no mutability lint fires; only the names are the subject.

(def square 42)
(def my-variable 10)
(def add-two (fn (x y) (+ x y)))
(def number? (fn (x) (int? x)))
(def set-value! (fn (x v) v))
(def foo-bar-baz 123)
