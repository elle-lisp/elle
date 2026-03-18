## Type Predicates

| Predicate | Matches | Notes |
|-----------|---------|-------|
| `(fn? x)` | closure or native-fn | any callable value |
| `(closure? x)` | user-defined closures only | `(fn ...)` forms |
| `(native-fn? x)` | native (built-in) functions only | aliases: `native?`, `primitive?` |
| `(nil? x)` | nil | |
| `(boolean? x)` | true or false | alias: `bool?` |
| `(integer? x)` | 48-bit signed integers | alias: `int?` |
| `(float? x)` | IEEE 754 doubles | |
| `(number? x)` | integer or float | |
| `(string? x)` | immutable or mutable strings | |
| `(symbol? x)` | symbols | |
| `(keyword? x)` | keywords | |
| `(pair? x)` | cons cells | |
| `(list? x)` | cons cells or empty list | |
| `(array? x)` | immutable or mutable arrays | alias: `tuple?` |
| `(struct? x)` | immutable or mutable structs | alias: `table?` |
| `(bytes? x)` | immutable or mutable bytes | |
| `(mutable? x)` | any mutable collection | |

### type-of

`(type-of x)` returns the type as a keyword:

```lisp
(type-of 42)        #=> :integer
(type-of "hello")   #=> :string
(type-of +)         #=> :native-fn
(type-of (fn [x] x)) #=> :closure
(type-of nil)       #=> :nil
```
