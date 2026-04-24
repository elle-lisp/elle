# elle fmt

Opinionated code formatter for Elle. One canonical style.

## Usage

```
elle fmt [OPTIONS] <file...>     Format files in place
elle fmt --check <file...>       Check mode (exit 1 if changes needed)
elle fmt < input.lisp            Format stdin to stdout
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--check` | off | Don't write; list files that need formatting, enforce column limits |
| `--line-length=N` | 80 | Target line width |
| `--indent-width=N` | 2 | Spaces per indent level |

### Column enforcement (--check only)

- **Warning at column 60**: opening delimiter past column 60 suggests
  lifting a lambda or refactoring.
- **Error at column 80**: opening delimiter past column 80 means too
  much nesting; `--check` exits 1.

## Rule set

### General principles

- **Idempotent.** `format(format(x)) == format(x)`, always.
- **2-space indent.** Every nesting level adds 2 spaces.
- **Trailing newline.** Output always ends with `\n`.
- **Shebang preserved.** `#!/usr/bin/env elle` stays on line 1.
- **Comments preserved.** Inline comments stay inline; block comments
  stay on their own line.

### Definitions

`defn`, `defmacro`: header on first line, body always breaks.

```scheme
(defn fib [n]
  (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))
```

`def`: inline if fits, break with +2 if not.

```scheme
(def x 5)
(def long-name
  (compute-something))
```

### Bindings

`let`, `let*`, `letrec`: one binding pair per line, names aligned to the
column after `[`. Body indented +2.

```scheme
(let [x 5
      y 10]
  (+ x y))

(let* [a (compute-a)
       b (compute-b a)]
  (combine a b))
```

### Conditionals

`if`: trivial branches inline. Compound branches break with +2 indent,
aligned relative to `(if`.

```scheme
(if (> x 0) x (- x))

(if (> x 0)
  (begin
    (print x)
    x)
  (begin
    (print "negative")
    (- x)))
```

`cond`: flat alternating pairs. Trivial body stays with test;
compound body breaks +2.

```scheme
(cond
  (< x 0) "negative"
  (= x 0) "zero"
  true "positive")

(cond
  (< x 0)
    (begin
      (log "negative")
      "negative")
  true "non-negative")
```

`match`: flat alternating pairs after expr, same layout as cond.

```scheme
(match x
  1 "one"
  2 "two"
  _ "other")
```

`case`: flat alternating pairs. Non-trivial results break +2.

```scheme
(case status
  :ok (handle-ok)
  :error
    (begin
      (log-error)
      (retry)))
```

### Loops and control

`when`, `unless`: test on first line, body +2.

```scheme
(when (> x 0)
  (print "positive")
  x)
```

`while`: single body inline, multi-body breaks.

`each`: header (`each item [in] collection`) always on one line, body +2.
The `in` keyword is optional.

```scheme
(each item in items
  (print item))

(each block (get cfg :blocks)
  (process block))
```

`forever`: single body inline, multi-body breaks like begin.

```scheme
(forever (pump))

(forever
  (read-input)
  (process)
  (flush))
```

`begin`: always breaks, body +2.

`block`: like begin, `:name` stays on the block line.

```scheme
(block :main
  (setup)
  (run))
```

### Functions

`fn`: single body tries inline; multi-body breaks. Body aligns relative to
`(fn`'s actual column (via `Align`), so lambdas inside bindings indent correctly.

```scheme
(fn [x] (+ x 1))

(letrec [loop (fn (i)
                (if (>= i n)
                  result
                  (loop (+ i 1))))]
  (loop 0))
```

### Generic calls

Short head (first-arg column <= line_length / 4): columnar alignment via
`Align`. Args align to the first arg's column.

```scheme
(map (fn [x] (+ x 1))
     items)
```

Long head: fall back to +2 indent.

```scheme
(some-very-long-function arg1
  arg2
  arg3)
```

### Logical operators

`and`, `or`, `not`: columnar alignment like generic calls.

```scheme
(when (and (nil? first-error)
           (or (= s :error) (not done?)))
  (handle-error))
```

### Threading macros

`->`, `->>`, `some->`, `some->>`: value on first line, steps aligned
with value.

```scheme
(-> data
    (transform)
    (filter valid?)
    (collect))

(->> items
     (map inc)
     (filter even?)
     (take 5))
```

### Parameterize

Bindings each on a new line, aligned to column after `(parameterize (`.

```scheme
(parameterize ((*scheduler* sched)
               (*spawn* (get sched :spawn))
               (*shutdown* (get sched :shutdown)))
  body)
```

### Collections

Arrays, sets, structs: inline if fits; broken elements align to column
after the opening delimiter.

```scheme
[1 2 3]

{:error :type-error
 :reason :not-a-sequence
 :message "not a sequence"}
```

Structs group elements as key-value pairs.

### Comments

- Inline comments: 2 spaces before `#`, stay on the same line.
- Block comments: own line, indented with surrounding code.
- `CommentBreak` prevents double-newline when a trailing comment
  precedes the inter-sibling line break.
