# format

<!-- audited: 2026-09-16 -->

`string/format`: template parsing, format specifications, and the two
substitution modes.

Up: [..](../AGENTS.md)

## Layout

| File | Contains |
|------|----------|
| `format.rs` | The primitive entry point and its registration |
| `format/parse.rs` | `parse_placeholders` — extracts `{...}`, handling `{{` and `}}` |
| `format/dispatch.rs` | `format_positional`, `format_named` — mode selection |
| `format/value.rs` | `format_value`, `format_raw`, `apply_width_align` |
| `format/build.rs` | `build_output`, `unescape_into` |
| `formatspec.rs` | `parse_format_spec` — alignment, fill, width, precision, type |

**Signature:** `(string/format template [args...])` or
`(string/format template :key val ...)`

## Modes

**Positional mode:** Arguments are substituted in order.

```lisp
(string/format "{} + {} = {}" 1 2 3)  #=> "1 + 2 = 3"
(string/format "Hello, {}!" "Alice")  #=> "Hello, Alice!"
```

**Named mode:** Arguments are keyword-value pairs, substituted by name.

```lisp
(string/format "{name} is {age}" :name "Alice" :age 30)  #=> "Alice is 30"
(string/format "{greeting}, {name}!" :greeting "Hello" :name "Bob")  #=> "Hello, Bob!"
```

A template cannot mix the two. Both `{}` and `{name}` in one template is an
error.

## Format specifications

Syntax: `{[name][:spec]}` where spec is `[[fill]align][width][.precision][type]`.

**Alignment:** `<` (left), `>` (right), `^` (center). Default: right for
numbers, left for strings.

**Fill character:** Any char before alignment. Default: space. Example:
`{:*^10}` centers with `*` padding.

**Width:** Minimum field width. Example: `{:10}` pads to 10 chars.

**Precision:** For floats, decimal places. For strings, max chars. Example:
`{:.2f}` gives 2 decimal places.

**Type:** `d` (decimal), `x` (hex lowercase), `X` (hex uppercase), `o` (octal),
`b` (binary), `f` (float), `e` (scientific), `s` (string).

**Examples:**

- `{:.2f}` — float with 2 decimal places
- `{:>10}` — right-align to 10 chars
- `{:<10}` — left-align to 10 chars
- `{:^10}` — center to 10 chars
- `{:05d}` — zero-pad integer to 5 digits
- `{:x}` — hex lowercase
- `{:X}` — hex uppercase
- `{:o}` — octal
- `{:b}` — binary
- `{:e}` — scientific notation
- `{:*^10}` — center with `*` fill to 10 chars

## Brace escaping

`{{` becomes `{`, `}}` becomes `}`. Escaping is processed in literal segments,
outside placeholders.

```lisp
(string/format "literal {{braces}}")  #=> "literal {braces}"
```

## Error cases

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Template not string | `type-error` | `"string/format: template must be string, got {type}"` |
| Unmatched `{` | `format-error` | `"string/format: unmatched '{' in template"` |
| Unmatched `}` | `format-error` | `"string/format: unmatched '}' in template"` |
| Positional arg count mismatch | `format-error` | `"string/format: expected N arguments, got M"` |
| Mixed positional/named | `format-error` | `"string/format: cannot mix positional and named arguments"` |
| Odd keyword args | `format-error` | `"string/format: odd number of keyword arguments"` |
| Non-keyword in named position | `type-error` | `"string/format: expected keyword, got {type}"` |
| Missing named key | `format-error` | `"string/format: missing key '{name}'"` |
| Extra named key | `format-error` | `"string/format: unexpected key '{name}'"` |
| Invalid format spec | `format-error` | `"string/format: invalid format spec '{spec}'"` |
| Type mismatch in format | `format-error` | `"string/format: cannot format {type} with spec '{char}'"` |

## Invariants

1. **No mixing modes.** Positional and named placeholders cannot coexist in the
   same template.
2. **Arity enforcement.** Positional mode requires exactly as many args as
   placeholders. Named mode requires even args (key-value pairs).
3. **Type safety.** Format specs are validated against value types — `d`
   requires an integer, `f` requires a number.
4. **Brace escaping.** `{{` and `}}` are unescaped only in literal segments,
   never inside placeholders.
