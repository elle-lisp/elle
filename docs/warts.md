# Warts

<!-- audited: 2026-09-22 -->

Where Elle surprises a programmer from another Lisp: the intentional differences,
then the known limitations.

## Intentional differences

The three that shape whole programs are in [AGENTS.md](../AGENTS.md) and
[QUICKSTART.md](../QUICKSTART.md); this is the full list.

- **`nil` and `()` are distinct.** `nil` is falsy; `()` is the empty list and is
  truthy. Use `empty?` for end-of-list. [empty-list.md](empty-list.md) gives the
  rationale.
- **Only `nil` and `false` are falsy.** `0`, `""` and `()` are truthy.
- **`#` starts a comment, and `;` splices.** `;[1 2 3]` spreads into the
  surrounding form.
- **`assign` mutates; `set` builds a set.**
- **`let` is sequential.** Bindings are flat pairs, and each sees the ones
  before it.
- **A bare literal is immutable; `@` makes it mutable.** `[1 2]` is an array,
  `@[1 2]` an `@array`.
- **Strings count grapheme clusters.** `length`, indexing and slicing work in
  grapheme clusters, not bytes or code points.
- **Integers wrap.** Integer arithmetic is 64-bit two's complement, and `/` on
  two integers truncates.
- **`print` and `println` write their arguments with no separator.** A string
  prints without quotes and a keyword without its colon.

```lisp
(assert (if () true false))
(assert (= (length "café") 4))
(assert (= (+ 9223372036854775807 1) -9223372036854775808))
(assert (= (/ 7 2) 3))
(assert (= (set 1 2) |1 2|))
(let [a 1 b (+ a 1)] (assert (= b 2)))
```

## Known limitations

- **Non-tail recursion stops at depth 200** (#1207). Tail calls, mutual ones
  included, run in constant stack.
- **A macro cannot be exported from a module.** A module's `defmacro` is not a
  value its export struct can name ([macros.md](macros.md)).
- **A macro cannot return an improper list.** The expander converts a macro's
  result to syntax, and syntax has no improper-list form.
- **A mutable container stored into itself leaks its region** (#1228). Other
  mutable cycles free with their owner where the ownership forest can own them
  ([regions/semantics.md](regions/semantics.md)).
